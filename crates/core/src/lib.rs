pub mod backend;
pub mod command;
pub mod error;
pub mod event;
pub mod model;
pub mod runtime;

pub use backend::TelegramBackend;
pub use command::CoreCommand;
pub use error::{CoreError, Result};
pub use event::CoreEvent;
pub use model::{AccountState, AuthorizationState, ChatSummary, MessageSummary};
pub use runtime::{spawn_core, CoreHandle};
