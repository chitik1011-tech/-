use std::time::{SystemTime, UNIX_EPOCH};

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
    next_message_id: i64,
}

impl Default for MockTelegramBackend {
    fn default() -> Self {
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
                    last_message: Some(MessageSummary {
                        id: 1,
                        chat_id: 2,
                        sender_id: 42,
                        text: "Core online".into(),
                        timestamp_unix: unix_timestamp(),
                        outgoing: false,
                    }),
                },
            ],
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
        match command {
            CoreCommand::Start => {
                self.authorization = AuthorizationState::WaitingForPhoneNumber;
                Ok(vec![CoreEvent::AuthorizationChanged(
                    self.authorization.clone(),
                )])
            }
            CoreCommand::SetPhoneNumber(phone) => {
                if phone.trim().is_empty() {
                    return Err(CoreError::InvalidInput("phone number is empty".into()));
                }

                self.authorization = AuthorizationState::WaitingForCode;
                Ok(vec![CoreEvent::AuthorizationChanged(
                    self.authorization.clone(),
                )])
            }
            CoreCommand::SubmitCode(code) => {
                if code.trim().is_empty() {
                    return Err(CoreError::InvalidInput("login code is empty".into()));
                }

                self.authorization = AuthorizationState::Ready;
                Ok(vec![CoreEvent::AuthorizationChanged(
                    self.authorization.clone(),
                )])
            }
            CoreCommand::SubmitPassword(password) => {
                if password.is_empty() {
                    return Err(CoreError::InvalidInput("password is empty".into()));
                }

                self.authorization = AuthorizationState::Ready;
                Ok(vec![CoreEvent::AuthorizationChanged(
                    self.authorization.clone(),
                )])
            }
            CoreCommand::LoadChats { limit } => {
                let chats = self.chats.iter().take(limit).cloned().collect();
                Ok(vec![CoreEvent::ChatsLoaded(chats)])
            }
            CoreCommand::SearchMessages(_) => Err(CoreError::Backend(
                "message search is not implemented by the mock backend".into(),
            )),
            CoreCommand::SendText { chat_id, text } => {
                if self.authorization != AuthorizationState::Ready {
                    return Err(CoreError::Backend("not authorized".into()));
                }

                let text = text.trim().to_owned();
                if text.is_empty() {
                    return Err(CoreError::InvalidInput("message text is empty".into()));
                }

                let message = MessageSummary {
                    id: self.next_message_id,
                    chat_id,
                    sender_id: 0,
                    text,
                    timestamp_unix: unix_timestamp(),
                    outgoing: true,
                };
                self.next_message_id += 1;

                if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
                    chat.last_message = Some(message.clone());
                }

                Ok(vec![CoreEvent::MessageAdded(message)])
            }
            CoreCommand::Shutdown => Ok(Vec::new()),
        }
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
