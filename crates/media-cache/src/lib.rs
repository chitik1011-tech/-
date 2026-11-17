//! App-private, bounded local cache for downloaded media.
//!
//! The caller must provide a private cache directory through [CacheConfig]. This crate
//! never chooses a platform path. References are converted to opaque SHA-256 file names;
//! raw references are not written to disk.

use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, MutexGuard, OnceLock, Weak,
    },
    time::SystemTime,
};
use thiserror::Error;

const TEMP_MARKER: &str = ".part-";
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
static SHARED_STATES: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<State>>>>> = OnceLock::new();

/// Configuration for a media cache.
///
/// `root` should be an app-private directory supplied by the host application.
#[derive(Clone, Debug)]
pub struct CacheConfig {
    pub root: PathBuf,
    pub max_bytes: u64,
}

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("cache I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("cache state lock was poisoned")]
    LockPoisoned,
    #[error("media item exceeds the configured cache limit")]
    ItemTooLarge,
    #[error("downloaded media size mismatch: expected {expected} bytes, got {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("cache limit cannot be met while all eviction candidates are in use")]
    CacheFull,
    #[error("media item is currently in use")]
    InUse,
    #[error("cache root is not a directory")]
    InvalidRoot,
}

#[derive(Default)]
struct State {
    leases: HashMap<String, usize>,
    last_access: HashMap<String, u64>,
    clock: u64,
}

struct Inner {
    root: PathBuf,
    max_bytes: u64,
    state: Arc<Mutex<State>>,
}

/// A bounded cache rooted in a directory chosen by the caller.
#[derive(Clone)]
pub struct MediaCache {
    inner: Arc<Inner>,
}

impl MediaCache {
    /// Open or initialize a cache in the caller-supplied app-private directory.
    ///
    /// Incomplete temporary files from an interrupted write are removed at startup.
    pub fn new(config: CacheConfig) -> Result<Self, CacheError> {
        fs::create_dir_all(&config.root)?;
        let root = fs::canonicalize(config.root)?;
        if !fs::metadata(&root)?.is_dir() {
            return Err(CacheError::InvalidRoot);
        }
        let (state, is_new_state) = shared_state(&root)?;
        let cache = Self {
            inner: Arc::new(Inner {
                root,
                max_bytes: config.max_bytes,
                state,
            }),
        };
        if is_new_state {
            let mut state = cache.lock_state()?;
            cache.recover_temporary_files()?;
            // Completed entries survive process restarts. Reapply the current
            // bound before serving any cache hits, including after a config or
            // app-version change lowers `max_bytes`.
            cache.enforce_limit(&mut state)?;
        }
        Ok(cache)
    }

    /// Maximum size of one completed cache entry and its staging write.
    pub fn max_bytes(&self) -> u64 {
        self.inner.max_bytes
    }

    /// Return a lease for a cached reference, if present.
    ///
    /// The file is protected from eviction until the returned lease is dropped.
    pub fn get(&self, reference: &str) -> Result<Option<CacheLease>, CacheError> {
        self.get_with_expected_size(reference, None)
    }

