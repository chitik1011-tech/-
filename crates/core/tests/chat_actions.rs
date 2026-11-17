use tg_mobile_core::{
    spawn_core, CoreCommand, CoreEvent, CoreState, MessageSummary, MockTelegramBackend,
};

async fn load_chats(core: &tg_mobile_core::CoreHandle) {
    core.send(CoreCommand::LoadChats { limit: 20 })
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let state = core.state();
            if state.chat(2).is_some() && state.chat_capabilities.contains_key(&2) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn authorize() -> tg_mobile_core::CoreHandle {
    let core = spawn_core(MockTelegramBackend::default());
    core.send(CoreCommand::Start).await.unwrap();
    core.send(CoreCommand::SetPhoneNumber("test-phone".into()))
        .await
        .unwrap();
    core.send(CoreCommand::SubmitCode("test-code".into()))
        .await
        .unwrap();
    load_chats(&core).await;
    core
}

async fn expect_chat_action_event(
    events: &mut tokio::sync::broadcast::Receiver<CoreEvent>,
    expected: CoreEvent,
) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            match events
                .recv()
                .await
                .expect("event stream closed before chat action completed")
            {
                // LoadChats can publish this after its state snapshot is visible to `load_chats`.
                // It is unrelated to the action under test; all other unexpected events fail.
                CoreEvent::ChatCapabilitiesUpdated(_) => {}
                CoreEvent::Error(error) => {
                    panic!("chat action emitted an error before {expected:?}: {error:?}")
                }
                event if event == expected => return,
                event => panic!("unexpected event before {expected:?}: {event:?}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for chat action event {expected:?}"));
}

async fn expect_chat_action_error(
    events: &mut tokio::sync::broadcast::Receiver<CoreEvent>,
    expected_message: &str,
) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            match events
                .recv()
                .await
                .expect("event stream closed before chat action failed")
            {
                // LoadChats can publish this after its state snapshot is visible to `load_chats`.
                // Ignore only that unrelated event; a successful action is an immediate failure.
                CoreEvent::ChatCapabilitiesUpdated(_) => {}
                CoreEvent::Error(error) if error.contains(expected_message) => return,
                CoreEvent::Error(error) => {
                    panic!("unexpected chat action error: {error:?}; expected {expected_message:?}")
                }
                event => panic!("unexpected event before chat action error: {event:?}"),
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for chat action error {expected_message:?}"));
}

#[test]
fn chat_action_commands_validate_ids_and_round_trip_on_wire() {
    let actions = [
        CoreCommand::ArchiveChat { chat_id: 7 },
        CoreCommand::UnarchiveChat { chat_id: 7 },
        CoreCommand::MuteChat { chat_id: 7 },
        CoreCommand::UnmuteChat { chat_id: 7 },
        CoreCommand::PinChat { chat_id: 7 },
        CoreCommand::UnpinChat { chat_id: 7 },
        CoreCommand::MarkChatUnread { chat_id: 7 },
        CoreCommand::MarkChatRead { chat_id: 7 },
    ];

    for action in actions {
        assert!(action.validate().is_ok());
        assert_eq!(
            tg_mobile_core::decode_command(&serde_json::to_string(&action).unwrap()).unwrap(),
            action
        );
    }

    for action in [
        CoreCommand::ArchiveChat { chat_id: 0 },
        CoreCommand::UnarchiveChat { chat_id: 0 },
        CoreCommand::MuteChat { chat_id: 0 },
        CoreCommand::UnmuteChat { chat_id: 0 },
        CoreCommand::PinChat { chat_id: 0 },
        CoreCommand::UnpinChat { chat_id: 0 },
        CoreCommand::MarkChatUnread { chat_id: 0 },
        CoreCommand::MarkChatRead { chat_id: 0 },
    ] {
        assert!(action.validate().is_err());
    }
}

