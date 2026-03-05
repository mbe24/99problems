use crate::model::Conversation;
use anyhow::Result;
use std::io::Write;

pub mod json;
pub mod jsonl;
pub mod text;
pub mod yaml;

/// A pluggable streaming formatter for conversations.
pub trait StreamFormatter {
    /// Write optional format prefix.
    ///
    /// # Errors
    ///
    /// Returns an error if writing fails.
    fn begin(&mut self, out: &mut dyn Write) -> Result<()>;

    /// Write one conversation item.
    ///
    /// # Errors
    ///
    /// Returns an error if writing fails.
    fn write_item(&mut self, out: &mut dyn Write, conversation: &Conversation) -> Result<()>;

    /// Write optional format suffix.
    ///
    /// # Errors
    ///
    /// Returns an error if writing fails.
    fn finish(&mut self, out: &mut dyn Write) -> Result<()>;
}
