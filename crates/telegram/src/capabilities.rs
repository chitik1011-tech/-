use std::time::{SystemTime, UNIX_EPOCH};

use grammers_client::{Client, peer::Peer, tl};
use tg_mobile_core::{CapabilityValue as Value, ChatCapabilities, ChatKind};

/// Build a capability snapshot only from Telegram's peer rights and flags.
/// Missing permission metadata remains Unknown; it is never promoted to allow.
pub(crate) fn from_peer(peer: &Peer, chat_id: i64) -> ChatCapabilities {
    match peer {
        Peer::User(user) => match &user.raw {
            tl::enums::User::User(user) => {
                private_snapshot(chat_id, !user.deleted && !user.bot_nochats)
            }
            tl::enums::User::Empty(_) => ChatCapabilities {
                chat_id,
                chat_kind: ChatKind::Private,
                ..ChatCapabilities::default()
            },
        },
        Peer::Community(_) => ChatCapabilities {
            chat_id,
            chat_kind: ChatKind::Unknown,
            ..ChatCapabilities::default()
        },
        Peer::Group(group) => match &group.raw {
            tl::enums::Chat::Chat(chat) => from_group(
                chat_id,
                chat.creator,
                chat.left,
                chat.admin_rights.as_ref().and_then(|rights| match rights {
                    tl::enums::ChatAdminRights::Rights(rights) => Some(rights),
                }),
                None,
                chat.default_banned_rights
                    .as_ref()
                    .and_then(|rights| match rights {
                        tl::enums::ChatBannedRights::Rights(rights) => Some(rights),
                    }),
                false,
                false,
                false,
                false,
                chat.noforwards,
            ),
            tl::enums::Chat::Channel(channel) => from_channel(
                chat_id,
                channel.creator,
                channel.left,
                channel.broadcast,
                channel.restricted,
                channel.join_to_send,
                channel.noforwards,
                channel
                    .admin_rights
                    .as_ref()
                    .and_then(|rights| match rights {
                        tl::enums::ChatAdminRights::Rights(rights) => Some(rights),
                    }),
                channel
                    .banned_rights
                    .as_ref()
                    .and_then(|rights| match rights {
                        tl::enums::ChatBannedRights::Rights(rights) => Some(rights),
                    }),
                channel
                    .default_banned_rights
                    .as_ref()
                    .and_then(|rights| match rights {
                        tl::enums::ChatBannedRights::Rights(rights) => Some(rights),
                    }),
                channel.slowmode_enabled,
                channel.min,
            ),
            tl::enums::Chat::Forbidden(_) | tl::enums::Chat::ChannelForbidden(_) => {
                denied_snapshot(chat_id, ChatKind::Group)
            }
            tl::enums::Chat::Empty(_)
            | tl::enums::Chat::Community(_)
            | tl::enums::Chat::CommunityForbidden(_) => ChatCapabilities {
                chat_id,
                chat_kind: ChatKind::Unknown,
                ..ChatCapabilities::default()
            },
        },
        Peer::Channel(channel) => from_channel(
            chat_id,
            channel.raw.creator,
            channel.raw.left,
            true,
            channel.raw.restricted,
            channel.raw.join_to_send,
            channel.raw.noforwards,
            channel
                .raw
                .admin_rights
                .as_ref()
                .and_then(|rights| match rights {
                    tl::enums::ChatAdminRights::Rights(rights) => Some(rights),
                }),
            channel
                .raw
                .banned_rights
                .as_ref()
                .and_then(|rights| match rights {
                    tl::enums::ChatBannedRights::Rights(rights) => Some(rights),
                }),
            channel
                .raw
                .default_banned_rights
                .as_ref()
                .and_then(|rights| match rights {
                    tl::enums::ChatBannedRights::Rights(rights) => Some(rights),
                }),
            channel.raw.slowmode_enabled,
            channel.raw.min,
        ),
    }
}

