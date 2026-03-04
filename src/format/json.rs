use super::Formatter;
use crate::model::Conversation;
use anyhow::Result;

pub struct JsonFormatter;

impl Formatter for JsonFormatter {
    fn format(&self, conversations: &[Conversation]) -> Result<String> {
        Ok(serde_json::to_string_pretty(conversations)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Comment;

    fn sample() -> Vec<Conversation> {
        vec![Conversation {
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
        }]
    }

    #[test]
    fn formats_valid_json() {
        let out = JsonFormatter.format(&sample()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed[0]["id"], "42");
        assert_eq!(parsed[0]["title"], "Test issue");
        assert_eq!(parsed[0]["comments"][0]["author"], "user1");
    }

    #[test]
    fn empty_slice_produces_empty_array() {
        let out = JsonFormatter.format(&[]).unwrap();
        assert_eq!(out.trim(), "[]");
    }
}
