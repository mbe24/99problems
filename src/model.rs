use serde::{Deserialize, Serialize};

/// Optional metadata enrichment for a conversation (populated in `rich` profile).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConversationMeta {
    /// Web URL of the issue or pull request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Login or display name of the issue/PR author.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// ISO 8601 creation timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// ISO 8601 last-update timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Labels applied to the issue or pull request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
}

/// An attachment reference (metadata/URL only; no file content is downloaded in v1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// URL of the attached resource.
    pub url: String,
    /// Original filename, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// MIME content type, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub state: String,
    pub body: Option<String>,
    pub comments: Vec<Comment>,
    /// Optional metadata group; present only in the `rich` profile or when requested via
    /// `--fields meta`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ConversationMeta>,
    /// Optional attachment references; present only in the `rich` profile or when requested
    /// via `--fields attachments`. No file content is downloaded (v1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<Attachment>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub author: Option<String>,
    pub created_at: String,
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_side: Option<String>,
}
