use crate::{
    command::CoreCommand,
    error::{CoreError, Result},
    event::CoreEvent,
    state::CoreState,
};

pub fn decode_command(input: &str) -> Result<CoreCommand> {
    serde_json::from_str(input).map_err(|error| CoreError::Serialization(error.to_string()))
}

pub fn encode_event(event: &CoreEvent) -> Result<String> {
    serde_json::to_string(event).map_err(|error| CoreError::Serialization(error.to_string()))
}

pub fn encode_state(state: &CoreState) -> Result<String> {
    let mut value =
        serde_json::to_value(state).map_err(|error| CoreError::Serialization(error.to_string()))?;
    let Some(object) = value.as_object_mut() else {
        return Err(CoreError::Serialization(
            "core state did not serialize as an object".into(),
        ));
    };
    object.insert(
        "restore_complete".into(),
        serde_json::Value::Bool(state.restore_complete),
    );
    serde_json::to_string(&value).map_err(|error| CoreError::Serialization(error.to_string()))
}
