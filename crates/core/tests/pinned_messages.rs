use tg_mobile_core::{ChatSummary, CoreCommand, CoreEvent, CoreState};

#[test]
fn pin_commands_validate_ids_and_round_trip_over_the_canonical_wire() {
    let pin = CoreCommand::PinMessage {
        chat_id: -42,
        message_id: 8,
    };
    assert_eq!(pin.kind(), "pin_message");
    assert!(pin.validate().is_ok());
    assert!(CoreCommand::UnpinMessage {
        chat_id: 1,
        message_id: 0,
    }
    .validate()
    .is_err());
    assert!(CoreCommand::LoadPinnedMessages { chat_id: 0 }
        .validate()
        .is_err());

    let encoded = serde_json::to_string(&pin).expect("serialize command");
    assert_eq!(tg_mobile_core::decode_command(&encoded).unwrap(), pin);
}

#[test]
fn pinned_snapshots_and_updates_are_idempotent_and_reject_stale_pts() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::PinnedMessagesLoaded {
        chat_id: 9,
        message_ids: vec![7, 3, 7, 0],
        request_revision: Some(0),
    });
    assert_eq!(state.pinned_messages.get(&9), Some(&vec![3, 7]));

    state.reduce(&CoreEvent::PinnedMessagesChanged {
        chat_id: 9,
        message_ids: vec![11, 7],
        pinned: true,
        pts: Some(20),
    });
    state.reduce(&CoreEvent::PinnedMessagesChanged {
        chat_id: 9,
        message_ids: vec![7, 11],
        pinned: true,
        pts: Some(20),
    });
    assert_eq!(state.pinned_messages.get(&9), Some(&vec![3, 7, 11]));

    // A delayed earlier unpin cannot roll the newer server state back.
    state.reduce(&CoreEvent::PinnedMessagesChanged {
        chat_id: 9,
        message_ids: vec![7],
        pinned: false,
        pts: Some(19),
    });
    assert_eq!(state.pinned_messages.get(&9), Some(&vec![3, 7, 11]));

    state.reduce(&CoreEvent::PinnedMessagesChanged {
        chat_id: 9,
        message_ids: vec![7],
        pinned: false,
        pts: Some(21),
    });
    assert_eq!(state.pinned_messages.get(&9), Some(&vec![3, 11]));
}

#[test]
fn deleted_messages_are_removed_from_pinned_state_and_persistence_is_compatible() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::PinnedMessagesLoaded {
        chat_id: 9,
        message_ids: vec![3, 7],
        request_revision: Some(0),
    });
    state.reduce(&CoreEvent::MessagesDeleted {
        chat_id: Some(9),
        message_ids: vec![7],
    });
    assert_eq!(state.pinned_messages.get(&9), Some(&vec![3]));

    let serialized = serde_json::to_value(&state).expect("serialize state");
    let restored: CoreState = serde_json::from_value(serialized).expect("restore state");
    assert_eq!(restored.pinned_messages, state.pinned_messages);

    let mut old_snapshot = serde_json::to_value(CoreState::default()).unwrap();
    old_snapshot
        .as_object_mut()
        .unwrap()
        .remove("pinned_messages");
    old_snapshot
        .as_object_mut()
        .unwrap()
        .remove("pinned_message_pts");
    old_snapshot
        .as_object_mut()
        .unwrap()
        .remove("pinned_message_revision");
    let migrated: CoreState = serde_json::from_value(old_snapshot).unwrap();
    assert!(migrated.pinned_messages.is_empty());
    assert!(migrated.pinned_message_pts.is_empty());
    assert!(migrated.pinned_message_revision.is_empty());
}

