use std::collections::{HashSet, VecDeque};

use grammers_client::{
    Client,
    media::{Document, Media, Photo, PhotoSize},
    message::Message,
    sender::InvocationError,
    session::types::{PeerKind, PeerRef},
    tl,
};
use tg_mobile_core::{
    AttachmentDownloadState, AttachmentKind, AttachmentThumbnail, CoreError, ForwardedFrom,
    MediaAttachment, MessageSummary, PollSummary, ReplyMetadata,
};
use tokio::sync::Mutex;

use crate::{error::api_error, ids::peer_id_to_core, polls::poll_from_message};

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

    summary.attachments = message
        .media()
        .and_then(media_to_attachment)
        .into_iter()
        .collect();
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

fn media_to_attachment(media: Media) -> Option<MediaAttachment> {
    match media {
        Media::Photo(photo) => Some(photo_attachment(&photo)),
        Media::Document(document) => Some(document_attachment(&document, None)),
        Media::Sticker(sticker) => Some(document_attachment(
            &sticker.document,
            Some(AttachmentKind::Sticker),
        )),
        _ => None,
    }
}

fn photo_attachment(photo: &Photo) -> MediaAttachment {
    let (width, height) = photo_dimensions(photo);
    MediaAttachment {
        kind: AttachmentKind::Photo,
        file_reference: format!("photo:{}", photo.id()),
        size_bytes: photo.size().and_then(|size| u64::try_from(size).ok()),
        mime_type: Some("image/jpeg".into()),
        width,
        height,
        duration_ms: None,
        filename: None,
        thumbnail: None,
        download_generation: 0,
        download_state: AttachmentDownloadState::NotDownloaded,
    }
}

fn document_attachment(
    document: &Document,
    forced_kind: Option<AttachmentKind>,
) -> MediaAttachment {
    let kind = forced_kind.unwrap_or_else(|| document_kind(document));
    let (width, height) = document
        .resolution()
        .and_then(|(width, height)| positive_dimensions(width, height))
        .unwrap_or((None, None));
    let duration_ms = document.duration().and_then(|seconds| {
        (seconds.is_finite() && seconds >= 0.0).then(|| (seconds * 1000.0).round() as u64)
    });
    let thumbnail = best_thumbnail(document);
    MediaAttachment {
        kind,
        file_reference: format!("document:{}", document.id()),
        size_bytes: document.size().and_then(|size| u64::try_from(size).ok()),
        mime_type: document.mime_type().map(str::to_owned),
        width,
        height,
        duration_ms,
        filename: document.name().map(str::to_owned),
        thumbnail,
        download_generation: 0,
        download_state: AttachmentDownloadState::NotDownloaded,
    }
}

fn document_kind(document: &Document) -> AttachmentKind {
    if let Some(tl::enums::Document::Document(raw)) = document.raw.document.as_ref() {
        for attribute in &raw.attributes {
            match attribute {
                tl::enums::DocumentAttribute::Video(video) => {
                    return if video.round_message {
                        AttachmentKind::VideoNote
                    } else {
                        AttachmentKind::Video
                    };
                }
                tl::enums::DocumentAttribute::Audio(audio) => {
                    return if audio.voice {
                        AttachmentKind::Voice
                    } else {
                        AttachmentKind::Audio
                    };
                }
                tl::enums::DocumentAttribute::Sticker(_) => return AttachmentKind::Sticker,
                tl::enums::DocumentAttribute::Animated => return AttachmentKind::Animation,
                _ => {}
            }
        }
    }

    if document.mime_type().is_some_and(|mime| mime == "image/gif") {
        AttachmentKind::Animation
    } else {
        AttachmentKind::Document
    }
}

