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
        is_contact: true,
        is_bot: false,
        avatar_photo_id: None,
    }
}

#[test]
fn load_contacts_command_round_trips_through_the_canonical_wire() {
    let command = CoreCommand::LoadContacts {
        offset: 100,
        limit: MAX_CONTACT_PAGE_LIMIT,
        query: None,
        request_id: None,
    };
    let json = serde_json::to_string(&command).unwrap();
    assert_eq!(decode_command(&json).unwrap(), command);
    assert_eq!(
        decode_command(r#"{"type":"load_contacts","payload":{"offset":0,"limit":100}}"#,).unwrap(),
        CoreCommand::LoadContacts {
            offset: 0,
            limit: 100,
            query: None,
            request_id: None,
        }
    );
    assert!(CoreCommand::LoadContacts {
        offset: 0,
        limit: 0,
        query: None,
        request_id: None,
    }
    .validate()
    .is_err());
    assert!(CoreCommand::LoadContacts {
        offset: 0,
        limit: MAX_CONTACT_PAGE_LIMIT + 1,
        query: None,
        request_id: None,
    }
    .validate()
    .is_err());
    let search = CoreCommand::LoadContacts {
        offset: 0,
        limit: 25,
        query: Some("ada_l".into()),
        request_id: Some(7),
    };
    assert!(search.validate().is_ok());
    assert!(CoreCommand::LoadContacts {
        offset: 0,
        limit: 25,
        query: Some("a".into()),
        request_id: Some(8),
    }
    .validate()
    .is_err());
    assert!(CoreCommand::LoadContacts {
        offset: 1,
        limit: 25,
        query: Some("ada".into()),
        request_id: Some(9),
    }
    .validate()
    .is_err());
    assert!(CoreCommand::LoadContacts {
        offset: 0,
        limit: 25,
        query: Some("ada".into()),
        request_id: None,
    }
    .validate()
    .is_err());
}

#[test]
fn add_contact_by_username_validates_and_round_trips_through_wire() {
    let command = CoreCommand::AddContactByUsername {
        username: "@Example_User".into(),
    };
    assert_eq!(command.kind(), "add_contact_by_username");
    assert!(command.validate().is_ok());
    assert!(CoreCommand::AddContactByUsername {
        username: " Example_User ".into(),
    }
    .validate()
    .is_ok());
    let encoded = serde_json::to_string(&command).unwrap();
    assert_eq!(decode_command(&encoded).unwrap(), command);

    for username in ["", "@bad name", "tg://user?id=42", "https://t.me/+invite"] {
        assert!(CoreCommand::AddContactByUsername {
            username: username.into(),
        }
        .validate()
        .is_err());
    }
}

#[test]
fn successful_contact_add_updates_book_even_while_a_list_request_is_active() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ContactsLoadStarted {
        request_id: 3,
        offset: 0,
        query: Some("unrelated search".into()),
    });
    let contact = contact(44, "Katherine Johnson");
    let event = CoreEvent::ContactAdded(contact.clone());
    let wire = encode_event(&event).unwrap();
    let decoded: CoreEvent = serde_json::from_str(&wire).unwrap();
    assert_eq!(decoded, event);

    state.reduce(&decoded);
    assert_eq!(state.contacts.get(44), Some(&contact));
    assert!(state.last_error.is_none());
}

#[test]
fn refreshed_contact_pages_reduce_to_canonical_jni_state_without_phone_fields() {
    let mut state = CoreState::default();
    let first = CoreEvent::ContactsLoaded {
        offset: 0,
        total: 2,
        contacts: vec![contact(41, "Ada Lovelace")],
        query: None,
        request_id: None,
    };
    assert!(encode_event(&first).unwrap().contains("contacts_loaded"));
    state.reduce(&first);
    state.reduce(&CoreEvent::ContactsLoaded {
        offset: 1,
        total: 2,
        contacts: vec![contact(42, "Grace Hopper")],
        query: None,
        request_id: None,
    });
    let encoded = encode_state(&state).unwrap();
    assert!(encoded.contains("Ada Lovelace"));
    assert!(encoded.contains("Grace Hopper"));
    assert!(!encoded.contains("phone"));

    let refreshed = CoreEvent::ContactsLoaded {
        offset: 0,
        total: 1,
        contacts: vec![contact(99, "New contact")],
        query: None,
        request_id: None,
    };
    state.reduce(&refreshed);
    assert_eq!(state.contacts.len(), 1);
    assert!(state.contacts.get(41).is_none());
    assert!(state.contacts.get(99).is_some());
}

#[test]
fn newer_contact_search_rejects_late_successes_and_failures() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ContactsLoadStarted {
        request_id: 10,
        offset: 0,
        query: Some("ada".into()),
    });
    state.reduce(&CoreEvent::ContactsLoadStarted {
        request_id: 11,
        offset: 0,
        query: Some("grace".into()),
    });
    state.reduce(&CoreEvent::ContactsLoadStarted {
        request_id: 9,
        offset: 0,
        query: Some("stale".into()),
    });

    state.reduce(&CoreEvent::ContactsLoaded {
        offset: 0,
        total: 1,
        contacts: vec![contact(41, "Ada Lovelace")],
        query: Some("ada".into()),
        request_id: Some(10),
    });
    state.reduce(&CoreEvent::ContactsLoadFailed {
        request_id: Some(10),
        offset: 0,
        query: Some("ada".into()),
        error: "stale failure".into(),
    });
    assert!(state.contacts.is_empty());
    assert!(state.last_error.is_none());

    let mut global_user = contact(42, "Grace Hopper");
    global_user.is_contact = false;
    state.reduce(&CoreEvent::ContactsLoaded {
        offset: 0,
        total: 1,
        contacts: vec![global_user],
        query: Some("grace".into()),
        request_id: Some(11),
    });
    assert_eq!(state.contacts.len(), 1);
    assert!(state.contacts.get(41).is_none());
    assert_eq!(
        state
            .contacts
            .get(42)
            .map(|contact| contact.display_name.as_str()),
        Some("Grace Hopper")
    );
    assert!(!state.contacts.get(42).unwrap().is_contact);
    state.reduce(&CoreEvent::ContactsLoadStarted {
        request_id: 10,
        offset: 0,
        query: Some("stale".into()),
    });
    state.reduce(&CoreEvent::ContactsLoadFailed {
        request_id: Some(10),
        offset: 0,
        query: Some("stale".into()),
        error: "late failure".into(),
    });
    assert!(state.last_error.is_none());
    assert!(state.contacts.get(42).is_some());

    state.reduce(&CoreEvent::ContactsLoadStarted {
        request_id: 12,
        offset: 0,
        query: Some("new query".into()),
    });
    state.reduce(&CoreEvent::ContactsLoadFailed {
        request_id: Some(12),
        offset: 0,
        query: Some("new query".into()),
        error: "search failed".into(),
    });
    assert_eq!(state.last_error.as_deref(), Some("search failed"));
    assert_eq!(state.contacts.len(), 1);
    assert_eq!(
        state
            .contacts
            .get(42)
            .map(|contact| contact.display_name.as_str()),
        Some("Grace Hopper")
    );
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
        query: None,
        request_id: None,
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
