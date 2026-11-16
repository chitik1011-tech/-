use std::{
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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
    send_stage: Arc<AtomicBool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let active = match transfers.start_upload(transfer_token) {
            Ok(token) => token,
            Err(_) => return,
        };

        let metadata = match tokio::fs::metadata(&media.path).await {
            Ok(metadata) if metadata.is_file() && metadata.len() == media.size_bytes => metadata,
            Ok(_) => {
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
            Err(_) => {
                fail_attempt(
                    &transfers,
                    active,
                    &updates,
                    client_id,
                    chat_id,
                    media,
                    caption,
                    "Selected file is no longer available",
                    false,
                    transfer_generation,
                    session_generation,
                    session_epoch,
                )
                .await;
                return;
            }
        };
        let _ = metadata;

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
            transferred: 0,
            last_reported: 0,
            threshold: (media.size_bytes / 100).max(256 * 1024).min(media.size_bytes),
            token: active,
            transfers: Arc::clone(&transfers),
            updates: updates.clone(),
            base,
            session_generation,
            session_epoch,
        };
        let uploaded: Uploaded = match client
            .upload_stream(
                &mut reader,
                media.size_bytes as usize,
                media.file_name.clone(),
            )
            .await
        {
            Ok(uploaded) => uploaded,
            Err(_) => {
                fail_attempt(
                    &transfers,
                    active,
                    &updates,
                    client_id,
                    chat_id,
                    media,
                    caption,
                    "Telegram media upload failed",
                    true,
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
        send_stage.store(true, Ordering::Release);
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

        match send_media_idempotent(
            &client,
            peer,
            uploaded,
            media.kind,
            &media.mime_type,
            caption.clone(),
            client_id,
        )
        .await
        {
            Ok(message) => {
                let _ = transfers.complete(active);
                if dedup.should_emit(&message).await {
                    let _ = updates.send(Ok(CoreEvent::MessageAdded(message.clone()))).await;
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
                    error,
                    CoreError::TransientSend(_) | CoreError::TransientTransport(_) | CoreError::FloodWait { .. }
                );
                if transfers.fail(active).is_ok() {
                    let failed = match error {
                        CoreError::SendOutcomeUnknown(reason) => OutgoingSend::awaiting_confirmation_media(
                            client_id, chat_id, media, caption, reason, transfer_generation,
                        ),
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
        let result = Pin::new(&mut this.inner).poll_read(cx, buffer);
        if matches!(&result, Poll::Ready(Ok(()))) {
            let read = buffer.filled().len().saturating_sub(before) as u64;
            if read > 0 {
                this.transferred = this.transferred.saturating_add(read);
                let current = this.transferred;
                if current.saturating_sub(this.last_reported) >= this.threshold {
                    if this.transfers.report_progress(this.token, current).is_err() {
                        return result;
                    }
                    let progress = this
                        .base
                        .with_upload_progress(current, this.base.transfer_generation)
                        .with_session_token(this.session_generation, this.session_epoch);
                    let _ = this.updates.try_send(Ok(CoreEvent::MessageSendStateChanged(progress)));
                    this.last_reported = current;
                }
            }
        }
        result
    }
}
