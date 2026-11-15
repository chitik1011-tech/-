use std::sync::Arc;

use grammers_client::{
    Client,
    sender::{SenderPool, SenderPoolHandle},
    session::{storages::SqliteSession, updates::UpdatesLike},
};
use tg_mobile_core::{
    AuthorizationState, BackendFuture, BackendUpdateReceiver, BackendUpdateSender, CoreCommand,
    CoreError, CoreEvent, TelegramBackend, backend_update_channel,
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
    messages::{MessageDeduplicator, send_text},
    updates::{TelegramUpdate, UpdateWorkerInput, run_update_worker},
};

const MESSAGE_DEDUP_CAPACITY: usize = 4096;

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
    auth: AuthFlow,
    peers: PeerRegistry,
    dedup: Arc<MessageDeduplicator>,
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
            auth: AuthFlow::default(),
            peers: new_peer_registry(),
            dedup: Arc::new(MessageDeduplicator::new(MESSAGE_DEDUP_CAPACITY)),
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
            events: self.update_tx.clone(),
            core_updates: self
                .core_update_rx
                .is_none()
                .then(|| self.core_update_tx.clone()),
            shutdown: shutdown_rx,
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
            CoreCommand::Shutdown => self.shutdown().await?,
        };

        Ok(command_events)
    }
}

impl TelegramBackend for GrammersBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async move { self.handle_command(command).await })
    }

    fn take_updates(&mut self) -> Option<BackendUpdateReceiver> {
        self.core_update_rx.take()
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
