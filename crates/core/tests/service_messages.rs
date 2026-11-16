use tg_mobile_core::{
    CoreEvent, CoreState, MessageSummary, ServiceAction, ServiceIcon, ServiceMessage,
    ServicePresentation,
};

fn message(service: Option<ServiceMessage>) -> MessageSummary {
    MessageSummary {
        id: 10,
        chat_id: 7,
        sender_id: 12,
        text: String::new(),
        timestamp_unix: 123,
        outgoing: false,
        attachments: Vec::new(),
        forwarded_from: None,
        edited: false,
        edit_timestamp_unix: None,
        service_message: service.is_some(),
        service,
        media_caption: None,
        reply_to: None,
    }
}

#[test]
fn typed_service_action_round_trips_through_event_and_persisted_state() {
    let action = ServiceAction::UserJoined {
        user_id: Some(12),
        inviter_id: Some(9),
    };
    let typed = ServiceMessage {
        presentation: ServicePresentation::for_action(&action),
        action: action.clone(),
        actor_id: Some(12),
    };
    let event = CoreEvent::MessageAdded(message(Some(typed.clone())));
    let wire = tg_mobile_core::encode_event(&event).unwrap();
    let decoded: CoreEvent = serde_json::from_str(&wire).unwrap();
    assert_eq!(decoded, event);

    let mut state = CoreState::default();
    state.reduce(&decoded);
    let saved = tg_mobile_core::encode_state(&state).unwrap();
    let restored: CoreState = serde_json::from_str(&saved).unwrap();
    let message = &restored.messages_for_chat(7)[0];
    assert_eq!(message.service.as_ref().unwrap(), &typed);
    assert_eq!(message.service.as_ref().unwrap().action, action);
    assert_eq!(
        message.service.as_ref().unwrap().presentation.icon,
        ServiceIcon::People
    );
    assert!(
        message.text.is_empty(),
        "service actions must not be flattened into message text"
    );
}

#[test]
fn old_messages_without_typed_metadata_still_deserialize() {
    let legacy = serde_json::json!({
        "id": 2,
        "chat_id": 7,
        "sender_id": 4,
        "text": "legacy",
        "timestamp_unix": 10,
        "outgoing": false,
        "service_message": true
    });
    let decoded: MessageSummary = serde_json::from_value(legacy).unwrap();
    assert!(decoded.service.is_none());
    assert!(decoded.service_message);
    assert_eq!(decoded.text, "legacy");
}

#[test]
fn malformed_service_action_is_rejected_without_corrupting_persisted_state() {
    let malformed = serde_json::json!({
        "id": 2,
        "chat_id": 7,
        "sender_id": 4,
        "text": "",
        "timestamp_unix": 10,
        "outgoing": false,
        "service": { "action": { "type": "user_added", "user_ids": "not-an-array" } }
    });
    assert!(serde_json::from_value::<MessageSummary>(malformed).is_err());
}

#[test]
fn every_action_maps_to_a_stable_presentation_icon() {
    let cases = [
        (
            ServiceAction::UserJoined {
                user_id: Some(1),
                inviter_id: None,
            },
            ServiceIcon::People,
        ),
        (
            ServiceAction::UserLeft { user_id: 1 },
            ServiceIcon::PersonLeave,
        ),
        (
            ServiceAction::UserRemoved { user_id: 1 },
            ServiceIcon::People,
        ),
        (
            ServiceAction::UserAdded { user_ids: vec![1] },
            ServiceIcon::People,
        ),
        (
            ServiceAction::TitleChanged { title: "x".into() },
            ServiceIcon::Edit,
        ),
        (
            ServiceAction::PhotoChanged { photo_id: Some(1) },
            ServiceIcon::Photo,
        ),
        (ServiceAction::PhotoRemoved, ServiceIcon::Photo),
        (
            ServiceAction::MessagePinned {
                message_id: Some(1),
            },
            ServiceIcon::Pin,
        ),
        (
            ServiceAction::GroupCreated {
                title: "x".into(),
                user_ids: vec![],
            },
            ServiceIcon::People,
        ),
        (
            ServiceAction::ChannelCreated { title: "x".into() },
            ServiceIcon::Channel,
        ),
        (
            ServiceAction::GroupMigratedToSupergroup { supergroup_id: -100 },
            ServiceIcon::Migration,
        ),
        (
            ServiceAction::ChannelMigratedFromGroup {
                group_id: 1,
                title: "x".into(),
            },
            ServiceIcon::Migration,
        ),
        (ServiceAction::HistoryCleared, ServiceIcon::Clear),
        (ServiceAction::Unknown, ServiceIcon::Info),
    ];
    for (action, icon) in cases {
        assert_eq!(ServicePresentation::for_action(&action).icon, icon);
    }
}