    /// Return a cache lease only when the entry also matches a known media size.
    ///
    /// A mismatch is treated as a cache miss so the caller can fetch a fresh copy.
    pub fn get_with_expected_size(
        &self,
        reference: &str,
        expected_size: Option<u64>,
    ) -> Result<Option<CacheLease>, CacheError> {
        let key = cache_key(reference);
        let path = self.final_path(&key);
        let mut state = self.lock_state()?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => metadata,
            Ok(_) => {
                state.last_access.remove(&key);
                return Ok(None);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                state.last_access.remove(&key);
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        if expected_size.is_some_and(|expected| metadata.len() != expected) {
            return Ok(None);
        }
        self.ensure_confined(&path)?;
        mark_accessed(&mut state, &key);
        *state.leases.entry(key.clone()).or_default() += 1;
        Ok(Some(CacheLease {
            path,
            key,
            inner: Arc::clone(&self.inner),
        }))
    }

    /// Start writing a cache entry. Call [CacheWriter::commit] when the complete
    /// download has been written; dropping a writer discards its temporary file.
    pub fn begin_write(&self, reference: &str) -> Result<CacheWriter, CacheError> {
        let key = cache_key(reference);
        let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_path = self.inner.root.join(format!(
            ".{key}{TEMP_MARKER}{}-{unique}",
            std::process::id()
        ));
        let _state = self.lock_state()?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        Ok(CacheWriter {
            cache: self.clone(),
            key,
            temp_path,
            file: Some(file),
            written_bytes: 0,
            max_bytes: self.inner.max_bytes,
            committed: false,
        })
    }

    /// Remove one completed cache entry by its opaque reference.
    ///
    /// Temporary files are not considered entries, and a leased file is never removed.
    pub fn remove(&self, reference: &str) -> Result<bool, CacheError> {
        let key = cache_key(reference);
        let path = self.final_path(&key);
        let mut state = self.lock_state()?;
        if state.leases.get(&key).copied().unwrap_or(0) > 0 {
            return Err(CacheError::InUse);
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_file() => metadata,
            Ok(_) => {
                state.last_access.remove(&key);
                return Ok(false);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                state.last_access.remove(&key);
                return Ok(false);
            }
            Err(error) => return Err(error.into()),
        };
        let _ = metadata;
        self.ensure_confined(&path)?;
        fs::remove_file(path)?;
        state.last_access.remove(&key);
        Ok(true)
    }

    /// Current total bytes occupied by regular cache files.
    pub fn used_bytes(&self) -> Result<u64, CacheError> {
        Ok(self.entries()?.iter().map(|entry| entry.size).sum())
    }

    fn final_path(&self, key: &str) -> PathBuf {
        self.inner.root.join(format!("{key}.media"))
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, State>, CacheError> {
        self.inner
            .state
            .lock()
            .map_err(|_| CacheError::LockPoisoned)
    }

    fn ensure_confined(&self, path: &Path) -> Result<(), CacheError> {
        let canonical_parent = fs::canonicalize(path.parent().ok_or(CacheError::InvalidRoot)?)?;
        if canonical_parent != self.inner.root {
            return Err(CacheError::InvalidRoot);
        }
        Ok(())
    }

    fn recover_temporary_files(&self) -> Result<(), CacheError> {
        for entry in fs::read_dir(&self.inner.root)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') && name.contains(TEMP_MARKER) {
                let path = entry.path();
                if fs::symlink_metadata(&path)?.file_type().is_file() {
                    fs::remove_file(path)?;
                }
            }
        }
        Ok(())
    }

    fn entries(&self) -> Result<Vec<Entry>, CacheError> {
        let mut entries = Vec::new();
        for item in fs::read_dir(&self.inner.root)? {
            let item = item?;
            let path = item.path();
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.file_type().is_file() {
                continue;
            }
            let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(key) = name.strip_suffix(".media") else {
                continue;
            };
            if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                continue;
            }
            self.ensure_confined(&path)?;
            entries.push(Entry {
                key: key.to_owned(),
                path,
                size: metadata.len(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            });
        }
        Ok(entries)
    }

    fn enforce_limit(&self, state: &mut State) -> Result<(), CacheError> {
        let entries = self.entries()?;
        let mut total: u64 = entries.iter().map(|entry| entry.size).sum();
        if total <= self.inner.max_bytes {
            return Ok(());
        }
        let mut candidates = entries;
        candidates.sort_by(|a, b| {
            let a_access = state.last_access.get(&a.key);
            let b_access = state.last_access.get(&b.key);
            match (a_access, b_access) {
                (Some(a_access), Some(b_access)) => {
                    a_access.cmp(b_access).then_with(|| a.key.cmp(&b.key))
                }
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (None, None) => a.modified.cmp(&b.modified).then_with(|| a.key.cmp(&b.key)),
            }
        });
        for entry in candidates {
            if total <= self.inner.max_bytes {
                break;
            }
            if state.leases.get(&entry.key).copied().unwrap_or(0) > 0 {
                continue;
            }
            fs::remove_file(entry.path)?;
            state.last_access.remove(&entry.key);
            total = total.saturating_sub(entry.size);
        }
        if total > self.inner.max_bytes {
            return Err(CacheError::CacheFull);
        }
        Ok(())
    }

