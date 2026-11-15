use std::{future::Future, pin::Pin};

use crate::{
    command::CoreCommand,
    error::Result,
    event::CoreEvent,
};

pub type BackendFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<CoreEvent>>> + Send + 'a>>;

pub trait TelegramBackend: Send + 'static {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a>;
}