/// Add slow-mode remaining time from the full-channel response when Telegram
/// exposes it. A failed or unavailable full fetch stays Unknown.
pub(crate) async fn fill_slow_mode(client: &Client, peer: &Peer, caps: &mut ChatCapabilities) {
    if caps.slow_mode_enabled != Value::Known(true) {
        return;
    }
    let channel = match peer {
        Peer::Channel(channel) => &channel.raw,
        Peer::Group(group) => match &group.raw {
            tl::enums::Chat::Channel(channel) => channel,
            _ => return,
        },
        _ => return,
    };
    let Some(access_hash) = channel.access_hash.filter(|_| !channel.min) else {
        return;
    };
    let result = client
        .invoke(&tl::functions::channels::GetFullChannel {
            channel: tl::enums::InputChannel::Channel(tl::types::InputChannel {
                channel_id: channel.id,
                access_hash,
            }),
        })
        .await;
    let Ok(full) = result else { return };
    let tl::enums::messages::ChatFull::Full(full) = full else {
        return;
    };
    let tl::enums::ChatFull::ChannelFull(full_chat) = full.full_chat else {
        return;
    };
    let Some(next_send) = full_chat.slowmode_next_send_date else {
        caps.slow_mode_remaining_seconds = Value::Known(None);
        return;
    };
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return;
    };
    let remaining = i64::from(next_send) - now.as_secs() as i64;
    caps.slow_mode_remaining_seconds = Value::Known(Some(remaining.max(0).min(86_400) as u32));
}

