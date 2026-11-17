use std::{io::Write, path::PathBuf, sync::Arc};

use grammers_client::{Client, media::Media};
use tg_media_cache::{CacheLease, MediaCache};
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

pub(crate) fn event(
    target: &MediaTarget,
    generation: u64,
    state: AttachmentDownloadState,
) -> CoreEvent {
    CoreEvent::MediaTransferUpdated {
        chat_id: target.chat_id,
        message_id: target.message_id,
        attachment_index: target.attachment_index,
        generation,
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
        TransferState::Download(DownloadState::Failed) => {
            AttachmentDownloadState::Failed { reason: None }
        }
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
            // Emit Failed only if this task still owns an active transfer.
            // Cancellation advances the generation and makes this token stale,
            // so late I/O errors cannot overwrite the user's Cancelled state.
            if let Some(failed_state) = failure_state(&transfers, queued_token, reason) {
                let _ = updates
                    .send(Ok(event(&target, queued_token.generation, failed_state)))
                    .await;
            }
        }
    })
}

fn validate_cache_size(total_bytes: Option<u64>, cache_limit: u64) -> Result<(), String> {
    if total_bytes.is_some_and(|total| total > cache_limit) {
        return Err(format!(
            "media is larger than the {cache_limit}-byte download cache limit"
        ));
    }
    Ok(())
}

fn failure_state(
    transfers: &TransferManager,
    token: TransferToken,
    reason: String,
) -> Option<AttachmentDownloadState> {
    transfers.fail(token).ok()?;
    Some(AttachmentDownloadState::Failed {
        reason: Some(reason),
    })
}

/// Keeps a committed file only if its Downloaded update was accepted by the
/// core event stream. If the task is aborted while publishing completion, its
/// lease is released before removing the file so a cancelled transfer cannot
/// leave an unreported cache entry behind.
struct PendingDownloadPublication {
    cache: MediaCache,
    reference: String,
    lease: Option<CacheLease>,
    path: PathBuf,
    published: bool,
}

impl PendingDownloadPublication {
    fn new(cache: MediaCache, reference: String, lease: CacheLease) -> Self {
        let path = lease.path().to_path_buf();
        Self {
            cache,
            reference,
            lease: Some(lease),
            path,
            published: false,
        }
    }

    fn local_path(&self) -> PathBuf {
        self.path.clone()
    }

    fn mark_published(&mut self) {
        self.published = true;
    }
}

impl Drop for PendingDownloadPublication {
    fn drop(&mut self) {
        // Drop the lease before removal; CacheLease intentionally protects the
        // file from eviction and explicit removal while the completion event
        // is being published.
        self.lease.take();
        if !self.published {
            let _ = self.cache.remove(&self.reference);
        }
    }
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

    validate_cache_size(target.total_bytes, cache.max_bytes())?;

    emit_snapshot(updates, target, transfers, active).await;

    // CacheWriter keeps partial bytes in a hidden temporary entry until atomic commit.
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
        writer.write_all(&chunk).map_err(|error| {
            if error.kind() == std::io::ErrorKind::FileTooLarge {
                format!(
                    "media is larger than the {}-byte download cache limit",
                    cache.max_bytes()
                )
            } else {
                "could not write downloaded media".to_owned()
            }
        })?;
        transferred = transferred.saturating_add(chunk.len() as u64);
        transfers
            .report_progress(active, transferred)
            .map_err(|_| "download progress state became invalid".to_owned())?;
        emit_snapshot(updates, target, transfers, active).await;
    }

    let lease = match target.total_bytes {
        Some(expected_size) => writer.commit_with_expected_size(expected_size),
        None => writer.commit(),
    }
    .map_err(|_| "could not commit completed media to cache".to_owned())?;
    let mut pending_publication =
        PendingDownloadPublication::new(cache.clone(), target.reference.clone(), lease);
    let completed = transfers
        .complete(active)
        .map_err(|_| "download completion state became invalid".to_owned())?;

    let snapshot = transfers
        .get(completed)
        .map_err(|_| "completed download state is unavailable".to_owned())?;
    if updates
        .send(Ok(event(
            target,
            snapshot.token.generation,
            state_from_snapshot(&snapshot, Some(pending_publication.local_path())),
        )))
        .await
        .is_ok()
    {
        pending_publication.mark_published();
    }
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
        .send(Ok(event(
            target,
            snapshot.token.generation,
            state_from_snapshot(&snapshot, None),
        )))
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };
    use tg_media_cache::CacheConfig;
    use tg_mobile_core::{TransferDirection, TransferManager};

    static TEST_CACHE_ID: AtomicU64 = AtomicU64::new(0);

    fn test_cache() -> (PathBuf, MediaCache) {
        let root = std::env::temp_dir().join(format!(
            "tg-download-publication-{}-{}",
            std::process::id(),
            TEST_CACHE_ID.fetch_add(1, Ordering::Relaxed),
        ));
        let cache = MediaCache::new(CacheConfig {
            root: root.clone(),
            max_bytes: 1024,
        })
        .expect("cache");
        (root, cache)
    }

    fn committed_download(cache: &MediaCache, reference: &str) -> CacheLease {
        let mut writer = cache.begin_write(reference).expect("writer");
        writer.write_all(b"media").expect("write");
        writer.commit().expect("commit")
    }

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
    fn known_media_larger_than_the_cache_is_rejected_before_network_io() {
        assert!(validate_cache_size(Some(11), 10).is_err());
        assert!(validate_cache_size(Some(10), 10).is_ok());
        assert!(validate_cache_size(None, 10).is_ok());
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

    #[test]
    fn late_download_error_does_not_replace_cancelled_state() {
        let manager = TransferManager::new();
        let remote = manager
            .request(TransferDirection::Download, "cancelled-download", None)
            .unwrap();
        let queued = manager.queue_download(remote.token).unwrap();
        let active = manager.start_download(queued).unwrap();
        let cancelled = manager.cancel(active).unwrap();

        assert_eq!(
            failure_state(&manager, active, "network failed".into()),
            None
        );
        assert_eq!(
            state_from_snapshot(&manager.get(cancelled).unwrap(), None),
            AttachmentDownloadState::Cancelled
        );
    }

    #[test]
    fn aborting_before_completion_publication_removes_the_committed_file() {
        let (root, cache) = test_cache();
        let reference = "cancelled-after-cache-commit";
        let lease = committed_download(&cache, reference);
        let publication =
            PendingDownloadPublication::new(cache.clone(), reference.to_owned(), lease);

        assert!(cache.get(reference).expect("get").is_some());
        drop(publication);

        assert!(cache.get(reference).expect("get after abort").is_none());
        drop(cache);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn published_completion_keeps_the_committed_file() {
        let (root, cache) = test_cache();
        let reference = "published-download";
        let lease = committed_download(&cache, reference);
        let mut publication =
            PendingDownloadPublication::new(cache.clone(), reference.to_owned(), lease);

        publication.mark_published();
        drop(publication);

        assert!(cache.get(reference).expect("get after publish").is_some());
        drop(cache);
        let _ = fs::remove_dir_all(root);
    }
}
