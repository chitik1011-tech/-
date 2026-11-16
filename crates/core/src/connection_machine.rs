//! Generation-safe connection lifecycle.
//!
//! Every connection attempt is identified by a [`ConnectionToken`] that only
//! [`ConnectionMachine`] can create. The token holds a session generation,
//! bumped for every new login session, and a connection epoch, bumped whenever
//! an attempt starts or ends. Asynchronous callbacks must present the token
//! they were started with. A token from another generation or epoch is
//! reported as [`Outcome::Stale`] and never changes the machine.

use crate::connection::ConnectionState;
use serde::{Deserialize, Serialize};

/// Identifies one login session. Bumped by [`ConnectionMachine::start_session`].
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct SessionGeneration(u64);

/// Identifies one connection attempt inside the current session.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct ConnectionEpoch(u64);

/// Token that every asynchronous callback for a connection attempt must present.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct ConnectionToken {
    generation: SessionGeneration,
    epoch: ConnectionEpoch,
}

impl ConnectionToken {
    pub fn generation(&self) -> SessionGeneration {
        self.generation
    }

    pub fn epoch(&self) -> ConnectionEpoch {
        self.epoch
    }

    /// Returns the token for the next attempt epoch within the same session.
    pub fn next_epoch(self) -> Self {
        Self {
            generation: self.generation,
            epoch: self.epoch.next(),
        }
    }
}

/// Lifecycle phase of the current connection attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkPhase {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Synchronizing,
    Ready,
    Reconnecting,
    Failed,
}

/// Input to the state machine. Every input is applied together with a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LinkEvent {
    /// Transport handshake finished for the attempt.
    TransportConnected,
    /// Post-connect synchronization started for the attempt.
    SyncStarted,
    /// Post-connect synchronization finished for the attempt.
    SyncCompleted,
    /// Transport dropped: network loss, Wi-Fi/mobile switch, Telegram reset.
    TransportLost,
    /// The attempt failed. `retryable: false` means a fatal error such as revoked auth.
    ConnectFailed { retryable: bool },
    /// Backoff timer or user requested another attempt.
    Retry,
    /// The account logged out. The session is dropped.
    Logout,
    /// The process is shutting down.
    Shutdown,
}

/// Result of an accepted input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The phase changed from `from` to `to`.
    Changed { from: LinkPhase, to: LinkPhase },
    /// The input repeats what the machine already did. Nothing changed.
    Duplicate,
    /// The token belongs to another generation or epoch. Nothing changed.
    Stale,
}

/// Errors for inputs that are impossible in the current phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    InvalidTransition { from: LinkPhase, event: LinkEvent },
}

/// Connection lifecycle state machine. Owned by the Rust core and driven
/// only through [`ConnectionMachine::start_session`] and [`ConnectionMachine::apply`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ConnectionMachine {
    phase: LinkPhase,
    token: ConnectionToken,
}

enum Step {
    Move { to: LinkPhase, new_attempt: bool },
    Duplicate,
    Invalid,
}

fn go(to: LinkPhase, new_attempt: bool) -> Step {
    Step::Move { to, new_attempt }
}

