use std::collections::{HashSet, VecDeque};

use grammers_client::{
    Client,
    media::{Document, Geo, Media, Photo, PhotoSize},
    message::{InputMessage, Message},
    sender::InvocationError,
    session::types::{PeerKind, PeerRef},
    tl,
};
use tg_mobile_core::{
    AttachmentDownloadState, AttachmentKind, AttachmentThumbnail, CoreError, ForwardedFrom,
    GeoPoint, MediaAttachment, MessageLocation, MessageReaction, MessageReactionType, MessageSummary,
    PollSummary, ReplyMetadata, ServiceAction, ServiceMessage, Venue,
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
    summary.forwarded_from = message.forward_header().map(|header| {
        let tl::enums::MessageFwdHeader::Header(header) = header;
        let mut forwarded = ForwardedFrom {
            timestamp_unix: Some(i64::from(header.date)),
            sender_display_name: header.from_name,
            ..ForwardedFrom::default()
        };
        if let Some(peer) = header.from_id {
            match peer {
                tl::enums::Peer::User(peer) => {
                    forwarded.sender_id = Some(peer.user_id);
                }
                tl::enums::Peer::Chat(peer) => {
                    forwarded.chat_id = Some(peer.chat_id);
                }
                tl::enums::Peer::Channel(peer) => {
                    forwarded.channel_id = Some(peer.channel_id);
                }
            }
        }
        forwarded
    });
    summary.edited = edit_timestamp_unix.is_some();
    summary.edit_timestamp_unix = edit_timestamp_unix;
    summary.location = message.media().as_ref().and_then(media_to_location);
    summary.service_message = message.action().is_some();
    summary.service = message.action().map(|action| {
        service_action_to_message(
            action,
            summary.sender_id,
            message.reply_to_message_id().map(i64::from),
        )
    });
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
    summary.reactions = message_reactions(&message.raw);

    summary
}

fn geo_to_core(geo: &Geo) -> Option<GeoPoint> {
    GeoPoint::new(geo.raw.lat, geo.raw.long, geo.raw.accuracy_radius).ok()
}

fn media_to_location(media: &Media) -> Option<MessageLocation> {
    match media {
        Media::Geo(geo) => geo_to_core(geo).map(|point| MessageLocation::Location { point }),
        Media::Venue(venue) => {
            let point = venue.geo.as_ref().and_then(geo_to_core);
            Venue::new(
                point,
                venue.title().to_owned(),
                venue.address().to_owned(),
                venue.provider().to_owned(),
                venue.venue_id().to_owned(),
                (!venue.venue_type().is_empty()).then(|| venue.venue_type().to_owned()),
            )
            .ok()
            .map(|venue| MessageLocation::Venue { venue })
        }
        _ => None,
    }
}

