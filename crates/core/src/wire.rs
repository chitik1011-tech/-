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
    serde_json::to_string(state).map_err(|error| CoreError::Serialization(error.to_string()))
}
