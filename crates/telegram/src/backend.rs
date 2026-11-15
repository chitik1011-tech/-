use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use grammers_client::{
    Client,
    sender::{SenderPool, SenderPoolHandle},
    session::{storages::SqliteSession, updates::UpdatesLike},
    tl,
};
use tg_mobile_core::{
    AuthorizationState, BackendFuture, BackendUpdateReceiver, BackendUpdateSender, ConnectionToken,
    CoreCommand, CoreError, CoreEvent, MessageLinkResolution, MessagePeer, OutgoingSend,
    PeerReference, PeerResolution, TelegramBackend, backend_update_channel,
};
use tokio::{
    sync::{broadcast, mpsc, watch},
    task::JoinHandle,
};

use crate::{
    auth::AuthFlow,
    chats::{PeerRegistry, find_peer, load_chats, new_peer_registry},
    config::TelegramConfig,
    error::{TelegramBackendError, session_error},
    messages::{
        MessageDeduplicator, load_messages, load_messages_page, mark_read, search_messages_page,
        send_text, send_text_idempotent,
    },
    polls::{PollRegistry, new_poll_registry, poll_by_id, polls_from_updates, remember_poll},
    updates::{TelegramUpdate, UpdateWorkerInput, run_update_worker},
};

const MESSAGE_DEDUP_CAPACITY: usize = 4096;
const RELIABLE_SEND_CACHE_CAPACITY: usize = 4096;

#[derive(Debug, Clone)]
struct ReliableSendRecord {
    chat_id: i64,
    text: String,
    message_id: i64,
}

/// Real Telegram implementation of the core `TelegramBackend` trait.
///
/// Networking is driven by grammers' async `SenderPoolRunner`; no blocking FFI
/// calls are used. Incoming Telegram updates are consumed on a dedicated Tokio
/// task and are also available through [`Self::subscribe_updates`].
pub struct GrammersBackend {
    config: TelegramConfig,
    client: Client,
    pool_handle: SenderPoolHandle,
    pool_task: Option<JoinHandle<()>>,
    network_updates: Option<mpsc::Receiver<UpdatesLike>>,
    update_task: Option<JoinHandle<()>>,
    update_shutdown: Option<watch::Sender<bool>>,
    update_tx: broadcast::Sender<TelegramUpdate>,
    core_update_tx: BackendUpdateSender,
    core_update_rx: Option<BackendUpdateReceiver>,
    connection_token_tx: watch::Sender<Option<ConnectionToken>>,
    auth: AuthFlow,
    peers: PeerRegistry,
    dedup: Arc<MessageDeduplicator>,
    polls: PollRegistry,
    reliable_sends: HashMap<i64, ReliableSendRecord>,
    reliable_send_order: VecDeque<i64>,
    stopped: bool,
}

impl GrammersBackend {
    pub async fn connect(config: TelegramConfig) -> tg_mobile_core::Result<Self> {
        config.validate().map_err(CoreError::from)?;

        let session = Arc::new(
            SqliteSession::open(&config.session_path)
                .await
                .map_err(|error| session_error("open session database", error))?,
        );
        let SenderPool {
            runner,
            handle,
            updates,
        } = SenderPool::new(session, config.api_id);
        let pool_handle = handle.thin.clone();
        let client = Client::new(handle);
        let pool_task = tokio::spawn(runner.run());
        let (update_tx, _) = broadcast::channel(config.update_channel_capacity);
        let (core_update_tx, core_update_rx) = backend_update_channel();
        let (connection_token_tx, _) = watch::channel(None);

        Ok(Self {
            config,
            client,
            pool_handle,
            pool_task: Some(pool_task),
            network_updates: Some(updates),
            update_task: None,
            update_shutdown: None,
            update_tx,
            core_update_tx,
            core_update_rx: Some(core_update_rx),
            connection_token_tx,
            auth: AuthFlow::default(),
            peers: new_peer_registry(),
            dedup: Arc::new(MessageDeduplicator::new(MESSAGE_DEDUP_CAPACITY)),
            polls: new_poll_registry(),
            reliable_sends: HashMap::new(),
            reliable_send_order: VecDeque::new(),
            stopped: false,
        })
    }

