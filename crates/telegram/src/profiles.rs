use std::io::Write;

use grammers_client::{
    Client,
    peer::{Participant, Peer, Role},
    session::types::PeerRef,
    tl,
};
use tg_media_cache::MediaCache;
use tg_mobile_core::{AvatarReference, ChatMemberSummary, PeerKind, PeerPresence, PeerProfile};

const PROFILE_MEMBER_PREVIEW_LIMIT: usize = 100;

/// Convert the profile fields available in Grammers' resolved peer snapshot.
/// Full details remain optional when Telegram did not include them in the peer.
pub(crate) fn profile_from_peer(peer_id: i64, peer: &Peer) -> PeerProfile {
    let mut profile = PeerProfile::unknown(peer_id);
    profile.display_name = peer.name().map(str::to_owned);
    profile.username = peer.username().map(str::to_owned);

    match peer {
        Peer::User(user) => {
            profile.display_name = Some(user.full_name());
            profile.kind = if user.is_bot() {
                PeerKind::Bot
            } else {
                PeerKind::User
            };
            profile.first_name = user.first_name().map(str::to_owned);
            profile.last_name = user.last_name().map(str::to_owned);
            profile.is_bot = Some(user.is_bot());
            profile.is_verified = Some(user.verified());
            profile.is_scam = Some(user.scam());
            profile.presence = match user.status() {
                tl::enums::UserStatus::Online(_) => PeerPresence::Online,
                tl::enums::UserStatus::Offline(status) => PeerPresence::Offline {
                    last_seen_unix: i64::from(status.was_online),
                },
                _ => PeerPresence::Unknown,
            };
            if let tl::enums::User::User(raw) = &user.raw {
                profile.is_premium = Some(raw.premium);
                profile.is_fake = Some(raw.fake);
                if let Some(photo) = user.photo() {
                    profile.avatar = Some(AvatarReference {
                        file_id: photo.photo_id.to_string(),
                        access_hash: None,
                        file_reference: None,
                        data_center_id: Some(photo.dc_id),
                        size_bytes: None,
                        mime_type: Some("image/jpeg".into()),
                        local_path: None,
                    });
                }
            }
        }
        Peer::Group(group) => {
            profile.kind = PeerKind::BasicGroup;
            if let tl::enums::Chat::Chat(raw) = &group.raw {
                set_member_count(&mut profile, Some(raw.participants_count));
            }
        }
        Peer::Channel(channel) => {
            profile.kind = channel_profile_kind(
                channel.raw.broadcast,
                channel.raw.megagroup,
                channel.raw.gigagroup,
            );
            set_member_count(&mut profile, channel.raw.participants_count);
            profile.is_verified = Some(channel.raw.verified);
            profile.is_scam = Some(channel.raw.scam);
            profile.is_fake = Some(channel.raw.fake);
            profile.can_send_messages = channel
                .admin_rights()
                .map(|rights| rights.post_messages)
                .or_else(|| match channel.raw.default_banned_rights.as_ref() {
                    Some(tl::enums::ChatBannedRights::Rights(rights)) => Some(!rights.send_plain),
                    None => None,
                });
            if let Some(rights) = channel.admin_rights() {
                profile.can_invite_users = Some(rights.invite_users);
                profile.can_change_info = Some(rights.change_info);
                profile.can_post_messages = Some(rights.post_messages);
            }
        }
        // Grammers 0.12 exposes Community as a minimal snapshot without a
        // full-community metadata RPC; keep unsupported type details unknown.
        Peer::Community(_) => {}
    }
    profile
}

/// Return only participants Telegram allows this account to inspect. Telegram
/// may reject this request for broadcast channels or restricted group members;
/// callers preserve that as an unavailable list rather than inventing one.
pub(crate) async fn visible_members(
    client: &Client,
    peer: &Peer,
    peer_ref: PeerRef,
) -> Option<(Vec<ChatMemberSummary>, bool)> {
    if !matches!(peer, Peer::Group(_) | Peer::Channel(_)) {
        return None;
    }

    let mut participants = client.iter_participants(peer_ref);
    let mut members = Vec::with_capacity(PROFILE_MEMBER_PREVIEW_LIMIT);
    while members.len() < PROFILE_MEMBER_PREVIEW_LIMIT {
        match participants.next().await.ok()? {
            Some(participant) => members.push(member_summary(participant)),
            None => return Some((members, false)),
        }
    }

    let truncated = participants.next().await.ok()?.is_some();
    Some((members, truncated))
}

fn member_summary(participant: Participant) -> ChatMemberSummary {
    let role = match &participant.role {
        Role::Creator(_) => "owner",
        Role::Admin(_) => "admin",
        _ => "member",
    };
    ChatMemberSummary {
        user_id: crate::ids::peer_id_to_core(participant.user.id()),
        display_name: participant.user.full_name(),
        username: participant.user.username().map(str::to_owned),
        role: role.to_owned(),
    }
}

