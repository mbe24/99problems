use super::StreamFormatter;
use crate::model::Conversation;
use anyhow::Result;
use std::io::Write;

pub struct JsonLinesFormatter;

impl StreamFormatter for JsonLinesFormatter {
    fn begin(&mut self, _out: &mut dyn Write) -> Result<()> {
        Ok(())
    }

    fn write_item(&mut self, out: &mut dyn Write, conversation: &Conversation) -> Result<()> {
        serde_json::to_writer(&mut *out, conversation)?;
        out.write_all(b"\n")?;
        Ok(())
    }

    fn finish(&mut self, _out: &mut dyn Write) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Conversation, ConversationMetadata};

    #[test]
    fn emits_one_json_object_per_line() {
        let mut formatter = JsonLinesFormatter;
        let mut out = Vec::new();
        formatter.begin(&mut out).unwrap();
        formatter
            .write_item(
                &mut out,
                &Conversation {
                    id: "1".into(),
                    title: "t".into(),
                    state: "open".into(),
                    body: None,
                    comments: vec![],
                    metadata: ConversationMetadata::empty(),
                },
            )
            .unwrap();
        formatter.finish(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 1);
        let parsed: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(parsed["id"], "1");
    }
}
