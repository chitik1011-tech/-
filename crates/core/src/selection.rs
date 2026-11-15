//! Transient selection state for message bulk actions.
//!
//! A selection is intentionally separate from [`crate::state::CoreState`]: it is
//! an interaction detail and must never be written to the persisted core state.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::model::MessageSummary;

/// A user action that changes the current message selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionAction {
    Select {
        chat_id: i64,
        message_id: i64,
    },
    Unselect {
        chat_id: i64,
        message_id: i64,
    },
    SelectRange {
        chat_id: i64,
        anchor_id: i64,
        target_id: i64,
    },
    Clear,
}

/// Actions available for the current selection, derived by Rust from messages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SelectionCapabilities {
    pub can_delete: bool,
    pub can_forward: bool,
    pub can_copy: bool,
    pub can_reply: bool,
    pub can_edit: bool,
}

/// A rejected selection action. Failed actions leave the selection unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SelectionError {
    #[error("message {message_id} was not found in chat {chat_id}")]
    MessageNotFound { chat_id: i64, message_id: i64 },
    #[error("cannot mix chat {requested_chat_id} with selected chat {selected_chat_id}")]
    MixedChats {
        selected_chat_id: i64,
        requested_chat_id: i64,
    },
    #[error("range endpoints were not found in chat {chat_id}")]
    RangeEndpointNotFound { chat_id: i64 },
    #[error("range crosses chat boundaries")]
    MixedChatRange,
}

/// Ephemeral message selection. It deliberately does not implement serialization.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageSelection {
    chat_id: Option<i64>,
    selected: BTreeMap<i64, MessageSummary>,
}

impl MessageSelection {
    /// Applies one selection action against the currently loaded message rows.
    ///
    /// A selection can only contain messages from one chat. `SelectRange`
    /// includes both endpoints and rejects a range that crosses chat rows.
    pub fn reduce(
        &mut self,
        action: SelectionAction,
        messages: &[MessageSummary],
    ) -> Result<(), SelectionError> {
        match action {
            SelectionAction::Clear => {
                self.clear();
                Ok(())
            }
            SelectionAction::Select {
                chat_id,
                message_id,
            } => {
                self.ensure_chat(chat_id)?;
                let message = find_message(messages, chat_id, message_id)?;
                self.chat_id = Some(chat_id);
                self.selected.insert(message_id, message.clone());
                Ok(())
            }
            SelectionAction::Unselect {
                chat_id,
                message_id,
            } => {
                self.ensure_chat(chat_id)?;
                self.selected.remove(&message_id);
                self.reset_empty_chat();
                Ok(())
            }
            SelectionAction::SelectRange {
                chat_id,
                anchor_id,
                target_id,
            } => {
                self.ensure_chat(chat_id)?;
                let start = messages
                    .iter()
                    .position(|message| message.chat_id == chat_id && message.id == anchor_id)
                    .ok_or(SelectionError::RangeEndpointNotFound { chat_id })?;
                let end = messages
                    .iter()
                    .position(|message| message.chat_id == chat_id && message.id == target_id)
                    .ok_or(SelectionError::RangeEndpointNotFound { chat_id })?;
                let (first, last) = if start <= end {
                    (start, end)
                } else {
                    (end, start)
                };
                let range = &messages[first..=last];
                if range.iter().any(|message| message.chat_id != chat_id) {
                    return Err(SelectionError::MixedChatRange);
                }

                self.chat_id = Some(chat_id);
                for message in range {
                    self.selected.insert(message.id, message.clone());
                }
                Ok(())
            }
        }
    }

    /// Returns the chat associated with this selection, if any.
    pub const fn chat_id(&self) -> Option<i64> {
        self.chat_id
    }

    /// Returns selected message ids in ascending order.
    pub fn selected_ids(&self) -> Vec<i64> {
        self.selected.keys().copied().collect()
    }

    /// Returns the number of selected messages.
    pub fn len(&self) -> usize {
        self.selected.len()
    }

    /// Returns whether no messages are selected.
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// Clears all transient selection state.
    pub fn clear(&mut self) {
        self.selected.clear();
        self.chat_id = None;
    }

    /// Computes message action capabilities from the selected message data.
    ///
    /// The current message model has no server-provided moderation flags, so
    /// deletion is conservatively limited to outgoing messages. Forwarding is
    /// available for any non-empty selection; copying requires text on every
    /// selected message; reply requires one message; and edit requires one
    /// outgoing text message.
    pub fn capabilities(&self) -> SelectionCapabilities {
        let selected = self.selected.values().collect::<Vec<_>>();
        let non_empty = !selected.is_empty();
        let single = selected.len() == 1;
        let sole = selected.first();

        SelectionCapabilities {
            can_delete: non_empty && selected.iter().all(|message| message.outgoing),
            can_forward: non_empty,
            can_copy: non_empty && selected.iter().all(|message| !message.text.is_empty()),
            can_reply: single,
            can_edit: sole.is_some_and(|message| message.outgoing && !message.text.is_empty()),
        }
    }

