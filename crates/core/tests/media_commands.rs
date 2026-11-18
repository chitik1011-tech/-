use std::path::PathBuf;

use tg_mobile_core::{
    decode_command, AttachmentDownloadState, AttachmentKind, CoreCommand, CoreEvent, CoreState,
    LinkEvent, MediaAttachment, MessageSummary,
};

fn message_with_photo() -> MessageSummary {
    MessageSummary {
        id: 7,
        chat_id: 42,
        sender_id: 9,
        text: String::new(),
        timestamp_unix: 1,
        outgoing: false,
        reactions: Vec::new(),
        reaction_revision: None,
        location: None,
        attachments: vec![MediaAttachment {
            kind: AttachmentKind::Photo,
            file_reference: "photo:100".into(),
            size_bytes: Some(100),
            mime_type: Some("image/jpeg".into()),
            width: Some(640),
            height: Some(480),
            duration_ms: None,
            voice_waveform: None,
            filename: None,
            thumbnail: None,
            download_generation: 0,
            download_state: AttachmentDownloadState::NotDownloaded,
        }],
        forwarded_from: None,
        edited: false,
        edit_timestamp_unix: None,
        service_message: false,
        service: None,
        media_caption: None,
        reply_to: None,
    }
}

#[test]
fn media_commands_use_the_canonical_json_wire_format() {
    let cases = [
        (
            r#"{"type":"download_media","payload":{"chat_id":42,"message_id":7,"attachment_index":0}}"#,
            CoreCommand::DownloadMedia {
                chat_id: 42,
                message_id: 7,
                attachment_index: 0,
            },
        ),
        (
            r#"{"type":"retry_media_download","payload":{"chat_id":42,"message_id":7,"attachment_index":0}}"#,
            CoreCommand::RetryMediaDownload {
                chat_id: 42,
                message_id: 7,
                attachment_index: 0,
            },
        ),
        (
            r#"{"type":"cancel_media_download","payload":{"chat_id":42,"message_id":7,"attachment_index":0}}"#,
            CoreCommand::CancelMediaDownload {
                chat_id: 42,
                message_id: 7,
                attachment_index: 0,
            },
        ),
    ];

    for (json, expected) in cases {
        let decoded = decode_command(json).unwrap();
        assert_eq!(decoded, expected);
        decoded.validate().unwrap();
        assert_eq!(
            decode_command(&serde_json::to_string(&decoded).unwrap()).unwrap(),
            decoded
        );
    }
}

#[test]
fn invalid_media_targets_are_rejected_before_backend_dispatch() {
    assert!(CoreCommand::DownloadMedia {
        chat_id: 0,
        message_id: 7,
        attachment_index: 0,
    }
    .validate()
    .is_err());
    assert!(CoreCommand::DownloadMedia {
        chat_id: 42,
        message_id: 0,
        attachment_index: 0,
    }
    .validate()
    .is_err());
    assert!(CoreCommand::DownloadMedia {
        chat_id: 42,
        message_id: 7,
        attachment_index: 32,
    }
    .validate()
    .is_err());
}

#[test]
fn media_transfer_events_update_only_the_target_attachment() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageAdded(message_with_photo()));

    state.reduce(&CoreEvent::MediaTransferUpdated {
        chat_id: 42,
        message_id: 7,
        attachment_index: 0,
        session_generation: 0,
        generation: 0,
        state: AttachmentDownloadState::Downloading {
            bytes_transferred: 25,
            total_bytes: Some(100),
        },
    });
    assert_eq!(
        state.messages_for_chat(42)[0].attachments[0].download_state,
        AttachmentDownloadState::Downloading {
            bytes_transferred: 25,
            total_bytes: Some(100),
        }
    );

    let path = PathBuf::from("/private/cache/item.media");
    state.reduce(&CoreEvent::MediaTransferUpdated {
        chat_id: 42,
        message_id: 7,
        attachment_index: 0,
        session_generation: 0,
        generation: 0,
        state: AttachmentDownloadState::Downloaded {
            local_path: path.clone(),
        },
    });
    assert_eq!(
        state.messages_for_chat(42)[0].attachments[0].download_state,
        AttachmentDownloadState::Downloaded { local_path: path }
    );

    state.reduce(&CoreEvent::MediaTransferUpdated {
        chat_id: 42,
        message_id: 7,
        attachment_index: 0,
        session_generation: 0,
        generation: 1,
        state: AttachmentDownloadState::NotDownloaded,
    });
    assert_eq!(
        state.messages_for_chat(42)[0].attachments[0].download_state,
        AttachmentDownloadState::NotDownloaded
    );
}

