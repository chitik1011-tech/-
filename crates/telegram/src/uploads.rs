use std::{
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    task::{Context, Poll},
};

use grammers_client::{Client, media::Uploaded, session::types::PeerRef};
use tg_mobile_core::{
    BackendUpdateSender, CoreError, CoreEvent, OutgoingMediaAttachment, OutgoingSend,
    OutgoingSendState, TransferManager, TransferToken,
};
use tokio::{
    fs::File,
    io::{AsyncRead, ReadBuf},
    task::JoinHandle,
};

use crate::messages::{MessageDeduplicator, send_media_idempotent};

const STAGE_UPLOADING: u8 = 0;
const STAGE_SENDING: u8 = 1;
const STAGE_CANCELLED: u8 = 2;

#[derive(Debug, Default)]
pub(crate) struct MediaSendStage(AtomicU8);

impl MediaSendStage {
    pub(crate) fn try_begin_send(&self) -> bool {
        self.0
            .compare_exchange(
                STAGE_UPLOADING,
                STAGE_SENDING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub(crate) fn try_cancel(&self) -> bool {
        self.0
            .compare_exchange(
                STAGE_UPLOADING,
                STAGE_CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }
}

/// Spawn a single upload/send attempt. TransferManager owns cancellation and
/// invalidates progress callbacks from any cancelled generation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_media_send(
    client: Client,
    peer: PeerRef,
    media: OutgoingMediaAttachment,
    caption: String,
    client_id: i64,
    chat_id: i64,
    transfer_token: TransferToken,
    transfer_generation: u64,
    session_generation: u64,
    session_epoch: u64,
    transfers: Arc<TransferManager>,
    updates: BackendUpdateSender,
    dedup: Arc<MessageDeduplicator>,
    send_stage: Arc<MediaSendStage>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let active = match transfers.start_upload(transfer_token) {
            Ok(token) => token,
            Err(_) => return,
        };

        let file = match File::open(&media.path).await {
            Ok(file) => file,
            Err(_) => {
                fail_attempt(
                    &transfers,
                    active,
                    &updates,
                    client_id,
                    chat_id,
                    media,
                    caption,
                    "Selected file could not be opened",
                    false,
                    transfer_generation,
                    session_generation,
                    session_epoch,
                )
                .await;
                return;
            }
        };
        // Validate the same opened file that will be streamed. Checking the path
        // first leaves a replace-between-stat-and-open race that can upload a
        // different file (or a truncated file) than the queued outbox payload.
        let metadata = match file.metadata().await {
            Ok(metadata) => metadata,
            Err(_) => {
                fail_attempt(
                    &transfers,
                    active,
                    &updates,
                    client_id,
                    chat_id,
                    media,
                    caption,
                    "Selected file could not be verified",
                    false,
                    transfer_generation,
                    session_generation,
                    session_epoch,
                )
                .await;
                return;
            }
        };
        if !metadata.is_file() || metadata.len() != media.size_bytes {
            fail_attempt(
                &transfers,
                active,
                &updates,
                client_id,
                chat_id,
                media,
                caption,
                "Selected file changed after it was queued",
                false,
                transfer_generation,
                session_generation,
                session_epoch,
            )
            .await;
            return;
        }
        let upload_size = match usize::try_from(media.size_bytes) {
            Ok(size) => size,
            Err(_) => {
                fail_attempt(
                    &transfers,
                    active,
                    &updates,
                    client_id,
                    chat_id,
                    media,
                    caption,
                    "Selected file exceeds this device's upload size limit",
                    false,
                    transfer_generation,
                    session_generation,
                    session_epoch,
                )
                .await;
                return;
            }
        };

        let base = OutgoingSend::uploading_media(
            client_id,
            chat_id,
            media.clone(),
            caption.clone(),
            0,
            transfer_generation,
        )
        .with_session_token(session_generation, session_epoch);
        let mut reader = ProgressReader {
            inner: file,
            expected_bytes: media.size_bytes,
            transferred: 0,
            last_reported: 0,
            threshold: (media.size_bytes / 100)
                .max(256 * 1024)
                .min(media.size_bytes),
            token: active,
            transfers: Arc::clone(&transfers),
            updates: updates.clone(),
            base,
            session_generation,
            session_epoch,
        };
        let uploaded: Uploaded = match client
            .upload_stream(&mut reader, upload_size, media.file_name.clone())
            .await
        {
            Ok(uploaded) if reader.transferred == media.size_bytes => uploaded,
            Ok(_) => {
                fail_attempt(
                    &transfers,
                    active,
                    &updates,
                    client_id,
                    chat_id,
                    media,
                    caption,
                    "Selected file changed during upload",
                    false,
                    transfer_generation,
                    session_generation,
                    session_epoch,
                )
                .await;
                return;
            }
            Err(error) => {
                let file_changed = error.kind() == io::ErrorKind::UnexpectedEof;
                fail_attempt(
                    &transfers,
                    active,
                    &updates,
                    client_id,
                    chat_id,
                    media,
                    caption,
                    if file_changed {
                        "Selected file changed during upload"
                    } else {
                        "Telegram media upload failed"
                    },
                    !file_changed,
                    transfer_generation,
                    session_generation,
                    session_epoch,
                )
                .await;
                return;
            }
        };

        // From this point a server send may be in flight. Keep the same MTProto
        // random_id on retries; cancellation is refused once this phase starts.
        if !send_stage.try_begin_send() {
            return;
        }
        let sending = OutgoingSend::sending_media(
            client_id,
            chat_id,
            media.clone(),
            caption.clone(),
            transfer_generation,
        )
        .with_session_token(session_generation, session_epoch);
        let _ = updates
            .send(Ok(CoreEvent::MessageSendStateChanged(sending)))
            .await;

        match send_media_idempotent(&client, peer, uploaded, &media, caption.clone(), client_id)
            .await
        {
            Ok(message) => {
                let _ = transfers.complete(active);
                if dedup.should_emit(&message).await {
                    let _ = updates
                        .send(Ok(CoreEvent::MessageAdded(message.clone())))
                        .await;
                }
                let sent = OutgoingSend::sent_media(
                    client_id,
                    chat_id,
                    media,
                    caption,
                    message.id,
                    transfer_generation,
                )
                .with_session_token(session_generation, session_epoch);
                let _ = updates
                    .send(Ok(CoreEvent::MessageSendStateChanged(sent)))
                    .await;
            }
            Err(error) => {
                let retryable = matches!(
                    &error,
                    CoreError::TransientSend(_)
                        | CoreError::TransientTransport(_)
                        | CoreError::FloodWait { .. }
                );
                if transfers.fail(active).is_ok() {
                    let failed = match error {
                        CoreError::SendOutcomeUnknown(reason) => {
                            OutgoingSend::awaiting_confirmation_media(
                                client_id,
                                chat_id,
                                media,
                                caption,
                                reason,
                                transfer_generation,
                            )
                        }
                        CoreError::FloodWait { seconds } => OutgoingSend::failed_media(
                            client_id,
                            chat_id,
                            media,
                            caption,
                            format!("Telegram rate limit: wait {seconds} seconds"),
                            true,
                            transfer_generation,
                        ),
                        other => OutgoingSend::failed_media(
                            client_id,
                            chat_id,
                            media,
                            caption,
                            other.to_string(),
                            retryable,
                            transfer_generation,
                        ),
                    }
                    .with_session_token(session_generation, session_epoch);
                    let _ = updates
                        .send(Ok(CoreEvent::MessageSendStateChanged(failed)))
                        .await;
                }
            }
        }
    })
}

