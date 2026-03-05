use super::StreamFormatter;
use crate::model::Conversation;
use anyhow::Result;
use std::io::Write;

#[derive(Default)]
pub struct YamlStreamFormatter {
    wrote_item: bool,
}

impl YamlStreamFormatter {
    #[must_use]
    pub fn new() -> Self {
        Self { wrote_item: false }
    }
}

impl StreamFormatter for YamlStreamFormatter {
    fn begin(&mut self, _out: &mut dyn Write) -> Result<()> {
        Ok(())
    }

    fn write_item(&mut self, out: &mut dyn Write, conversation: &Conversation) -> Result<()> {
        let rendered = serde_yaml::to_string(conversation)?;
        if self.wrote_item {
            out.write_all(b"\n")?;
        }
        for (idx, line) in rendered.lines().enumerate() {
            if idx == 0 {
                out.write_all(b"- ")?;
                out.write_all(line.as_bytes())?;
            } else {
                out.write_all(b"\n  ")?;
                out.write_all(line.as_bytes())?;
            }
        }
        out.write_all(b"\n")?;
        self.wrote_item = true;
        Ok(())
    }

    fn finish(&mut self, out: &mut dyn Write) -> Result<()> {
        if !self.wrote_item {
            out.write_all(b"[]\n")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Comment, ConversationMetadata};

    fn sample() -> Conversation {
        Conversation {
            id: "7".into(),
            title: "YAML issue".into(),
            state: "open".into(),
            body: None,
            comments: vec![Comment {
                author: None,
                created_at: "2024-06-01T12:00:00Z".into(),
                body: Some("comment".into()),
                kind: None,
                review_path: None,
                review_line: None,
                review_side: None,
            }],
            metadata: ConversationMetadata::empty(),
        }
    }

    #[test]
    fn formats_valid_yaml() {
        let mut formatter = YamlStreamFormatter::new();
        let mut out = Vec::new();
        formatter.begin(&mut out).unwrap();
        formatter.write_item(&mut out, &sample()).unwrap();
        formatter.finish(&mut out).unwrap();

        let parsed: serde_yaml::Value = serde_yaml::from_slice(&out).unwrap();
        assert_eq!(parsed[0]["title"], "YAML issue");
    }

    #[test]
    fn empty_output_is_empty_yaml_list() {
        let mut formatter = YamlStreamFormatter::new();
        let mut out = Vec::new();
        formatter.begin(&mut out).unwrap();
        formatter.finish(&mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "[]\n");
    }
}