#[test]
fn old_chat_payload_defaults_new_action_flags() {
    let chat: tg_mobile_core::ChatSummary =
        serde_json::from_str(r#"{"id":9,"title":"legacy","unread_count":0,"last_message":null}"#)
            .unwrap();

    assert_eq!(chat.archived, None);
    assert_eq!(chat.muted, None);
    assert_eq!(chat.pinned, None);
    assert_eq!(chat.is_unread, None);
}

#[test]
fn incoming_message_for_unknown_chat_does_not_invent_chat_metadata() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageAdded(MessageSummary {
        id: 1,
        chat_id: 404,
        sender_id: 9,
        text: "hello".into(),
        timestamp_unix: 1,
        outgoing: false,
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
    }));

    assert!(state.chat(404).is_none());
    assert_eq!(state.messages_for_chat(404).len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_actions_emit_events_and_update_state_without_optimistic_success() {
    let core = authorize().await;
    let mut events = core.subscribe();

    let actions = [
        (
            CoreCommand::ArchiveChat { chat_id: 2 },
            CoreEvent::ChatArchived {
                chat_id: 2,
                archived: true,
            },
        ),
        (
            CoreCommand::MuteChat { chat_id: 2 },
            CoreEvent::ChatMuted {
                chat_id: 2,
                muted: true,
            },
        ),
        (
            CoreCommand::PinChat { chat_id: 2 },
            CoreEvent::ChatPinned {
                chat_id: 2,
                pinned: true,
            },
        ),
        (
            CoreCommand::MarkChatUnread { chat_id: 2 },
            CoreEvent::ChatUnreadChanged {
                chat_id: 2,
                unread: true,
            },
        ),
    ];

    for (command, expected) in actions {
        core.send(command).await.unwrap();
        expect_chat_action_event(&mut events, expected).await;
    }

    let chat = core.state();
    let chat = chat.chat(2).unwrap();
    assert_eq!(chat.archived, Some(true));
    assert_eq!(chat.muted, Some(true));
    assert_eq!(chat.pinned, Some(true));
    assert_eq!(chat.is_unread, Some(true));

    for (command, expected) in [
        (
            CoreCommand::UnarchiveChat { chat_id: 2 },
            CoreEvent::ChatArchived {
                chat_id: 2,
                archived: false,
            },
        ),
        (
            CoreCommand::UnmuteChat { chat_id: 2 },
            CoreEvent::ChatMuted {
                chat_id: 2,
                muted: false,
            },
        ),
        (
            CoreCommand::UnpinChat { chat_id: 2 },
            CoreEvent::ChatPinned {
                chat_id: 2,
                pinned: false,
            },
        ),
        (
            CoreCommand::MarkChatRead { chat_id: 2 },
            CoreEvent::ChatUnreadChanged {
                chat_id: 2,
                unread: false,
            },
        ),
    ] {
        core.send(command).await.unwrap();
        expect_chat_action_event(&mut events, expected).await;
    }

    let chat = core.state();
    let chat = chat.chat(2).unwrap();
    assert_eq!(chat.archived, Some(false));
    assert_eq!(chat.muted, Some(false));
    assert_eq!(chat.pinned, Some(false));
    assert_eq!(chat.is_unread, Some(false));
    assert_eq!(chat.unread_count, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_chat_action_emits_error_and_keeps_cached_state_unchanged() {
    let core = authorize().await;
    let mut events = core.subscribe();

    core.send(CoreCommand::ArchiveChat { chat_id: 404 })
        .await
        .unwrap();
    expect_chat_action_error(&mut events, "404 not found").await;

    assert_eq!(core.state().chat(2).unwrap().archived, Some(false));
    assert!(core.state().chat(404).is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_action_requires_authorization() {
    let core = spawn_core(MockTelegramBackend::default());
    load_chats(&core).await;
    let mut events = core.subscribe();

    core.send(CoreCommand::MuteChat { chat_id: 2 })
        .await
        .unwrap();
    expect_chat_action_error(&mut events, "not authorized").await;
    assert_eq!(core.state().chat(2).unwrap().muted, Some(false));
}