    fn ensure_capacity_for(
        &self,
        key: &str,
        incoming_size: u64,
        state: &State,
    ) -> Result<(), CacheError> {
        let entries = self.entries()?;
        let replaced_size = entries
            .iter()
            .find(|entry| entry.key == key)
            .map_or(0, |entry| entry.size);
        let mut total = entries
            .iter()
            .map(|entry| entry.size)
            .sum::<u64>()
            .saturating_sub(replaced_size)
            .saturating_add(incoming_size);
        if total <= self.inner.max_bytes {
            return Ok(());
        }

        let mut candidates: Vec<_> = entries
            .into_iter()
            .filter(|entry| {
                entry.key != key && state.leases.get(&entry.key).copied().unwrap_or(0) == 0
            })
            .collect();
        candidates.sort_by(|a, b| {
            let a_access = state.last_access.get(&a.key);
            let b_access = state.last_access.get(&b.key);
            match (a_access, b_access) {
                (Some(a_access), Some(b_access)) => {
                    a_access.cmp(b_access).then_with(|| a.key.cmp(&b.key))
                }
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (None, None) => a.modified.cmp(&b.modified).then_with(|| a.key.cmp(&b.key)),
            }
        });
        for entry in candidates {
            if total <= self.inner.max_bytes {
                break;
            }
            total = total.saturating_sub(entry.size);
        }
        if total > self.inner.max_bytes {
            return Err(CacheError::CacheFull);
        }
        Ok(())
    }
}

struct Entry {
    key: String,
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

fn shared_state(root: &Path) -> Result<(Arc<Mutex<State>>, bool), CacheError> {
    let registry = SHARED_STATES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut registry = registry.lock().map_err(|_| CacheError::LockPoisoned)?;
    registry.retain(|_, state| state.strong_count() > 0);
    if let Some(state) = registry.get(root).and_then(Weak::upgrade) {
        return Ok((state, false));
    }
    let state = Arc::new(Mutex::new(State::default()));
    registry.insert(root.to_path_buf(), Arc::downgrade(&state));
    Ok((state, true))
}

fn mark_accessed(state: &mut State, key: &str) {
    state.clock = state.clock.wrapping_add(1);
    state.last_access.insert(key.to_owned(), state.clock);
}

fn cache_key(reference: &str) -> String {
    let digest = Sha256::digest(reference.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A live-use guard. Keep it alive for as long as the file is being read.
pub struct CacheLease {
    path: PathBuf,
    key: String,
    inner: Arc<Inner>,
}

impl CacheLease {
    /// Absolute path within the caller-provided cache directory.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for CacheLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.inner.state.lock() {
            if let Some(count) = state.leases.get_mut(&self.key) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    state.leases.remove(&self.key);
                }
            }
        }
    }
}

/// A pending cache write. Uncommitted writes are removed when dropped.
pub struct CacheWriter {
    cache: MediaCache,
    key: String,
    temp_path: PathBuf,
    file: Option<File>,
    written_bytes: u64,
    max_bytes: u64,
    committed: bool,
}

impl CacheWriter {
    /// Atomically publish the completed file and return a lease to it.
    pub fn commit(self) -> Result<CacheLease, CacheError> {
        self.commit_inner(None)
    }

    /// Atomically publish the completed file only if it matches a known size.
    ///
    /// If the write is short or long, the staging file is discarded and never
    /// becomes visible as a cache hit.
    pub fn commit_with_expected_size(self, expected_size: u64) -> Result<CacheLease, CacheError> {
        self.commit_inner(Some(expected_size))
    }

