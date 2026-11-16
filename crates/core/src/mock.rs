use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    backend::{BackendFuture, TelegramBackend},
    capabilities::{CapabilityValue, ChatCapabilities, ChatKind},
    command::CoreCommand,
    error::{CoreError, Result},
    event::CoreEvent,
    model::{AuthorizationState, ChatSummary, MessageLocation, MessageSummary, PeerProfile},
    send::OutgoingSend,
};

pub struct MockTelegramBackend {
    authorization: AuthorizationState,
    chats: Vec<ChatSummary>,
    next_message_id: i64,
    pinned_messages: BTreeMap<i64, Vec<i64>>,
    reliable_sends: BTreeMap<i64, (i64, String, i64)>,
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
                    archived: Some(false),
                    muted: Some(false),
                    pinned: Some(false),
                    is_unread: Some(false),
                    last_message: None,
                },
                ChatSummary {
                    id: 2,
                    title: "Rust Telegram".into(),
                    unread_count: 1,
                    archived: Some(false),
                    muted: Some(false),
                    pinned: Some(false),
                    is_unread: Some(true),
                    last_message: Some(MessageSummary {
                        id: 1,
                        chat_id: 2,
                        sender_id: 42,
                        text: "Core online".into(),
                        timestamp_unix: unix_timestamp(),
                        outgoing: false,
                        location: None,
                        attachments: Vec::new(),
                        forwarded_from: None,
                        edited: false,
                        edit_timestamp_unix: None,
                        service_message: false,
                        service: None,
                        media_caption: None,
                        reply_to: None,
                        reactions: Vec::new(),
                        reaction_revision: None,
                    }),
                },
            ],
            next_message_id: 2,
            pinned_messages: BTreeMap::new(),
            reliable_sends: BTreeMap::new(),
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
                let chats: Vec<_> = self.chats.iter().take(limit).cloned().collect();
                let mut events = vec![CoreEvent::ChatsLoaded(chats.clone())];
                events.extend(
                    chats
                        .into_iter()
                        .map(|chat| CoreEvent::ChatCapabilitiesUpdated(mock_capabilities(chat.id))),
                );
                Ok(events)
            }
            CoreCommand::LoadProfile { peer_id } => Ok(vec![CoreEvent::ProfileUpdated(
                PeerProfile::unknown(peer_id),
            )]),
            CoreCommand::LoadContacts { .. } => Err(CoreError::Backend(
                "contacts are unavailable in the mock backend".into(),
            )),
            CoreCommand::LoadMessages { chat_id, limit } => {
                let mut messages = self
                    .chats
                    .iter()
                    .filter(|chat| chat.id == chat_id)
                    .filter_map(|chat| chat.last_message.clone())
                    .take(limit)
                    .collect::<Vec<_>>();
                messages.sort_by_key(|message| (message.timestamp_unix, message.id));
                Ok(vec![CoreEvent::MessagesLoaded { chat_id, messages }])
            }
            CoreCommand::LoadMessagesPage {
                chat_id,
                before_message_id,
                limit,
            } => {
                let messages = self
                    .chats
                    .iter()
                    .filter(|chat| chat.id == chat_id)
                    .filter_map(|chat| chat.last_message.clone())
                    .filter(|message| {
                        before_message_id
                            .map(|before| message.id < before)
                            .unwrap_or(true)
                    })
                    .take(limit)
                    .collect::<Vec<_>>();
                Ok(vec![CoreEvent::MessagesPageLoaded {
                    chat_id,
                    before_message_id,
                    messages,
                    next_before_message_id: None,
                    has_more: false,
                }])
            }
            CoreCommand::LoadPinnedMessages { chat_id } => {
                Ok(vec![CoreEvent::PinnedMessagesLoaded {
                    chat_id,
                    message_ids: self
                        .pinned_messages
                        .get(&chat_id)
                        .cloned()
                        .unwrap_or_default(),
                    request_revision: None,
                }])
            }
            CoreCommand::PinMessage {
                chat_id,
                message_id,
            } => {
                let exists = self.chats.iter().any(|chat| {
                    chat.id == chat_id
                        && chat
                            .last_message
                            .as_ref()
                            .is_some_and(|message| message.id == message_id)
                });
                if !exists {
                    return Err(CoreError::Backend(
                        "message not found in mock backend".into(),
                    ));
                }
                let ids = self.pinned_messages.entry(chat_id).or_default();
                if !ids.contains(&message_id) {
                    ids.push(message_id);
                    ids.sort_unstable();
                }
                Ok(vec![CoreEvent::PinnedMessagesChanged {
                    chat_id,
                    message_ids: vec![message_id],
                    pinned: true,
                    pts: None,
                }])
            }
            CoreCommand::UnpinMessage {
                chat_id,
                message_id,
            } => {
                if let Some(ids) = self.pinned_messages.get_mut(&chat_id) {
                    ids.retain(|id| *id != message_id);
                    if ids.is_empty() {
                        self.pinned_messages.remove(&chat_id);
                    }
                }
                Ok(vec![CoreEvent::PinnedMessagesChanged {
                    chat_id,
                    message_ids: vec![message_id],
                    pinned: false,
                    pts: None,
                }])
            }
            CoreCommand::MarkRead {
                chat_id,
                max_message_id,
            } => {
                if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
                    let covers_latest = chat
                        .last_message
                        .as_ref()
                        .map(|message| message.id <= max_message_id)
                        .unwrap_or(true);
                    if covers_latest {
                        chat.unread_count = 0;
                    }
                }
                Ok(vec![CoreEvent::ChatRead {
                    chat_id,
                    max_message_id,
                }])
            }
            CoreCommand::SearchMessages(_) => Err(CoreError::Backend(
                "message search is not implemented by the mock backend".into(),
            )),
            CoreCommand::ForwardMessages(_) => Err(CoreError::Backend(
                "message forwarding is not implemented by the mock backend".into(),
            )),
            CoreCommand::ReplyMessage {
                chat_id,
                reply_to_message_id,
                text,
            } => {
                let mut events = self.handle_inner(CoreCommand::SendText { chat_id, text })?;
                for event in &mut events {
                    if let CoreEvent::MessageAdded(message) = event {
                        message.reply_to = Some(crate::model::ReplyMetadata {
                            message_id: reply_to_message_id,
                            chat_id: Some(chat_id),
                            sender_id: None,
                            sender_display_name: None,
                        });
                        if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
                            chat.last_message = Some(message.clone());
                        }
                    }
                }
                Ok(events)
            }
            CoreCommand::DeleteMessages { .. } => Err(CoreError::Backend(
                "message deletion is not implemented by the mock backend".into(),
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
                    location: None,
                    attachments: Vec::new(),
                    forwarded_from: None,
                    edited: false,
                    edit_timestamp_unix: None,
                    service_message: false,
                    service: None,
                    media_caption: None,
                    reply_to: None,
                    reactions: Vec::new(),
                    reaction_revision: None,
                };
                self.next_message_id += 1;

                if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
                    chat.last_message = Some(message.clone());
                }

                Ok(vec![CoreEvent::MessageAdded(message)])
            }
            CoreCommand::EditMessage { .. } => Err(CoreError::Backend(
                "message editing is not implemented by the mock backend".into(),
            )),
            CoreCommand::DeleteMessageForEveryone { .. } => Err(CoreError::Backend(
                "message deletion is not implemented by the mock backend".into(),
            )),
            CoreCommand::DeleteMessageForMe { .. } => Err(CoreError::Backend(
                "message deletion is not implemented by the mock backend".into(),
            )),
            CoreCommand::SendLocation { chat_id, location } => {
                if self.authorization != AuthorizationState::Ready {
                    return Err(CoreError::Backend("not authorized".into()));
                }
                location.validate()?;
                let message = MessageSummary {
                    id: self.next_message_id,
                    chat_id,
                    sender_id: 0,
                    text: String::new(),
                    timestamp_unix: unix_timestamp(),
                    outgoing: true,
                    location: Some(MessageLocation::Location { point: location }),
                    attachments: Vec::new(),
                    forwarded_from: None,
                    edited: false,
                    edit_timestamp_unix: None,
                    service_message: false,
                    service: None,
                    media_caption: None,
                    reply_to: None,
                    reactions: Vec::new(),
                    reaction_revision: None,
                };
                self.next_message_id += 1;
                if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
                    chat.last_message = Some(message.clone());
                }
                Ok(vec![CoreEvent::MessageAdded(message)])
            }
            CoreCommand::UpdateDraft(_) | CoreCommand::ClearDraft { .. } => Err(
                CoreError::Backend("draft commands are handled by the core runtime".into()),
            ),
            CoreCommand::SendTextReliable {
                client_id,
                chat_id,
                text,
            } => {
                if self.authorization != AuthorizationState::Ready {
                    return Err(CoreError::Backend("not authorized".into()));
                }

                let text = text.trim().to_owned();
                if text.is_empty() {
                    return Err(CoreError::InvalidInput("message text is empty".into()));
                }

                let sending = CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
                    client_id,
                    chat_id,
                    text.clone(),
                ));

                if let Some((known_chat_id, known_text, message_id)) =
                    self.reliable_sends.get(&client_id)
                {
                    if *known_chat_id != chat_id || known_text != &text {
                        return Ok(vec![
                            sending,
                            CoreEvent::MessageSendStateChanged(OutgoingSend::failed(
                                client_id,
                                chat_id,
                                text,
                                format!(
                                    "client message id {client_id} is already bound to another request"
                                ),
                                false,
                            )),
                        ]);
                    }
                    return Ok(vec![
                        sending,
                        CoreEvent::MessageSendStateChanged(OutgoingSend::sent(
                            client_id,
                            chat_id,
                            text,
                            *message_id,
                        )),
                    ]);
                }

                let message = MessageSummary {
                    id: self.next_message_id,
                    chat_id,
                    sender_id: 0,
                    text: text.clone(),
                    timestamp_unix: unix_timestamp(),
                    outgoing: true,
                    location: None,
                    attachments: Vec::new(),
                    forwarded_from: None,
                    edited: false,
                    edit_timestamp_unix: None,
                    service_message: false,
                    service: None,
                    media_caption: None,
                    reply_to: None,
                    reactions: Vec::new(),
                    reaction_revision: None,
                };
                self.next_message_id += 1;

                if let Some(chat) = self.chats.iter_mut().find(|chat| chat.id == chat_id) {
                    chat.last_message = Some(message.clone());
                }

                self.reliable_sends
                    .insert(client_id, (chat_id, text.clone(), message.id));

                Ok(vec![
                    sending,
                    CoreEvent::MessageAdded(message.clone()),
                    CoreEvent::MessageSendStateChanged(OutgoingSend::sent(
                        client_id, chat_id, text, message.id,
                    )),
                ])
            }
            CoreCommand::VotePoll { .. } => Err(CoreError::Backend(
                "poll voting is not implemented by the mock backend".into(),
            )),
            CoreCommand::DownloadMedia { .. }
            | CoreCommand::RetryMediaDownload { .. }
            | CoreCommand::CancelMediaDownload { .. } => Err(CoreError::Backend(
                "media downloads are not implemented by the mock backend".into(),
            )),
            CoreCommand::ArchiveChat { chat_id } => self.set_chat_action(
                chat_id,
                |chat| chat.archived = Some(true),
                CoreEvent::ChatArchived {
                    chat_id,
                    archived: true,
                },
            ),
            CoreCommand::UnarchiveChat { chat_id } => self.set_chat_action(
                chat_id,
                |chat| chat.archived = Some(false),
                CoreEvent::ChatArchived {
                    chat_id,
                    archived: false,
                },
            ),
            CoreCommand::MuteChat { chat_id } => self.set_chat_action(
                chat_id,
                |chat| chat.muted = Some(true),
                CoreEvent::ChatMuted {
                    chat_id,
                    muted: true,
                },
            ),
            CoreCommand::UnmuteChat { chat_id } => self.set_chat_action(
                chat_id,
                |chat| chat.muted = Some(false),
                CoreEvent::ChatMuted {
                    chat_id,
                    muted: false,
                },
            ),
            CoreCommand::PinChat { chat_id } => self.set_chat_action(
                chat_id,
                |chat| chat.pinned = Some(true),
                CoreEvent::ChatPinned {
                    chat_id,
                    pinned: true,
                },
            ),
            CoreCommand::UnpinChat { chat_id } => self.set_chat_action(
                chat_id,
                |chat| chat.pinned = Some(false),
                CoreEvent::ChatPinned {
                    chat_id,
                    pinned: false,
                },
            ),
            CoreCommand::MarkChatUnread { chat_id } => self.set_chat_action(
                chat_id,
                |chat| chat.is_unread = Some(true),
                CoreEvent::ChatUnreadChanged {
                    chat_id,
                    unread: true,
                },
            ),
            CoreCommand::MarkChatRead { chat_id } => self.set_chat_action(
                chat_id,
                |chat| {
                    chat.is_unread = Some(false);
                    chat.unread_count = 0;
                },
                CoreEvent::ChatUnreadChanged {
                    chat_id,
                    unread: false,
                },
            ),
            CoreCommand::AddReaction { .. } | CoreCommand::RemoveReaction { .. } => {
                Err(CoreError::Backend(
                    "message reactions are not implemented by the mock backend".into(),
                ))
            }
            CoreCommand::Logout => {
                self.authorization = AuthorizationState::Closed;
                Ok(vec![CoreEvent::AuthorizationChanged(
                    self.authorization.clone(),
                )])
            }
            CoreCommand::Shutdown => Ok(Vec::new()),
            CoreCommand::ResolvePeer { .. } => Err(CoreError::Backend(
                "peer resolution is unsupported by this backend".into(),
            )),
        }
    }

    fn set_chat_action(
        &mut self,
        chat_id: i64,
        update: impl FnOnce(&mut ChatSummary),
        event: CoreEvent,
    ) -> Result<Vec<CoreEvent>> {
        if self.authorization != AuthorizationState::Ready {
            return Err(CoreError::Backend("not authorized".into()));
        }
        let chat = self
            .chats
            .iter_mut()
            .find(|chat| chat.id == chat_id)
            .ok_or_else(|| CoreError::Backend(format!("chat {chat_id} not found")))?;
        update(chat);
        Ok(vec![event])
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn mock_capabilities(chat_id: i64) -> ChatCapabilities {
    let mut capabilities = ChatCapabilities {
        chat_id,
        chat_kind: ChatKind::Private,
        can_send_messages: CapabilityValue::Known(true),
        can_send_media: CapabilityValue::Known(true),
        can_forward_messages: CapabilityValue::Known(true),
        slow_mode_enabled: CapabilityValue::Known(false),
        slow_mode_remaining_seconds: CapabilityValue::Known(None),
        ..ChatCapabilities::default()
    };
    capabilities.refresh_derived();
    capabilities
}