fn service_action_to_message(
    action: &tl::enums::MessageAction,
    actor_id: i64,
    pinned_message_id: Option<i64>,
) -> ServiceMessage {
    use tl::enums::MessageAction as Action;

    let action = match action {
        Action::ChatCreate(value) => ServiceAction::GroupCreated {
            title: value.title.clone(),
            user_ids: value.users.clone(),
        },
        Action::ChatEditTitle(value) => ServiceAction::TitleChanged {
            title: value.title.clone(),
        },
        Action::ChatEditPhoto(value) => ServiceAction::PhotoChanged {
            photo_id: Some(Photo::from_raw(value.photo.clone()).id()),
        },
        Action::ChatDeletePhoto => ServiceAction::PhotoRemoved,
        Action::ChatAddUser(value)
            if value.users.len() == 1 && value.users.first().copied() == Some(actor_id) =>
        {
            ServiceAction::UserJoined {
                user_id: value.users.first().copied(),
                inviter_id: None,
            }
        }
        Action::ChatAddUser(value) => ServiceAction::UserAdded {
            user_ids: value.users.clone(),
        },
        Action::ChatDeleteUser(value) if value.user_id == actor_id => ServiceAction::UserLeft {
            user_id: value.user_id,
        },
        Action::ChatDeleteUser(value) => ServiceAction::UserRemoved {
            user_id: value.user_id,
        },
        Action::ChatJoinedByLink(value) => ServiceAction::UserJoined {
            user_id: (actor_id != 0).then_some(actor_id),
            inviter_id: Some(value.inviter_id),
        },
        Action::ChatJoinedByRequest => ServiceAction::UserJoined {
            user_id: (actor_id != 0).then_some(actor_id),
            inviter_id: None,
        },
        Action::ChannelCreate(value) => ServiceAction::ChannelCreated {
            title: value.title.clone(),
        },
        Action::ChatMigrateTo(value) => ServiceAction::GroupMigratedToSupergroup {
            supergroup_id: value.channel_id,
        },
        Action::ChannelMigrateFrom(value) => ServiceAction::ChannelMigratedFromGroup {
            group_id: value.chat_id,
            title: value.title.clone(),
        },
        Action::PinMessage => ServiceAction::MessagePinned {
            message_id: pinned_message_id,
        },
        Action::HistoryClear => ServiceAction::HistoryCleared,
        _ => ServiceAction::Unknown,
    };
    ServiceMessage::new(action, (actor_id != 0).then_some(actor_id))
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
        location: None,
        attachments: Vec::new(),
        forwarded_from: None,
        edited: false,
        edit_timestamp_unix: None,
        service_message: false,
        service: None,
        media_caption: None,
        reply_to: None,
        reactions: Vec::new(),
        reaction_revision: None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoadedMessages {
    pub messages: Vec<MessageSummary>,
    pub polls: Vec<PollSummary>,
}

pub(crate) fn message_reactions(message: &tl::enums::Message) -> Vec<MessageReaction> {
    let snapshot = match message {
        tl::enums::Message::Message(message) => message.reactions.as_ref(),
        _ => None,
    };
    snapshot.map(map_reactions).unwrap_or_default()
}

pub(crate) fn map_reactions(snapshot: &tl::enums::MessageReactions) -> Vec<MessageReaction> {
    let tl::enums::MessageReactions::Reactions(snapshot) = snapshot;
    let mut seen = std::collections::BTreeSet::new();
    snapshot
        .results
        .iter()
        .filter_map(|result| {
            let tl::enums::ReactionCount::Count(result) = result;
            let reaction = match &result.reaction {
                tl::enums::Reaction::Emoji(value) => {
                    MessageReactionType::Emoji(value.emoticon.clone())
                }
                tl::enums::Reaction::CustomEmoji(value) => {
                    MessageReactionType::CustomEmoji(value.document_id)
                }
                _ => return None,
            };
            let reaction = MessageReaction {
                reaction,
                count: u32::try_from(result.count).ok()?,
                chosen_by_me: result.chosen_order.is_some(),
            };
            reaction.reaction.validate().ok()?;
            seen.insert(reaction.reaction.clone()).then_some(reaction)
        })
        .collect()
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

pub(crate) async fn send_location(
    client: &Client,
    peer: PeerRef,
    location: GeoPoint,
) -> tg_mobile_core::Result<MessageSummary> {
    location.validate()?;
    let geo_point: tl::enums::InputGeoPoint = tl::types::InputGeoPoint {
        lat: location.latitude(),
        long: location.longitude(),
        accuracy_radius: location.accuracy_radius_meters(),
    }
    .into();
    let media = tl::types::InputMediaGeoPoint { geo_point };
    let message = client
        .send_message(peer, InputMessage::new().media(media))
        .await
        .map_err(|error| api_error("send location", error))?;
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
        media_to_location, message_summary_from_fields, service_action_to_message,
    };
    use grammers_client::{
        media::{Geo, Media, Venue as TelegramVenue},
        tl,
    };
    use tg_mobile_core::{MessageLocation, MessageReaction, MessageReactionType, ServiceAction};

    #[test]
    fn maps_reaction_counts_chosen_state_and_custom_emoji_ids() {
        let raw = tl::enums::MessageReactions::Reactions(tl::types::MessageReactions {
            min: false,
            can_see_list: true,
            reactions_as_tags: false,
            results: vec![
                tl::enums::ReactionCount::Count(tl::types::ReactionCount {
                    chosen_order: Some(0),
                    reaction: tl::enums::Reaction::Emoji(tl::types::ReactionEmoji {
                        emoticon: "👍".into(),
                    }),
                    count: 4,
                }),
                tl::enums::ReactionCount::Count(tl::types::ReactionCount {
                    chosen_order: None,
                    reaction: tl::enums::Reaction::CustomEmoji(tl::types::ReactionCustomEmoji {
                        document_id: 9876,
                    }),
                    count: 2,
                }),
            ],
            recent_reactions: None,
            top_reactors: None,
        });

        assert_eq!(
            super::map_reactions(&raw),
            vec![
                MessageReaction {
                    reaction: MessageReactionType::Emoji("👍".into()),
                    count: 4,
                    chosen_by_me: true,
                },
                MessageReaction {
                    reaction: MessageReactionType::CustomEmoji(9876),
                    count: 2,
                    chosen_by_me: false,
                },
            ]
        );
    }

    fn telegram_geo() -> tl::types::GeoPoint {
        tl::types::GeoPoint {
            long: 21.012_228_7,
            lat: 52.229_675_6,
            access_hash: 7,
            accuracy_radius: Some(15),
        }
    }

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
    fn maps_telegram_service_actions_to_typed_events() {
        use grammers_client::tl;

        let joined = service_action_to_message(
            &tl::enums::MessageAction::ChatJoinedByLink(tl::types::MessageActionChatJoinedByLink {
                inviter_id: 31,
            }),
            41,
            None,
        );
        assert_eq!(
            joined.action,
            ServiceAction::UserJoined {
                user_id: Some(41),
                inviter_id: Some(31)
            }
        );
        assert_eq!(joined.actor_id, Some(41));

        let added = service_action_to_message(
            &tl::enums::MessageAction::ChatAddUser(tl::types::MessageActionChatAddUser {
                users: vec![41, 42],
            }),
            31,
            None,
        );
        assert_eq!(
            added.action,
            ServiceAction::UserAdded {
                user_ids: vec![41, 42]
            }
        );
        let joined_by_add = service_action_to_message(
            &tl::enums::MessageAction::ChatAddUser(tl::types::MessageActionChatAddUser {
                users: vec![41],
            }),
            41,
            None,
        );
        assert_eq!(
            joined_by_add.action,
            ServiceAction::UserJoined {
                user_id: Some(41),
                inviter_id: None
            }
        );

        let left = service_action_to_message(
            &tl::enums::MessageAction::ChatDeleteUser(tl::types::MessageActionChatDeleteUser {
                user_id: 31,
            }),
            31,
            None,
        );
        assert_eq!(left.action, ServiceAction::UserLeft { user_id: 31 });
        let removed = service_action_to_message(
            &tl::enums::MessageAction::ChatDeleteUser(tl::types::MessageActionChatDeleteUser {
                user_id: 31,
            }),
            41,
            None,
        );
        assert_eq!(removed.action, ServiceAction::UserRemoved { user_id: 31 });

        let pinned = service_action_to_message(&tl::enums::MessageAction::PinMessage, 41, Some(73));
        assert_eq!(
            pinned.action,
            ServiceAction::MessagePinned {
                message_id: Some(73)
            }
        );
        let cleared = service_action_to_message(&tl::enums::MessageAction::HistoryClear, 41, None);
        assert_eq!(cleared.action, ServiceAction::HistoryCleared);

        let migrated = service_action_to_message(
            &tl::enums::MessageAction::ChatMigrateTo(tl::types::MessageActionChatMigrateTo {
                channel_id: -100,
            }),
            41,
            None,
        );
        assert_eq!(
            migrated.action,
            ServiceAction::GroupMigratedToSupergroup {
                supergroup_id: -100
            }
        );
        let created = service_action_to_message(
            &tl::enums::MessageAction::ChatCreate(tl::types::MessageActionChatCreate {
                title: "Team".into(),
                users: vec![41, 42],
            }),
            41,
            None,
        );
        assert_eq!(
            created.action,
            ServiceAction::GroupCreated {
                title: "Team".into(),
                user_ids: vec![41, 42]
            }
        );

        let title = service_action_to_message(
            &tl::enums::MessageAction::ChatEditTitle(tl::types::MessageActionChatEditTitle {
                title: "Renamed".into(),
            }),
            41,
            None,
        );
        assert_eq!(
            title.action,
            ServiceAction::TitleChanged {
                title: "Renamed".into()
            }
        );

        let photo = service_action_to_message(
            &tl::enums::MessageAction::ChatEditPhoto(tl::types::MessageActionChatEditPhoto {
                photo: tl::enums::Photo::Empty(tl::types::PhotoEmpty { id: 99 }),
            }),
            41,
            None,
        );
        assert_eq!(
            photo.action,
            ServiceAction::PhotoChanged { photo_id: Some(99) }
        );
        let removed_photo =
            service_action_to_message(&tl::enums::MessageAction::ChatDeletePhoto, 41, None);
        assert_eq!(removed_photo.action, ServiceAction::PhotoRemoved);

        let channel = service_action_to_message(
            &tl::enums::MessageAction::ChannelCreate(tl::types::MessageActionChannelCreate {
                title: "Updates".into(),
            }),
            41,
            None,
        );
        assert_eq!(
            channel.action,
            ServiceAction::ChannelCreated {
                title: "Updates".into()
            }
        );
        let channel_from_group = service_action_to_message(
            &tl::enums::MessageAction::ChannelMigrateFrom(
                tl::types::MessageActionChannelMigrateFrom {
                    title: "Updates".into(),
                    chat_id: 17,
                },
            ),
            41,
            None,
        );
        assert_eq!(
            channel_from_group.action,
            ServiceAction::ChannelMigratedFromGroup {
                group_id: 17,
                title: "Updates".into()
            }
        );
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

    #[test]
    fn maps_telegram_geo_media_to_core_location() {
        let media = Media::Geo(Geo {
            raw: telegram_geo(),
        });
        let mapped = media_to_location(&media).expect("geo must map");
        let MessageLocation::Location { point } = mapped else {
            panic!("expected location");
        };
        assert_eq!(point.latitude(), 52.229_675_6);
        assert_eq!(point.longitude(), 21.012_228_7);
        assert_eq!(point.accuracy_radius_meters(), Some(15));
    }

    #[test]
    fn maps_telegram_venue_media_to_core_venue() {
        let raw = tl::types::MessageMediaVenue {
            geo: telegram_geo().into(),
            title: "Central Library".into(),
            address: "1 Example Street".into(),
            provider: "foursquare".into(),
            venue_id: "venue-1".into(),
            venue_type: "library".into(),
        };
        let media = Media::Venue(TelegramVenue::from_raw_media(raw));
        let mapped = media_to_location(&media).expect("venue must map");
        let MessageLocation::Venue { venue } = mapped else {
            panic!("expected venue");
        };
        assert_eq!(venue.title(), "Central Library");
        assert_eq!(venue.address(), "1 Example Street");
        assert_eq!(venue.provider(), "foursquare");
        assert_eq!(venue.venue_id(), "venue-1");
        assert_eq!(venue.venue_type(), Some("library"));
        assert_eq!(venue.point().unwrap().accuracy_radius_meters(), Some(15));
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