fn best_thumbnail(document: &Document) -> Option<AttachmentThumbnail> {
    let thumb = document
        .thumbs()
        .into_iter()
        .filter_map(|thumb| thumbnail_dimensions(&thumb).map(|dimensions| (thumb, dimensions)))
        .max_by_key(|(_, (width, height))| u64::from(*width) * u64::from(*height))?;
    let (thumb, (width, height)) = thumb;
    Some(AttachmentThumbnail {
        file_reference: format!("document-thumb:{}:{}", document.id(), thumb.photo_type()),
        size_bytes: u64::try_from(thumb.size()).ok(),
        mime_type: Some("image/jpeg".into()),
        width: Some(width),
        height: Some(height),
    })
}

fn photo_dimensions(photo: &Photo) -> (Option<u32>, Option<u32>) {
    photo
        .thumbs()
        .iter()
        .filter_map(thumbnail_dimensions)
        .max_by_key(|(width, height)| u64::from(*width) * u64::from(*height))
        .map(|(width, height)| (Some(width), Some(height)))
        .unwrap_or((None, None))
}

fn thumbnail_dimensions(thumb: &PhotoSize) -> Option<(u32, u32)> {
    let (width, height) = match thumb {
        PhotoSize::Size(size) => (size.width, size.height),
        PhotoSize::Cached(size) => (size.width, size.height),
        PhotoSize::Progressive(size) => (size.width, size.height),
        PhotoSize::Empty(_) | PhotoSize::Stripped(_) | PhotoSize::Path(_) => return None,
    };
    positive_dimensions(width, height).and_then(|(width, height)| width.zip(height))
}

