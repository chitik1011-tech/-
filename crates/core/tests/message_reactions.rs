use tg_mobile_core::{
    decode_command, ChatSummary, CoreCommand, CoreEvent, CoreState, MessageReaction,
    MessageReactionType, MessageSummary, ReactionRevision,
};

const CHAT_ID: i64 = 72;
const MESSAGE_ID: i64 = 18;

fn message() -> MessageSummary {
    MessageSummary {
        id: MESSAGE_ID,
        chat_id: CHAT_ID,
        sender_id: 5,
        text: "hello".into(),
        timestamp_unix: 100,
        outgoing: false,
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

fn emoji(value: &str, count: u32, chosen_by_me: bool) -> MessageReaction {
    MessageReaction {
        reaction: MessageReactionType::Emoji(value.into()),
        count,
        chosen_by_me,
    }
}

fn revision(sequence: u64) -> ReactionRevision {
    ReactionRevision {
        token: Default::default(),
        sequence,
    }
}

fn loaded_state() -> CoreState {
    let mut state = CoreState::default();
    state.account.chats.push(ChatSummary {
        id: CHAT_ID,
        title: "Chat".into(),
        unread_count: 0,
        archived: None,
        muted: None,
        pinned: None,
        is_unread: None,
        last_message: None,
    });
    state.reduce(&CoreEvent::MessageAdded(message()));
    state
}

fn update(reactions: Vec<MessageReaction>, sequence: u64) -> CoreEvent {
    CoreEvent::MessageReactionsUpdated {
        chat_id: CHAT_ID,
        message_id: MESSAGE_ID,
        reactions,
        revision: revision(sequence),
    }
}

#[test]
fn reaction_update_updates_cached_message_and_chat_preview() {
    let mut state = loaded_state();
    let snapshot = vec![emoji("👍", 3, true), emoji("❤️", 2, false)];
    state.reduce(&update(snapshot.clone(), 1));

    assert_eq!(state.messages_for_chat(CHAT_ID)[0].reactions, snapshot);
    let preview = state.chat(CHAT_ID).unwrap().last_message.as_ref().unwrap();
    assert_eq!(preview.reactions, snapshot);
    assert_eq!(preview.reaction_revision, Some(revision(1)));
}

#[test]
fn duplicate_and_stale_snapshots_do_not_revert_reactions() {
    let mut state = loaded_state();
    let current = vec![emoji("❤️", 7, true)];
    state.reduce(&update(current.clone(), 4));
    state.reduce(&update(current.clone(), 4));
    state.reduce(&update(vec![emoji("👍", 1, false)], 3));

    assert_eq!(state.messages_for_chat(CHAT_ID)[0].reactions, current);
    assert_eq!(
        state.messages_for_chat(CHAT_ID)[0].reaction_revision,
        Some(revision(4))
    );
}

#[test]
fn custom_emoji_document_id_is_preserved_in_state_and_json() {
    let mut state = loaded_state();
    let custom = MessageReaction {
        reaction: MessageReactionType::CustomEmoji(9_876_543_210),
        count: 1,
        chosen_by_me: true,
    };
    state.reduce(&update(vec![custom.clone()], 1));
    let encoded = serde_json::to_string(&state).unwrap();
    let restored: CoreState = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        restored.messages_for_chat(CHAT_ID)[0].reactions,
        vec![custom]
    );
    assert!(restored.validate().is_ok());
}

#[test]
fn reaction_commands_validate_targets_and_round_trip_wire_format() {
    let command = CoreCommand::AddReaction {
        chat_id: CHAT_ID,
        message_id: MESSAGE_ID,
        reaction: MessageReactionType::Emoji("🔥".into()),
    };
    assert!(command.validate().is_ok());
    let json = serde_json::to_string(&command).unwrap();
    assert_eq!(decode_command(&json).unwrap(), command);

    let remove = CoreCommand::RemoveReaction {
        chat_id: CHAT_ID,
        message_id: MESSAGE_ID,
    };
    assert!(remove.validate().is_ok());
    assert!(CoreCommand::AddReaction {
        chat_id: 0,
        message_id: MESSAGE_ID,
        reaction: MessageReactionType::Emoji("👍".into()),
    }
    .validate()
    .is_err());
    assert!(CoreCommand::RemoveReaction {
        chat_id: CHAT_ID,
        message_id: 0,
    }
    .validate()
    .is_err());
    assert!(MessageReactionType::CustomEmoji(0).validate().is_err());
}

#[test]
fn authoritative_snapshot_survives_message_reload_without_reaction_metadata() {
    let mut state = loaded_state();
    let confirmed = vec![emoji("❤️", 6, true)];
    state.reduce(&update(confirmed.clone(), 1));
    state.reduce(&CoreEvent::MessagesLoaded {
        chat_id: CHAT_ID,
        messages: vec![message()],
    });

    assert_eq!(state.messages_for_chat(CHAT_ID)[0].reactions, confirmed);
    assert_eq!(
        state.messages_for_chat(CHAT_ID)[0].reaction_revision,
        Some(revision(1))
    );
}

#[test]
fn newer_connection_revision_wins_and_deleted_messages_cannot_be_resurrected() {
    let mut machine = tg_mobile_core::ConnectionMachine::default();
    let old_token = machine.token();
    let _ = machine.start_session();
    let new_token = machine.token();
    let mut state = loaded_state();
    let mk_update = |token, sequence, reactions| CoreEvent::MessageReactionsUpdated {
        chat_id: CHAT_ID,
        message_id: MESSAGE_ID,
        reactions,
        revision: ReactionRevision { token, sequence },
    };

    state.reduce(&mk_update(old_token, 50, vec![emoji("👍", 1, false)]));
    state.reduce(&mk_update(new_token, 1, vec![emoji("❤️", 2, true)]));
    state.reduce(&mk_update(old_token, 99, vec![emoji("🔥", 3, false)]));
    assert_eq!(
        state.messages_for_chat(CHAT_ID)[0].reactions,
        vec![emoji("❤️", 2, true)]
    );

    state.reduce(&CoreEvent::MessagesDeleted {
        chat_id: Some(CHAT_ID),
        message_ids: vec![MESSAGE_ID],
    });
    state.reduce(&mk_update(new_token, 2, vec![emoji("👍", 5, true)]));
    assert!(state.messages_for_chat(CHAT_ID).is_empty());
}

#[test]
fn invalid_duplicate_reactions_are_rejected_without_changing_snapshot() {
    let mut state = loaded_state();
    let initial = vec![emoji("👍", 1, false)];
    state.reduce(&update(initial.clone(), 1));
    state.reduce(&update(
        vec![emoji("🔥", 2, true), emoji("🔥", 3, false)],
        2,
    ));

    assert_eq!(state.messages_for_chat(CHAT_ID)[0].reactions, initial);
    assert!(state
        .last_error
        .as_deref()
        .unwrap_or_default()
        .contains("duplicate"));
}
