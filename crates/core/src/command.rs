#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreCommand {
    Start,
    SetPhoneNumber(String),
    SubmitCode(String),
    SubmitPassword(String),
    LoadChats { limit: usize },
    SendText { chat_id: i64, text: String },
    Shutdown,
}
