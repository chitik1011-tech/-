use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{error::{CoreError, Result}, event::CoreEvent, model::MessageRef};

pub const MAX_FORWARD_MESSAGES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardMessagesRequest {
    /// Source message references in the order Telegram should forward them.
    pub sources: Vec<MessageRef>,
    /// One destination chat for the complete ordered batch.
    pub destination_chat_id: i64,
    /// Stable caller supplied id used to correlate and deduplicate retries.
    pub request_id: String,
}

impl ForwardMessagesRequest {
    pub fn validate(&self) -> Result<()> {
        if self.sources.is_empty() || self.sources.len() > MAX_FORWARD_MESSAGES {
            return Err(CoreError::InvalidInput(format!("forward source count must be in 1..={MAX_FORWARD_MESSAGES}")));
        }
        if self.destination_chat_id == 0 {
            return Err(CoreError::InvalidInput("destination chat id must be non-zero".into()));
        }
        if self.request_id.trim().is_empty() {
            return Err(CoreError::InvalidInput("forward request id is empty".into()));
        }
        let mut seen = HashSet::new();
        for source in &self.sources {
            if source.chat_id == 0 || source.message_id <= 0 {
                return Err(CoreError::InvalidInput("source chat id must be non-zero and message id positive".into()));
            }
            if !seen.insert((source.chat_id, source.message_id)) {
                return Err(CoreError::InvalidInput("forward source references must be unique".into()));
            }
        }
        Ok(())
    }
}

/// Per-source outcome. Successful IDs identify newly forwarded Telegram messages;
/// message text is absent because forwarding must preserve source attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ForwardOutcome {
    Forwarded { source: MessageRef, destination_message_id: i64 },
    Failed { source: MessageRef, error: String },
}

impl ForwardOutcome {
    pub fn source(&self) -> &MessageRef {
        match self { Self::Forwarded { source, .. } | Self::Failed { source, .. } => source }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardMessagesResult {
    pub request_id: String,
    pub destination_chat_id: i64,
    /// One outcome per requested source, in request order; partial failures are explicit.
    pub outcomes: Vec<ForwardOutcome>,
}

/// Enforces request correlation, source membership, order, and one outcome per source.
pub fn normalize_forward_events(request: &ForwardMessagesRequest, events: Vec<CoreEvent>) -> Vec<CoreEvent> {
    let mut others = Vec::new();
    let mut first: HashMap<MessageRef, ForwardOutcome> = HashMap::new();
    for event in events {
        if let CoreEvent::MessagesForwarded(result) = event {
            if result.request_id == request.request_id && result.destination_chat_id == request.destination_chat_id {
                for outcome in result.outcomes {
                    let source = outcome.source();
                    if request.sources.contains(source) && !first.contains_key(source) {
                        let normalized = match outcome {
                            ForwardOutcome::Forwarded { source, destination_message_id } if destination_message_id > 0 =>
                                ForwardOutcome::Forwarded { source, destination_message_id },
                            ForwardOutcome::Forwarded { source, .. } =>
                                ForwardOutcome::Failed { source, error: "backend returned an invalid destination message id".into() },
                            failed => failed,
                        };
                        first.insert(source.clone(), normalized);
                    }
                }
            }
        } else {
            others.push(event);
        }
    }
    let outcomes = request.sources.iter().map(|source| {
        first.remove(source).unwrap_or_else(|| ForwardOutcome::Failed {
            source: source.clone(),
            error: "backend did not report an outcome".into(),
        })
    }).collect();
    others.push(CoreEvent::MessagesForwarded(ForwardMessagesResult {
        request_id: request.request_id.clone(),
        destination_chat_id: request.destination_chat_id,
        outcomes,
    }));
    others
}
