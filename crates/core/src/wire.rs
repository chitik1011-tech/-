use crate::{
    command::CoreCommand,
    error::{CoreError, Result},
    event::CoreEvent,
    outgoing::OutgoingMessageCommand,
    state::CoreState,
};

pub fn decode_command(input: &str) -> Result<CoreCommand> {
    serde_json::from_str(input).map_err(|error| CoreError::Serialization(error.to_string()))
}

pub fn encode_event(event: &CoreEvent) -> Result<String> {
    serde_json::to_string(event).map_err(|error| CoreError::Serialization(error.to_string()))
}

pub fn encode_state(state: &CoreState) -> Result<String> {
    serde_json::to_string(state).map_err(|error| CoreError::Serialization(error.to_string()))
}


/// Decode a stable-ID message send request for the core-owned outbox.
pub fn decode_outgoing_command(input: &str) -> Result<OutgoingMessageCommand> {
    let command: OutgoingMessageCommand = serde_json::from_str(input)
        .map_err(|error| CoreError::Serialization(error.to_string()))?;
    command.validate()?;
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::{decode_outgoing_command, encode_event};
    use crate::{event::CoreEvent, outgoing::{OutgoingMessage, OutgoingMessageStatus}};

    #[test]
    fn decodes_send_text_with_stable_client_id() {
        let command = decode_outgoing_command(
            r#"{"type":"send_text_with_id","payload":{"client_id":"pending-1","chat_id":42,"text":"hello"}}"#,
        );
        assert!(matches!(
            command,
            Ok(crate::outgoing::OutgoingMessageCommand::SendTextWithId {
                client_id,
                chat_id: 42,
                text,
            }) if client_id == "pending-1" && text == "hello"
        ));
    }

    #[test]
    fn lifecycle_event_has_stable_wire_shape() {
        let event = CoreEvent::OutgoingMessageChanged(OutgoingMessage {
            client_id: "pending-1".into(),
            chat_id: 42,
            text: "hello".into(),
            status: OutgoingMessageStatus::Sending,
            server_message_id: None,
            error: None,
        });
        let value: serde_json::Value = serde_json::from_str(&encode_event(&event).unwrap()).unwrap();
        assert_eq!(value["type"], "outgoing_message_changed");
        assert_eq!(value["payload"]["client_id"], "pending-1");
        assert_eq!(value["payload"]["status"], "sending");
    }
}
