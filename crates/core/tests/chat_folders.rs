use std::collections::BTreeSet;

use tg_mobile_core::{ChatFolder, ChatFolderFacts, CoreState, FolderAction};

fn folder(id: i64, title: &str, position: u32) -> ChatFolder {
    ChatFolder {
        id,
        title: title.into(),
        included_chat_ids: BTreeSet::new(),
        excluded_chat_ids: BTreeSet::new(),
        include_contacts: false,
        include_non_contacts: false,
        include_groups: false,
        include_channels: false,
        include_bots: false,
        include_muted: false,
        include_read: false,
        include_archived: false,
        position,
    }
}

fn facts(id: i64) -> ChatFolderFacts {
    ChatFolderFacts {
        id,
        is_user: false,
        is_contact: false,
        is_group: false,
        is_channel: false,
        is_bot: false,
        muted: false,
        read: false,
        archived: false,
    }
}

#[test]
fn category_and_status_filters_are_evaluated_without_backend_assumptions() {
    let mut folder = folder(1, "Work", 0);
    folder.include_contacts = true;
    folder.include_groups = true;
    folder.include_muted = true;

    let mut contact = facts(10);
    contact.is_user = true;
    contact.is_contact = true;
    assert!(folder.matches(&contact));

    let mut non_contact = facts(13);
    non_contact.is_user = true;
    folder.include_non_contacts = true;
    assert!(folder.matches(&non_contact));

    let mut muted_group = facts(11);
    muted_group.is_group = true;
    muted_group.muted = true;
    assert!(folder.matches(&muted_group));

    let mut read_group = muted_group;
    read_group.read = true;
    assert!(!folder.matches(&read_group));
    folder.include_read = true;
    assert!(folder.matches(&read_group));

    let mut archived = facts(12);
    archived.is_channel = true;
    archived.archived = true;
    assert!(!folder.matches(&archived));
    folder.include_channels = true;
    folder.include_archived = true;
    assert!(folder.matches(&archived));
}

#[test]
fn explicit_include_and_exclude_have_stable_precedence() {
    let mut folder = folder(1, "Pinned", 0);
    folder.included_chat_ids.insert(10);
    folder.excluded_chat_ids.insert(11);
    folder.included_chat_ids.insert(11);
    let mut included = facts(10);
    included.archived = true;
    assert!(folder.matches(&included));
    let mut excluded = facts(11);
    excluded.is_user = true;
    excluded.is_contact = true;
    folder.include_contacts = true;
    assert!(!folder.matches(&excluded));
}

#[test]
fn reducer_orders_upserts_reorders_and_removals_deterministically() {
    let mut state = CoreState::default();
    state
        .reduce_folder(FolderAction::Upsert(folder(1, "First", 0)))
        .unwrap();
    state
        .reduce_folder(FolderAction::Upsert(folder(2, "Second", 1)))
        .unwrap();
    state
        .reduce_folder(FolderAction::Upsert(folder(3, "Before", 0)))
        .unwrap();
    assert_eq!(
        state
            .folders
            .iter()
            .map(|item| (item.id, item.position))
            .collect::<Vec<_>>(),
        [(3, 0), (1, 1), (2, 2)]
    );

    state
        .reduce_folder(FolderAction::Reorder {
            folder_ids: vec![2, 3, 1],
        })
        .unwrap();
    assert_eq!(
        state.folders.iter().map(|item| item.id).collect::<Vec<_>>(),
        [2, 3, 1]
    );
    state
        .reduce_folder(FolderAction::Remove { folder_id: 3 })
        .unwrap();
    assert_eq!(
        state
            .folders
            .iter()
            .map(|item| (item.id, item.position))
            .collect::<Vec<_>>(),
        [(2, 0), (1, 1)]
    );
    state.validate().unwrap();
}

#[test]
fn reducer_rejects_invalid_folder_data_and_incomplete_order() {
    let mut state = CoreState::default();
    assert!(state
        .reduce_folder(FolderAction::Upsert(folder(0, "Bad", 0)))
        .is_err());
    assert!(state
        .reduce_folder(FolderAction::Upsert(folder(1, "  ", 0)))
        .is_err());

    state
        .reduce_folder(FolderAction::Upsert(folder(1, "One", 0)))
        .unwrap();
    assert!(state
        .reduce_folder(FolderAction::Reorder { folder_ids: vec![] })
        .is_err());
    let mut conflicting = folder(2, "Two", 1);
    conflicting.included_chat_ids.insert(-9);
    conflicting.excluded_chat_ids.insert(-9);
    state
        .reduce_folder(FolderAction::Upsert(conflicting))
        .unwrap();
    let mut overlap = facts(-9);
    overlap.is_group = true;
    assert!(!state.folders[1].matches(&overlap));
}

#[test]
fn folder_membership_serialization_is_deterministic_and_defaults_old_data() {
    let mut value = folder(7, "Archive", 0);
    value.included_chat_ids.extend([30, 10, 20]);
    value.include_bots = true;
    let first = serde_json::to_string(&value).unwrap();
    let second = serde_json::to_string(&value).unwrap();
    assert_eq!(first, second);
    assert_eq!(serde_json::from_str::<ChatFolder>(&first).unwrap(), value);

    let old_state = r#"{
        "running": true,
        "account": {"authorization":"Closed","chats":[]},
        "messages": {},
        "last_error": null
    }"#;
    let restored: CoreState = serde_json::from_str(old_state).unwrap();
    assert!(restored.folders.is_empty());
    restored.validate().unwrap();
}
