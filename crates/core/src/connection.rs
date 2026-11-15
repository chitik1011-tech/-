use serde::{Deserialize, Serialize};

/// Backend-neutral transport state exposed by the Rust core.
///
/// This state is intentionally separate from authorization: an authorized
/// account can still be temporarily disconnected from Telegram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Connecting,
    Ready,
    #[default]
    Disconnected,
}