fn from_group(
    chat_id: i64,
    creator: bool,
    left: bool,
    admin: Option<&tl::types::ChatAdminRights>,
    banned: Option<&tl::types::ChatBannedRights>,
    defaults: Option<&tl::types::ChatBannedRights>,
    broadcast: bool,
    restricted: bool,
    join_to_send: bool,
    slow_mode_enabled: bool,
    noforwards: bool,
) -> ChatCapabilities {
    build_snapshot(
        chat_id,
        if broadcast {
            ChatKind::Channel
        } else {
            ChatKind::Group
        },
        creator,
        left,
        broadcast,
        restricted,
        join_to_send,
        noforwards,
        admin,
        banned,
        defaults,
        slow_mode_enabled,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn from_channel(
    chat_id: i64,
    creator: bool,
    left: bool,
    broadcast: bool,
    restricted: bool,
    join_to_send: bool,
    noforwards: bool,
    admin: Option<&tl::types::ChatAdminRights>,
    banned: Option<&tl::types::ChatBannedRights>,
    defaults: Option<&tl::types::ChatBannedRights>,
    slow_mode_enabled: bool,
    is_min: bool,
) -> ChatCapabilities {
    let mut capabilities = build_snapshot(
        chat_id,
        if broadcast {
            ChatKind::Channel
        } else {
            ChatKind::Supergroup
        },
        creator,
        left,
        broadcast,
        restricted,
        join_to_send,
        noforwards,
        admin,
        banned,
        defaults,
        slow_mode_enabled,
        is_min,
    );
    capabilities.slow_mode_enabled = if is_min {
        Value::Unknown
    } else {
        Value::Known(slow_mode_enabled)
    };
    capabilities.slow_mode_remaining_seconds = if is_min || slow_mode_enabled {
        Value::Unknown
    } else {
        Value::Known(None)
    };
    capabilities.refresh_derived();
    capabilities
}

#[allow(clippy::too_many_arguments)]
fn build_snapshot(
    chat_id: i64,
    chat_kind: ChatKind,
    creator: bool,
    left: bool,
    broadcast: bool,
    restricted: bool,
    join_to_send: bool,
    noforwards: bool,
    admin: Option<&tl::types::ChatAdminRights>,
    banned: Option<&tl::types::ChatBannedRights>,
    defaults: Option<&tl::types::ChatBannedRights>,
    slow_mode_enabled: bool,
    is_min: bool,
) -> ChatCapabilities {
    if is_min {
        return ChatCapabilities {
            chat_id,
            chat_kind,
            can_forward_messages: Value::Known(!noforwards),
            slow_mode_enabled: Value::Unknown,
            ..ChatCapabilities::default()
        };
    }
    let admin = admin;
    let member_banned =
        |field: fn(&tl::types::ChatBannedRights) -> bool| banned.map(field).or_else(|| Some(false));
    let default_banned = |field: fn(&tl::types::ChatBannedRights) -> bool| {
        defaults.map(field).or_else(|| Some(false))
    };
    let denied = left || restricted;
    let permission = |admin_value: Option<bool>, member: Option<bool>, default: Option<bool>| {
        if denied {
            Value::Known(false)
        } else {
            effective_permission(admin_value, member, default)
        }
    };
    let can_send_messages = if broadcast {
        if creator {
            Value::Known(true)
        } else {
            Value::Known(admin.is_some_and(|rights| rights.post_messages))
        }
    } else if join_to_send {
        Value::Known(false)
    } else {
        permission(
            None,
            member_banned(|r| r.send_messages),
            default_banned(|r| r.send_messages),
        )
    };
    let can_send_media = if broadcast {
        can_send_messages
    } else {
        permission(
            None,
            member_banned(|r| r.send_media),
            default_banned(|r| r.send_media),
        )
    };
    let can_send_polls = permission(
        None,
        member_banned(|r| r.send_polls),
        default_banned(|r| r.send_polls),
    );
    let can_send_stickers = permission(
        None,
        member_banned(|r| r.send_stickers),
        default_banned(|r| r.send_stickers),
    );
    let admin_permission = |field: fn(&tl::types::ChatAdminRights) -> bool,
                            member: Option<bool>,
                            default: Option<bool>| {
        if denied {
            Value::Known(false)
        } else if let Some(rights) = admin {
            Value::Known(creator || field(rights))
        } else if broadcast {
            Value::Known(false)
        } else {
            effective_permission(None, member, default)
        }
    };
    let can_manage_chat = if denied {
        Value::Known(false)
    } else if creator {
        Value::Known(true)
    } else if let Some(rights) = admin {
        Value::Known(
            rights.change_info
                || rights.ban_users
                || rights.add_admins
                || rights.manage_call
                || rights.manage_topics
                || rights.other,
        )
    } else {
        Value::Known(false)
    };
    let banned_status = if denied {
        Value::Known(true)
    } else {
        Value::Known(banned.is_some_and(|rights| rights.view_messages))
    };
    let read_only = if left || (broadcast && can_send_messages == Value::Known(false)) {
        Value::Known(true)
    } else if denied {
        Value::Known(true)
    } else {
        Value::Known(false)
    };
    let mut capabilities = ChatCapabilities {
        chat_id,
        chat_kind,
        can_send_messages,
        can_send_media,
        can_send_polls,
        can_send_stickers,
        can_pin_messages: admin_permission(
            |r| r.pin_messages,
            member_banned(|r| r.pin_messages),
            default_banned(|r| r.pin_messages),
        ),
        can_delete_messages: admin.map_or(Value::Unknown, |rights| {
            Value::Known(creator || rights.delete_messages)
        }),
        can_edit_messages: admin.map_or(Value::Unknown, |rights| {
            Value::Known(creator || rights.edit_messages)
        }),
        can_forward_messages: Value::Known(!noforwards),
        can_invite_users: admin_permission(
            |r| r.invite_users,
            member_banned(|r| r.invite_users),
            default_banned(|r| r.invite_users),
        ),
        can_manage_chat,
        can_post_messages: if broadcast {
            if creator {
                Value::Known(true)
            } else {
                admin.map_or(Value::Known(false), |rights| {
                    Value::Known(rights.post_messages)
                })
            }
        } else {
            Value::Unknown
        },
        can_edit_others_messages: admin.map_or(Value::Known(false), |rights| {
            Value::Known(creator || rights.edit_messages)
        }),
        read_only,
        restricted: Value::Known(restricted || left),
        banned: banned_status,
        slow_mode_enabled: if chat_kind == ChatKind::Supergroup || chat_kind == ChatKind::Channel {
            Value::Known(slow_mode_enabled)
        } else {
            Value::Known(false)
        },
        slow_mode_remaining_seconds: if slow_mode_enabled {
            Value::Unknown
        } else {
            Value::Known(None)
        },
        ..ChatCapabilities::default()
    };
    capabilities.refresh_derived();
    capabilities
}

fn effective_permission(
    admin: Option<bool>,
    member_banned: Option<bool>,
    default_banned: Option<bool>,
) -> Value<bool> {
    if let Some(allowed) = admin {
        return Value::Known(allowed);
    }
    if member_banned == Some(true) || default_banned == Some(true) {
        return Value::Known(false);
    }
    if member_banned == Some(false) || default_banned == Some(false) {
        return Value::Known(true);
    }
    Value::Unknown
}

fn denied_snapshot(chat_id: i64, chat_kind: ChatKind) -> ChatCapabilities {
    ChatCapabilities {
        chat_id,
        chat_kind,
        can_send_messages: Value::Known(false),
        can_send_media: Value::Known(false),
        can_send_polls: Value::Known(false),
        can_send_stickers: Value::Known(false),
        can_pin_messages: Value::Known(false),
        can_delete_messages: Value::Known(false),
        can_edit_messages: Value::Known(false),
        can_forward_messages: Value::Known(false),
        can_invite_users: Value::Known(false),
        can_manage_chat: Value::Known(false),
        can_post_messages: Value::Known(false),
        can_edit_others_messages: Value::Known(false),
        read_only: Value::Known(true),
        restricted: Value::Known(true),
        banned: Value::Known(true),
        slow_mode_enabled: Value::Unknown,
        slow_mode_remaining_seconds: Value::Unknown,
        can_send_now: Value::Known(false),
    }
}

/// Private chats have no group-rights payload. Existing non-deleted dialog peers
/// remain usable through Telegram RPC, which is authoritative for send failures.
fn private_snapshot(chat_id: i64, can_send: bool) -> ChatCapabilities {
    let mut capabilities = ChatCapabilities {
        chat_id,
        chat_kind: ChatKind::Private,
        can_send_messages: Value::Known(can_send),
        can_send_media: Value::Known(can_send),
        can_send_polls: Value::Known(can_send),
        can_send_stickers: Value::Known(can_send),
        read_only: Value::Known(!can_send),
        restricted: Value::Known(false),
        banned: Value::Known(false),
        slow_mode_enabled: Value::Known(false),
        slow_mode_remaining_seconds: Value::Known(None),
        ..ChatCapabilities::default()
    };
    capabilities.refresh_derived();
    capabilities
}

#[cfg(test)]
mod tests {
    use super::{effective_permission, private_snapshot};
    use tg_mobile_core::CapabilityValue as Value;

    #[test]
    fn admin_rights_override_default_bans_and_unknown_stays_unknown() {
        assert_eq!(
            effective_permission(Some(true), Some(true), Some(true)),
            Value::Known(true)
        );
        assert_eq!(
            effective_permission(None, Some(true), Some(false)),
            Value::Known(false)
        );
        assert_eq!(effective_permission(None, None, None), Value::Unknown);
    }

    #[test]
    fn existing_private_peers_remain_rpc_sendable_without_group_rights() {
        let private = private_snapshot(42, true);
        assert_eq!(private.can_send_now, Value::Known(true));
        assert_eq!(private.can_send_media, Value::Known(true));
        let deleted = private_snapshot(42, false);
        assert_eq!(deleted.can_send_now, Value::Known(false));
    }
}
