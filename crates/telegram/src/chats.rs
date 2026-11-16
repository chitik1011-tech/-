use std::{collections::HashMap, sync::Arc};

use grammers_client::{Client, session::types::{PeerKind, PeerRef}, tl};
use tg_mobile_core::model::ChatKind;
use tg_mobile_core::{ChatCapabilities, ChatSummary};
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

        let kind = match dialog.peer_id().kind() {
            PeerKind::User => ChatKind::Private,
            PeerKind::Chat => ChatKind::Group,
            PeerKind::Channel => ChatKind::Channel,
        };
        result.push(ChatSummary {
            kind: Some(kind),
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

    Ok((result, capabilities))
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
