use std::{future::Future, pin::Pin};

use tokio::sync::mpsc;

use crate::{command::CoreCommand, error::Result, event::CoreEvent};

pub const DEFAULT_BACKEND_UPDATE_CAPACITY: usize = 256;

pub type BackendFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<CoreEvent>>> + Send + 'a>>;
pub type BackendUpdateSender = mpsc::Sender<Result<CoreEvent>>;
pub type BackendUpdateReceiver = mpsc::Receiver<Result<CoreEvent>>;

pub fn backend_update_channel() -> (BackendUpdateSender, BackendUpdateReceiver) {
    mpsc::channel(DEFAULT_BACKEND_UPDATE_CAPACITY)
}

pub trait TelegramBackend: Send + 'static {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a>;

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        None
    }
}