    /// Subscribe to the richer backend-native update stream.
    ///
    /// Core consumes supported events through `TelegramBackend::take_updates`;
    /// this richer stream additionally exposes protocol events that do not yet
    /// have a `CoreEvent` representation.
    pub fn subscribe_updates(&self) -> broadcast::Receiver<TelegramUpdate> {
        self.update_tx.subscribe()
    }

    pub fn authorization_state(&self) -> AuthorizationState {
        self.auth.state()
    }

    async fn ensure_update_worker(&mut self) -> tg_mobile_core::Result<()> {
        if self
            .update_task
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return Ok(());
        }

        if self.update_task.take().is_some() && self.network_updates.is_none() {
            return Err(TelegramBackendError::UpdateStreamUnavailable.into());
        }

        let network_updates = self
            .network_updates
            .take()
            .ok_or(TelegramBackendError::UpdateStreamUnavailable)?;
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let input = UpdateWorkerInput {
            client: self.client.clone(),
            network_updates,
            catch_up: self.config.catch_up_updates,
            peers: Arc::clone(&self.peers),
            dedup: Arc::clone(&self.dedup),
            polls: Arc::clone(&self.polls),
            events: self.update_tx.clone(),
            core_updates: self
                .core_update_rx
                .is_none()
                .then(|| self.core_update_tx.clone()),
            shutdown: shutdown_rx,
            connection_token: self.connection_token_tx.subscribe(),
        };
        self.update_shutdown = Some(shutdown_tx);
        self.update_task = Some(tokio::spawn(run_update_worker(input)));
        Ok(())
    }

    async fn require_ready(&mut self) -> tg_mobile_core::Result<()> {
        if self.auth.state() != AuthorizationState::Ready {
            let state = self.auth.start(&self.client).await?;
            if state != AuthorizationState::Ready {
                return Err(CoreError::InvalidInput(
                    "Telegram authorization is not ready".into(),
                ));
            }
        }
        self.ensure_update_worker().await
    }

    fn known_reliable_send(
        &self,
        client_id: i64,
        chat_id: i64,
        text: &str,
    ) -> tg_mobile_core::Result<Option<i64>> {
        let Some(record) = self.reliable_sends.get(&client_id) else {
            return Ok(None);
        };

        if record.chat_id != chat_id || record.text != text {
            return Err(CoreError::InvalidInput(format!(
                "client message id {client_id} is already bound to another request"
            )));
        }

        Ok(Some(record.message_id))
    }

    fn remember_reliable_send(
        &mut self,
        client_id: i64,
        chat_id: i64,
        text: String,
        message_id: i64,
    ) {
        if !self.reliable_sends.contains_key(&client_id) {
            self.reliable_send_order.push_back(client_id);
        }
        self.reliable_sends.insert(
            client_id,
            ReliableSendRecord {
                chat_id,
                text,
                message_id,
            },
        );

        while self.reliable_send_order.len() > RELIABLE_SEND_CACHE_CAPACITY {
            if let Some(oldest) = self.reliable_send_order.pop_front() {
                self.reliable_sends.remove(&oldest);
            }
        }
    }

    async fn handle_reliable_send(
        &mut self,
        client_id: i64,
        chat_id: i64,
        text: String,
    ) -> Vec<CoreEvent> {
        let sending = CoreEvent::MessageSendStateChanged(OutgoingSend::sending(
            client_id,
            chat_id,
            text.clone(),
        ));

        match self.known_reliable_send(client_id, chat_id, &text) {
            Ok(Some(message_id)) => {
                return vec![
                    sending,
                    CoreEvent::MessageSendStateChanged(OutgoingSend::sent(
                        client_id, chat_id, text, message_id,
                    )),
                ];
            }
            Ok(None) => {}
            Err(error) => {
                return vec![
                    sending,
                    CoreEvent::MessageSendStateChanged(OutgoingSend::failed(
                        client_id,
                        chat_id,
                        text,
                        error.to_string(),
                        false,
                    )),
                ];
            }
        }

        let result = async {
            self.require_ready().await?;
            let peer = find_peer(&self.peers, chat_id)
                .await
                .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
            send_text_idempotent(&self.client, peer, text.clone(), client_id).await
        }
        .await;

        match result {
            Ok(message) => {
                self.remember_reliable_send(client_id, chat_id, text.clone(), message.id);

                let mut events = vec![sending];
                if self.dedup.should_emit(&message).await {
                    events.push(CoreEvent::MessageAdded(message.clone()));
                }
                events.push(CoreEvent::MessageSendStateChanged(OutgoingSend::sent(
                    client_id, chat_id, text, message.id,
                )));
                events
            }
            Err(error) => {
                let state = reliable_send_error_state(client_id, chat_id, text, error);
                vec![sending, CoreEvent::MessageSendStateChanged(state)]
            }
        }
    }

    async fn shutdown(&mut self) -> tg_mobile_core::Result<Vec<CoreEvent>> {
        if self.stopped {
            return Ok(Vec::new());
        }

        if let Some(shutdown) = self.update_shutdown.take() {
            let _ = shutdown.send(true);
        }
        if let Some(task) = self.update_task.take() {
            task.await.map_err(|error| {
                CoreError::Backend(format!(
                    "Telegram update worker failed during shutdown: {error}"
                ))
            })?;
        }

        let _ = self.pool_handle.quit();
        if let Some(task) = self.pool_task.take() {
            task.await.map_err(|error| {
                CoreError::Backend(format!(
                    "Telegram sender pool failed during shutdown: {error}"
                ))
            })?;
        }

        self.auth.close();
        self.stopped = true;
        Ok(vec![CoreEvent::AuthorizationChanged(
            AuthorizationState::Closed,
        )])
    }

    async fn handle_command(
        &mut self,
        command: CoreCommand,
    ) -> tg_mobile_core::Result<Vec<CoreEvent>> {
        command.validate()?;

        if self.stopped && !matches!(command, CoreCommand::Shutdown) {
            return Err(TelegramBackendError::Stopped.into());
        }

        let command_events = match command {
            CoreCommand::Start => {
                let state = self.auth.start(&self.client).await?;
                if state == AuthorizationState::Ready {
                    self.ensure_update_worker().await?;
                }
                vec![CoreEvent::AuthorizationChanged(state)]
            }
            CoreCommand::SetPhoneNumber(phone) => {
                let state = self
                    .auth
                    .set_phone_number(&self.client, &self.config.api_hash, phone)
                    .await?;
                vec![CoreEvent::AuthorizationChanged(state)]
            }
            CoreCommand::SubmitCode(code) => {
                let state = self.auth.submit_code(&self.client, code).await?;
                if state == AuthorizationState::Ready {
                    self.ensure_update_worker().await?;
                }
                vec![CoreEvent::AuthorizationChanged(state)]
            }
            CoreCommand::SubmitPassword(password) => {
                let state = self.auth.submit_password(&self.client, password).await?;
                if state == AuthorizationState::Ready {
                    self.ensure_update_worker().await?;
                }
                vec![CoreEvent::AuthorizationChanged(state)]
            }
            CoreCommand::LoadChats { limit } => {
                self.require_ready().await?;
                let chats = load_chats(&self.client, &self.peers, limit).await?;
                vec![CoreEvent::ChatsLoaded(chats)]
            }
            CoreCommand::ResolvePeer { input } => {
                self.require_ready().await?;
                let reference = tg_mobile_core::parse_peer_reference(&input)?;
                let resolution = self.resolve_reference(reference).await?;
                vec![CoreEvent::PeerResolved(resolution)]
            }
            CoreCommand::LoadMessages { chat_id, limit } => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let loaded = load_messages(&self.client, peer, limit).await?;
                for poll in &loaded.polls {
                    remember_poll(&self.polls, poll.clone()).await;
                }
                let mut events = vec![CoreEvent::MessagesLoaded {
                    chat_id,
                    messages: loaded.messages,
                }];
                events.extend(loaded.polls.into_iter().map(CoreEvent::PollUpdated));
                events
            }
            CoreCommand::LoadMessagesPage {
                chat_id,
                before_message_id,
                limit,
            } => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let page = load_messages_page(&self.client, peer, before_message_id, limit).await?;
                for poll in &page.polls {
                    remember_poll(&self.polls, poll.clone()).await;
                }
                let mut events = vec![CoreEvent::MessagesPageLoaded {
                    chat_id,
                    before_message_id,
                    messages: page.messages,
                    next_before_message_id: page.next_before_message_id,
                    has_more: page.has_more,
                }];
                events.extend(page.polls.into_iter().map(CoreEvent::PollUpdated));
                events
            }
            CoreCommand::MarkRead {
                chat_id,
                max_message_id,
            } => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                mark_read(&self.client, peer, max_message_id).await?;
                vec![CoreEvent::ChatRead {
                    chat_id,
                    max_message_id,
                }]
            }
            CoreCommand::SearchMessages(request) => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, request.chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(request.chat_id))?;
                let page = search_messages_page(
                    &self.client,
                    peer,
                    &request.query,
                    request.before_message_id,
                    request.limit,
                )
                .await?;

                vec![CoreEvent::MessageSearchCompleted(
                    tg_mobile_core::MessageSearchPage {
                        request_id: request.request_id,
                        chat_id: request.chat_id,
                        query: request.query.trim().to_owned(),
                        requested_before_message_id: request.before_message_id,
                        messages: page.messages,
                        next_before_message_id: page.next_before_message_id,
                        has_more: page.has_more,
                    },
                )]
            }
            CoreCommand::SendText { chat_id, text } => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let message = send_text(&self.client, peer, text).await?;
                if self.dedup.should_emit(&message).await {
                    vec![CoreEvent::MessageAdded(message)]
                } else {
                    Vec::new()
                }
            }
            CoreCommand::SendTextReliable {
                client_id,
                chat_id,
                text,
            } => self.handle_reliable_send(client_id, chat_id, text).await,
            CoreCommand::VotePoll {
                poll_id,
                chat_id,
                message_id,
                option_ids,
            } => {
                self.require_ready().await?;

                if let Some(poll) = poll_by_id(&self.polls, poll_id).await {
                    if poll.chat_id != chat_id || poll.message_id != message_id {
                        return Err(CoreError::InvalidInput(
                            "poll vote identity does not match Telegram poll cache".into(),
                        ));
                    }
                    poll.validate_vote_selection(&option_ids)?;
                }

                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let msg_id = i32::try_from(message_id).map_err(|_| {
                    CoreError::InvalidInput("poll message id exceeds Telegram range".into())
                })?;

                let updates = self
                    .client
                    .invoke(&tl::functions::messages::SendVote {
                        peer: peer.clone().into(),
                        msg_id,
                        options: option_ids,
                    })
                    .await
                    .map_err(|error| crate::error::api_error("vote on poll", error))?;

                let polls = polls_from_updates(&updates, &self.polls).await?;
                Ok::<_, CoreError>(
                    polls.into_iter().map(CoreEvent::PollUpdated).collect::<Vec<_>>(),
                )?
            }
            CoreCommand::ArchiveChat { chat_id } | CoreCommand::UnarchiveChat { chat_id } => {
                self.require_ready().await?;
                let archived = matches!(command, CoreCommand::ArchiveChat { .. });
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                self.client
                    .invoke(&tl::functions::folders::EditPeerFolders {
                        folder_peers: vec![
                            tl::types::InputFolderPeer {
                                peer: peer.clone().into(),
                                folder_id: if archived { 1 } else { 0 },
                            }
                            .into(),
                        ],
                    })
                    .await
                    .map_err(|error| crate::error::api_error("update chat archive", error))?;
                vec![CoreEvent::ChatArchived { chat_id, archived }]
            }
            CoreCommand::MuteChat { chat_id } | CoreCommand::UnmuteChat { chat_id } => {
                self.require_ready().await?;
                let muted = matches!(command, CoreCommand::MuteChat { .. });
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                self.client
                    .invoke(&tl::functions::account::UpdateNotifySettings {
                        peer: tl::types::InputNotifyPeer {
                            peer: peer.clone().into(),
                        }
                        .into(),
                        settings: tl::types::InputPeerNotifySettings {
                            show_previews: None,
                            silent: None,
                            mute_until: Some(if muted { i32::MAX } else { 0 }),
                            sound: None,
                            stories_muted: None,
                            stories_hide_sender: None,
                            stories_sound: None,
                        }
                        .into(),
                    })
                    .await
                    .map_err(|error| crate::error::api_error("update chat notifications", error))?;
                vec![CoreEvent::ChatMuted { chat_id, muted }]
            }
            CoreCommand::PinChat { chat_id } | CoreCommand::UnpinChat { chat_id } => {
                self.require_ready().await?;
                let pinned = matches!(command, CoreCommand::PinChat { .. });
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                self.client
                    .invoke(&tl::functions::messages::ToggleDialogPin {
                        pinned,
                        peer: tl::types::InputDialogPeer {
                            peer: peer.clone().into(),
                        }
                        .into(),
                    })
                    .await
                    .map_err(|error| crate::error::api_error("update chat pin", error))?;
                vec![CoreEvent::ChatPinned { chat_id, pinned }]
            }
            CoreCommand::MarkChatUnread { chat_id } | CoreCommand::MarkChatRead { chat_id } => {
                self.require_ready().await?;
                let unread = matches!(command, CoreCommand::MarkChatUnread { .. });
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                if unread {
                    self.client
                        .invoke(&tl::functions::messages::MarkDialogUnread {
                            unread: true,
                            parent_peer: None,
                            peer: tl::types::InputDialogPeer {
                                peer: peer.clone().into(),
                            }
                            .into(),
                        })
                        .await
                        .map_err(|error| crate::error::api_error("mark chat unread", error))?;
                } else {
                    self.client
                        .mark_as_read(peer)
                        .await
                        .map_err(|error| crate::error::api_error("mark chat read", error))?;
                }
                vec![CoreEvent::ChatUnreadChanged { chat_id, unread }]
            }
            CoreCommand::UpdateDraft(_) | CoreCommand::ClearDraft { .. } => {
                return Err(CoreError::Backend(
                    "draft commands are handled by the core runtime".into(),
                ));
            }
            CoreCommand::Shutdown => self.shutdown().await?,
        };

        Ok(command_events)
    }

    async fn resolve_reference(
        &mut self,
        reference: PeerReference,
    ) -> tg_mobile_core::Result<PeerResolution> {
        let (username, message_link) = match reference {
            PeerReference::Username { username } => (username, None),
            PeerReference::MessageLink(link) => match &link.peer {
                MessagePeer::Username { username } => (username.clone(), Some(link)),
                MessagePeer::InternalChannelId { .. } => {
                    return Ok(PeerResolution::Unsupported {
                        reason: "private /c/ message links require a previously loaded peer".into(),
                    });
                }
            },
            PeerReference::NumericUserId { .. } => {
                return Ok(PeerResolution::Unknown {
                    reason: "numeric user IDs cannot be resolved without a known access hash"
                        .into(),
                });
            }
        };
        let Some(peer) = self
            .client
            .resolve_username(&username)
            .await
            .map_err(|error| crate::error::api_error("resolve Telegram username", error))?
        else {
            return Ok(PeerResolution::NotFound);
        };
        let chat_id = crate::ids::peer_id_to_core(peer.id());
        if let Some(peer_ref) = peer
            .to_ref()
            .await
            .map_err(|error| CoreError::Backend(error.to_string()))?
        {
            crate::chats::remember_peer(&self.peers, chat_id, peer_ref).await;
        }
        if let Some(link) = message_link {
            return Ok(PeerResolution::MessageLink(MessageLinkResolution {
                chat_id,
                message_id: link.message_id,
                metadata: link.metadata,
            }));
        }
        use grammers_client::peer::Peer;
        Ok(match peer {
            Peer::User(user) if user.is_bot() => PeerResolution::Bot {
                user_id: chat_id,
                username: user.username().map(str::to_owned),
            },
            Peer::User(user) => PeerResolution::User {
                user_id: chat_id,
                username: user.username().map(str::to_owned),
            },
            Peer::Group(group) if group.is_megagroup() => PeerResolution::Supergroup {
                chat_id,
                title: group.title().map(str::to_owned),
            },
            Peer::Group(group) => PeerResolution::Group {
                chat_id,
                title: group.title().map(str::to_owned),
            },
            Peer::Channel(channel) => PeerResolution::Channel {
                chat_id,
                title: Some(channel.title().to_owned()),
            },
            Peer::Community(_) => PeerResolution::Unsupported {
                reason: "Telegram community peers are not yet supported".into(),
            },
        })
    }
}