#[allow(clippy::too_many_arguments)]
async fn fail_attempt(
    transfers: &TransferManager,
    token: TransferToken,
    updates: &BackendUpdateSender,
    client_id: i64,
    chat_id: i64,
    media: OutgoingMediaAttachment,
    caption: String,
    reason: &str,
    retryable: bool,
    transfer_generation: u64,
    session_generation: u64,
    session_epoch: u64,
) {
    if transfers.fail(token).is_ok() {
        let failed = OutgoingSend::failed_media(
            client_id,
            chat_id,
            media,
            caption,
            reason,
            retryable,
            transfer_generation,
        )
        .with_session_token(session_generation, session_epoch);
        let _ = updates
            .send(Ok(CoreEvent::MessageSendStateChanged(failed)))
            .await;
    }
}

struct ProgressReader {
    inner: File,
    expected_bytes: u64,
    transferred: u64,
    last_reported: u64,
    threshold: u64,
    token: TransferToken,
    transfers: Arc<TransferManager>,
    updates: BackendUpdateSender,
    base: OutgoingSend,
    session_generation: u64,
    session_epoch: u64,
}

impl AsyncRead for ProgressReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buffer.filled().len();
        let remaining_capacity = buffer.remaining();
        let result = Pin::new(&mut this.inner).poll_read(cx, buffer);
        if matches!(&result, Poll::Ready(Ok(()))) {
            let read = buffer.filled().len().saturating_sub(before) as u64;
            if read == 0 && remaining_capacity > 0 && this.transferred < this.expected_bytes {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "selected media file ended before its queued size",
                )));
            }
            if read > 0 {
                this.transferred = this.transferred.saturating_add(read);
                let current = this.transferred;
                if current.saturating_sub(this.last_reported) >= this.threshold {
                    if this.transfers.report_progress(this.token, current).is_err() {
                        return Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "media upload was cancelled",
                        )));
                    }
                    let progress = this
                        .base
                        .with_upload_progress(current, this.base.transfer_generation)
                        .with_session_token(this.session_generation, this.session_epoch);
                    let _ = this
                        .updates
                        .try_send(Ok(CoreEvent::MessageSendStateChanged(progress)));
                    this.last_reported = current;
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };
    use tg_mobile_core::{OutgoingMediaKind, TransferDirection, backend_update_channel};

    static TEST_FILE_ID: AtomicU64 = AtomicU64::new(0);

    fn test_file_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "tg-upload-size-{}-{}",
            std::process::id(),
            TEST_FILE_ID.fetch_add(1, Ordering::Relaxed),
        ))
    }

    #[tokio::test]
    async fn progress_reader_rejects_premature_eof_from_a_truncated_media_file() {
        use tokio::io::AsyncReadExt;

        let path = test_file_path();
        fs::write(&path, b"12345").expect("create original file");
        let file = File::open(&path).await.expect("open file");
        tokio::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .await
            .expect("open writable file handle")
            .set_len(3)
            .await
            .expect("truncate after opening read handle");

        let media = OutgoingMediaAttachment {
            kind: OutgoingMediaKind::Document,
            path: path.to_string_lossy().into_owned(),
            file_name: "document.bin".into(),
            mime_type: "application/octet-stream".into(),
            size_bytes: 5,
            voice_duration_ms: None,
            voice_waveform: None,
            video_duration_ms: None,
            video_width: None,
            video_height: None,
        };
        let transfers = Arc::new(TransferManager::new());
        let pending = transfers
            .request(TransferDirection::Upload, "media-test", Some(5))
            .expect("request upload");
        let active = transfers.start_upload(pending.token).expect("start upload");
        let (updates, _receiver) = backend_update_channel();
        let base = OutgoingSend::uploading_media(1, 2, media, "", 0, 1);
        let mut reader = ProgressReader {
            inner: file,
            expected_bytes: 5,
            transferred: 0,
            last_reported: 0,
            threshold: 5,
            token: active,
            transfers,
            updates,
            base,
            session_generation: 1,
            session_epoch: 1,
        };
        let mut bytes = Vec::new();

        let error = reader
            .read_to_end(&mut bytes)
            .await
            .expect_err("a truncated file must not be treated as a successful upload stream");
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(bytes, b"123");
        drop(reader);
        fs::remove_file(path).expect("remove temporary test file");
    }

    #[test]
    fn upload_cancellation_and_send_submission_are_mutually_exclusive() {
        let cancelled = MediaSendStage::default();
        assert!(cancelled.try_cancel());
        assert!(!cancelled.try_begin_send());
        assert!(!cancelled.try_cancel());

        let sending = MediaSendStage::default();
        assert!(sending.try_begin_send());
        assert!(!sending.try_cancel());
        assert!(!sending.try_begin_send());
    }

    #[test]
    fn concurrent_cancel_and_send_submission_have_exactly_one_winner() {
        use std::sync::Barrier;

        for _ in 0..128 {
            let stage = Arc::new(MediaSendStage::default());
            let barrier = Arc::new(Barrier::new(3));
            let cancel_stage = Arc::clone(&stage);
            let cancel_barrier = Arc::clone(&barrier);
            let cancel = std::thread::spawn(move || {
                cancel_barrier.wait();
                cancel_stage.try_cancel()
            });
            let send_stage = Arc::clone(&stage);
            let send_barrier = Arc::clone(&barrier);
            let send = std::thread::spawn(move || {
                send_barrier.wait();
                send_stage.try_begin_send()
            });

            barrier.wait();
            let cancelled = cancel.join().expect("cancel thread should finish");
            let sending = send.join().expect("send thread should finish");
            assert_ne!(cancelled, sending, "exactly one transition must win");
        }
    }
}
