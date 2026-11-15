use std::collections::{HashSet, VecDeque};

use grammers_client::{
    Client,
    message::Message,
    session::types::{PeerKind, PeerRef},
    tl,
};
use tg_mobile_core::{CoreError, ForwardedFrom, MessageSummary, ReplyMetadata};
use tokio::sync::Mutex;

use crate::{error::api_error, ids::peer_id_to_core};

pub(crate) fn message_to_summary(message: &Message) -> MessageSummary {
    let chat_id = peer_id_to_core(message.peer_id());
    let text = message.text();
    let edit_timestamp_unix = message.edit_date().map(|date| date.as_second());

    let mut summary = message_summary_from_fields(MessageFields {
        id: i64::from(message.id()),
        chat_id,
        sender_id: message.sender_id().map(peer_id_to_core).unwrap_or(0),
        text,
        timestamp_unix: message.date().as_second(),
        outgoing: message.outgoing(),
    });

    summary.forwarded_from = message
        .forward_header()
        .is_some()
        .then(ForwardedFrom::default);
    summary.edited = edit_timestamp_unix.is_some();
    summary.edit_timestamp_unix = edit_timestamp_unix;
    summary.service_message = message.action().is_some();
    summary.media_caption =
        (message.media().is_some() && !text.is_empty()).then(|| text.to_owned());
    summary.reply_to = message
        .reply_to_message_id()
        .map(|message_id| ReplyMetadata {
            message_id: i64::from(message_id),
            chat_id: Some(chat_id),
            sender_id: None,
            sender_display_name: None,
        });

    summary
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
        attachments: Vec::new(),
        forwarded_from: None,
        edited: false,
        edit_timestamp_unix: None,
        service_message: false,
        media_caption: None,
        reply_to: None,
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

pub(crate) async fn send_text_idempotent(
    client: &Client,
    peer: PeerRef,
    text: String,
    random_id: i64,
) -> tg_mobile_core::Result<MessageSummary> {
    if text.is_empty() {
        return Err(CoreError::InvalidInput(
            "message text must not be empty".into(),
        ));
    }
    if random_id == 0 {
        return Err(CoreError::InvalidInput(
            "client message id must be non-zero".into(),
        ));
    }

    let updates = client
        .invoke(&tl::functions::messages::SendMessage {
            no_webpage: false,
            silent: false,
            background: false,
            clear_draft: false,
            peer: peer.into(),
            reply_to: None,
            message: text,
            random_id,
            reply_markup: None,
            entities: None,
            schedule_date: None,
            schedule_repeat_period: None,
            send_as: None,
            noforwards: false,
            update_stickersets_order: false,
            invert_media: false,
            quick_reply_shortcut: None,
            effect: None,
            allow_paid_floodskip: false,
            allow_paid_stars: None,
            suggested_post: None,
            rich_message: None,
        })
        .await
        .map_err(|error| api_error("send idempotent message", error))?;

    let message_id = sent_message_id(&updates, random_id).ok_or_else(|| {
        CoreError::Backend(
            "Telegram accepted send request but returned no resolvable message id".into(),
        )
    })?;

    let mut messages = client
        .get_messages_by_id(peer, &[message_id])
        .await
        .map_err(|error| api_error("fetch idempotently sent message", error))?;

    let message = messages.pop().flatten().ok_or_else(|| {
        CoreError::Backend(format!(
            "Telegram accepted send request but message {message_id} could not be fetched"
        ))
    })?;

    Ok(message_to_summary(&message))
}

fn sent_message_id(updates: &tl::enums::Updates, random_id: i64) -> Option<i32> {
    match updates {
        tl::enums::Updates::UpdateShortSentMessage(update) => Some(update.id),
        tl::enums::Updates::Updates(updates) => {
            sent_message_id_from_updates(&updates.updates, random_id)
        }
        tl::enums::Updates::Combined(updates) => {
            sent_message_id_from_updates(&updates.updates, random_id)
        }
        tl::enums::Updates::UpdateShort(update) => {
            sent_message_id_from_update(&update.update, random_id)
        }
        tl::enums::Updates::UpdateShortMessage(update) => Some(update.id),
        tl::enums::Updates::UpdateShortChatMessage(update) => Some(update.id),
        tl::enums::Updates::TooLong => None,
    }
}

fn sent_message_id_from_updates(
    updates: &[tl::enums::Update],
    random_id: i64,
) -> Option<i32> {
    if let Some(message_id) = updates.iter().find_map(|update| match update {
        tl::enums::Update::MessageId(update) if update.random_id == random_id => Some(update.id),
        _ => None,
    }) {
        return Some(message_id);
    }

    let mut message_ids = updates.iter().filter_map(|update| match update {
        tl::enums::Update::NewMessage(update) => Some(update.message.id()),
        tl::enums::Update::NewChannelMessage(update) => Some(update.message.id()),
        tl::enums::Update::NewScheduledMessage(update) => Some(update.message.id()),
        _ => None,
    });
    let only = message_ids.next()?;
    message_ids.next().is_none().then_some(only)
}

fn sent_message_id_from_update(update: &tl::enums::Update, random_id: i64) -> Option<i32> {
    match update {
        tl::enums::Update::MessageId(update) if update.random_id == random_id => Some(update.id),
        tl::enums::Update::NewMessage(update) => Some(update.message.id()),
        tl::enums::Update::NewChannelMessage(update) => Some(update.message.id()),
        tl::enums::Update::NewScheduledMessage(update) => Some(update.message.id()),
        _ => None,
    }
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
