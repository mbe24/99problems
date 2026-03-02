use crate::model::Conversation;
use anyhow::Result;

pub mod json;
pub mod yaml;

/// A pluggable output formatter for conversations.
pub trait Formatter {
    fn format(&self, conversations: &[Conversation]) -> Result<String>;
}
