use crate::model::Conversation;
use clap::ValueEnum;

/// Controls which fields are included in the serialized output.
///
/// - `slim` – minimal payload: `id`, `title`, `state` only.
/// - `standard` – default output shape (same as pre-profile behaviour): all core fields.
/// - `rich` – full output including the optional `meta` and `attachments` groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputProfile {
    /// Minimal payload: id, title, state only (no body, comments, or metadata).
    Slim,
    /// Standard output (default): id, title, state, body, and comments.
    Standard,
    /// Full output: all fields including metadata and attachment references.
    Rich,
}

/// Apply an [`OutputProfile`] (or an explicit field list) to a conversation before
/// serialization.
///
/// Field names recognised by the filter:
/// `id`, `title`, `state`, `body`, `comments`, `meta`, `attachments`.
///
/// `id`, `title`, and `state` are always present; they cannot be removed.
///
/// When `fields` is `Some`, it takes precedence over `profile`.
pub fn project(
    mut conv: Conversation,
    profile: OutputProfile,
    fields: Option<&[String]>,
) -> Conversation {
    if let Some(fields) = fields {
        apply_field_filter(&mut conv, fields);
    } else {
        apply_profile(&mut conv, profile);
    }
    conv
}

fn apply_profile(conv: &mut Conversation, profile: OutputProfile) {
    match profile {
        OutputProfile::Slim => {
            conv.body = None;
            conv.comments.clear();
            conv.meta = None;
            conv.attachments = None;
        }
        OutputProfile::Standard => {
            // Standard is the default: preserve body and comments, strip optional groups.
            conv.meta = None;
            conv.attachments = None;
        }
        OutputProfile::Rich => {
            // Rich: keep everything; adapters populate meta/attachments where available.
        }
    }
}

fn apply_field_filter(conv: &mut Conversation, fields: &[String]) {
    if !fields.iter().any(|f| f == "body") {
        conv.body = None;
    }
    if !fields.iter().any(|f| f == "comments") {
        conv.comments.clear();
    }
    if !fields.iter().any(|f| f == "meta") {
        conv.meta = None;
    }
    if !fields.iter().any(|f| f == "attachments") {
        conv.attachments = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Attachment, Comment, Conversation, ConversationMeta};

    fn rich_conversation() -> Conversation {
        Conversation {
            id: "1".into(),
            title: "Test".into(),
            state: "open".into(),
            body: Some("Body text".into()),
            comments: vec![Comment {
                author: Some("alice".into()),
                created_at: "2024-01-01T00:00:00Z".into(),
                body: Some("A comment".into()),
                kind: None,
                review_path: None,
                review_line: None,
                review_side: None,
            }],
            meta: Some(ConversationMeta {
                url: Some("https://github.com/owner/repo/issues/1".into()),
                author: Some("bob".into()),
                created_at: Some("2024-01-01T00:00:00Z".into()),
                updated_at: Some("2024-01-02T00:00:00Z".into()),
                labels: Some(vec!["bug".into()]),
            }),
            attachments: Some(vec![Attachment {
                url: "https://example.com/file.png".into(),
                filename: Some("file.png".into()),
                content_type: Some("image/png".into()),
            }]),
        }
    }

    #[test]
    fn slim_profile_strips_body_comments_meta_attachments() {
        let projected = project(rich_conversation(), OutputProfile::Slim, None);
        assert!(projected.body.is_none());
        assert!(projected.comments.is_empty());
        assert!(projected.meta.is_none());
        assert!(projected.attachments.is_none());
        // Core fields preserved.
        assert_eq!(projected.id, "1");
        assert_eq!(projected.title, "Test");
        assert_eq!(projected.state, "open");
    }

    #[test]
    fn standard_profile_strips_meta_and_attachments_keeps_body_and_comments() {
        let projected = project(rich_conversation(), OutputProfile::Standard, None);
        assert!(projected.body.is_some());
        assert!(!projected.comments.is_empty());
        assert!(projected.meta.is_none());
        assert!(projected.attachments.is_none());
    }

    #[test]
    fn rich_profile_keeps_all_fields() {
        let projected = project(rich_conversation(), OutputProfile::Rich, None);
        assert!(projected.body.is_some());
        assert!(!projected.comments.is_empty());
        assert!(projected.meta.is_some());
        assert!(projected.attachments.is_some());
    }

    #[test]
    fn fields_filter_overrides_profile() {
        // With --fields body,comments, meta and attachments should be absent even for Rich.
        let fields = vec!["body".to_string(), "comments".to_string()];
        let projected = project(rich_conversation(), OutputProfile::Rich, Some(&fields));
        assert!(projected.body.is_some());
        assert!(!projected.comments.is_empty());
        assert!(projected.meta.is_none());
        assert!(projected.attachments.is_none());
    }

    #[test]
    fn fields_filter_meta_only() {
        let fields = vec!["meta".to_string()];
        let projected = project(rich_conversation(), OutputProfile::Rich, Some(&fields));
        assert!(projected.body.is_none());
        assert!(projected.comments.is_empty());
        assert!(projected.meta.is_some());
        assert!(projected.attachments.is_none());
    }

    #[test]
    fn fields_filter_preserves_core_id_title_state() {
        let fields: Vec<String> = vec![];
        let projected = project(rich_conversation(), OutputProfile::Slim, Some(&fields));
        assert_eq!(projected.id, "1");
        assert_eq!(projected.title, "Test");
        assert_eq!(projected.state, "open");
    }

    #[test]
    fn standard_profile_with_no_meta_is_noop_for_existing_output() {
        // Conversations without meta/attachments are unmodified by standard projection.
        let conv = Conversation {
            id: "2".into(),
            title: "Plain".into(),
            state: "closed".into(),
            body: Some("plain body".into()),
            comments: vec![],
            meta: None,
            attachments: None,
        };
        let projected = project(conv, OutputProfile::Standard, None);
        assert!(projected.meta.is_none());
        assert!(projected.attachments.is_none());
        assert_eq!(projected.body.as_deref(), Some("plain body"));
    }
}
