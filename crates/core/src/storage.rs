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

const STATE_SCHEMA_VERSION: u32 = 1;

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
            *slot = Some(state.clone());
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
            state: state.clone(),
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
        Ok(stored) => {
            if stored.schema_version != STATE_SCHEMA_VERSION {
                return Err(CoreError::Storage(format!(
                    "unsupported state schema version: {}",
                    stored.schema_version
                )));
            }
            Ok(stored.state)
        }
        Err(versioned_error) => {
            serde_json::from_str::<CoreState>(content).map_err(|legacy_error| {
                CoreError::Serialization(format!(
                    "invalid persisted state: versioned={versioned_error}; legacy={legacy_error}"
                ))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_state_version_one_defaults_new_message_metadata() {
        let legacy = r#"{
            "schema_version": 1,
            "state": {
                "running": true,
                "account": {
                    "authorization": "Ready",
                    "chats": []
                },
                "messages": {
                    "3": [{
                        "id": 7,
                        "chat_id": 3,
                        "sender_id": 9,
                        "text": "old message",
                        "timestamp_unix": 100,
                        "outgoing": false
                    }]
                },
                "last_error": null
            }
        }"#;

        let state = decode_stored_state(legacy).expect("version-one state should deserialize");
        let message = &state.messages_for_chat(3)[0];
        assert_eq!(message.forwarded_from, None);
        assert!(!message.edited);
        assert_eq!(message.edit_timestamp_unix, None);
        assert_eq!(message.reply_to, None);
    }
}
