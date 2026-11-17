use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    path::Path,
    sync::Arc,
    time::Duration,
};

use grammers_client::{
    Client,
    media::Media,
    message::InputReactions,
    sender::{SenderPool, SenderPoolHandle},
    session::{storages::SqliteSession, types::PeerKind, updates::UpdatesLike},
    tl,
};
use tg_media_cache::{CacheConfig, MediaCache};
use tg_mobile_core::{
    AttachmentDownloadState, AuthorizationState, BackendFuture, BackendUpdateReceiver,
    BackendUpdateSender, ConnectionToken, CoreCommand, CoreError, CoreEvent, DownloadState,
    ForwardMessagesResult, ForwardOutcome, MessageLinkResolution, MessagePeer, MessageSummary,
    OutgoingMediaAttachment, OutgoingSend, PeerReference, PeerResolution, TelegramBackend,
    TransferDirection, TransferManager, TransferState, TransferToken, UploadState,
    backend_update_channel,
};
use tokio::{
    sync::{broadcast, mpsc, watch},
    task::JoinHandle,
    time::timeout,
};

use crate::{
    auth::AuthFlow,
    chats::{PeerRegistry, clear_peer_registry, find_peer, load_chats, new_peer_registry},
    config::TelegramConfig,
    contacts::load_contacts,
    downloads::{MediaTarget, event as media_event, spawn_download, state_from_snapshot},
    error::{TelegramBackendError, api_error, session_error},
    messages::{
        MessageDeduplicator, load_messages, load_messages_page, load_pinned_message_ids, mark_read,
        message_to_summary, search_messages_page, send_location, send_reply, send_text,
        send_text_idempotent,
    },
    polls::{
        PollRegistry, clear_poll_registry, new_poll_registry, poll_by_id, polls_from_updates,
        remember_poll,
    },
    updates::{TelegramUpdate, UpdateWorkerInput, run_update_worker},
    uploads::{MediaSendStage, spawn_media_send},
};

const MESSAGE_DEDUP_CAPACITY: usize = 4096;
const RELIABLE_SEND_CACHE_CAPACITY: usize = 4096;
const MEDIA_CACHE_MAX_BYTES: u64 = 512 * 1024 * 1024;
const INITIAL_AUTHORIZATION_CHECK_TIMEOUT: Duration = Duration::from_secs(20);

/// Bound the initial MTProto authorization probe so an unavailable network
/// cannot leave startup waiting forever. This deliberately does not wrap later
/// login or update-stream operations.
async fn with_initial_connection_timeout<T>(
    future: impl Future<Output = tg_mobile_core::Result<T>>,
) -> tg_mobile_core::Result<T> {
    timeout(INITIAL_AUTHORIZATION_CHECK_TIMEOUT, future)
        .await
        .map_err(|_| CoreError::from(TelegramBackendError::InitialConnectionTimeout))?
}

#[derive(Debug, Clone)]
struct ReliableSendRecord {
    chat_id: i64,
    text: String,
    reply_to_message_id: Option<i64>,
    message_id: i64,
}

struct ActiveMediaTask {
    reference: String,
    handle: JoinHandle<()>,
}

struct ActiveMediaSendTask {
    reference: String,
    transfer_token: TransferToken,
    transfer_generation: u64,
    session_generation: u64,
    session_epoch: u64,
    chat_id: i64,
    media: OutgoingMediaAttachment,
    caption: String,
    send_stage: Arc<MediaSendStage>,
    handle: JoinHandle<()>,
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
    media_cache: MediaCache,
    transfers: Arc<TransferManager>,
    media_tasks: HashMap<(i64, i64, usize), ActiveMediaTask>,
    media_send_tasks: HashMap<i64, ActiveMediaSendTask>,
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
        let media_cache_root = config
            .session_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("media-cache");
        let media_cache = MediaCache::new(CacheConfig {
            root: media_cache_root,
            max_bytes: MEDIA_CACHE_MAX_BYTES,
        })
        .map_err(|error| {
            CoreError::Backend(format!("could not initialize media cache: {error}"))
        })?;

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
            media_cache,
            transfers: Arc::new(TransferManager::new()),
            media_tasks: HashMap::new(),
            media_send_tasks: HashMap::new(),
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

    fn authorization_events(&self, state: AuthorizationState) -> Vec<CoreEvent> {
        let delivery = if state == AuthorizationState::WaitingForCode {
            self.auth.login_code_delivery()
        } else {
            None
        };
        vec![
            CoreEvent::AuthorizationChanged(state),
            CoreEvent::AuthorizationCodeDeliveryChanged(delivery),
        ]
    }