#[test]
fn pin_failures_keep_the_exact_target_and_do_not_clear_other_pins() {
    let mut state = CoreState::default();
    state.pinned_messages.insert(9, vec![3, 7]);
    let failure = CoreEvent::PinnedMessageFailed {
        chat_id: 9,
        message_id: 7,
        pinned: false,
        error: "CHAT_ADMIN_REQUIRED".into(),
    };

    assert_eq!(failure.kind(), "pinned_message_failed");
    let wire = serde_json::to_value(&failure).expect("serialize pin failure");
    assert_eq!(wire["payload"]["chat_id"], 9);
    assert_eq!(wire["payload"]["message_id"], 7);
    assert_eq!(wire["payload"]["pinned"], false);
    state.reduce(&failure);
    assert_eq!(state.pinned_messages.get(&9), Some(&vec![3, 7]));
    assert_eq!(state.last_error.as_deref(), Some("CHAT_ADMIN_REQUIRED"));
}

#[test]
fn delayed_snapshot_cannot_restore_a_deleted_pinned_message() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::PinnedMessagesLoaded {
        chat_id: 9,
        message_ids: vec![3, 7],
        request_revision: Some(0),
    });
    let request_revision = state.pinned_message_revision[&9];

    state.reduce(&CoreEvent::MessagesDeleted {
        chat_id: Some(9),
        message_ids: vec![7],
    });
    state.reduce(&CoreEvent::PinnedMessagesLoaded {
        chat_id: 9,
        message_ids: vec![3, 7],
        request_revision: Some(request_revision),
    });

    assert_eq!(state.pinned_messages.get(&9), Some(&vec![3]));
}

#[test]
fn delayed_pinned_snapshot_cannot_roll_back_a_newer_update() {
    let mut state = CoreState::default();
    let request_revision = state
        .pinned_message_revision
        .get(&9)
        .copied()
        .unwrap_or_default();

    state.reduce(&CoreEvent::PinnedMessagesChanged {
        chat_id: 9,
        message_ids: vec![12],
        pinned: true,
        pts: Some(20),
    });
    state.reduce(&CoreEvent::PinnedMessagesLoaded {
        chat_id: 9,
        message_ids: vec![3],
        request_revision: Some(request_revision),
    });

    assert_eq!(state.pinned_messages.get(&9), Some(&vec![12]));
}

#[test]
fn no_pts_pin_rpc_ack_cannot_override_a_newer_unpin_update() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::PinnedMessagesLoaded {
        chat_id: 9,
        message_ids: vec![7, 8],
        request_revision: Some(0),
    });
    state.reduce(&CoreEvent::PinnedMessagesChanged {
        chat_id: 9,
        message_ids: vec![7],
        pinned: false,
        pts: Some(21),
    });
    // A delayed earlier PinMessage RPC acknowledgement has no pts and cannot
    // supersede Telegram's newer authoritative update.
    state.reduce(&CoreEvent::PinnedMessagesChanged {
        chat_id: 9,
        message_ids: vec![7],
        pinned: true,
        pts: None,
    });

    assert_eq!(state.pinned_messages.get(&9), Some(&vec![8]));
    assert_eq!(state.pinned_message_pts.get(&9), Some(&21));
}

#[test]
fn chats_loaded_prunes_pinned_bookkeeping_outside_the_bounded_chat_cache() {
    let mut state = CoreState::default();
    for chat_id in 1..=2_000 {
        state.reduce(&CoreEvent::PinnedMessagesChanged {
            chat_id,
            message_ids: vec![7],
            pinned: true,
            pts: Some(1),
        });
    }
    assert_eq!(state.pinned_message_pts.len(), 2_000);

    state.reduce(&CoreEvent::ChatsLoaded(vec![ChatSummary {
        id: 2_000,
        title: "recent chat".into(),
        unread_count: 0,
        archived: None,
        muted: None,
        pinned: None,
        is_unread: None,
        last_message: None,
    }]));

    assert_eq!(state.pinned_message_pts.keys().copied().collect::<Vec<_>>(), vec![2_000]);
    assert_eq!(state.pinned_message_revision.keys().copied().collect::<Vec<_>>(), vec![2_000]);
    assert_eq!(state.pinned_messages.keys().copied().collect::<Vec<_>>(), vec![2_000]);
}