    fn commit_inner(mut self, expected_size: Option<u64>) -> Result<CacheLease, CacheError> {
        let mut file = self.file.take().ok_or_else(|| {
            CacheError::Io(io::Error::new(
                io::ErrorKind::Other,
                "cache writer is closed",
            ))
        })?;
        file.flush()?;
        file.sync_all()?;
        let size = file.metadata()?.len();
        drop(file);
        if let Some(expected) = expected_size {
            if size != expected {
                return Err(CacheError::SizeMismatch {
                    expected,
                    actual: size,
                });
            }
        }
        if size > self.cache.inner.max_bytes {
            return Err(CacheError::ItemTooLarge);
        }

        let mut state = self.cache.lock_state()?;
        if state.leases.get(&self.key).copied().unwrap_or(0) > 0 {
            return Err(CacheError::InUse);
        }
        self.cache.ensure_capacity_for(&self.key, size, &state)?;
        let final_path = self.cache.final_path(&self.key);
        self.cache.ensure_confined(&self.temp_path)?;
        fs::rename(&self.temp_path, &final_path)?;
        self.committed = true;
        mark_accessed(&mut state, &self.key);
        *state.leases.entry(self.key.clone()).or_default() += 1;

        if let Err(error) = self.cache.enforce_limit(&mut state) {
            let _ = fs::remove_file(&final_path);
            state.last_access.remove(&self.key);
            if let Some(count) = state.leases.get_mut(&self.key) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    state.leases.remove(&self.key);
                }
            }
            return Err(error);
        }
        Ok(CacheLease {
            path: final_path,
            key: self.key.clone(),
            inner: Arc::clone(&self.cache.inner),
        })
    }
}

impl Write for CacheWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let remaining = self.max_bytes.saturating_sub(self.written_bytes);
        if remaining == 0 {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "cache entry exceeds its configured size limit",
            ));
        }
        let allowed = buffer.len().min(usize::try_from(remaining).unwrap_or(usize::MAX));
        let written = self.file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "cache writer is closed"))?
            .write(&buffer[..allowed])?;
        self.written_bytes = self.written_bytes.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "cache writer is closed"))?
            .flush()
    }
}