pub(crate) async fn hydrate_profile(
    client: &grammers_client::Client,
    peer: &Peer,
    profile: &mut PeerProfile,
    cache: &MediaCache,
) {
    if let Ok(Some(photo)) = peer.photo(true).await {
        let file_id = profile
            .avatar
            .as_ref()
            .map(|avatar| avatar.file_id.clone())
            .unwrap_or_else(|| format!("peer:{}", profile.peer_id));
        let mut avatar = tg_mobile_core::AvatarReference {
            file_id,
            access_hash: None,
            file_reference: None,
            data_center_id: None,
            size_bytes: None,
            mime_type: Some("image/jpeg".into()),
            local_path: None,
        };
        let cache_key = format!("peer-avatar:{}:{}", profile.peer_id, avatar.file_id);
        if let Ok(Some(lease)) = cache.get(&cache_key) {
            avatar.local_path = Some(lease.path().to_path_buf());
        } else if let Ok(mut writer) = cache.begin_write(&cache_key) {
            let mut download = client.iter_download(&photo);
            let mut succeeded = true;
            loop {
                match download.next().await {
                    Ok(Some(chunk)) => {
                        if writer.write_all(&chunk).is_err() {
                            succeeded = false;
                            break;
                        }
                    }
                    Err(_) => {
                        succeeded = false;
                        break;
                    }
                    Ok(None) => break,
                }
            }
            if succeeded {
                if let Ok(lease) = writer.commit() {
                    avatar.local_path = Some(lease.path().to_path_buf());
                }
            }
        }
        profile.avatar = Some(avatar);
    }

    match peer {
        Peer::User(user) => {
            if let tl::enums::User::User(raw) = &user.raw {
                if let Some(access_hash) = raw.access_hash {
                    let request = tl::functions::users::GetFullUser {
                        id: tl::enums::InputUser::User(tl::types::InputUser {
                            user_id: raw.id,
                            access_hash,
                        }),
                    };
                    if let Ok(tl::enums::users::UserFull::Full(full)) =
                        client.invoke(&request).await
                    {
                        if let tl::enums::UserFull::Full(full_user) = full.full_user {
                            if let Some(about) = full_user.about.as_deref() {
                                set_profile_about(profile, about);
                            }
                        }
                    }
                }
            }
        }
        Peer::Group(group) => {
            if let tl::enums::Chat::Chat(raw) = &group.raw {
                let request = tl::functions::messages::GetFullChat { chat_id: raw.id };
                if let Ok(tl::enums::messages::ChatFull::Full(full)) = client.invoke(&request).await
                {
                    set_profile_about(profile, &full.full_chat.about());
                }
            }
        }
        Peer::Channel(channel) => {
            if let Some(access_hash) = channel.raw.access_hash {
                let request = tl::functions::channels::GetFullChannel {
                    channel: tl::enums::InputChannel::Channel(tl::types::InputChannel {
                        channel_id: channel.raw.id,
                        access_hash,
                    }),
                };
                if let Ok(tl::enums::messages::ChatFull::Full(full)) = client.invoke(&request).await
                {
                    set_profile_about(profile, &full.full_chat.about());
                    if let tl::enums::ChatFull::ChannelFull(channel_full) = &full.full_chat {
                        set_member_count(profile, channel_full.participants_count);
                    }
                }
            }
        }
        Peer::Community(_) => {}
    }
}

fn set_profile_about(profile: &mut PeerProfile, value: &str) {
    let value = value.trim();
    profile.bio = (!value.is_empty()).then(|| value.to_owned());
}

fn set_member_count(profile: &mut PeerProfile, count: Option<i32>) {
    profile.member_count = count.and_then(|count| u32::try_from(count).ok());
}

fn channel_profile_kind(broadcast: bool, megagroup: bool, gigagroup: bool) -> PeerKind {
    if megagroup || gigagroup {
        PeerKind::Supergroup
    } else if broadcast {
        PeerKind::Channel
    } else {
        PeerKind::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_profile_never_invents_personal_fields() {
        let profile = PeerProfile::unknown(-42);
        assert_eq!(profile.peer_id, -42);
        assert_eq!(profile.display_name, None);
        assert_eq!(profile.username, None);
        assert_eq!(profile.is_bot, None);
        assert_eq!(profile.is_premium, None);
        assert_eq!(profile.presence, PeerPresence::Unknown);
    }

    #[test]
    fn channel_type_is_mapped_without_inventing_group_type() {
        assert_eq!(channel_profile_kind(true, false, false), PeerKind::Channel);
        assert_eq!(
            channel_profile_kind(false, true, false),
            PeerKind::Supergroup
        );
        assert_eq!(
            channel_profile_kind(false, false, true),
            PeerKind::Supergroup
        );
        assert_eq!(channel_profile_kind(false, false, false), PeerKind::Unknown);
    }

    #[test]
    fn full_profile_description_and_member_count_use_only_available_values() {
        let mut profile = PeerProfile::unknown(51);
        set_profile_about(&mut profile, "  A public description  ");
        set_member_count(&mut profile, Some(42));
        assert_eq!(profile.bio.as_deref(), Some("A public description"));
        assert_eq!(profile.member_count, Some(42));

        set_profile_about(&mut profile, "  ");
        set_member_count(&mut profile, None);
        assert_eq!(profile.bio, None);
        assert_eq!(profile.member_count, None);
    }

    #[test]
    fn invalid_member_count_and_community_kind_remain_unknown() {
        let mut profile = PeerProfile::unknown(-901);
        set_member_count(&mut profile, Some(-1));
        assert_eq!(profile.member_count, None);
        assert_eq!(profile.kind, PeerKind::Unknown);
    }
}
