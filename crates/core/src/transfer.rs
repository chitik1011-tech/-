//! Backend-neutral download and upload transfer lifecycle.
//!
//! A transfer key is an opaque string. For downloads, callers can use the
//! attachment's Telegram `file_reference` directly; this module deliberately
//! does not define or copy media metadata types.
//!
//! Progress is monotonic within the lifetime of a keyed transfer. Retrying a
//! failed or cancelled transfer advances its generation, so callbacks carrying
//! an earlier token cannot mutate the retried operation.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransferDirection {
    Download,
    Upload,
}

/// Stable, opaque deduplication identity for one transfer operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransferKey {
    pub direction: TransferDirection,
    /// For downloads, the Telegram file reference; for uploads, a caller-owned
    /// idempotency key that remains stable across duplicate requests.
    pub reference: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransferToken {
    pub id: u64,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    Remote,
    Queued,
    Downloading,
    Downloaded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadState {
    Pending,
    Uploading,
    Uploaded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferState {
    Download(DownloadState),
    Upload(UploadState),
}

impl TransferState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Download(
                DownloadState::Downloaded | DownloadState::Failed | DownloadState::Cancelled
            ) | Self::Upload(UploadState::Uploaded | UploadState::Failed | UploadState::Cancelled)
        )
    }

    fn is_active(self) -> bool {
        matches!(
            self,
            Self::Download(DownloadState::Downloading) | Self::Upload(UploadState::Uploading)
        )
    }

    fn direction(self) -> TransferDirection {
        match self {
            Self::Download(_) => TransferDirection::Download,
            Self::Upload(_) => TransferDirection::Upload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferSnapshot {
    pub token: TransferToken,
    pub key: TransferKey,
    pub state: TransferState,
    pub bytes_transferred: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TransferError {
    #[error("transfer token is stale")]
    StaleToken,
    #[error("transfer does not exist")]
    NotFound,
    #[error("invalid transition from {state:?} for {direction:?}")]
    InvalidTransition {
        direction: TransferDirection,
        state: TransferState,
    },
    #[error("progress exceeds declared total bytes")]
    ProgressExceedsTotal,
    #[error("transfer generation or identifier space exhausted")]
    IdExhausted,
    #[error("transfer registry lock was poisoned")]
    LockPoisoned,
}

#[derive(Debug, Clone)]
struct TransferRecord {
    key: TransferKey,
    state: TransferState,
    generation: u64,
    bytes_transferred: u64,
    total_bytes: Option<u64>,
}

#[derive(Debug, Default)]
struct TransferInner {
    next_id: u64,
    records: HashMap<u64, TransferRecord>,
    keys: HashMap<TransferKey, u64>,
}

/// Thread-safe registry providing request deduplication and guarded state
/// transitions for downloads and uploads.
#[derive(Debug, Default)]
pub struct TransferManager {
    inner: Mutex<TransferInner>,
}

impl TransferManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create or return an existing transfer for this key. Duplicate requests,
    /// including concurrent requests, resolve to the same id and state.
    pub fn request(
        &self,
        direction: TransferDirection,
        reference: impl Into<String>,
        total_bytes: Option<u64>,
    ) -> Result<TransferSnapshot, TransferError> {
        let key = TransferKey {
            direction,
            reference: reference.into(),
        };
        let mut inner = self.lock()?;
        if let Some(id) = inner.keys.get(&key).copied() {
            let record = inner.records.get(&id).ok_or(TransferError::NotFound)?;
            return Ok(snapshot(id, record));
        }

        let id = inner
            .next_id
            .checked_add(1)
            .ok_or(TransferError::IdExhausted)?;
        inner.next_id = id;
        let state = match direction {
            TransferDirection::Download => TransferState::Download(DownloadState::Remote),
            TransferDirection::Upload => TransferState::Upload(UploadState::Pending),
        };
        let record = TransferRecord {
            key: key.clone(),
            state,
            generation: 1,
            bytes_transferred: 0,
            total_bytes,
        };
        inner.keys.insert(key, id);
        inner.records.insert(id, record);
        let record = inner.records.get(&id).ok_or(TransferError::NotFound)?;
        Ok(snapshot(id, record))
    }

    pub fn queue_download(&self, token: TransferToken) -> Result<TransferToken, TransferError> {
        self.transition(
            token,
            TransferState::Download(DownloadState::Remote),
            TransferState::Download(DownloadState::Queued),
        )
    }

    pub fn start_download(&self, token: TransferToken) -> Result<TransferToken, TransferError> {
        self.transition(
            token,
            TransferState::Download(DownloadState::Queued),
            TransferState::Download(DownloadState::Downloading),
        )
    }

    pub fn start_upload(&self, token: TransferToken) -> Result<TransferToken, TransferError> {
        self.transition(
            token,
            TransferState::Upload(UploadState::Pending),
            TransferState::Upload(UploadState::Uploading),
        )
    }

    /// Record absolute transferred bytes. Out-of-order updates are harmless:
    /// the stored byte count never decreases.
    pub fn report_progress(
        &self,
        token: TransferToken,
        bytes_transferred: u64,
    ) -> Result<TransferToken, TransferError> {
        self.mutate(token, |record| {
            if !record.state.is_active() {
                return Err(invalid(record));
            }
            if record
                .total_bytes
                .is_some_and(|total| bytes_transferred > total)
            {
                return Err(TransferError::ProgressExceedsTotal);
            }
            record.bytes_transferred = record.bytes_transferred.max(bytes_transferred);
            Ok(())
        })
    }

    pub fn complete(&self, token: TransferToken) -> Result<TransferToken, TransferError> {
        self.mutate(token, |record| {
            record.state = match record.state {
                TransferState::Download(DownloadState::Downloading) => {
                    TransferState::Download(DownloadState::Downloaded)
                }
                TransferState::Upload(UploadState::Uploading) => {
                    TransferState::Upload(UploadState::Uploaded)
                }
                _ => return Err(invalid(record)),
            };
            Ok(())
        })
    }

    pub fn fail(&self, token: TransferToken) -> Result<TransferToken, TransferError> {
        self.mutate(token, |record| {
            if !record.state.is_active() {
                return Err(invalid(record));
            }
            record.state = match record.key.direction {
                TransferDirection::Download => TransferState::Download(DownloadState::Failed),
                TransferDirection::Upload => TransferState::Upload(UploadState::Failed),
            };
            Ok(())
        })
    }

    /// Cancel a queued or active transfer. Successful and already-terminal
    /// transfers are immutable.
    pub fn cancel(&self, token: TransferToken) -> Result<TransferToken, TransferError> {
        self.mutate(token, |record| {
            if record.state.is_terminal() {
                return Err(invalid(record));
            }
            record.state = match record.key.direction {
                TransferDirection::Download => TransferState::Download(DownloadState::Cancelled),
                TransferDirection::Upload => TransferState::Upload(UploadState::Cancelled),
            };
            Ok(())
        })
    }

    /// Retry a failed or cancelled transfer, preserving monotonic byte progress
    /// and returning a new token generation for callbacks from the new attempt.
    pub fn retry(&self, token: TransferToken) -> Result<TransferToken, TransferError> {
        self.mutate(token, |record| {
            let retry_state = match (record.key.direction, record.state) {
                (
                    TransferDirection::Download,
                    TransferState::Download(DownloadState::Failed | DownloadState::Cancelled),
                ) => TransferState::Download(DownloadState::Queued),
                (
                    TransferDirection::Upload,
                    TransferState::Upload(UploadState::Failed | UploadState::Cancelled),
                ) => TransferState::Upload(UploadState::Pending),
                _ => return Err(invalid(record)),
            };
            record.generation = record
                .generation
                .checked_add(1)
                .ok_or(TransferError::IdExhausted)?;
            record.state = retry_state;
            Ok(())
        })
    }

    pub fn get(&self, token: TransferToken) -> Result<TransferSnapshot, TransferError> {
        let inner = self.lock()?;
        let record = inner
            .records
            .get(&token.id)
            .ok_or(TransferError::NotFound)?;
        check_generation(token, record)?;
        Ok(snapshot(token.id, record))
    }

    fn transition(
        &self,
        token: TransferToken,
        expected: TransferState,
        next: TransferState,
    ) -> Result<TransferToken, TransferError> {
        self.mutate(token, |record| {
            if record.state != expected {
                return Err(invalid(record));
            }
            record.state = next;
            Ok(())
        })
    }

    fn mutate(
        &self,
        token: TransferToken,
        operation: impl FnOnce(&mut TransferRecord) -> Result<(), TransferError>,
    ) -> Result<TransferToken, TransferError> {
        let mut inner = self.lock()?;
        let record = inner
            .records
            .get_mut(&token.id)
            .ok_or(TransferError::NotFound)?;
        check_generation(token, record)?;
        operation(record)?;
        Ok(TransferToken {
            id: token.id,
            generation: record.generation,
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, TransferInner>, TransferError> {
        self.inner.lock().map_err(|_| TransferError::LockPoisoned)
    }
}

fn check_generation(token: TransferToken, record: &TransferRecord) -> Result<(), TransferError> {
    if token.generation != record.generation {
        Err(TransferError::StaleToken)
    } else {
        Ok(())
    }
}

fn invalid(record: &TransferRecord) -> TransferError {
    TransferError::InvalidTransition {
        direction: record.state.direction(),
        state: record.state,
    }
}

fn snapshot(id: u64, record: &TransferRecord) -> TransferSnapshot {
    TransferSnapshot {
        token: TransferToken {
            id,
            generation: record.generation,
        },
        key: record.key.clone(),
        state: record.state,
        bytes_transferred: record.bytes_transferred,
        total_bytes: record.total_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn download_lifecycle_tracks_progress_and_terminal_state() {
        let manager = TransferManager::new();
        let remote = manager
            .request(TransferDirection::Download, "opaque-telegram-ref", Some(10))
            .unwrap();
        assert_eq!(remote.state, TransferState::Download(DownloadState::Remote));
        let queued = manager.queue_download(remote.token).unwrap();
        let active = manager.start_download(queued).unwrap();
        manager.report_progress(active, 7).unwrap();
        manager.report_progress(active, 3).unwrap();
        let completed = manager.complete(active).unwrap();
        let snapshot = manager.get(completed).unwrap();
        assert_eq!(snapshot.bytes_transferred, 7);
        assert_eq!(
            snapshot.state,
            TransferState::Download(DownloadState::Downloaded)
        );
        assert!(snapshot.state.is_terminal());
        assert!(matches!(
            manager.report_progress(completed, 8),
            Err(TransferError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn upload_lifecycle_and_failure_retry_reject_old_callbacks() {
        let manager = TransferManager::new();
        let pending = manager
            .request(TransferDirection::Upload, "upload-key-1", Some(12))
            .unwrap();
        let active = manager.start_upload(pending.token).unwrap();
        manager.report_progress(active, 8).unwrap();
        let failed = manager.fail(active).unwrap();
        let retry = manager.retry(failed).unwrap();
        assert_eq!(retry.generation, failed.generation + 1);
        assert_eq!(
            manager.get(retry).unwrap().state,
            TransferState::Upload(UploadState::Pending)
        );
        assert_eq!(manager.get(retry).unwrap().bytes_transferred, 8);
        assert_eq!(
            manager.report_progress(failed, 9),
            Err(TransferError::StaleToken)
        );
        let active_retry = manager.start_upload(retry).unwrap();
        assert_eq!(
            manager.complete(active_retry).unwrap().generation,
            retry.generation
        );
    }

    #[test]
    fn duplicate_requests_share_one_transfer() {
        let manager = TransferManager::new();
        let first = manager
            .request(TransferDirection::Download, "same-ref", Some(4))
            .unwrap();
        let second = manager
            .request(TransferDirection::Download, "same-ref", Some(99))
            .unwrap();
        assert_eq!(first.token, second.token);
        assert_eq!(second.total_bytes, Some(4));
    }

    #[test]
    fn cancellation_is_terminal_until_explicit_retry() {
        let manager = TransferManager::new();
        let pending = manager
            .request(TransferDirection::Upload, "cancel-me", None)
            .unwrap();
        let cancelled = manager.cancel(pending.token).unwrap();
        assert_eq!(
            manager.get(cancelled).unwrap().state,
            TransferState::Upload(UploadState::Cancelled)
        );
        assert!(matches!(
            manager.start_upload(cancelled),
            Err(TransferError::InvalidTransition { .. })
        ));
        let retry = manager.retry(cancelled).unwrap();
        assert_eq!(
            manager.get(retry).unwrap().state,
            TransferState::Upload(UploadState::Pending)
        );
    }

    #[test]
    fn concurrent_duplicate_requests_are_deduplicated() {
        let manager = Arc::new(TransferManager::new());
        let workers = (0..12)
            .map(|_| {
                let manager = Arc::clone(&manager);
                thread::spawn(move || {
                    (0..100)
                        .map(|_| {
                            manager
                                .request(TransferDirection::Download, "shared", None)
                                .unwrap()
                                .token
                                .id
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect::<Vec<_>>();
        let ids = workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        assert!(ids.iter().all(|id| *id == ids[0]));
    }

    #[test]
    fn concurrent_out_of_order_progress_remains_monotonic() {
        let manager = Arc::new(TransferManager::new());
        let remote = manager
            .request(TransferDirection::Download, "progress", Some(1_000))
            .unwrap();
        let queued = manager.queue_download(remote.token).unwrap();
        let active = manager.start_download(queued).unwrap();
        let workers = (0..8)
            .map(|worker_index| {
                let manager = Arc::clone(&manager);
                thread::spawn(move || {
                    for bytes in (worker_index + 1..=1_000).step_by(8).rev() {
                        manager.report_progress(active, bytes).unwrap();
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(manager.get(active).unwrap().bytes_transferred, 1_000);
    }
}
