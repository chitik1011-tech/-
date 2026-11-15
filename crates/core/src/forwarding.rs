use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{
    error::{CoreError, Result},
    event::CoreEvent,
};

pub const MAX_FORWARD_MESSAGES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardMessagesRequest {
    /// Correlation value for this user operation. It does not make an ambiguous
    /// transport retry safe; Telegram does not expose a caller-provided idempotency key.
    pub request_id: String,
    pub source_chat_id: i64,
    /// Message IDs in the order Telegram should forward them.
    pub message_ids: Vec<i64>,
    pub destination_chat_id: i64,
}

impl ForwardMessagesRequest {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.trim().is_empty() {
            return Err(CoreError::InvalidInput(
                "forward request id is empty".into(),
            ));
        }
        if self.source_chat_id == 0 || self.destination_chat_id == 0 {
            return Err(CoreError::InvalidInput(
                "forward chat id must be non-zero".into(),
            ));
        }
        if self.message_ids.is_empty() || self.message_ids.len() > MAX_FORWARD_MESSAGES {
            return Err(CoreError::InvalidInput(format!(
                "forward message count must be in 1..={MAX_FORWARD_MESSAGES}"
            )));
        }
        let mut seen = HashSet::new();
        if self
            .message_ids
            .iter()
            .any(|id| *id <= 0 || *id > i32::MAX as i64 || !seen.insert(*id))
        {
            return Err(CoreError::InvalidInput(
                "forward message ids must be positive and unique".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ForwardOutcome {
    Forwarded {
        source_message_id: i64,
        destination_message_id: i64,
    },
    Failed {
        source_message_id: i64,
        error: String,
    },
}

impl ForwardOutcome {
    pub fn source_message_id(&self) -> i64 {
        match self {
            Self::Forwarded {
                source_message_id, ..
            }
            | Self::Failed {
                source_message_id, ..
            } => *source_message_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardMessagesResult {
    pub request_id: String,
    pub source_chat_id: i64,
    pub destination_chat_id: i64,
    /// One outcome per input ID, in input order.
    pub outcomes: Vec<ForwardOutcome>,
}

/// Enforces correlation, source membership, unique outcomes, and request ordering.
pub fn normalize_forward_events(
    request: &ForwardMessagesRequest,
    events: Vec<CoreEvent>,
) -> Vec<CoreEvent> {
    let mut other_events = Vec::new();
    let mut outcomes = HashMap::new();
    for event in events {
        if let CoreEvent::MessagesForwarded(result) = event {
            if result.request_id == request.request_id
                && result.source_chat_id == request.source_chat_id
                && result.destination_chat_id == request.destination_chat_id
            {
                for outcome in result.outcomes {
                    let source_id = outcome.source_message_id();
                    if !request.message_ids.contains(&source_id)
                        || outcomes.contains_key(&source_id)
                    {
                        continue;
                    }
                    let normalized = match outcome {
                        ForwardOutcome::Forwarded {
                            source_message_id,
                            destination_message_id,
                        } if destination_message_id > 0 => ForwardOutcome::Forwarded {
                            source_message_id,
                            destination_message_id,
                        },
                        ForwardOutcome::Forwarded {
                            source_message_id, ..
                        } => ForwardOutcome::Failed {
                            source_message_id,
                            error: "backend returned an invalid forwarded message id".into(),
                        },
                        failure @ ForwardOutcome::Failed { .. } => failure,
                    };
                    outcomes.insert(source_id, normalized);
                }
            }
        } else {
            other_events.push(event);
        }
    }
    let ordered = request
        .message_ids
        .iter()
        .map(|id| {
            outcomes
                .remove(id)
                .unwrap_or_else(|| ForwardOutcome::Failed {
                    source_message_id: *id,
                    error: "backend did not report a forwarding outcome".into(),
                })
        })
        .collect();
    other_events.push(CoreEvent::MessagesForwarded(ForwardMessagesResult {
        request_id: request.request_id.clone(),
        source_chat_id: request.source_chat_id,
        destination_chat_id: request.destination_chat_id,
        outcomes: ordered,
    }));
    other_events
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> ForwardMessagesRequest {
        ForwardMessagesRequest {
            request_id: "r1".into(),
            source_chat_id: 10,
            message_ids: vec![8, 2, 5],
            destination_chat_id: 20,
        }
    }
    #[test]
    fn validates_bounds_uniqueness_and_ids() {
        assert!(request().validate().is_ok());
        let mut invalid = request();
        invalid.message_ids = vec![1, 1];
        assert!(invalid.validate().is_err());
        let mut invalid = request();
        invalid.message_ids = vec![0];
        assert!(invalid.validate().is_err());
        let mut invalid = request();
        invalid.message_ids = vec![1; MAX_FORWARD_MESSAGES + 1];
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn normalizes_partial_duplicate_and_out_of_order_results() {
        let req = request();
        let events = normalize_forward_events(
            &req,
            vec![CoreEvent::MessagesForwarded(ForwardMessagesResult {
                request_id: "r1".into(),
                source_chat_id: 10,
                destination_chat_id: 20,
                outcomes: vec![
                    ForwardOutcome::Forwarded {
                        source_message_id: 5,
                        destination_message_id: 55,
                    },
                    ForwardOutcome::Forwarded {
                        source_message_id: 5,
                        destination_message_id: 56,
                    },
                    ForwardOutcome::Forwarded {
                        source_message_id: 8,
                        destination_message_id: 58,
                    },
                ],
            })],
        );
        let CoreEvent::MessagesForwarded(result) = events.last().unwrap() else {
            panic!("missing result")
        };
        assert_eq!(
            result.outcomes,
            vec![
                ForwardOutcome::Forwarded {
                    source_message_id: 8,
                    destination_message_id: 58
                },
                ForwardOutcome::Failed {
                    source_message_id: 2,
                    error: "backend did not report a forwarding outcome".into()
                },
                ForwardOutcome::Forwarded {
                    source_message_id: 5,
                    destination_message_id: 55
                },
            ]
        );
    }

    #[test]
    fn canonical_command_and_event_json_round_trip() {
        let request = request();
        let command = crate::CoreCommand::ForwardMessages(request.clone());
        let json = serde_json::to_string(&command).unwrap();
        assert_eq!(crate::decode_command(&json).unwrap(), command);

        let event = CoreEvent::MessagesForwarded(ForwardMessagesResult {
            request_id: request.request_id,
            source_chat_id: request.source_chat_id,
            destination_chat_id: request.destination_chat_id,
            outcomes: vec![ForwardOutcome::Forwarded {
                source_message_id: 8,
                destination_message_id: 80,
            }],
        });
        let json = crate::wire::encode_event(&event).unwrap();
        assert_eq!(serde_json::from_str::<CoreEvent>(&json).unwrap(), event);
    }
}
