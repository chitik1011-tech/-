use std::collections::{HashSet, VecDeque};

use grammers_client::{
    message::Message,
    session::types::{PeerKind, PeerRef},
    tl, Client,
};
use tg_mobile_core::{CoreError, MessageSummary};
use tokio::sync::Mutex;

use crate::{error::api_error, ids::peer_id_to_core};

pub(crate) fn message_to_summary(message: &Message) -> MessageSummary {
    message_summary_from_fields(MessageFields {
        id: i64::from(message.id()),
        chat_id: peer_id_to_core(message.peer_id()),
        sender_id: message.sender_id().map(peer_id_to_core).unwrap_or(0),
        text: message.text(),
        timestamp_unix: message.date().as_second(),
        outgoing: message.outgoing(),
    })
}

struct MessageFields<'a> {
    id: i64,
    chat_id: i64,
    sender_id: i64,
    text: &'a str,
    timestamp_unix: i64,
    outgoing: bool,
}

fn message_summary_from_fields(fields: MessageFields<'_>) -> MessageSummary {
    MessageSummary {
        id: fields.id,
        chat_id: fields.chat_id,
        sender_id: fields.sender_id,
        text: fields.text.to_owned(),
        timestamp_unix: fields.timestamp_unix,
        outgoing: fields.outgoing,
    }
}

pub(crate) async fn load_messages(
    client: &Client,
    peer: PeerRef,
    limit: usize,
) -> tg_mobile_core::Result<Vec<MessageSummary>> {
    let mut iterator = client.iter_messages(peer).limit(limit);
    let mut messages = Vec::with_capacity(limit.min(100));

    while let Some(message) = iterator
        .next()
        .await
        .map_err(|error| api_error("load messages", error))?
    {
        messages.push(message_to_summary(&message));
    }

    messages.reverse();
    Ok(messages)
}

pub(crate) async fn mark_read(
    client: &Client,
    peer: PeerRef,
    max_message_id: i64,
) -> tg_mobile_core::Result<()> {
    let max_id = i32::try_from(max_message_id)
        .map_err(|_| CoreError::InvalidInput("max message id exceeds Telegram range".into()))?;

    match peer.id.kind() {
        PeerKind::Channel => {
            let channel: tl::enums::InputChannel = peer.into();
            client
                .invoke(&tl::functions::channels::ReadHistory { channel, max_id })
                .await
                .map_err(|error| api_error("mark channel read", error))?;
        }
        PeerKind::User | PeerKind::Chat => {
            let peer: tl::enums::InputPeer = peer.into();
            client
                .invoke(&tl::functions::messages::ReadHistory { peer, max_id })
                .await
                .map_err(|error| api_error("mark chat read", error))?;
        }
    }

    Ok(())
}

pub(crate) async fn send_text(
    client: &Client,
    peer: PeerRef,
    text: String,
) -> tg_mobile_core::Result<MessageSummary> {
    if text.is_empty() {
        return Err(CoreError::InvalidInput(
            "message text must not be empty".into(),
        ));
    }

    let message = client
        .send_message(peer, text)
        .await
        .map_err(|error| api_error("send message", error))?;
    Ok(message_to_summary(&message))
}

#[derive(Debug)]
struct RecentMessages {
    capacity: usize,
    order: VecDeque<(i64, i64)>,
    keys: HashSet<(i64, i64)>,
}

impl RecentMessages {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            order: VecDeque::with_capacity(capacity.max(1)),
            keys: HashSet::with_capacity(capacity.max(1)),
        }
    }

    fn insert_if_new(&mut self, key: (i64, i64)) -> bool {
        if !self.keys.insert(key) {
            return false;
        }

        self.order.push_back(key);
        if self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.keys.remove(&oldest);
            }
        }
        true
    }
}

#[derive(Debug)]
pub(crate) struct MessageDeduplicator {
    recent: Mutex<RecentMessages>,
}

impl MessageDeduplicator {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            recent: Mutex::new(RecentMessages::new(capacity)),
        }
    }

    pub(crate) async fn should_emit(&self, message: &MessageSummary) -> bool {
        self.recent
            .lock()
            .await
            .insert_if_new((message.chat_id, message.id))
    }
}

#[cfg(test)]
mod tests {
    use super::{MessageDeduplicator, MessageFields, message_summary_from_fields};

    #[test]
    fn maps_message_fields_without_losing_identifiers() {
        let summary = message_summary_from_fields(MessageFields {
            id: 77,
            chat_id: -1_000_000_000_123,
            sender_id: 12,
            text: "hello",
            timestamp_unix: 1_700_000_000,
            outgoing: true,
        });

        assert_eq!(summary.id, 77);
        assert_eq!(summary.chat_id, -1_000_000_000_123);
        assert_eq!(summary.sender_id, 12);
        assert_eq!(summary.text, "hello");
        assert_eq!(summary.timestamp_unix, 1_700_000_000);
        assert!(summary.outgoing);
    }

    #[tokio::test]
    async fn deduplicates_by_chat_and_message_id() {
        let dedup = MessageDeduplicator::new(8);
        let first = message_summary_from_fields(MessageFields {
            id: 10,
            chat_id: 1,
            sender_id: 1,
            text: "one",
            timestamp_unix: 0,
            outgoing: false,
        });
        let same_id_other_chat = message_summary_from_fields(MessageFields {
            id: 10,
            chat_id: 2,
            sender_id: 1,
            text: "two",
            timestamp_unix: 0,
            outgoing: false,
        });

        assert!(dedup.should_emit(&first).await);
        assert!(!dedup.should_emit(&first).await);
        assert!(dedup.should_emit(&same_id_other_chat).await);
    }
}
