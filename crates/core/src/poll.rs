use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// Telegram allows at most ten answers in one poll.
pub const MAX_POLL_OPTIONS: usize = 10;

/// One answer of a poll as reported by Telegram.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollOption {
    pub text: String,
    /// Votes for this answer. `None` when Telegram hides the results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voter_count: Option<u32>,
    /// Whether the current account selected this answer.
    #[serde(default)]
    pub chosen: bool,
    /// Quiz marking. `None` when the poll is not a quiz or Telegram does not reveal it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correct: Option<bool>,
}

/// Backend-neutral snapshot of a Telegram poll attached to a message.
///
/// `id` is the Telegram poll identifier and is unique across chats. Results are
/// replaced by later snapshots with the same identifier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollSummary {
    pub id: i64,
    pub chat_id: i64,
    pub message_id: i64,
    pub question: String,
    pub options: Vec<PollOption>,
    /// Total number of voters. `None` when Telegram does not report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_voters: Option<u32>,
    #[serde(default)]
    pub public_voters: bool,
    #[serde(default)]
    pub multiple_choice: bool,
    #[serde(default)]
    pub quiz: bool,
    #[serde(default)]
    pub closed: bool,
}

impl PollSummary {
    /// Rejects snapshots that Telegram cannot produce or that would make the
    /// UI show contradictory state.
    pub fn validate(&self) -> Result<()> {
        if self.id == 0 {
            return Err(CoreError::InvalidInput(
                "poll id must be non-zero".to_owned(),
            ));
        }
        if self.chat_id == 0 {
            return Err(CoreError::InvalidInput(
                "poll chat id must be non-zero".to_owned(),
            ));
        }
        if self.message_id <= 0 {
            return Err(CoreError::InvalidInput(
                "poll message id must be positive".to_owned(),
            ));
        }
        if self.question.trim().is_empty() {
            return Err(CoreError::InvalidInput(
                "poll question must not be empty".to_owned(),
            ));
        }
        if self.options.len() < 2 || self.options.len() > MAX_POLL_OPTIONS {
            return Err(CoreError::InvalidInput(format!(
                "poll must have between 2 and {MAX_POLL_OPTIONS} options"
            )));
        }
        if self
            .options
            .iter()
            .any(|option| option.text.trim().is_empty())
        {
            return Err(CoreError::InvalidInput(
                "poll option text must not be empty".to_owned(),
            ));
        }

        let chosen = self.options.iter().filter(|option| option.chosen).count();
        if chosen > 1 && !self.multiple_choice {
            return Err(CoreError::InvalidInput(
                "single-choice poll cannot have several chosen options".to_owned(),
            ));
        }
        if self.quiz && self.multiple_choice {
            return Err(CoreError::InvalidInput(
                "quiz poll cannot allow multiple answers".to_owned(),
            ));
        }
        if !self.quiz && self.options.iter().any(|option| option.correct.is_some()) {
            return Err(CoreError::InvalidInput(
                "only quiz polls can mark correct answers".to_owned(),
            ));
        }
        let correct = self
            .options
            .iter()
            .filter(|option| option.correct == Some(true))
            .count();
        if correct > 1 {
            return Err(CoreError::InvalidInput(
                "quiz poll can have at most one correct answer".to_owned(),
            ));
        }

        Ok(())
    }
}
