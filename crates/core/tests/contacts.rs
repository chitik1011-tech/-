use tg_mobile_core::contacts::{Contact, ContactBook};
use tg_mobile_core::{
    CoreCommand, CoreEvent, CoreState, MockTelegramBackend, TelegramBackend, MAX_CONTACT_PAGE_LIMIT,
};

fn contact(user_id: i64, name: &str, username: Option<&str>) -> Contact {
    Contact {
        user_id,
        display_name: name.into(),
        username: username.map(str::to_owned),
        is_imported: true,
        is_contact: true,
    }
}

#[test]
fn command_validation_rejects_bad_contacts_and_invalid_pages() {
    assert!(CoreCommand::AddContact(contact(0, "Person", None))
        .validate()
        .is_err());
    assert!(CoreCommand::AddContact(contact(1, " ", None))
        .validate()
        .is_err());
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
    assert!(CoreCommand::DeleteContact { user_id: 0 }
        .validate()
        .is_err());
}

#[test]
fn reducer_merges_pages_and_add_delete_events_idempotently() {
    let mut state = CoreState::default();
    let first = contact(2, "Two", Some("TwoName")).normalized().unwrap();
    let second = contact(1, "One", None);

    state.reduce(&CoreEvent::ContactsLoaded {
        offset: 0,
        total: 2,
        contacts: vec![first.clone(), second.clone()],
    });
    state.reduce(&CoreEvent::ContactsLoaded {
        offset: 0,
        total: 2,
        contacts: vec![first.clone(), second.clone()],
    });
    assert_eq!(state.contacts.len(), 2);
    assert_eq!(
        state.contacts.page(0, 10).unwrap(),
        vec![second, first.clone()]
    );

    state.reduce(&CoreEvent::ContactAdded(first.clone()));
    assert_eq!(state.contacts.len(), 2);
    state.reduce(&CoreEvent::ContactDeleted { user_id: 2 });
    state.reduce(&CoreEvent::ContactDeleted { user_id: 2 });
    assert_eq!(state.contacts.len(), 1);
    assert!(state.contacts.get(2).is_none());
}

#[test]
fn contacts_serialize_without_phone_number_fields() {
    let mut contacts = ContactBook::default();
    contacts.upsert(contact(42, "A Person", None)).unwrap();

    let json = serde_json::to_string(&contacts).unwrap();
    assert!(!json.to_ascii_lowercase().contains("phone"));
    assert!(json.contains("\"user_id\":42"));
}

#[tokio::test]
async fn mock_backend_returns_explicit_unsupported_error_for_contacts() {
    let mut backend = MockTelegramBackend::default();
    let result = backend
        .handle(CoreCommand::LoadContacts {
            offset: 0,
            limit: 20,
        })
        .await;
    assert!(result.is_err());
    let message = result.unwrap_err().to_string();
    assert!(message.contains("contacts backend is unavailable"));
}
