use tg_mobile_core::{
    spawn_core, CoreCommand, CoreError, MockTelegramBackend, MAX_CHAT_LOAD_LIMIT,
    MAX_MESSAGE_LOAD_LIMIT,
};

#[test]
fn command_validation_accepts_supported_inputs() {
    assert!(CoreCommand::Start.validate().is_ok());
    assert!(CoreCommand::SetPhoneNumber("test-phone".into())
        .validate()
        .is_ok());
    assert!(CoreCommand::SubmitCode("test-code".into())
        .validate()
        .is_ok());
    assert!(CoreCommand::SubmitPassword("password".into())
        .validate()
        .is_ok());
    assert!(CoreCommand::LoadChats {
        limit: MAX_CHAT_LOAD_LIMIT
    }
    .validate()
    .is_ok());
    assert!(CoreCommand::LoadMessages {
        chat_id: 1,
        limit: MAX_MESSAGE_LOAD_LIMIT,
    }
    .validate()
    .is_ok());
    assert!(CoreCommand::SendText {
        chat_id: 1,
        text: "hello".into(),
    }
    .validate()
    .is_ok());
    assert!(CoreCommand::Shutdown.validate().is_ok());
}

#[test]
fn command_validation_rejects_invalid_inputs() {
    let invalid = [
        CoreCommand::SetPhoneNumber("   ".into()),
        CoreCommand::SubmitCode("   ".into()),
        CoreCommand::SubmitPassword(String::new()),
        CoreCommand::LoadChats { limit: 0 },
        CoreCommand::LoadChats {
            limit: MAX_CHAT_LOAD_LIMIT + 1,
        },
        CoreCommand::LoadMessages {
            chat_id: 0,
            limit: 10,
        },
        CoreCommand::LoadMessages {
            chat_id: 1,
            limit: 0,
        },
        CoreCommand::LoadMessages {
            chat_id: 1,
            limit: MAX_MESSAGE_LOAD_LIMIT + 1,
        },
        CoreCommand::SendText {
            chat_id: 0,
            text: "hello".into(),
        },
        CoreCommand::SendText {
            chat_id: 1,
            text: "   ".into(),
        },
    ];

    for command in invalid {
        assert!(matches!(
            command.validate(),
            Err(CoreError::InvalidInput(_))
        ));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn core_handle_rejects_invalid_command_before_backend() {
    let core = spawn_core(MockTelegramBackend::default());

    let error = core
        .send(CoreCommand::SendText {
            chat_id: 0,
            text: "hello".into(),
        })
        .await
        .unwrap_err();

    assert!(matches!(error, CoreError::InvalidInput(_)));
}
