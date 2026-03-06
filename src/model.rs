use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub state: String,
    pub body: Option<String>,
    pub comments: Vec<Comment>,
    pub metadata: ConversationMetadata,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMetadata {
    pub links: Vec<ConversationLink>,
    pub link_count: usize,
}

impl ConversationMetadata {
    #[must_use]
    pub fn with_links(mut links: Vec<ConversationLink>) -> Self {
        links.sort_by(|a, b| {
            a.id.cmp(&b.id)
                .then_with(|| a.relation.cmp(&b.relation))
                .then_with(|| a.kind.cmp(&b.kind))
        });
        links.dedup_by(|a, b| a.id == b.id && a.relation == b.relation && a.kind == b.kind);
        let link_count = links.len();
        Self { links, link_count }
    }

    #[must_use]
    pub fn empty() -> Self {
        Self {
            links: Vec::new(),
            link_count: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationLink {
    pub id: String,
    pub relation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}
