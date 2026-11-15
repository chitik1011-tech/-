use serde::{Deserialize, Serialize};

/// Backend-neutral connection status exposed to the Android shell.
///
/// Backends should emit state changes through `CoreEvent::ConnectionChanged`;
/// Kotlin should render these events and must not poll the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Connecting,
    Connected,
    Reconnecting,
    #[default]
    Offline,
    Failed,
}

impl ConnectionState {
    /// Returns whether a distinct next state is valid from this state.
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Offline, Self::Connecting | Self::Reconnecting | Self::Failed)
                | (
                    Self::Connecting,
                    Self::Connected | Self::Reconnecting | Self::Offline | Self::Failed
                )
                | (
                    Self::Connected,
                    Self::Reconnecting | Self::Offline | Self::Failed
                )
                | (
                    Self::Reconnecting,
                    Self::Connecting | Self::Connected | Self::Offline | Self::Failed
                )
                | (
                    Self::Failed,
                    Self::Connecting | Self::Reconnecting | Self::Offline
                )
        )
    }
}

/// Small transition guard for backend/update-stream adapters.
///
/// Duplicate and impossible transitions are ignored. A valid transition returns
/// the new state so callers can emit one `CoreEvent::ConnectionChanged`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ConnectionStateMachine {
    current: ConnectionState,
}

impl ConnectionStateMachine {
    pub const fn new(initial: ConnectionState) -> Self {
        Self { current: initial }
    }

    pub const fn current(&self) -> ConnectionState {
        self.current
    }

    pub fn transition(&mut self, next: ConnectionState) -> Option<ConnectionState> {
        if !self.current.can_transition_to(next) {
            return None;
        }

        self.current = next;
        Some(next)
    }
}