fn reliable_send_error_state(
    client_id: i64,
    chat_id: i64,
    text: String,
    error: CoreError,
) -> OutgoingSend {
    match error {
        CoreError::SendOutcomeUnknown(reason) => {
            OutgoingSend::awaiting_confirmation(client_id, chat_id, text, reason)
        }
        CoreError::FloodWait { seconds } => {
            OutgoingSend::flood_wait(client_id, chat_id, text, seconds)
        }
        CoreError::TransientTransport(reason) | CoreError::TransientSend(reason) => {
            OutgoingSend::failed(client_id, chat_id, text, reason, true)
        }
        CoreError::SendRejected(reason) => OutgoingSend::rejected(client_id, chat_id, text, reason),
        other => OutgoingSend::rejected(client_id, chat_id, text, other.to_string()),
    }
}

#[cfg(test)]
mod reliable_send_error_tests {
    use tg_mobile_core::{CoreError, OutgoingSendState};

    use super::reliable_send_error_state;

    #[test]
    fn retryable_errors_are_distinct_from_permanent_and_ambiguous_errors() {
        let retryable = reliable_send_error_state(
            7,
            9,
            "hello".into(),
            CoreError::TransientSend("server busy".into()),
        );
        assert!(matches!(
            retryable.state,
            OutgoingSendState::Failed {
                retryable: true,
                ..
            }
        ));

        let permanent = reliable_send_error_state(
            7,
            9,
            "hello".into(),
            CoreError::SendRejected("MESSAGE_TOO_LONG".into()),
        );
        assert!(matches!(
            permanent.state,
            OutgoingSendState::Rejected { .. }
        ));

        let ambiguous = reliable_send_error_state(
            7,
            9,
            "hello".into(),
            CoreError::SendOutcomeUnknown("connection closed".into()),
        );
        assert!(matches!(
            ambiguous.state,
            OutgoingSendState::AwaitingConfirmation { .. }
        ));
    }
}

impl TelegramBackend for GrammersBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async move { self.handle_command(command).await })
    }

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        self.core_update_rx.take()
    }

    fn set_connection_token(&mut self, token: ConnectionToken) {
        self.connection_token_tx.send_replace(Some(token));
    }
}

impl Drop for GrammersBackend {
    fn drop(&mut self) {
        if let Some(shutdown) = self.update_shutdown.take() {
            let _ = shutdown.send(true);
        }
        if let Some(task) = self.update_task.take() {
            task.abort();
        }
        let _ = self.pool_handle.quit();
        if let Some(task) = self.pool_task.take() {
            task.abort();
        }
    }
}
