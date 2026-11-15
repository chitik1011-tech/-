use std::{future::Future, pin::Pin};

use tokio::sync::mpsc;

use crate::{
    command::CoreCommand, connection_machine::ConnectionToken, error::Result, event::CoreEvent,
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

    /// Supplies the current generation token to asynchronous backend workers.
    /// Backends that do not emit background updates may leave the default hook.
    fn set_connection_token(&mut self, _token: ConnectionToken) {}

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        None
    }
}
