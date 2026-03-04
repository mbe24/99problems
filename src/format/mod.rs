use crate::model::Conversation;
use anyhow::Result;

pub mod json;
pub mod yaml;

/// A pluggable output formatter for conversations.
pub trait Formatter {
    /// Format conversations into a serialized output string.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    fn format(&self, conversations: &[Conversation]) -> Result<String>;
}