    fn ensure_chat(&self, requested_chat_id: i64) -> Result<(), SelectionError> {
        if let Some(selected_chat_id) = self.chat_id {
            if selected_chat_id != requested_chat_id {
                return Err(SelectionError::MixedChats {
                    selected_chat_id,
                    requested_chat_id,
                });
            }
        }
        Ok(())
    }

    fn reset_empty_chat(&mut self) {
        if self.selected.is_empty() {
            self.chat_id = None;
        }
    }
}

fn find_message(
    messages: &[MessageSummary],
    chat_id: i64,
    message_id: i64,
) -> Result<&MessageSummary, SelectionError> {
    messages
        .iter()
        .find(|message| message.chat_id == chat_id && message.id == message_id)
        .ok_or(SelectionError::MessageNotFound {
            chat_id,
            message_id,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(chat_id: i64, id: i64, outgoing: bool, text: &str) -> MessageSummary {
        MessageSummary {
            id,
            chat_id,
            sender_id: if outgoing { 1 } else { 2 },
            text: text.to_owned(),
            timestamp_unix: id,
            outgoing,
        }
    }

    #[test]
    fn rejects_cross_chat_selection_without_changing_current_selection() {
        let messages = [
            message(10, 1, true, "hello"),
            message(20, 1, false, "world"),
        ];
        let mut selection = MessageSelection::default();
        selection
            .reduce(
                SelectionAction::Select {
                    chat_id: 10,
                    message_id: 1,
                },
                &messages,
            )
            .unwrap();

        let error = selection
            .reduce(
                SelectionAction::Select {
                    chat_id: 20,
                    message_id: 1,
                },
                &messages,
            )
            .unwrap_err();

        assert_eq!(
            error,
            SelectionError::MixedChats {
                selected_chat_id: 10,
                requested_chat_id: 20,
            }
        );
        assert_eq!(selection.chat_id(), Some(10));
        assert_eq!(selection.selected_ids(), vec![1]);
    }

    #[test]
    fn range_selects_inclusive_rows_and_clear_resets_selection() {
        let messages = [
            message(10, 1, true, "one"),
            message(10, 2, true, "two"),
            message(10, 3, true, "three"),
        ];
        let mut selection = MessageSelection::default();

        selection
            .reduce(
                SelectionAction::SelectRange {
                    chat_id: 10,
                    anchor_id: 3,
                    target_id: 1,
                },
                &messages,
            )
            .unwrap();
        assert_eq!(selection.selected_ids(), vec![1, 2, 3]);

        selection.reduce(SelectionAction::Clear, &messages).unwrap();
        assert!(selection.is_empty());
        assert_eq!(selection.chat_id(), None);
    }

    #[test]
    fn derives_capabilities_from_selected_message_properties() {
        let messages = [
            message(10, 1, true, "one"),
            message(10, 2, true, "two"),
            message(10, 3, false, "incoming"),
            message(10, 4, false, ""),
        ];
        let mut selection = MessageSelection::default();

        assert_eq!(selection.capabilities(), SelectionCapabilities::default());

        selection
            .reduce(
                SelectionAction::Select {
                    chat_id: 10,
                    message_id: 1,
                },
                &messages,
            )
            .unwrap();
        assert_eq!(
            selection.capabilities(),
            SelectionCapabilities {
                can_delete: true,
                can_forward: true,
                can_copy: true,
                can_reply: true,
                can_edit: true,
            }
        );

        selection
            .reduce(
                SelectionAction::Select {
                    chat_id: 10,
                    message_id: 3,
                },
                &messages,
            )
            .unwrap();
        assert_eq!(
            selection.capabilities(),
            SelectionCapabilities {
                can_delete: false,
                can_forward: true,
                can_copy: true,
                can_reply: false,
                can_edit: false,
            }
        );
    }

    #[test]
    fn rejects_ranges_that_cross_chat_rows_atomically() {
        let messages = [
            message(10, 1, true, "one"),
            message(20, 1, true, "other chat"),
            message(10, 2, true, "two"),
        ];
        let mut selection = MessageSelection::default();

        assert_eq!(
            selection.reduce(
                SelectionAction::SelectRange {
                    chat_id: 10,
                    anchor_id: 1,
                    target_id: 2,
                },
                &messages,
            ),
            Err(SelectionError::MixedChatRange)
        );
        assert!(selection.is_empty());
    }
}
