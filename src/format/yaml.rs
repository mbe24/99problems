use anyhow::Result;
use crate::model::Conversation;
use super::Formatter;

pub struct YamlFormatter;

impl Formatter for YamlFormatter {
    fn format(&self, conversations: &[Conversation]) -> Result<String> {
        Ok(serde_yaml::to_string(conversations)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Comment;

    fn sample() -> Vec<Conversation> {
        vec![Conversation {
            id: 7,
            title: "YAML issue".into(),
            state: "open".into(),
            body: None,
            comments: vec![Comment {
                author: None,
                created_at: "2024-06-01T12:00:00Z".into(),
                body: Some("comment".into()),
            }],
        }]
    }

    #[test]
    fn formats_valid_yaml() {
        let out = YamlFormatter.format(&sample()).unwrap();
        assert!(out.contains("title: YAML issue"));
        assert!(out.contains("id: 7"));
    }

    #[test]
    fn empty_slice_produces_empty_yaml_list() {
        let out = YamlFormatter.format(&[]).unwrap();
        assert_eq!(out.trim(), "[]");
    }
}
