use std::{io::Write, path::PathBuf, sync::Arc};

use grammers_client::{media::Media, Client};
use tg_media_cache::MediaCache;
use tg_mobile_core::{
    AttachmentDownloadState, BackendUpdateSender, CoreEvent, DownloadState, TransferManager,
    TransferSnapshot, TransferState, TransferToken,
};
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub(crate) struct MediaTarget {
    pub chat_id: i64,
    pub message_id: i64,
    pub attachment_index: usize,
    pub reference: String,
    pub total_bytes: Option<u64>,
}

pub(crate) fn event(target: &MediaTarget, state: AttachmentDownloadState) -> CoreEvent {
    CoreEvent::MediaTransferUpdated {
        chat_id: target.chat_id,
        message_id: target.message_id,
        attachment_index: target.attachment_index,
        state,
    }
}

pub(crate) fn state_from_snapshot(
    snapshot: &TransferSnapshot,
    local_path: Option<PathBuf>,
) -> AttachmentDownloadState {
    match snapshot.state {
        TransferState::Download(DownloadState::Remote) => AttachmentDownloadState::NotDownloaded,
        TransferState::Download(DownloadState::Queued) => AttachmentDownloadState::Queued,
        TransferState::Download(DownloadState::Downloading) => {
            AttachmentDownloadState::Downloading {
                bytes_transferred: snapshot.bytes_transferred,
                total_bytes: snapshot.total_bytes,
            }
        }
        TransferState::Download(DownloadState::Downloaded) => local_path
            .map(|local_path| AttachmentDownloadState::Downloaded { local_path })
            .unwrap_or(AttachmentDownloadState::NotDownloaded),
        TransferState::Download(DownloadState::Failed) => AttachmentDownloadState::Failed {
            reason: None,
        },
        TransferState::Download(DownloadState::Cancelled) => AttachmentDownloadState::Cancelled,
        TransferState::Upload(_) => AttachmentDownloadState::Failed {
            reason: Some("invalid media transfer direction".into()),
        },
    }
}

pub(crate) fn spawn_download(
    client: Client,
    media: Media,
    cache: MediaCache,
    transfers: Arc<TransferManager>,
    updates: BackendUpdateSender,
    target: MediaTarget,
    queued_token: TransferToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        if let Err(reason) = run_download(
            &client,
            &media,
            &cache,
            &transfers,
            &updates,
            &target,
            queued_token,
        )
        .await
        {
            if let Ok(snapshot) = transfers.get(queued_token) {
                let _ = transfers.fail(snapshot.token);
            }
            let _ = updates
                .send(Ok(event(
                    &target,
                    AttachmentDownloadState::Failed {
                        reason: Some(reason),
                    },
                )))
                .await;
        }
    })
}

async fn run_download(
    client: &Client,
    media: &Media,
    cache: &MediaCache,
    transfers: &TransferManager,
    updates: &BackendUpdateSender,
    target: &MediaTarget,
    queued_token: TransferToken,
) -> Result<(), String> {
    let active = transfers
        .start_download(queued_token)
        .map_err(|_| "download state changed before it could start".to_owned())?;

    emit_snapshot(updates, target, transfers, active).await;

    let mut writer = cache
        .begin_write(&target.reference)
        .map_err(|_| "could not prepare private media cache".to_owned())?;
    let mut download = client.iter_download(media);
    let mut transferred = 0u64;

    while let Some(chunk) = download
        .next()
        .await
        .map_err(|_| "Telegram media download failed".to_owned())?
    {
        writer
            .write_all(&chunk)
            .map_err(|_| "could not write downloaded media".to_owned())?;
        transferred = transferred.saturating_add(chunk.len() as u64);
        transfers
            .report_progress(active, transferred)
            .map_err(|_| "download progress state became invalid".to_owned())?;
        emit_snapshot(updates, target, transfers, active).await;
    }

    let lease = writer
        .commit()
        .map_err(|_| "could not commit completed media to cache".to_owned())?;
    let local_path = lease.path().to_path_buf();
    let completed = transfers
        .complete(active)
        .map_err(|_| "download completion state became invalid".to_owned())?;

    let snapshot = transfers
        .get(completed)
        .map_err(|_| "completed download state is unavailable".to_owned())?;
    let _ = updates
        .send(Ok(event(
            target,
            state_from_snapshot(&snapshot, Some(local_path)),
        )))
        .await;
    Ok(())
}

async fn emit_snapshot(
    updates: &BackendUpdateSender,
    target: &MediaTarget,
    transfers: &TransferManager,
    token: TransferToken,
) {
    let Ok(snapshot) = transfers.get(token) else {
        return;
    };
    let _ = updates
        .send(Ok(event(target, state_from_snapshot(&snapshot, None))))
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tg_mobile_core::{TransferDirection, TransferManager};

    #[test]
    fn maps_transfer_progress_to_attachment_state() {
        let manager = TransferManager::new();
        let remote = manager
            .request(TransferDirection::Download, "media", Some(100))
            .unwrap();
        let queued = manager.queue_download(remote.token).unwrap();
        assert_eq!(
            state_from_snapshot(&manager.get(queued).unwrap(), None),
            AttachmentDownloadState::Queued
        );

        let active = manager.start_download(queued).unwrap();
        manager.report_progress(active, 25).unwrap();
        assert_eq!(
            state_from_snapshot(&manager.get(active).unwrap(), None),
            AttachmentDownloadState::Downloading {
                bytes_transferred: 25,
                total_bytes: Some(100),
            }
        );
    }

    #[test]
    fn completed_transfer_without_cache_path_is_not_reported_as_downloaded() {
        let manager = TransferManager::new();
        let remote = manager
            .request(TransferDirection::Download, "evicted", None)
            .unwrap();
        let queued = manager.queue_download(remote.token).unwrap();
        let active = manager.start_download(queued).unwrap();
        let completed = manager.complete(active).unwrap();

        assert_eq!(
            state_from_snapshot(&manager.get(completed).unwrap(), None),
            AttachmentDownloadState::NotDownloaded
        );
    }
}
