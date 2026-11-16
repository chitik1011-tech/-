//! Telegram protocol backend for the Rust-first mobile client.
//!
//! The implementation uses `grammers` (MTProto in Rust) and intentionally keeps
//! credentials and session paths outside the crate.

mod auth;
mod backend;
mod chats;
mod config;
mod contacts;
mod downloads;
mod error;
mod ids;
mod messages;
mod polls;
mod profiles;
mod updates;

pub use backend::GrammersBackend;
pub use config::TelegramConfig;
pub use error::TelegramBackendError;
pub use updates::{ConnectionState, TelegramUpdate};
