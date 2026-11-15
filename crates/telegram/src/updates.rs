use std::sync::Arc;

use grammers_client::{Client, session::updates::UpdatesLike, update::Update};
use tg_mobile_core::{BackendUpdateSender, CoreError, CoreEvent, MessageSummary};
use tokio::sync::{broadcast, mpsc, watch};

use crate::{
    chats::{PeerRegistry, remember_peer},
    ids::channel_id_to_core,
    messages::{MessageDeduplicator, message_to_summary},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Ready,
    Disconnected,
}

/// Backend-native update model.
///
/// It is intentionally richer than `CoreEvent`; the current core API only has
/// `MessageAdded`, but this stream already preserves edits/deletes and leaves
/// explicit extension points for chat/unread/typing/connection changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramUpdate {
    MessageAdded(MessageSummary),
    MessageEdited(MessageSummary),
    MessagesDeleted {
        chat_id: Option<i64>,
        message_ids: Vec<i64>,
    },
    ChatChanged {
        chat_id: i64,
    },
    UnreadCountChanged {
        chat_id: i64,
        unread_count: u32,
    },
    Typing {
        chat_id: i64,
        sender_id: i64,
        active: bool,
    },
    ConnectionStateChanged(ConnectionState),
    Error(String),
    Unhandled,
}

impl TelegramUpdate {
    pub(crate) fn to_core_event(&self) -> Option<CoreEvent> {
        match self {
            Self::MessageAdded(message) => Some(CoreEvent::MessageAdded(message.clone())),
            Self::Error(message) => Some(CoreEvent::Error(message.clone())),
            _ => None,
        }
    }
}

pub(crate) struct UpdateWorkerInput {
    pub client: Client,
    pub network_updates: mpsc::Receiver<UpdatesLike>,
    pub catch_up: bool,
    pub peers: PeerRegistry,
    pub dedup: Arc<MessageDeduplicator>,
    pub events: broadcast::Sender<TelegramUpdate>,
    pub core_updates: Option<BackendUpdateSender>,
    pub shutdown: watch::Receiver<bool>,
}

pub(crate) async fn run_update_worker(mut input: UpdateWorkerInput) {
    let _ = input.events.send(TelegramUpdate::ConnectionStateChanged(
        ConnectionState::Connecting,
    ));

    let _catch_up = input.catch_up;

    let mut stream = match input
        .client
        .stream_updates(input.network_updates, ())
        .await
    {
        Ok(stream) => stream,
        Err(error) => {
            emit_error(
                &input.events,
                &input.core_updates,
                format!("create Telegram update stream: {error}"),
            )
            .await;
            let _ = input.events.send(TelegramUpdate::ConnectionStateChanged(
                ConnectionState::Disconnected,
            ));
            return;
        }
    };

    let _ = input.events.send(TelegramUpdate::ConnectionStateChanged(
        ConnectionState::Ready,
    ));

    loop {
        tokio::select! {
            changed = input.shutdown.changed() => {
                if changed.is_err() || *input.shutdown.borrow() {
                    break;
                }
            }
            update = stream.next() => {
                match update {
                    Ok(update) => {
                        if let Some(event) = map_update(update, &input.peers, &input.dedup).await {
                            emit_update(&input.events, &input.core_updates, event).await;
                        }
                    }
                    Err(error) => {
                        emit_error(
                            &input.events,
                            &input.core_updates,
                            format!("Telegram update stream: {error}"),
                        )
                        .await;
                        break;
                    }
                }
            }
        }
    }

    if let Err(error) = stream.sync_update_state().await {
        emit_error(
            &input.events,
            &input.core_updates,
            format!("persist Telegram update state: {error}"),
        )
        .await;
    }
    let _ = input.events.send(TelegramUpdate::ConnectionStateChanged(
        ConnectionState::Disconnected,
    ));
}

async fn emit_update(
    native: &broadcast::Sender<TelegramUpdate>,
    core: &Option<BackendUpdateSender>,
    update: TelegramUpdate,
) {
    let core_event = update.to_core_event();
    let _ = native.send(update);
    if let (Some(core), Some(event)) = (core.as_ref(), core_event) {
        let _ = core.send(Ok(event)).await;
    }
}

async fn emit_error(
    native: &broadcast::Sender<TelegramUpdate>,
    core: &Option<BackendUpdateSender>,
    message: String,
) {
    let _ = native.send(TelegramUpdate::Error(message.clone()));
    if let Some(core) = core.as_ref() {
        let _ = core.send(Err(CoreError::Backend(message))).await;
    }
}

async fn map_update(
    update: Update,
    peers: &PeerRegistry,
    dedup: &MessageDeduplicator,
) -> Option<TelegramUpdate> {
    match update {
        Update::NewMessage(message) => {
            let summary = message_to_summary(&message);
            if let Ok(Some(peer)) = message.peer_ref().await {
                remember_peer(peers, summary.chat_id, peer).await;
            }
            dedup
                .should_emit(&summary)
                .await
                .then_some(TelegramUpdate::MessageAdded(summary))
        }
        Update::MessageEdited(message) => {
            let summary = message_to_summary(&message);
            if let Ok(Some(peer)) = message.peer_ref().await {
                remember_peer(peers, summary.chat_id, peer).await;
            }
            Some(TelegramUpdate::MessageEdited(summary))
        }
        Update::MessageDeleted(deletion) => Some(TelegramUpdate::MessagesDeleted {
            chat_id: deletion.channel_id().map(channel_id_to_core),
            message_ids: deletion.messages().iter().copied().map(i64::from).collect(),
        }),
        Update::Raw(_) => Some(TelegramUpdate::Unhandled),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::TelegramUpdate;
    use tg_mobile_core::{CoreEvent, MessageSummary};

    #[test]
    fn only_supported_update_types_cross_current_core_boundary() {
        let message = MessageSummary {
            id: 1,
            chat_id: 2,
            sender_id: 3,
            text: "hello".into(),
            timestamp_unix: 4,
            outgoing: false,
        };
        assert!(matches!(
            TelegramUpdate::MessageAdded(message).to_core_event(),
            Some(CoreEvent::MessageAdded(_))
        ));
        assert!(TelegramUpdate::Unhandled.to_core_event().is_none());
    }
}
