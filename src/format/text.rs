use super::StreamFormatter;
use crate::model::{Comment, Conversation, ConversationLink};
use anyhow::Result;
use std::io::Write;

#[derive(Default)]
pub struct TextFormatter {
    index: usize,
}

impl TextFormatter {
    #[must_use]
    pub fn new() -> Self {
        Self { index: 0 }
    }
}

impl StreamFormatter for TextFormatter {
    fn begin(&mut self, _out: &mut dyn Write) -> Result<()> {
        Ok(())
    }

    fn write_item(&mut self, out: &mut dyn Write, conversation: &Conversation) -> Result<()> {
        self.index += 1;
        writeln!(out, "Conversation {}", self.index)?;
        writeln!(out, "id: {}", conversation.id)?;
        writeln!(out, "title: {}", conversation.title)?;
        writeln!(out, "state: {}", conversation.state)?;
        writeln!(
            out,
            "body: {}",
            conversation.body.as_deref().unwrap_or("(none)")
        )?;
        writeln!(out, "comments: {}", conversation.comments.len())?;
        for (idx, comment) in conversation.comments.iter().enumerate() {
            render_comment(out, idx, comment)?;
        }
        writeln!(out, "links: {}", conversation.metadata.link_count)?;
        for (idx, link) in conversation.metadata.links.iter().enumerate() {
            render_link(out, idx, link)?;
        }
        writeln!(out, "---")?;
        Ok(())
    }

    fn finish(&mut self, _out: &mut dyn Write) -> Result<()> {
        Ok(())
    }
}

fn render_comment(out: &mut dyn Write, index: usize, comment: &Comment) -> Result<()> {
    writeln!(
        out,
        "  [{}] {} {}",
        index + 1,
        comment.created_at,
        comment.author.as_deref().unwrap_or("unknown")
    )?;
    if let Some(kind) = comment.kind.as_deref() {
        writeln!(out, "      kind: {kind}")?;
    }
    if let Some(path) = comment.review_path.as_deref() {
        writeln!(out, "      review_path: {path}")?;
    }
    if let Some(line) = comment.review_line {
        writeln!(out, "      review_line: {line}")?;
    }
    if let Some(side) = comment.review_side.as_deref() {
        writeln!(out, "      review_side: {side}")?;
    }
    writeln!(
        out,
        "      {}",
        comment.body.as_deref().unwrap_or("(no body)")
    )?;
    Ok(())
}

fn render_link(out: &mut dyn Write, index: usize, link: &ConversationLink) -> Result<()> {
    writeln!(
        out,
        "  [{}] relation={}{} id={}",
        index + 1,
        link.relation,
        link.kind
            .as_deref()
            .map_or(String::new(), |kind| format!(" kind={kind}")),
        link.id
    )?;
    Ok(())
}
