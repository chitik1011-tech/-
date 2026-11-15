use std::{collections::HashMap, sync::Arc};

use grammers_client::{Client, session::types::PeerRef, tl};
use tg_mobile_core::ChatSummary;
use tokio::sync::RwLock;

use crate::{error::api_error, ids::peer_id_to_core, messages::message_to_summary};

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

pub(crate) async fn load_chats(
    client: &Client,
    registry: &PeerRegistry,
    limit: usize,
) -> tg_mobile_core::Result<Vec<ChatSummary>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let mut dialogs = client.iter_dialogs().limit(limit);
    let mut result = Vec::with_capacity(limit.min(100));

    while let Some(dialog) = dialogs
        .next()
        .await
        .map_err(|error| api_error("load chats", error))?
    {
        let chat_id = peer_id_to_core(dialog.peer_id());
        remember_peer(registry, chat_id, dialog.peer_ref()).await;

        result.push(ChatSummary {
            id: chat_id,
            title: dialog.peer().name().unwrap_or_default().to_owned(),
            unread_count: unread_count(&dialog.raw),
            archived: None,
            muted: None,
            pinned: None,
            is_unread: None,
            last_message: dialog.last_message.as_ref().map(message_to_summary),
        });
    }

    Ok(result)
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
