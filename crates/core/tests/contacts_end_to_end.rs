use tg_mobile_core::{
    decode_command, encode_event, encode_state, Contact, CoreCommand, CoreEvent, CoreState,
    JsonFileStateStore, StateStore, MAX_CONTACT_PAGE_LIMIT,
};

fn contact(user_id: i64, display_name: &str) -> Contact {
    Contact {
        user_id,
        display_name: display_name.into(),
        username: None,
        is_mutual: false,
        is_bot: false,
        avatar_photo_id: None,
    }
}

#[test]
fn load_contacts_command_round_trips_through_the_canonical_wire() {
    let command = CoreCommand::LoadContacts {
        offset: 100,
        limit: MAX_CONTACT_PAGE_LIMIT,
    };
    let json = serde_json::to_string(&command).unwrap();
    assert_eq!(decode_command(&json).unwrap(), command);
    assert!(CoreCommand::LoadContacts {
        offset: 0,
        limit: 0
    }
    .validate()
    .is_err());
    assert!(CoreCommand::LoadContacts {
        offset: 0,
        limit: MAX_CONTACT_PAGE_LIMIT + 1,
    }
    .validate()
    .is_err());
}

#[test]
fn refreshed_contact_pages_reduce_to_canonical_jni_state_without_phone_fields() {
    let mut state = CoreState::default();
    let first = CoreEvent::ContactsLoaded {
        offset: 0,
        total: 2,
        contacts: vec![contact(41, "Ada Lovelace")],
    };
    assert!(encode_event(&first).unwrap().contains("contacts_loaded"));
    state.reduce(&first);
    state.reduce(&CoreEvent::ContactsLoaded {
        offset: 1,
        total: 2,
        contacts: vec![contact(42, "Grace Hopper")],
    });
    let encoded = encode_state(&state).unwrap();
    assert!(encoded.contains("Ada Lovelace"));
    assert!(encoded.contains("Grace Hopper"));
    assert!(!encoded.contains("phone"));

    let refreshed = CoreEvent::ContactsLoaded {
        offset: 0,
        total: 1,
        contacts: vec![contact(99, "New contact")],
    };
    state.reduce(&refreshed);
    assert_eq!(state.contacts.len(), 1);
    assert!(state.contacts.get(41).is_none());
    assert!(state.contacts.get(99).is_some());
}

#[tokio::test]
async fn contact_snapshot_is_live_but_omitted_from_persistence() {
    let path = std::env::temp_dir().join(format!("tg-mobile-contacts-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let store = JsonFileStateStore::new(&path);
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ContactsLoaded {
        offset: 0,
        total: 1,
        contacts: vec![contact(123, "Linus Torvalds")],
    });
    assert!(tg_mobile_core::encode_state(&state)
        .unwrap()
        .contains("Linus Torvalds"));
    store.save(&state).await.unwrap();
    let persisted = std::fs::read_to_string(&path).unwrap();
    assert!(!persisted.contains("Linus Torvalds"));
    assert!(!persisted.contains("avatar_photo_id"));
    let restored = store.load().await.unwrap().unwrap();
    assert!(restored.contacts.is_empty());
    let _ = std::fs::remove_file(path);
}
