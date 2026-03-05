use super::StreamFormatter;
use crate::model::Conversation;
use anyhow::Result;
use std::io::Write;

#[derive(Default)]
pub struct JsonStreamFormatter {
    wrote_item: bool,
}

impl JsonStreamFormatter {
    #[must_use]
    pub fn new() -> Self {
        Self { wrote_item: false }
    }
}

impl StreamFormatter for JsonStreamFormatter {
    fn begin(&mut self, out: &mut dyn Write) -> Result<()> {
        out.write_all(b"[\n")?;
        Ok(())
    }

    fn write_item(&mut self, out: &mut dyn Write, conversation: &Conversation) -> Result<()> {
        if self.wrote_item {
            out.write_all(b",\n")?;
        }
        let rendered = serde_json::to_string_pretty(conversation)?;
        out.write_all(rendered.as_bytes())?;
        self.wrote_item = true;
        Ok(())
    }

    fn finish(&mut self, out: &mut dyn Write) -> Result<()> {
        if self.wrote_item {
            out.write_all(b"\n]\n")?;
        } else {
            out.write_all(b"]\n")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Comment;

    fn sample() -> Conversation {
        Conversation {
            id: "42".into(),
            title: "Test issue".into(),
            state: "closed".into(),
            body: Some("Body text".into()),
            comments: vec![Comment {
                author: Some("user1".into()),
                created_at: "2024-01-01T00:00:00Z".into(),
                body: Some("A comment".into()),
                kind: None,
                review_path: None,
                review_line: None,
                review_side: None,
            }],
            meta: None,
            attachments: None,
        }
    }

    #[test]
    fn formats_valid_json_array() {
        let mut formatter = JsonStreamFormatter::new();
        let mut out = Vec::new();
        formatter.begin(&mut out).unwrap();
        formatter.write_item(&mut out, &sample()).unwrap();
        formatter.finish(&mut out).unwrap();

        let parsed: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(parsed[0]["id"], "42");
    }

    #[test]
    fn empty_output_is_empty_array() {
        let mut formatter = JsonStreamFormatter::new();
        let mut out = Vec::new();
        formatter.begin(&mut out).unwrap();
        formatter.finish(&mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "[\n]\n");
    }
}
