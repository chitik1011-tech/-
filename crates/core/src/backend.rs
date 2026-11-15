use std::{future::Future, pin::Pin};

use tokio::sync::mpsc;

use crate::{
    command::CoreCommand,
    error::{CoreError, Result},
    event::CoreEvent,
    forwarding::ForwardMessagesRequest,
};

pub const DEFAULT_BACKEND_UPDATE_CAPACITY: usize = 256;

pub type BackendFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<CoreEvent>>> + Send + 'a>>;
pub type BackendUpdateSender = mpsc::Sender<Result<CoreEvent>>;
pub type BackendUpdateReceiver = mpsc::Receiver<Result<CoreEvent>>;

pub fn backend_update_channel() -> (BackendUpdateSender, BackendUpdateReceiver) {
    mpsc::channel(DEFAULT_BACKEND_UPDATE_CAPACITY)
}

pub trait TelegramBackend: Send + 'static {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a>;

    /// Forwards the referenced Telegram messages to one destination in source order.
    ///
    /// Implementations must call the backend's native forward operation; copying message
    /// text does not preserve Telegram forwarding semantics. Backends without this
    /// capability return an error by default.
    fn forward_messages<'a>(&'a mut self, _request: ForwardMessagesRequest) -> BackendFuture<'a> {
        Box::pin(async {
            Err(CoreError::Backend(
                "message forwarding is unsupported by this backend".into(),
            ))
        })
    }

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        None
    }
}
