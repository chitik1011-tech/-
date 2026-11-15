use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    backend::{BackendFuture, TelegramBackend},
    command::CoreCommand,
    error::{CoreError, Result},
    event::CoreEvent,
    model::{AuthorizationState, ChatSummary, MessageSummary},
};

pub struct MockTelegramBackend {
    authorization: AuthorizationState,
    chats: Vec<ChatSummary>,
    messages: BTreeMap<i64, Vec<MessageSummary>>,
    next_message_id: i64,
}

impl Default for MockTelegramBackend {
    fn default() -> Self {
        let initial_message = MessageSummary {
            id: 1,
            chat_id: 2,
            sender_id: 42,
            text: "Core online".into(),
            timestamp_unix: unix_timestamp(),
            outgoing: false,
        };
        let mut messages = BTreeMap::new();
        messages.insert(2, vec![initial_message.clone()]);

        Self {
            authorization: AuthorizationState::Closed,
            chats: vec![
                ChatSummary {
                    id: 1,
                    title: "Saved Messages".into(),
                    unread_count: 0,
                    last_message: None,
                },
                ChatSummary {
                    id: 2,
                    title: "Rust Telegram".into(),
                    unread_count: 1,
                    last_message: Some(initial_message),
                },
            ],
            messages,
            next_message_id: 2,
        }
    }
}

impl TelegramBackend for MockTelegramBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async move { self.handle_inner(command) })
    }
}

impl MockTelegramBackend {
    fn handle_inner(&mut self, command: CoreCommand) -> Result<Vec<CoreEvent>> {
        command.validate()?;

        match command {
            CoreCommand::Start => {
                self.authorization = AuthorizationState::WaitingForPhoneNumber;
                Ok(vec![CoreEvent::AuthorizationChanged(
                    self.authorization.clone(),
                )])
            }
            CoreCommand::SetPhoneNumber(_) => {
                self.authorization = AuthorizationState::WaitingForCode;
                Ok(vec![CoreEvent::AuthorizationChanged(
                    self.authorization.clone(),
                )])
            }
            CoreCommand::SubmitCode(_) | CoreCommand::SubmitPassword(_) => {
                self.authorization = AuthorizationState::Ready;
                Ok(vec![CoreEvent::AuthorizationChanged(
                    self.authorization.clone(),
                )])
            }
            CoreCommand::LoadChats { limit } => {
                let chats = self.chats.iter().take(limit).cloned().collect();
                Ok(vec![CoreEvent::ChatsLoaded(chats)])
            }
            CoreCommand::SendText { chat_id, text } => {
                self.require_authorized()?;
                let message = self.new_outgoing_message(chat_id, text)?;
                self.store_message(&message);

                Ok(vec![CoreEvent::MessageAdded(message)])
            }
            CoreCommand::ReplyMessage {
                chat_id,
                reply_to_message_id,
                text,
            } => {
                self.require_authorized()?;
                if !self.message_exists(chat_id, reply_to_message_id) {
                    return Err(CoreError::Backend("reply target was not found".into()));
                }

                let message = self.new_outgoing_message(chat_id, text)?;
                self.store_message(&message);
                Ok(vec![CoreEvent::MessageReplied {
                    message,
                    reply_to_message_id,
                }])
            }
            CoreCommand::EditMessage {
                chat_id,
                message_id,
                text,
            } => {
                self.require_authorized()?;
                let edited = {
                    let messages = self
                        .messages
                        .get_mut(&chat_id)
                        .ok_or_else(|| CoreError::Backend("message was not found".into()))?;
                    let message = messages
                        .iter_mut()
                        .find(|message| message.id == message_id)
                        .ok_or_else(|| CoreError::Backend("message was not found".into()))?;
                    message.text = text.trim().to_owned();
                    message.clone()
                };

                if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
                    if chat
                        .last_message
                        .as_ref()
                        .is_some_and(|last| last.id == message_id)
                    {
                        chat.last_message = Some(edited.clone());
                    }
                }

                Ok(vec![CoreEvent::MessageEdited(edited)])
            }
            CoreCommand::DeleteMessage {
                chat_id,
                message_id,
            } => {
                self.require_authorized()?;
                let next_last = {
                    let messages = self
                        .messages
                        .get_mut(&chat_id)
                        .ok_or_else(|| CoreError::Backend("message was not found".into()))?;
                    let original_len = messages.len();
                    messages.retain(|message| message.id != message_id);
                    if messages.len() == original_len {
                        return Err(CoreError::Backend("message was not found".into()));
                    }
                    messages.last().cloned()
                };

                if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
                    if chat
                        .last_message
                        .as_ref()
                        .is_some_and(|last| last.id == message_id)
                    {
                        chat.last_message = next_last;
                    }
                }
                if self.messages.get(&chat_id).is_some_and(Vec::is_empty) {
                    self.messages.remove(&chat_id);
                }

                Ok(vec![CoreEvent::MessageDeleted {
                    chat_id,
                    message_id,
                }])
            }
            CoreCommand::Shutdown => Ok(Vec::new()),
        }
    }

    fn require_authorized(&self) -> Result<()> {
        if self.authorization == AuthorizationState::Ready {
            Ok(())
        } else {
            Err(CoreError::Backend("not authorized".into()))
        }
    }

    fn message_exists(&self, chat_id: i64, message_id: i64) -> bool {
        self.messages
            .get(&chat_id)
            .is_some_and(|messages| messages.iter().any(|message| message.id == message_id))
    }

    fn new_outgoing_message(&mut self, chat_id: i64, text: String) -> Result<MessageSummary> {
        let next_message_id = self
            .next_message_id
            .checked_add(1)
            .ok_or_else(|| CoreError::Backend("mock message id space exhausted".into()))?;
        let message = MessageSummary {
            id: self.next_message_id,
            chat_id,
            sender_id: 0,
            text: text.trim().to_owned(),
            timestamp_unix: unix_timestamp(),
            outgoing: true,
        };
        self.next_message_id = next_message_id;
        Ok(message)
    }

    fn store_message(&mut self, message: &MessageSummary) {
        let messages = self.messages.entry(message.chat_id).or_default();
        messages.push(message.clone());
        messages.sort_by_key(|message| (message.timestamp_unix, message.id));

        if let Some(chat) = self
            .chats
            .iter_mut()
            .find(|chat| chat.id == message.chat_id)
        {
            chat.last_message = Some(message.clone());
        }
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
