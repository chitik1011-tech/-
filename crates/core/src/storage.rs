use std::{
    fs,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, RwLock},
};

use crate::{
    error::{CoreError, Result},
    state::CoreState,
};

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

            match content {
                Some(content) => serde_json::from_str(&content)
                    .map(Some)
                    .map_err(|error| CoreError::Serialization(error.to_string())),
                None => Ok(None),
            }
        })
    }

    fn save<'a>(&'a self, state: &'a CoreState) -> StoreFuture<'a, ()> {
        let path = self.path.clone();
        let encoded = serde_json::to_vec(state)
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