#[test]
fn stale_progress_cannot_overwrite_cancellation_or_a_newer_retry() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::MessageAdded(message_with_photo()));

    state.reduce(&CoreEvent::MediaTransferUpdated {
        chat_id: 42,
        message_id: 7,
        attachment_index: 0,
        session_generation: 0,
        generation: 2,
        state: AttachmentDownloadState::Cancelled,
    });
    state.reduce(&CoreEvent::MediaTransferUpdated {
        chat_id: 42,
        message_id: 7,
        attachment_index: 0,
        session_generation: 0,
        generation: 1,
        state: AttachmentDownloadState::Downloading {
            bytes_transferred: 40,
            total_bytes: Some(100),
        },
    });
    assert_eq!(
        state.messages_for_chat(42)[0].attachments[0].download_state,
        AttachmentDownloadState::Cancelled
    );

    state.reduce(&CoreEvent::MediaTransferUpdated {
        chat_id: 42,
        message_id: 7,
        attachment_index: 0,
        session_generation: 0,
        generation: 3,
        state: AttachmentDownloadState::Queued,
    });
    state.reduce(&CoreEvent::MediaTransferUpdated {
        chat_id: 42,
        message_id: 7,
        attachment_index: 0,
        session_generation: 0,
        generation: 2,
        state: AttachmentDownloadState::Cancelled,
    });
    assert_eq!(
        state.messages_for_chat(42)[0].attachments[0].download_state,
        AttachmentDownloadState::Queued
    );
    assert_eq!(
        state.messages_for_chat(42)[0].attachments[0].download_generation,
        3
    );
}

#[test]
fn transfer_updates_from_a_replaced_account_cannot_mutate_same_id_messages() {
    let mut state = CoreState::default();
    state.reduce(&CoreEvent::ConnectionSessionStarted);
    for event in [
        LinkEvent::TransportConnected,
        LinkEvent::SyncStarted,
        LinkEvent::SyncCompleted,
    ] {
        let token = state.connection_machine.token();
        state.reduce(&CoreEvent::ConnectionLifecycle { token, event });
    }
    let old_session_generation = state.connection_machine.token().generation().as_u64();
    state.reduce(&CoreEvent::MessageAdded(message_with_photo()));

    state.reduce(&CoreEvent::ConnectionLifecycle {
        token: state.connection_machine.token(),
        event: LinkEvent::Logout,
    });
    state.reduce(&CoreEvent::ConnectionSessionStarted);
    let new_session_generation = state.connection_machine.token().generation().as_u64();
    assert_ne!(old_session_generation, new_session_generation);
    state.reduce(&CoreEvent::MessageAdded(message_with_photo()));

    for event in [
        LinkEvent::TransportConnected,
        LinkEvent::SyncStarted,
        LinkEvent::SyncCompleted,
        LinkEvent::TransportLost,
    ] {
        let token = state.connection_machine.token();
        state.reduce(&CoreEvent::ConnectionLifecycle { token, event });
    }
    assert_eq!(
        state.connection_machine.token().generation().as_u64(),
        new_session_generation,
        "a reconnect changes the connection epoch, not the account session"
    );

    state.reduce(&CoreEvent::MediaTransferUpdated {
        chat_id: 42,
        message_id: 7,
        attachment_index: 0,
        session_generation: old_session_generation,
        generation: 1,
        state: AttachmentDownloadState::Downloaded {
            local_path: PathBuf::from("/old-account/cache/item.media"),
        },
    });
    assert_eq!(
        state.messages_for_chat(42)[0].attachments[0].download_state,
        AttachmentDownloadState::NotDownloaded
    );

    state.reduce(&CoreEvent::MediaTransferUpdated {
        chat_id: 42,
        message_id: 7,
        attachment_index: 0,
        session_generation: new_session_generation,
        generation: 1,
        state: AttachmentDownloadState::Queued,
    });
    assert_eq!(
        state.messages_for_chat(42)[0].attachments[0].download_state,
        AttachmentDownloadState::Queued
    );
}