fn step(phase: LinkPhase, event: LinkEvent) -> Step {
    use LinkEvent as E;
    use LinkPhase as P;

    match (phase, event) {
        (P::Connecting, E::TransportConnected) => go(P::Connected, false),
        (P::Connected | P::Synchronizing | P::Ready, E::TransportConnected) => Step::Duplicate,
        (P::Connected, E::SyncStarted) => go(P::Synchronizing, false),
        (P::Synchronizing, E::SyncCompleted) => go(P::Ready, false),
        (P::Ready, E::SyncCompleted) => Step::Duplicate,
        (P::Connecting | P::Connected | P::Synchronizing | P::Ready, E::TransportLost) => {
            go(P::Reconnecting, true)
        }
        (P::Reconnecting, E::TransportLost) => Step::Duplicate,
        (P::Connecting, E::ConnectFailed { retryable: true }) => go(P::Reconnecting, true),
        (P::Connecting, E::ConnectFailed { retryable: false }) => go(P::Failed, true),
        (P::Reconnecting, E::ConnectFailed { retryable: true }) => Step::Duplicate,
        (P::Reconnecting, E::ConnectFailed { retryable: false }) => go(P::Failed, true),
        (P::Synchronizing, E::ConnectFailed { retryable: false }) => go(P::Failed, true),
        // Loss/failure already advanced the epoch before backoff. Retry starts
        // that new attempt under the token it was given, so old callbacks stay stale.
        (P::Reconnecting | P::Failed, E::Retry) => go(P::Connecting, false),
        (P::Connecting | P::Connected | P::Synchronizing | P::Ready, E::Retry) => Step::Duplicate,
        (P::Disconnected, E::Logout | E::Shutdown) => Step::Duplicate,
        (_, E::Logout | E::Shutdown) => go(P::Disconnected, true),
        _ => Step::Invalid,
    }
}

impl SessionGeneration {
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl ConnectionEpoch {
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl ConnectionMachine {
    pub fn phase(&self) -> LinkPhase {
        self.phase
    }

    /// Token of the current attempt. Pass it to every async operation started
    /// for this attempt.
    pub fn token(&self) -> ConnectionToken {
        self.token
    }

    /// Invalidates outstanding tokens when persisted state is restored.
    pub fn reset_disconnected(&mut self) {
        self.phase = LinkPhase::Disconnected;
        self.token = ConnectionToken {
            generation: self.token.generation.next(),
            epoch: self.token.epoch.next(),
        };
    }

    /// Transport state in the shape already exposed to the UI.
    pub fn transport_state(&self) -> ConnectionState {
        match self.phase {
            LinkPhase::Connecting | LinkPhase::Connected | LinkPhase::Synchronizing => {
                ConnectionState::Connecting
            }
            LinkPhase::Ready => ConnectionState::Ready,
            LinkPhase::Disconnected | LinkPhase::Reconnecting | LinkPhase::Failed => {
                ConnectionState::Disconnected
            }
        }
    }

    /// Starts a new login session with a new generation.
    ///
    /// Idempotent: while a session is already active the call returns
    /// [`Outcome::Duplicate`] and keeps the current generation.
    pub fn start_session(&mut self) -> Outcome {
        if self.is_active() {
            return Outcome::Duplicate;
        }

        let from = self.phase;
        self.token = ConnectionToken {
            generation: self.token.generation.next(),
            epoch: self.token.epoch.next(),
        };
        self.phase = LinkPhase::Connecting;
        Outcome::Changed {
            from,
            to: LinkPhase::Connecting,
        }
    }

    /// Applies one input that was produced for `token`.
    ///
    /// A token from another generation or epoch yields `Ok(Outcome::Stale)` and
    /// leaves the machine untouched. An impossible input for the current phase
    /// yields `Err(LinkError::InvalidTransition)` and leaves the machine untouched.
    pub fn apply(
        &mut self,
        token: ConnectionToken,
        event: LinkEvent,
    ) -> Result<Outcome, LinkError> {
        if token != self.token {
            return Ok(Outcome::Stale);
        }

        match step(self.phase, event) {
            Step::Move { to, new_attempt } => {
                let from = self.phase;
                self.phase = to;
                if new_attempt {
                    self.token = self.token.next_epoch();
                }
                Ok(Outcome::Changed { from, to })
            }
            Step::Duplicate => Ok(Outcome::Duplicate),
            Step::Invalid => Err(LinkError::InvalidTransition {
                from: self.phase,
                event,
            }),
        }
    }

    fn is_active(&self) -> bool {
        matches!(
            self.phase,
            LinkPhase::Connecting
                | LinkPhase::Connected
                | LinkPhase::Synchronizing
                | LinkPhase::Ready
                | LinkPhase::Reconnecting
        )
    }
}
