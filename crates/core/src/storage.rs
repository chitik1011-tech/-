use std::{
    fs,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, RwLock},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{CoreError, Result},
    state::CoreState,
};

const STATE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
struct StoredState {
    schema_version: u32,
    state: CoreState,
}

pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub trait StateStore: Send + Sync + 'static {
    fn load<'a>(&'a self) -> StoreFuture<'a, Option<CoreState>>;
    fn save<'a>(&'a self, state: &'a CoreState) -> StoreFuture<'a, ()>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopStateStore;

impl StateStore for NoopStateStore {
    fn load<'a>(&'a self) -> StoreFuture<'a, Option<CoreState>> {
        Box::pin(async { Ok(None) })
    }

    fn save<'a>(&'a self, _state: &'a CoreState) -> StoreFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}
#[derive(Debug, Clone, Default)]
pub struct MemoryStateStore {
    state: Arc<RwLock<Option<CoreState>>>,
}

impl MemoryStateStore {
    pub fn snapshot(&self) -> Result<Option<CoreState>> {
        self.state
            .read()
            .map(|state| state.clone())
            .map_err(|_| CoreError::Storage("memory state lock poisoned".into()))
    }
}

impl StateStore for MemoryStateStore {
    fn load<'a>(&'a self) -> StoreFuture<'a, Option<CoreState>> {
        Box::pin(async move { self.snapshot() })
    }

    fn save<'a>(&'a self, state: &'a CoreState) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            let mut slot = self
                .state
                .write()
                .map_err(|_| CoreError::Storage("memory state lock poisoned".into()))?;
            *slot = Some(state.persistence_snapshot());
            Ok(())
        })
    }
}

#[derive(Debug, Clone)]
pub struct JsonFileStateStore {
    path: PathBuf,
}

impl JsonFileStateStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl StateStore for JsonFileStateStore {
    fn load<'a>(&'a self) -> StoreFuture<'a, Option<CoreState>> {
        let path = self.path.clone();

        Box::pin(async move {
            let content = tokio::task::spawn_blocking(move || {
                if !path.exists() {
                    return Ok(None);
                }

                fs::read_to_string(&path)
                    .map(Some)
                    .map_err(|error| CoreError::Storage(error.to_string()))
            })
            .await
            .map_err(|error| CoreError::Storage(format!("state load task failed: {error}")))??;

            content
                .map(|content| decode_stored_state(&content))
                .transpose()
        })
    }

    fn save<'a>(&'a self, state: &'a CoreState) -> StoreFuture<'a, ()> {
        let path = self.path.clone();
        let stored = StoredState {
            schema_version: STATE_SCHEMA_VERSION,
            state: state.persistence_snapshot(),
        };
        let encoded = serde_json::to_vec(&stored)
            .map_err(|error| CoreError::Serialization(error.to_string()));

        Box::pin(async move {
            let encoded = encoded?;

            tokio::task::spawn_blocking(move || {
                if let Some(parent) = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                {
                    fs::create_dir_all(parent)
                        .map_err(|error| CoreError::Storage(error.to_string()))?;
                }

                let temporary_path = path.with_extension("tmp");
                fs::write(&temporary_path, encoded)
                    .map_err(|error| CoreError::Storage(error.to_string()))?;
                fs::rename(&temporary_path, &path)
                    .map_err(|error| CoreError::Storage(error.to_string()))?;
                Ok(())
            })
            .await
            .map_err(|error| CoreError::Storage(format!("state save task failed: {error}")))?
        })
    }
}

fn decode_stored_state(content: &str) -> Result<CoreState> {
    match serde_json::from_str::<StoredState>(content) {
        Ok(stored) if (1..=STATE_SCHEMA_VERSION).contains(&stored.schema_version) => {
            let mut state = stored.state;
            state.contacts = Default::default();
            state
                .validate()
                .map_err(|error| CoreError::Storage(error.to_string()))?;
            Ok(state)
        }
        Ok(stored) => Err(CoreError::Storage(format!(
            "unsupported state schema version: {}",
            stored.schema_version
        ))),
        Err(versioned_error) => {
            let state = serde_json::from_str::<CoreState>(content).map_err(|legacy_error| {
                CoreError::Serialization(format!(
                    "invalid persisted state: versioned={versioned_error}; legacy={legacy_error}"
                ))
            })?;
            let mut state = state;
            state.contacts = Default::default();
            state
                .validate()
                .map_err(|error| CoreError::Storage(error.to_string()))?;
            Ok(state)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::decode_stored_state;

    #[test]
    fn versioned_state_without_folders_migrates_with_empty_folder_list() {
        let old_state = r#"{
            "running": true,
            "account": {"authorization":"Closed","chats":[]},
            "messages": {},
            "last_error": null
        }"#;
        let stored = format!(r#"{{"schema_version":1,"state":{old_state}}}"#);
        let state = decode_stored_state(&stored).unwrap();
        assert!(state.folders.is_empty());
        assert!(state.drafts.is_empty());
        assert!(state.contacts.is_empty());
    }

    #[test]
    fn unversioned_legacy_state_migrates_without_drafts() {
        let old = r#"{"running":true,"account":{"authorization":"Closed","chats":[]},"messages":{},"last_error":null}"#;
        let state = decode_stored_state(old).unwrap();
        assert!(state.drafts.is_empty());
    }

    #[test]
    fn persisted_folder_invariant_violations_are_rejected() {
        let invalid = r#"{
            "schema_version":1,
            "state":{
                "running":true,
                "account":{"authorization":"Closed","chats":[]},
                "messages":{},
                "last_error":null,
                "folders":[
                    {"id":1,"title":"One","position":1},
                    {"id":1,"title":"Duplicate","position":0}
                ]
            }
        }"#;
        assert!(decode_stored_state(invalid).is_err());
    }
}
