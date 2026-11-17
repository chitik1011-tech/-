use std::{collections::HashMap, sync::Arc};

use grammers_client::{Client, session::types::PeerRef, tl};
use tg_mobile_core::{ChatCapabilities, ChatPage, ChatSummary};
use tokio::sync::RwLock;

use crate::{
    capabilities::from_peer, error::api_error, ids::peer_id_to_core, messages::message_to_summary,
};

pub(crate) type PeerRegistry = Arc<RwLock<HashMap<i64, PeerRef>>>;

pub(crate) fn new_peer_registry() -> PeerRegistry {
    Arc::new(RwLock::new(HashMap::new()))
}

pub(crate) async fn remember_peer(registry: &PeerRegistry, chat_id: i64, peer: PeerRef) {
    registry.write().await.insert(chat_id, peer);
}

pub(crate) async fn find_peer(registry: &PeerRegistry, chat_id: i64) -> Option<PeerRef> {
    registry.read().await.get(&chat_id).cloned()
}

pub(crate) async fn clear_peer_registry(registry: &PeerRegistry) {
    registry.write().await.clear();
}

pub(crate) async fn load_chats(
    client: &Client,
    registry: &PeerRegistry,
    limit: usize,
) -> tg_mobile_core::Result<(Vec<ChatSummary>, Vec<ChatCapabilities>)> {
    if limit == 0 {
        return Ok((Vec::new(), Vec::new()));
    }

    let mut dialogs = client.iter_dialogs().limit(limit);
    let mut result = Vec::with_capacity(limit.min(100));
    let mut capabilities = Vec::with_capacity(limit.min(100));

    while let Some(dialog) = dialogs
        .next()
        .await
        .map_err(|error| api_error("load chats", error))?
    {
        let chat_id = peer_id_to_core(dialog.peer_id());
        remember_peer(registry, chat_id, dialog.peer_ref()).await;
        capabilities.push(from_peer(dialog.peer(), chat_id));

        result.push(chat_summary(&dialog, chat_id));
    }

    Ok((result, capabilities))
}

/// Loads a cursor page using Grammers' real `messages.getDialogs` iterator.
/// Grammers 0.12 does not expose the iterator's internal MTProto offsets, so the
/// opaque cursor anchors the page after the last returned peer. The iterator
/// advances through real Telegram dialog results until it finds that peer.
pub(crate) async fn load_chats_page(
    client: &Client,
    registry: &PeerRegistry,
    cursor: Option<&str>,
    limit: usize,
) -> tg_mobile_core::Result<(ChatPage, Vec<ChatCapabilities>)> {
    let anchor = cursor
        .map(|value| value.parse::<i64>())
        .transpose()
        .map_err(|_| {
            crate::error::TelegramBackendError::InvalidInput("invalid chat cursor".into())
        })?;
    if anchor == Some(0) {
        return Err(
            crate::error::TelegramBackendError::InvalidInput("invalid chat cursor".into()).into(),
        );
    }

    const MAX_DIALOG_SCAN: usize = 100_000;
    let mut dialogs = client.iter_dialogs().limit(MAX_DIALOG_SCAN);
    let mut chats = Vec::with_capacity(limit);
    let mut capabilities = Vec::with_capacity(limit);
    let mut anchor_found = anchor.is_none();
    let mut has_more = false;
    while let Some(dialog) = dialogs
        .next()
        .await
        .map_err(|error| api_error("load chat page", error))?
    {
        let chat_id = peer_id_to_core(dialog.peer_id());
        if !anchor_found {
            if Some(chat_id) == anchor {
                anchor_found = true;
            }
            continue;
        }
        if chats.len() == limit {
            has_more = true;
            break;
        }
        remember_peer(registry, chat_id, dialog.peer_ref()).await;
        capabilities.push(from_peer(dialog.peer(), chat_id));
        chats.push(chat_summary(&dialog, chat_id));
    }

    if !anchor_found {
        return Err(crate::error::TelegramBackendError::InvalidInput(
            "chat page cursor is stale; reload the dialog list".into(),
        )
        .into());
    }
    let next_cursor = has_more.then(|| {
        chats
            .last()
            .map_or_else(String::new, |chat| chat.id.to_string())
    });
    Ok((
        ChatPage {
            requested_cursor: cursor.map(str::to_owned),
            chats,
            next_cursor,
        },
        capabilities,
    ))
}

fn chat_summary(dialog: &grammers_client::peer::Dialog, chat_id: i64) -> ChatSummary {
    ChatSummary {
        id: chat_id,
        title: dialog.peer().name().unwrap_or_default().to_owned(),
        unread_count: unread_count(&dialog.raw),
        unread_mentions_count: unread_mentions_count(&dialog.raw),
        unread_reactions_count: unread_reactions_count(&dialog.raw),
        archived: None,
        muted: None,
        pinned: None,
        is_unread: None,
        last_message: dialog.last_message.as_ref().map(message_to_summary),
    }
}

fn unread_mentions_count(dialog: &tl::enums::Dialog) -> Option<u32> {
    match dialog {
        tl::enums::Dialog::Dialog(dialog) => Some(normalize_unread(dialog.unread_mentions_count)),
        tl::enums::Dialog::Folder(_) | tl::enums::Dialog::Community(_) => None,
    }
}

fn unread_reactions_count(dialog: &tl::enums::Dialog) -> Option<u32> {
    match dialog {
        tl::enums::Dialog::Dialog(dialog) => Some(normalize_unread(dialog.unread_reactions_count)),
        tl::enums::Dialog::Folder(_) | tl::enums::Dialog::Community(_) => None,
    }
}

fn unread_count(dialog: &tl::enums::Dialog) -> u32 {
    let raw = match dialog {
        tl::enums::Dialog::Dialog(dialog) => dialog.unread_count,
        tl::enums::Dialog::Folder(_) | tl::enums::Dialog::Community(_) => 0,
    };
    normalize_unread(raw)
}

fn normalize_unread(value: i32) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::normalize_unread;

    #[test]
    fn unread_count_never_wraps_negative_values() {
        assert_eq!(normalize_unread(15), 15);
        assert_eq!(normalize_unread(-1), 0);
    }
}