fn positive_dimensions(width: i32, height: i32) -> Option<(Option<u32>, Option<u32>)> {
    let width = u32::try_from(width).ok().filter(|value| *value > 0);
    let height = u32::try_from(height).ok().filter(|value| *value > 0);
    (width.is_some() && height.is_some()).then_some((width, height))
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoadedMessages {
    pub messages: Vec<MessageSummary>,
    pub polls: Vec<PollSummary>,
}

pub(crate) async fn load_messages(
    client: &Client,
    peer: PeerRef,
    limit: usize,
) -> tg_mobile_core::Result<LoadedMessages> {
    let mut iterator = client.iter_messages(peer).limit(limit);
    let mut messages = Vec::with_capacity(limit.min(100));
    let mut polls = Vec::new();

    while let Some(message) = iterator
        .next()
        .await
        .map_err(|error| api_error("load messages", error))?
    {
        if let Some(poll) = poll_from_message(&message)? {
            polls.push(poll);
        }
        messages.push(message_to_summary(&message));
    }

    messages.reverse();
    polls.reverse();
    Ok(LoadedMessages { messages, polls })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MessagePage {
    pub messages: Vec<MessageSummary>,
    pub polls: Vec<PollSummary>,
    pub next_before_message_id: Option<i64>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchMessagePage {
    pub messages: Vec<MessageSummary>,
    pub next_before_message_id: Option<i64>,
    pub has_more: bool,
}

pub(crate) async fn load_messages_page(
    client: &Client,
    peer: PeerRef,
    before_message_id: Option<i64>,
    limit: usize,
) -> tg_mobile_core::Result<MessagePage> {
    let offset_id = before_message_id
        .map(|message_id| {
            i32::try_from(message_id).map_err(|_| {
                CoreError::InvalidInput("message page cursor exceeds Telegram range".into())
            })
        })
        .transpose()?;

    let fetch_limit = limit.saturating_add(1);
    let mut iterator = client.iter_messages(peer);
    if let Some(offset_id) = offset_id {
        iterator = iterator.offset_id(offset_id);
    }
    iterator = iterator.limit(fetch_limit);

    let mut messages = Vec::with_capacity(fetch_limit.min(201));
    let mut polls = Vec::new();
    while let Some(message) = iterator
        .next()
        .await
        .map_err(|error| api_error("load message page", error))?
    {
        if let Some(poll) = poll_from_message(&message)? {
            polls.push(poll);
        }
        messages.push(message_to_summary(&message));
    }

    Ok(finalize_message_page(messages, polls, limit))
}

fn finalize_message_page(
    mut messages: Vec<MessageSummary>,
    mut polls: Vec<PollSummary>,
    limit: usize,
) -> MessagePage {
    let has_extra = messages.len() > limit;
    if has_extra {
        messages.truncate(limit);
    }

    let kept_message_ids = messages
        .iter()
        .map(|message| message.id)
        .collect::<HashSet<_>>();
    polls.retain(|poll| kept_message_ids.contains(&poll.message_id));

    let next_before_message_id = has_extra
        .then(|| messages.last().map(|message| message.id))
        .flatten()
        .filter(|message_id| *message_id > 0);
    let has_more = next_before_message_id.is_some();
    messages.reverse();
    polls.reverse();

    MessagePage {
        messages,
        polls,
        next_before_message_id,
        has_more,
    }
}

pub(crate) async fn search_messages_page(
    client: &Client,
    peer: PeerRef,
    query: &str,
    before_message_id: Option<i64>,
    limit: usize,
) -> tg_mobile_core::Result<SearchMessagePage> {
    let query = query.trim();
    if query.is_empty() {
        return Err(CoreError::InvalidInput(
            "message search query must not be empty".into(),
        ));
    }

    let offset_id = before_message_id
        .map(|message_id| {
            i32::try_from(message_id).map_err(|_| {
                CoreError::InvalidInput("message search cursor exceeds Telegram range".into())
            })
        })
        .transpose()?;

    let fetch_limit = limit.saturating_add(1);
    let mut iterator = client.search_messages(peer).query(query).limit(fetch_limit);
    if let Some(offset_id) = offset_id {
        iterator = iterator.offset_id(offset_id);
    }

    let mut messages = Vec::with_capacity(fetch_limit.min(101));
    while let Some(message) = iterator
        .next()
        .await
        .map_err(|error| api_error("search messages", error))?
    {
        messages.push(message_to_summary(&message));
    }

    Ok(finalize_search_page(messages, limit))
}

fn finalize_search_page(mut messages: Vec<MessageSummary>, limit: usize) -> SearchMessagePage {
    let has_extra = messages.len() > limit;
    if has_extra {
        messages.truncate(limit);
    }

    let next_before_message_id = has_extra
        .then(|| messages.last().map(|message| message.id))
        .flatten()
        .filter(|message_id| *message_id > 0);

    SearchMessagePage {
        messages,
        next_before_message_id,
        has_more: next_before_message_id.is_some(),
    }
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
            message: text.clone(),
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
        .map_err(classify_send_error)?;

    let message_id = sent_message_id(&updates, random_id).ok_or_else(|| {
        CoreError::SendOutcomeUnknown(
            "Telegram accepted send request but returned no resolvable message id".into(),
        )
    })?;
    let date = sent_message_date(&updates).unwrap_or_default();
    Ok(message_summary_from_fields(MessageFields {
        id: i64::from(message_id),
        chat_id: peer_id_to_core(peer.id),
        sender_id: 0,
        text: &text,
        timestamp_unix: i64::from(date),
        outgoing: true,
    }))
}

fn classify_send_error(error: InvocationError) -> CoreError {
    match &error {
        InvocationError::Rpc(rpc) if rpc.name == "FLOOD_WAIT" => CoreError::FloodWait {
            seconds: rpc.value.unwrap_or_default(),
        },
        InvocationError::Rpc(rpc) if rpc.code >= 500 => {
            CoreError::TransientSend("Telegram temporary server failure".into())
        }
        InvocationError::Rpc(rpc) => CoreError::SendRejected(rpc.to_string()),
        _ => CoreError::SendOutcomeUnknown(error.to_string()),
    }
}

fn sent_message_date(updates: &tl::enums::Updates) -> Option<i32> {
    match updates {
        tl::enums::Updates::UpdateShortSentMessage(update) => Some(update.date),
        tl::enums::Updates::Updates(updates) => Some(updates.date),
        tl::enums::Updates::Combined(updates) => Some(updates.date),
        tl::enums::Updates::UpdateShort(update) => Some(update.date),
        tl::enums::Updates::UpdateShortMessage(update) => Some(update.date),
        tl::enums::Updates::UpdateShortChatMessage(update) => Some(update.date),
        tl::enums::Updates::TooLong => None,
    }
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

fn sent_message_id_from_updates(updates: &[tl::enums::Update], random_id: i64) -> Option<i32> {
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

#[cfg(test)]
mod send_error_tests {
    use grammers_client::sender::{InvocationError, RpcError};
    use tg_mobile_core::CoreError;

    use super::classify_send_error;

    fn rpc_error(code: i32, name: &str, value: Option<u32>) -> InvocationError {
        InvocationError::Rpc(RpcError {
            code,
            name: name.into(),
            value,
            caused_by: None,
        })
    }

    #[test]
    fn flood_wait_is_a_distinct_retry_class() {
        assert!(matches!(
            classify_send_error(rpc_error(420, "FLOOD_WAIT", Some(30))),
            CoreError::FloodWait { seconds: 30 }
        ));
    }

    #[test]
    fn permanent_rpc_rejection_is_not_marked_retryable() {
        assert!(matches!(
            classify_send_error(rpc_error(400, "MESSAGE_TOO_LONG", None)),
            CoreError::SendRejected(message) if message.contains("MESSAGE_TOO_LONG")
        ));
    }

    #[test]
    fn temporary_rpc_failure_is_retryable() {
        assert!(matches!(
            classify_send_error(rpc_error(500, "INTERNAL", None)),
            CoreError::TransientSend(_)
        ));
    }

    #[test]
    fn transport_failure_keeps_the_outcome_ambiguous() {
        assert!(matches!(
            classify_send_error(InvocationError::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out"
            ))),
            CoreError::SendOutcomeUnknown(message) if message.contains("timed out")
        ));
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
    use super::{
        MessageDeduplicator, MessageFields, finalize_message_page, finalize_search_page,
        message_summary_from_fields,
    };

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

    #[test]
    fn page_finalization_uses_one_extra_message_for_has_more() {
        let summary = |id| {
            message_summary_from_fields(MessageFields {
                id,
                chat_id: 1,
                sender_id: 1,
                text: "page",
                timestamp_unix: id,
                outgoing: false,
            })
        };

        let page = finalize_message_page(vec![summary(5), summary(4), summary(3)], Vec::new(), 2);
        assert_eq!(
            page.messages
                .iter()
                .map(|message| message.id)
                .collect::<Vec<_>>(),
            vec![4, 5]
        );
        assert_eq!(page.next_before_message_id, Some(4));
        assert!(page.has_more);

        let final_page = finalize_message_page(vec![summary(2), summary(1)], Vec::new(), 2);
        assert_eq!(
            final_page
                .messages
                .iter()
                .map(|message| message.id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(final_page.next_before_message_id, None);
        assert!(!final_page.has_more);
    }

    #[test]
    fn search_page_uses_extra_hit_as_continuation_signal() {
        let hit = |id| {
            message_summary_from_fields(MessageFields {
                id,
                chat_id: 7,
                sender_id: 42,
                text: "match",
                timestamp_unix: id,
                outgoing: false,
            })
        };

        let page = finalize_search_page(vec![hit(50), hit(40), hit(30)], 2);
        assert_eq!(
            page.messages
                .iter()
                .map(|message| message.id)
                .collect::<Vec<_>>(),
            vec![50, 40]
        );
        assert_eq!(page.next_before_message_id, Some(40));
        assert!(page.has_more);

        let final_page = finalize_search_page(vec![hit(20), hit(10)], 2);
        assert_eq!(final_page.next_before_message_id, None);
        assert!(!final_page.has_more);
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
