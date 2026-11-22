use tg_mobile_core::{CoreCommand, CoreError, decode_command};

#[test]
fn join_chat_command_validates_and_round_trips_public_and_invite_targets() {
    for input in ["@telegram_news", "https://t.me/community_name", "https://t.me/+AbC_123-x"] {
        let command = CoreCommand::JoinChat { input: input.into() };
        assert!(command.validate().is_ok(), "{input}");
        let encoded = serde_json::to_string(&command).unwrap();
        assert_eq!(decode_command(&encoded).unwrap(), command);
    }
}

#[test]
fn join_chat_command_rejects_invalid_links_before_the_backend() {
    for input in ["https://t.me.evil/+hash", "https://t.me/joinchat/bad.hash", ""] {
        let error = CoreCommand::JoinChat { input: input.into() }
            .validate()
            .unwrap_err();
        assert!(matches!(error, CoreError::InvalidInput(_)), "{input}");
    }
}