impl Drop for CacheWriter {
    fn drop(&mut self) {
        if !self.committed {
            self.file.take();
            let _ = fs::remove_file(&self.temp_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn cache(max_bytes: u64) -> (tempfile::TempDir, MediaCache) {
        let dir = tempfile::tempdir().expect("tempdir");
        let cache = MediaCache::new(CacheConfig {
            root: dir.path().join("private-cache"),
            max_bytes,
        })
        .expect("cache");
        (dir, cache)
    }

    #[test]
    fn stores_and_retrieves_using_opaque_deterministic_reference_name() {
        let (_dir, cache) = cache(128);
        let mut writer = cache
            .begin_write("telegram:file/reference")
            .expect("writer");
        writer.write_all(b"media bytes").expect("write");
        let lease = writer.commit().expect("commit");
        assert!(lease.path().starts_with(cache.inner.root.as_path()));
        assert_eq!(fs::read(lease.path()).expect("read"), b"media bytes");
        let second = cache
            .get("telegram:file/reference")
            .expect("get")
            .expect("hit");
        assert_eq!(lease.path(), second.path());
        assert_eq!(cache.used_bytes().expect("size"), 11);
    }

    #[test]
    fn rejects_incomplete_staging_write_without_publishing_a_cache_hit() {
        let (_dir, cache) = cache(128);
        let mut writer = cache.begin_write("truncated").expect("writer");
        writer.write_all(b"short").expect("write");

        assert!(matches!(
            writer.commit_with_expected_size(10),
            Err(CacheError::SizeMismatch {
                expected: 10,
                actual: 5
            })
        ));
        assert!(cache
            .get_with_expected_size("truncated", Some(10))
            .expect("get")
            .is_none());
        assert_eq!(cache.used_bytes().expect("size"), 0);
    }

    #[test]
    fn treats_existing_wrong_size_entry_as_cache_miss() {
        let (_dir, cache) = cache(128);
        let key = cache_key("old-truncated-entry");
        fs::write(cache.final_path(&key), b"short").expect("seed stale entry");

        assert!(cache
            .get_with_expected_size("old-truncated-entry", Some(10))
            .expect("get")
            .is_none());
        assert!(cache
            .get_with_expected_size("old-truncated-entry", Some(5))
            .expect("get matching size")
            .is_some());
    }

    #[test]
    fn missing_entry_lookup_prunes_stale_lru_metadata() {
        let (_dir, cache) = cache(128);
        let mut writer = cache.begin_write("externally-removed").expect("writer");
        writer.write_all(b"media").expect("write");
        let lease = writer.commit().expect("commit");
        let path = lease.path().to_path_buf();
        drop(lease);
        fs::remove_file(path).expect("external removal");

        assert!(cache
            .get("externally-removed")
            .expect("lookup")
            .is_none());
        assert!(cache
            .lock_state()
            .expect("cache state")
            .last_access
            .is_empty());
    }

    #[test]
    fn explicit_removal_is_confined_and_respects_leases() {
        let (_dir, cache) = cache(128);
        let mut writer = cache.begin_write("../unsafe-name").expect("writer");
        writer.write_all(b"safe").expect("write");
        let lease = writer.commit().expect("commit");
        assert!(lease.path().starts_with(cache.inner.root.as_path()));
        assert!(matches!(
            cache.remove("../unsafe-name"),
            Err(CacheError::InUse)
        ));
        drop(lease);
        assert!(cache.remove("../unsafe-name").expect("remove"));
        assert!(cache.get("../unsafe-name").expect("get").is_none());
        assert!(!cache.remove("../unsafe-name").expect("second remove"));
    }

    #[test]
    fn abandons_incomplete_writes_and_recovers_old_temps() {
        let (dir, cache) = cache(128);
        let writer = cache.begin_write("unfinished").expect("writer");
        let temp_path = writer.temp_path.clone();
        drop(writer);
        assert!(!temp_path.exists());

        let stale = cache.inner.root.join(".old.part-interrupted");
        fs::write(&stale, b"partial").expect("stale temp");
        drop(cache);
        let reopened = MediaCache::new(CacheConfig {
            root: dir.path().join("private-cache"),
            max_bytes: 128,
        })
        .expect("reopen");
        assert!(!stale.exists());
        assert_eq!(reopened.used_bytes().expect("size"), 0);
    }

    #[test]
    fn reopening_applies_the_current_limit_to_persisted_completed_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("private-cache");
        let cache = MediaCache::new(CacheConfig {
            root: root.clone(),
            max_bytes: 16,
        })
        .expect("initial cache");

        for (reference, bytes) in [("first", b"1234".as_slice()), ("second", b"5678")] {
            let mut writer = cache.begin_write(reference).expect("writer");
            writer.write_all(bytes).expect("write");
            drop(writer.commit().expect("commit"));
        }
        assert_eq!(cache.used_bytes().expect("initial size"), 8);
        drop(cache);

        let reopened = MediaCache::new(CacheConfig { root, max_bytes: 3 })
            .expect("reopen with smaller cache limit");

        assert!(reopened.used_bytes().expect("recovered size") <= 3);
        assert!(reopened.entries().expect("entries").len() <= 1);
    }

    #[test]
    fn evicts_oldest_unleased_entries_to_stay_within_limit() {
        let (_dir, cache) = cache(5);
        for (reference, bytes) in [("first", b"abc".as_slice()), ("second", b"def".as_slice())] {
            let mut writer = cache.begin_write(reference).expect("writer");
            writer.write_all(bytes).expect("write");
            drop(writer.commit().expect("commit"));
        }
        assert_eq!(cache.used_bytes().expect("size"), 3);
        assert!(cache.get("first").expect("get").is_none());
        assert!(cache.get("second").expect("get").is_some());
    }

    #[test]
    fn never_evicts_a_leased_file_and_rejects_over_limit_commit() {
        let (_dir, cache) = cache(5);
        let mut first = cache.begin_write("first").expect("writer");
        first.write_all(b"abc").expect("write");
        let lease = first.commit().expect("commit");
        let mut second = cache.begin_write("second").expect("writer");
        second.write_all(b"def").expect("write");
        assert!(matches!(second.commit(), Err(CacheError::CacheFull)));
        assert_eq!(fs::read(lease.path()).expect("leased read"), b"abc");
        assert_eq!(cache.used_bytes().expect("size"), 3);
    }

    #[test]
    fn staging_writer_never_writes_past_the_single_item_cache_limit() {
        let (_dir, cache) = cache(4);
        let mut writer = cache.begin_write("bounded-staging").expect("writer");
        writer.write_all(b"1234").expect("write to limit");
        let error = writer.write(b"5").expect_err("reject byte over limit");
        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
        assert_eq!(
            fs::metadata(&writer.temp_path)
                .expect("temp metadata")
                .len(),
            4
        );
        drop(writer);
        assert!(cache
            .get("bounded-staging")
            .expect("cache lookup")
            .is_none());
    }

    #[test]
    fn evicting_entries_prunes_their_lru_metadata() {
        let (_dir, cache) = cache(3);
        for index in 0..32 {
            let mut writer = cache
                .begin_write(&format!("media-{index}"))
                .expect("writer");
            writer.write_all(b"123").expect("write");
            drop(writer.commit().expect("commit"));

            let state = cache.lock_state().expect("cache state");
            assert!(state.last_access.len() <= 1);
        }
    }

    #[test]
    fn failed_commit_preserves_existing_entries_when_leases_block_capacity() {
        let (_dir, cache) = cache(5);
        let mut old = cache.begin_write("old").expect("old writer");
        old.write_all(b"ok").expect("write old");
        drop(old.commit().expect("commit old"));

        let mut leased_writer = cache.begin_write("leased").expect("leased writer");
        leased_writer.write_all(b"123").expect("write leased");
        let _lease = leased_writer.commit().expect("commit leased");

        let mut incoming = cache.begin_write("incoming").expect("incoming writer");
        incoming.write_all(b"456").expect("write incoming");
        assert!(matches!(incoming.commit(), Err(CacheError::CacheFull)));
        assert!(cache.get("old").expect("get old").is_some());
        assert_eq!(cache.used_bytes().expect("size"), 5);
    }

    #[test]
    fn leases_are_shared_between_handles_for_the_same_root() {
        let (_dir, first_cache) = cache(5);
        let second_cache = MediaCache::new(CacheConfig {
            root: first_cache.inner.root.clone(),
            max_bytes: 5,
        })
        .expect("second cache handle");

        let mut first = first_cache.begin_write("first").expect("writer");
        first.write_all(b"abc").expect("write");
        let lease = first.commit().expect("commit");

        let mut second = second_cache.begin_write("second").expect("writer");
        second.write_all(b"def").expect("write");
        assert!(matches!(second.commit(), Err(CacheError::CacheFull)));
        assert_eq!(fs::read(lease.path()).expect("leased read"), b"abc");
        assert_eq!(first_cache.used_bytes().expect("size"), 3);
    }

    #[cfg(unix)]
    #[test]
    fn ignores_symlink_entries() {
        use std::os::unix::fs::symlink;
        let (_dir, cache) = cache(32);
        let outside = cache.inner.root.parent().expect("parent").join("outside");
        fs::write(&outside, b"secret").expect("outside");
        symlink(&outside, cache.final_path(&cache_key("link"))).expect("symlink");
        assert!(cache.get("link").expect("get").is_none());
        assert_eq!(fs::read(outside).expect("outside unchanged"), b"secret");
    }
}