    fn authorization_error_events(
        &self,
        previous_state: AuthorizationState,
        error: &CoreError,
    ) -> Option<Vec<CoreEvent>> {
        let state = self.auth.state();
        if state == previous_state {
            return None;
        }
        let mut events = self.authorization_events(state);
        events.push(CoreEvent::Error(error.to_string()));
        Some(events)
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
            let state = with_initial_connection_timeout(self.auth.start(&self.client)).await?;
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
        reply_to_message_id: Option<i64>,
    ) -> tg_mobile_core::Result<Option<i64>> {
        let Some(record) = self.reliable_sends.get(&client_id) else {
            return Ok(None);
        };

        if record.chat_id != chat_id
            || record.text != text
            || record.reply_to_message_id != reply_to_message_id
        {
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
        reply_to_message_id: Option<i64>,
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
                reply_to_message_id,
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
        reply_to_message_id: Option<i64>,
    ) -> Vec<CoreEvent> {
        let sending = CoreEvent::MessageSendStateChanged(
            OutgoingSend::sending(client_id, chat_id, text.clone())
                .with_reply_target(reply_to_message_id),
        );

        match self.known_reliable_send(client_id, chat_id, &text, reply_to_message_id) {
            Ok(Some(message_id)) => {
                return vec![
                    sending,
                    CoreEvent::MessageSendStateChanged(
                        OutgoingSend::sent(client_id, chat_id, text, message_id)
                            .with_reply_target(reply_to_message_id),
                    ),
                ];
            }
            Ok(None) => {}
            Err(error) => {
                return vec![
                    sending,
                    CoreEvent::MessageSendStateChanged(
                        OutgoingSend::failed(client_id, chat_id, text, error.to_string(), false)
                            .with_reply_target(reply_to_message_id),
                    ),
                ];
            }
        }

        let result = async {
            self.require_ready().await?;
            let peer = find_peer(&self.peers, chat_id)
                .await
                .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
            send_text_idempotent(
                &self.client,
                peer,
                text.clone(),
                client_id,
                reply_to_message_id,
            )
            .await
        }
        .await;

        match result {
            Ok(message) => {
                self.remember_reliable_send(
                    client_id,
                    chat_id,
                    text.clone(),
                    reply_to_message_id,
                    message.id,
                );

                let mut events = vec![sending];
                if self.dedup.should_emit(&message).await {
                    events.push(CoreEvent::MessageAdded(message.clone()));
                }
                events.push(CoreEvent::MessageSendStateChanged(
                    OutgoingSend::sent(client_id, chat_id, text, message.id)
                        .with_reply_target(reply_to_message_id),
                ));
                events
            }
            Err(error) => {
                let state =
                    reliable_send_error_state(client_id, chat_id, text, reply_to_message_id, error);
                vec![sending, CoreEvent::MessageSendStateChanged(state)]
            }
        }
    }

    async fn handle_media_send(
        &mut self,
        client_id: i64,
        chat_id: i64,
        media: OutgoingMediaAttachment,
        caption: String,
        transfer_generation: u64,
        session_generation: u64,
        session_epoch: u64,
    ) -> tg_mobile_core::Result<Vec<CoreEvent>> {
        self.require_ready().await?;
        media.validate().map_err(CoreError::InvalidInput)?;
        self.media_send_tasks
            .retain(|_, task| !task.handle.is_finished());
        if self
            .media_send_tasks
            .get(&client_id)
            .is_some_and(|task| !task.handle.is_finished())
        {
            return Ok(Vec::new());
        }
        self.media_send_tasks.remove(&client_id);

        let peer = find_peer(&self.peers, chat_id)
            .await
            .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
        let reference = format!("outgoing:{client_id}");
        let snapshot = self
            .transfers
            .get_by_key(TransferDirection::Upload, &reference)
            .map_err(|error| CoreError::Backend(error.to_string()))?;
        let token = match snapshot {
            None => {
                self.transfers
                    .request(
                        TransferDirection::Upload,
                        reference.clone(),
                        Some(media.size_bytes),
                    )
                    .map_err(|error| CoreError::Backend(error.to_string()))?
                    .token
            }
            Some(snapshot) => match snapshot.state {
                TransferState::Upload(UploadState::Pending) => snapshot.token,
                TransferState::Upload(UploadState::Uploading) => {
                    let failed = self
                        .transfers
                        .fail(snapshot.token)
                        .map_err(|error| CoreError::Backend(error.to_string()))?;
                    self.transfers
                        .retry(failed)
                        .map_err(|error| CoreError::Backend(error.to_string()))?
                }
                TransferState::Upload(UploadState::Failed | UploadState::Cancelled) => self
                    .transfers
                    .retry(snapshot.token)
                    .map_err(|error| CoreError::Backend(error.to_string()))?,
                TransferState::Upload(UploadState::Uploaded) => {
                    return Err(CoreError::Backend(
                        "media upload already completed without a pending send".into(),
                    ));
                }
                TransferState::Download(_) => {
                    return Err(CoreError::Backend(
                        "media send resolved to a download transfer".into(),
                    ));
                }
            },
        };

        let send_stage = Arc::new(MediaSendStage::default());
        let handle = spawn_media_send(
            self.client.clone(),
            peer,
            media.clone(),
            caption.clone(),
            client_id,
            chat_id,
            token,
            transfer_generation,
            session_generation,
            session_epoch,
            Arc::clone(&self.transfers),
            self.core_update_tx.clone(),
            Arc::clone(&self.dedup),
            Arc::clone(&send_stage),
        );
        self.media_send_tasks.insert(
            client_id,
            ActiveMediaSendTask {
                reference,
                transfer_token: token,
                transfer_generation,
                session_generation,
                session_epoch,
                chat_id,
                media,
                caption,
                send_stage,
                handle,
            },
        );
        Ok(Vec::new())
    }

    fn cancel_media_send(&mut self, client_id: i64) -> tg_mobile_core::Result<Vec<CoreEvent>> {
        let Some(active) = self.media_send_tasks.get(&client_id) else {
            return Ok(Vec::new());
        };
        if !active.send_stage.try_cancel() {
            return Ok(Vec::new());
        }
        let fallback_generation = active.transfer_generation.saturating_add(1);
        let cancelled_generation = self
            .transfers
            .get_by_key(TransferDirection::Upload, &active.reference)
            .ok()
            .flatten()
            .filter(|snapshot| snapshot.token == active.transfer_token)
            .and_then(|snapshot| self.transfers.cancel(snapshot.token).ok())
            .map_or(fallback_generation, |token| token.generation);
        let Some(active) = self.media_send_tasks.remove(&client_id) else {
            return Ok(Vec::new());
        };
        active.handle.abort();
        let update = OutgoingSend::failed_media(
            client_id,
            active.chat_id,
            active.media,
            active.caption,
            "Media upload cancelled",
            true,
            active
                .transfer_generation
                .saturating_add(1)
                .max(cancelled_generation),
        )
        .with_session_token(active.session_generation, active.session_epoch);
        Ok(vec![CoreEvent::MessageSendStateChanged(update)])
    }

    fn cleanup_media_tasks(&mut self) {
        self.media_tasks
            .retain(|_, task| !task.handle.is_finished());
    }

    fn hydrate_cached_media(&self, messages: &mut [MessageSummary]) {
        for message in messages {
            for attachment in &mut message.attachments {
                match self
                    .media_cache
                    .get_with_expected_size(&attachment.file_reference, attachment.size_bytes)
                {
                    Ok(Some(lease)) => {
                        attachment.download_generation = self
                            .transfers
                            .get_by_key(TransferDirection::Download, &attachment.file_reference)
                            .ok()
                            .flatten()
                            .map_or(0, |snapshot| snapshot.token.generation);
                        attachment.download_state = AttachmentDownloadState::Downloaded {
                            local_path: lease.path().to_path_buf(),
                        };
                    }
                    Ok(None)
                        if matches!(
                            attachment.download_state,
                            AttachmentDownloadState::Downloaded { .. }
                        ) =>
                    {
                        attachment.download_state = AttachmentDownloadState::NotDownloaded;
                    }
                    Ok(None) | Err(_) => {}
                }
            }
        }
    }

    async fn resolve_media_target(
        &mut self,
        chat_id: i64,
        message_id: i64,
        attachment_index: usize,
    ) -> tg_mobile_core::Result<(MediaTarget, Media)> {
        self.require_ready().await?;
        let peer = find_peer(&self.peers, chat_id)
            .await
            .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
        let telegram_message_id = i32::try_from(message_id)
            .map_err(|_| CoreError::InvalidInput("message id exceeds Telegram range".into()))?;
        let message = self
            .client
            .get_messages_by_id(peer, &[telegram_message_id])
            .await
            .map_err(|error| crate::error::api_error("load media message", error))?
            .into_iter()
            .next()
            .flatten()
            .ok_or_else(|| CoreError::InvalidInput("media message no longer exists".into()))?;
        let summary = message_to_summary(&message);
        let attachment = summary
            .attachments
            .get(attachment_index)
            .ok_or_else(|| CoreError::InvalidInput("attachment no longer exists".into()))?;
        let media = message
            .media()
            .ok_or_else(|| CoreError::InvalidInput("message has no downloadable media".into()))?;

        Ok((
            MediaTarget {
                chat_id,
                message_id,
                attachment_index,
                reference: attachment.file_reference.clone(),
                total_bytes: attachment.size_bytes,
            },
            media,
        ))
    }

    async fn handle_media_download(
        &mut self,
        chat_id: i64,
        message_id: i64,
        attachment_index: usize,
        retry: bool,
    ) -> tg_mobile_core::Result<Vec<CoreEvent>> {
        self.cleanup_media_tasks();
        let key = (chat_id, message_id, attachment_index);
        let (target, media) = self
            .resolve_media_target(chat_id, message_id, attachment_index)
            .await?;

        if retry {
            let _ = self.media_cache.remove(&target.reference);
        } else if let Some(lease) = self
            .media_cache
            .get_with_expected_size(&target.reference, target.total_bytes)
            .map_err(|error| CoreError::Backend(format!("media cache lookup failed: {error}")))?
        {
            let generation = self
                .transfers
                .get_by_key(TransferDirection::Download, &target.reference)
                .map_err(|error| CoreError::Backend(error.to_string()))?
                .map_or(0, |snapshot| snapshot.token.generation);
            return Ok(vec![media_event(
                &target,
                generation,
                AttachmentDownloadState::Downloaded {
                    local_path: lease.path().to_path_buf(),
                },
            )]);
        }

        if let Some(active) = self.media_tasks.get(&key) {
            if active.reference == target.reference && !active.handle.is_finished() {
                if let Some(snapshot) = self
                    .transfers
                    .get_by_key(TransferDirection::Download, &target.reference)
                    .map_err(|error| CoreError::Backend(error.to_string()))?
                {
                    return Ok(vec![media_event(
                        &target,
                        snapshot.token.generation,
                        state_from_snapshot(&snapshot, None),
                    )]);
                }
            }
        }

        if let Some(previous) = self.media_tasks.remove(&key) {
            previous.handle.abort();
            if let Some(snapshot) = self
                .transfers
                .get_by_key(TransferDirection::Download, &previous.reference)
                .map_err(|error| CoreError::Backend(error.to_string()))?
            {
                if !snapshot.state.is_terminal() {
                    let _ = self.transfers.cancel(snapshot.token);
                }
            }
        }

        let snapshot = match self
            .transfers
            .get_by_key(TransferDirection::Download, &target.reference)
            .map_err(|error| CoreError::Backend(error.to_string()))?
        {
            Some(snapshot) => snapshot,
            None => self
                .transfers
                .request(
                    TransferDirection::Download,
                    target.reference.clone(),
                    target.total_bytes,
                )
                .map_err(|error| CoreError::Backend(error.to_string()))?,
        };

        let queued_token = match snapshot.state {
            TransferState::Download(DownloadState::Remote) => self
                .transfers
                .queue_download(snapshot.token)
                .map_err(|error| CoreError::Backend(error.to_string()))?,
            TransferState::Download(DownloadState::Queued) => snapshot.token,
            TransferState::Download(DownloadState::Downloading) => {
                let failed = self
                    .transfers
                    .fail(snapshot.token)
                    .map_err(|error| CoreError::Backend(error.to_string()))?;
                if retry {
                    self.transfers
                        .retry(failed)
                        .map_err(|error| CoreError::Backend(error.to_string()))?
                } else {
                    return Ok(vec![media_event(
                        &target,
                        failed.generation,
                        AttachmentDownloadState::Failed {
                            reason: Some("previous download was interrupted".into()),
                        },
                    )]);
                }
            }
            TransferState::Download(DownloadState::Downloaded) => self
                .transfers
                .restart_download(snapshot.token)
                .map_err(|error| CoreError::Backend(error.to_string()))?,
            TransferState::Download(DownloadState::Failed | DownloadState::Cancelled) if retry => {
                self.transfers
                    .retry(snapshot.token)
                    .map_err(|error| CoreError::Backend(error.to_string()))?
            }
            TransferState::Download(DownloadState::Failed) => {
                return Ok(vec![media_event(
                    &target,
                    snapshot.token.generation,
                    AttachmentDownloadState::Failed { reason: None },
                )]);
            }
            TransferState::Download(DownloadState::Cancelled) => {
                return Ok(vec![media_event(
                    &target,
                    snapshot.token.generation,
                    AttachmentDownloadState::Cancelled,
                )]);
            }
            TransferState::Upload(_) => {
                return Err(CoreError::Backend(
                    "media download resolved to an upload transfer".into(),
                ));
            }
        };

        let handle = spawn_download(
            self.client.clone(),
            media,
            self.media_cache.clone(),
            Arc::clone(&self.transfers),
            self.core_update_tx.clone(),
            target.clone(),
            queued_token,
        );
        self.media_tasks.insert(
            key,
            ActiveMediaTask {
                reference: target.reference.clone(),
                handle,
            },
        );

        Ok(vec![media_event(
            &target,
            queued_token.generation,
            AttachmentDownloadState::Queued,
        )])
    }

    fn cancel_media_download(
        &mut self,
        chat_id: i64,
        message_id: i64,
        attachment_index: usize,
    ) -> tg_mobile_core::Result<Vec<CoreEvent>> {
        self.cleanup_media_tasks();
        let key = (chat_id, message_id, attachment_index);
        let Some(active) = self.media_tasks.remove(&key) else {
            return Ok(Vec::new());
        };
        active.handle.abort();

        let target = MediaTarget {
            chat_id,
            message_id,
            attachment_index,
            reference: active.reference.clone(),
            total_bytes: None,
        };
        let mut generation = 0;
        if let Some(snapshot) = self
            .transfers
            .get_by_key(TransferDirection::Download, &active.reference)
            .map_err(|error| CoreError::Backend(error.to_string()))?
        {
            generation = snapshot.token.generation;
            if !snapshot.state.is_terminal() {
                generation = self
                    .transfers
                    .cancel(snapshot.token)
                    .map_err(|error| CoreError::Backend(error.to_string()))?
                    .generation;
            }
        }

        Ok(vec![media_event(
            &target,
            generation,
            AttachmentDownloadState::Cancelled,
        )])
    }

    async fn logout(&mut self) -> tg_mobile_core::Result<Vec<CoreEvent>> {
        AuthFlow::logout(&self.client).await?;

        for (_, task) in self.media_tasks.drain() {
            task.handle.abort();
        }
        for (_, task) in self.media_send_tasks.drain() {
            task.handle.abort();
        }
        self.transfers = Arc::new(TransferManager::new());
        clear_peer_registry(&self.peers).await;
        clear_poll_registry(&self.polls).await;
        self.dedup.clear().await;
        self.reliable_sends.clear();
        self.reliable_send_order.clear();
        self.auth.close();

        Ok(self.authorization_events(AuthorizationState::Closed))
    }

    async fn shutdown(&mut self) -> tg_mobile_core::Result<Vec<CoreEvent>> {
        if self.stopped {
            return Ok(Vec::new());
        }

        for (_, task) in self.media_tasks.drain() {
            task.handle.abort();
        }
        for (_, task) in self.media_send_tasks.drain() {
            task.handle.abort();
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
        Ok(self.authorization_events(AuthorizationState::Closed))
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
                let state = with_initial_connection_timeout(self.auth.start(&self.client)).await?;
                if state == AuthorizationState::Ready {
                    self.ensure_update_worker().await?;
                }
                self.authorization_events(state)
            }
            CoreCommand::Logout => self.logout().await?,
            CoreCommand::SetPhoneNumber(phone) => {
                let previous_state = self.auth.state();
                match self
                    .auth
                    .set_phone_number(&self.client, &self.config.api_hash, phone)
                    .await
                {
                    Ok(state) => self.authorization_events(state),
                    Err(error) => match self.authorization_error_events(previous_state, &error) {
                        Some(events) => events,
                        None => return Err(error),
                    },
                }
            }
            CoreCommand::SubmitCode(code) => {
                let previous_state = self.auth.state();
                match self.auth.submit_code(&self.client, code).await {
                    Ok(state) => {
                        if state == AuthorizationState::Ready {
                            self.ensure_update_worker().await?;
                        }
                        self.authorization_events(state)
                    }
                    Err(error) => match self.authorization_error_events(previous_state, &error) {
                        Some(events) => events,
                        None => return Err(error),
                    },
                }
            }
            CoreCommand::SubmitPassword(password) => {
                let previous_state = self.auth.state();
                match self.auth.submit_password(&self.client, password).await {
                    Ok(state) => {
                        if state == AuthorizationState::Ready {
                            self.ensure_update_worker().await?;
                        }
                        self.authorization_events(state)
                    }
                    Err(error) => match self.authorization_error_events(previous_state, &error) {
                        Some(events) => events,
                        None => return Err(error),
                    },
                }
            }
            CoreCommand::LoadChats { limit } => {
                self.require_ready().await?;
                let (chats, capabilities) = load_chats(&self.client, &self.peers, limit).await?;
                let mut events = vec![CoreEvent::ChatsLoaded(chats)];
                events.extend(
                    capabilities
                        .into_iter()
                        .map(CoreEvent::ChatCapabilitiesUpdated),
                );
                events
            }
            CoreCommand::LoadChatsPage { cursor, limit } => {
                self.require_ready().await?;
                let (page, capabilities) = crate::chats::load_chats_page(
                    &self.client,
                    &self.peers,
                    cursor.as_deref(),
                    limit,
                )
                .await?;
                let mut events = vec![CoreEvent::ChatsPageLoaded(page)];
                events.extend(
                    capabilities
                        .into_iter()
                        .map(CoreEvent::ChatCapabilitiesUpdated),
                );
                events
            }
            CoreCommand::LoadProfile { peer_id } => {
                self.require_ready().await?;
                let peer_ref = find_peer(&self.peers, peer_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(peer_id))?;
                let peer = self
                    .client
                    .resolve_peer(peer_ref)
                    .await
                    .map_err(|error| crate::error::api_error("load profile", error))?;
                let mut profile = crate::profiles::profile_from_peer(peer_id, &peer);
                crate::profiles::hydrate_profile(
                    &self.client,
                    &peer,
                    &mut profile,
                    &self.media_cache,
                )
                .await;
                vec![CoreEvent::ProfileUpdated(profile)]
            }
            CoreCommand::LoadContacts { offset, limit } => {
                self.require_ready().await?;
                let (total, contacts) =
                    load_contacts(&self.client, &self.peers, offset, limit).await?;
                vec![CoreEvent::ContactsLoaded {
                    offset,
                    total,
                    contacts,
                }]
            }
            CoreCommand::ResolvePeer { input } => {
                self.require_ready().await?;
                let reference = tg_mobile_core::parse_peer_reference(&input)?;
                let resolution = self.resolve_reference(reference).await?;
                vec![CoreEvent::PeerResolved(resolution)]
            }
            CoreCommand::LoadMessages { chat_id, limit } => {
                self.require_ready().await?;
                let peer_ref = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let peer = self
                    .client
                    .resolve_peer(peer_ref)
                    .await
                    .map_err(|error| crate::error::api_error("load chat permissions", error))?;
                let mut capabilities = crate::capabilities::from_peer(&peer, chat_id);
                crate::capabilities::fill_slow_mode(&self.client, &peer, &mut capabilities).await;
                let mut loaded = load_messages(&self.client, peer_ref, limit).await?;
                self.hydrate_cached_media(&mut loaded.messages);
                for poll in &loaded.polls {
                    remember_poll(&self.polls, poll.clone()).await;
                }
                let mut events = vec![
                    CoreEvent::ChatCapabilitiesUpdated(capabilities),
                    CoreEvent::MessagesLoaded {
                        chat_id,
                        messages: loaded.messages,
                    },
                ];
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
                let mut page =
                    load_messages_page(&self.client, peer, before_message_id, limit).await?;
                self.hydrate_cached_media(&mut page.messages);
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
            CoreCommand::LoadPinnedMessages { chat_id } => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let message_ids = load_pinned_message_ids(&self.client, peer).await?;
                vec![CoreEvent::PinnedMessagesLoaded {
                    chat_id,
                    message_ids,
                    request_revision: None,
                }]
            }
            CoreCommand::PinMessage {
                chat_id,
                message_id,
            }
            | CoreCommand::UnpinMessage {
                chat_id,
                message_id,
            } => {
                self.require_ready().await?;
                let pinned = matches!(command, CoreCommand::PinMessage { .. });
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let message_id = i32::try_from(message_id).map_err(|_| {
                    CoreError::InvalidInput("message id exceeds Telegram range".into())
                })?;
                let result = if pinned {
                    self.client.pin_message(peer, message_id).await
                } else {
                    self.client.unpin_message(peer, message_id).await
                };
                result.map_err(|error| crate::error::api_error("update pinned message", error))?;
                vec![CoreEvent::PinnedMessagesChanged {
                    chat_id,
                    message_ids: vec![i64::from(message_id)],
                    pinned,
                    pts: None,
                }]
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
            CoreCommand::AddReaction {
                chat_id,
                message_id,
                reaction,
            } => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let message_id = i32::try_from(message_id).map_err(|_| {
                    CoreError::InvalidInput("message id exceeds Telegram range".into())
                })?;
                let input = match reaction {
                    tg_mobile_core::MessageReactionType::Emoji(value) => {
                        InputReactions::emoticon(value)
                    }
                    tg_mobile_core::MessageReactionType::CustomEmoji(document_id) => {
                        InputReactions::custom_emoji(document_id)
                    }
                };
                self.client
                    .send_reactions(peer, message_id, input)
                    .await
                    .map_err(|error| crate::error::api_error("set message reaction", error))?;
                vec![CoreEvent::ReactionOperationSucceeded {
                    chat_id,
                    message_id: i64::from(message_id),
                }]
            }
            CoreCommand::RemoveReaction {
                chat_id,
                message_id,
            } => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let message_id = i32::try_from(message_id).map_err(|_| {
                    CoreError::InvalidInput("message id exceeds Telegram range".into())
                })?;
                self.client
                    .send_reactions(peer, message_id, InputReactions::remove())
                    .await
                    .map_err(|error| crate::error::api_error("remove message reaction", error))?;
                vec![CoreEvent::ReactionOperationSucceeded {
                    chat_id,
                    message_id: i64::from(message_id),
                }]
            }
            CoreCommand::DeleteMessages {
                chat_id,
                message_ids,
            } => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let ids = message_ids
                    .iter()
                    .map(|id| {
                        i32::try_from(*id).map_err(|_| {
                            CoreError::InvalidInput("message id exceeds Telegram range".into())
                        })
                    })
                    .collect::<tg_mobile_core::Result<Vec<_>>>()?;
                self.client
                    .delete_messages(peer, &ids)
                    .await
                    .map_err(|error| crate::error::api_error("delete messages", error))?;
                vec![CoreEvent::MessagesDeleted {
                    chat_id: Some(chat_id),
                    message_ids,
                }]
            }
            CoreCommand::SearchMessages(request) => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, request.chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(request.chat_id))?;
                let mut page = search_messages_page(
                    &self.client,
                    peer,
                    &request.query,
                    request.before_message_id,
                    request.limit,
                )
                .await?;
                self.hydrate_cached_media(&mut page.messages);

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
            CoreCommand::ReplyMessage {
                chat_id,
                reply_to_message_id,
                text,
            } => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let message = send_reply(&self.client, peer, text, reply_to_message_id).await?;
                if self.dedup.should_emit(&message).await {
                    vec![CoreEvent::MessageAdded(message)]
                } else {
                    Vec::new()
                }
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
            CoreCommand::SendLocation { chat_id, location } => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let message = send_location(&self.client, peer, location).await?;
                if self.dedup.should_emit(&message).await {
                    vec![CoreEvent::MessageAdded(message)]
                } else {
                    Vec::new()
                }
            }
            CoreCommand::EditMessage {
                chat_id,
                message_id,
                text,
            } => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let telegram_message_id = i32::try_from(message_id).map_err(|_| {
                    CoreError::InvalidInput("message id exceeds Telegram range".into())
                })?;
                let _target = self
                    .client
                    .get_messages_by_id(peer.clone(), &[telegram_message_id])
                    .await
                    .map_err(|error| crate::error::api_error("load message for editing", error))?
                    .into_iter()
                    .next()
                    .flatten()
                    .ok_or_else(|| {
                        CoreError::InvalidInput("message no longer exists in this chat".into())
                    })?;
                self.client
                    .edit_message(peer.clone(), telegram_message_id, text.as_str())
                    .await
                    .map_err(|error| crate::error::api_error("edit message", error))?;
                let updated = self
                    .client
                    .get_messages_by_id(peer, &[telegram_message_id])
                    .await
                    .map_err(|error| crate::error::api_error("confirm edited message", error))?
                    .into_iter()
                    .next()
                    .flatten()
                    .ok_or_else(|| {
                        CoreError::Backend(
                            "Telegram accepted the edit but returned no updated message".into(),
                        )
                    })?;
                vec![
                    CoreEvent::MessageEdited(message_to_summary(&updated)),
                    CoreEvent::MessageMutationSucceeded {
                        chat_id,
                        message_id,
                        action: tg_mobile_core::MessageMutationAction::Edit,
                    },
                ]
            }
            CoreCommand::DeleteMessageForMe {
                chat_id,
                message_id,
            } => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                if peer.id.kind() == PeerKind::Channel {
                    return Err(CoreError::InvalidInput(
                        "delete for this account is unsupported in channels".into(),
                    ));
                }
                let telegram_message_id = i32::try_from(message_id).map_err(|_| {
                    CoreError::InvalidInput("message id exceeds Telegram range".into())
                })?;
                let _target = self
                    .client
                    .get_messages_by_id(peer.clone(), &[telegram_message_id])
                    .await
                    .map_err(|error| {
                        crate::error::api_error("load message before deletion", error)
                    })?
                    .into_iter()
                    .next()
                    .flatten()
                    .ok_or_else(|| {
                        CoreError::InvalidInput("message no longer exists in this chat".into())
                    })?;
                let _: tl::enums::messages::AffectedMessages = self
                    .client
                    .invoke(&tl::functions::messages::DeleteMessages {
                        revoke: false,
                        id: vec![telegram_message_id],
                    })
                    .await
                    .map_err(|error| {
                        crate::error::api_error("delete message for this account", error)
                    })?;
                vec![
                    CoreEvent::MessagesDeleted {
                        chat_id: Some(chat_id),
                        message_ids: vec![message_id],
                    },
                    CoreEvent::MessageMutationSucceeded {
                        chat_id,
                        message_id,
                        action: tg_mobile_core::MessageMutationAction::DeleteForMe,
                    },
                ]
            }
            CoreCommand::DeleteMessageForEveryone {
                chat_id,
                message_id,
            } => {
                self.require_ready().await?;
                let peer = find_peer(&self.peers, chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(chat_id))?;
                let telegram_message_id = i32::try_from(message_id).map_err(|_| {
                    CoreError::InvalidInput("message id exceeds Telegram range".into())
                })?;
                let _target = self
                    .client
                    .get_messages_by_id(peer.clone(), &[telegram_message_id])
                    .await
                    .map_err(|error| {
                        crate::error::api_error("load message before deletion", error)
                    })?
                    .into_iter()
                    .next()
                    .flatten()
                    .ok_or_else(|| {
                        CoreError::InvalidInput("message no longer exists in this chat".into())
                    })?;
                self.client
                    .delete_messages(peer, &[telegram_message_id])
                    .await
                    .map_err(|error| {
                        crate::error::api_error("delete message for everyone", error)
                    })?;
                vec![
                    CoreEvent::MessagesDeleted {
                        chat_id: Some(chat_id),
                        message_ids: vec![message_id],
                    },
                    CoreEvent::MessageMutationSucceeded {
                        chat_id,
                        message_id,
                        action: tg_mobile_core::MessageMutationAction::DeleteForEveryone,
                    },
                ]
            }
            CoreCommand::SendTextReliable {
                client_id,
                chat_id,
                reply_to_message_id,
                text,
            } => {
                self.handle_reliable_send(client_id, chat_id, text, reply_to_message_id)
                    .await
            }
            CoreCommand::SendMediaReliable {
                client_id,
                chat_id,
                media,
                caption,
                transfer_generation,
                session_generation,
                session_epoch,
            } => {
                self.handle_media_send(
                    client_id,
                    chat_id,
                    media,
                    caption,
                    transfer_generation,
                    session_generation,
                    session_epoch,
                )
                .await?
            }
            CoreCommand::CancelMediaSend { client_id } => self.cancel_media_send(client_id)?,
            CoreCommand::DownloadMedia {
                chat_id,
                message_id,
                attachment_index,
            } => {
                self.handle_media_download(chat_id, message_id, attachment_index, false)
                    .await?
            }
            CoreCommand::RetryMediaDownload {
                chat_id,
                message_id,
                attachment_index,
            } => {
                self.handle_media_download(chat_id, message_id, attachment_index, true)
                    .await?
            }
            CoreCommand::CancelMediaDownload {
                chat_id,
                message_id,
                attachment_index,
            } => self.cancel_media_download(chat_id, message_id, attachment_index)?,
            CoreCommand::ForwardMessages(request) => {
                self.require_ready().await?;
                let source = find_peer(&self.peers, request.source_chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(request.source_chat_id))?;
                let destination = find_peer(&self.peers, request.destination_chat_id)
                    .await
                    .ok_or(TelegramBackendError::UnknownChat(
                        request.destination_chat_id,
                    ))?;
                let ids = request
                    .message_ids
                    .iter()
                    .map(|id| *id as i32)
                    .collect::<Vec<_>>();
                match self
                    .client
                    .forward_messages(destination, &ids, source)
                    .await
                {
                    Ok(forwarded) => {
                        let mut events = Vec::with_capacity(forwarded.len() + 1);
                        let mut outcomes = Vec::with_capacity(request.message_ids.len());
                        for (source_message_id, message) in
                            request.message_ids.iter().zip(forwarded)
                        {
                            match message {
                                Some(message) => {
                                    let summary = crate::messages::message_to_summary(&message);
                                    if summary.chat_id != request.destination_chat_id {
                                        outcomes.push(ForwardOutcome::Failed {
                                            source_message_id: *source_message_id,
                                            error: "Telegram returned a forwarded message for a different destination".into(),
                                        });
                                    } else {
                                        if self.dedup.should_emit(&summary).await {
                                            events.push(CoreEvent::MessageAdded(summary.clone()));
                                        }
                                        outcomes.push(ForwardOutcome::Forwarded {
                                            source_message_id: *source_message_id,
                                            destination_message_id: summary.id,
                                        });
                                    }
                                }
                                None => outcomes.push(ForwardOutcome::Failed {
                                    source_message_id: *source_message_id,
                                    error: "Telegram did not forward this message".into(),
                                }),
                            }
                        }
                        events.push(CoreEvent::MessagesForwarded(ForwardMessagesResult {
                            request_id: request.request_id,
                            source_chat_id: request.source_chat_id,
                            destination_chat_id: request.destination_chat_id,
                            outcomes,
                        }));
                        events
                    }
                    Err(error) => vec![CoreEvent::MessagesForwarded(ForwardMessagesResult {
                        request_id: request.request_id,
                        source_chat_id: request.source_chat_id,
                        destination_chat_id: request.destination_chat_id,
                        outcomes: request
                            .message_ids
                            .iter()
                            .map(|id| ForwardOutcome::Failed {
                                source_message_id: *id,
                                error: error.to_string(),
                            })
                            .collect(),
                    })],
                }
            }
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
                let mut events = vec![CoreEvent::PollVoteSubmitted { poll_id }];
                events.extend(polls.into_iter().map(CoreEvent::PollUpdated));
                events
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
    reply_to_message_id: Option<i64>,
    error: CoreError,
) -> OutgoingSend {
    let send = match error {
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
    };
    send.with_reply_target(reply_to_message_id)
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
            None,
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
            None,
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
            None,
            CoreError::SendOutcomeUnknown("connection closed".into()),
        );
        assert!(matches!(
            ambiguous.state,
            OutgoingSendState::AwaitingConfirmation { .. }
        ));
    }
}

#[cfg(test)]
mod initial_connection_timeout_tests {
    use std::future::pending;

    use tg_mobile_core::CoreError;

    use super::{INITIAL_AUTHORIZATION_CHECK_TIMEOUT, with_initial_connection_timeout};

    #[tokio::test(start_paused = true)]
    async fn stalled_initial_authorization_check_returns_distinct_retryable_error() {
        let check = with_initial_connection_timeout::<()>(pending());
        tokio::pin!(check);
        tokio::time::advance(INITIAL_AUTHORIZATION_CHECK_TIMEOUT).await;

        assert!(matches!(
            check.await,
            Err(CoreError::TransientTransport(message))
                if message.contains("initial connection timed out")
        ));
    }

    #[tokio::test]
    async fn completed_initial_authorization_check_preserves_its_result() {
        let result = with_initial_connection_timeout(async { Ok(true) }).await;
        assert!(matches!(result, Ok(true)));
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
        for (_, task) in self.media_tasks.drain() {
            task.handle.abort();
        }
        for (_, task) in self.media_send_tasks.drain() {
            task.handle.abort();
        }
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
