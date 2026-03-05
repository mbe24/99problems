use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueLink {
    pub id: String,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMetadata {
    pub links: Vec<IssueLink>,
    pub link_count: usize,
}

impl ConversationMetadata {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            links: vec![],
            link_count: 0,
        }
    }

    #[must_use]
    pub fn from_links(links: Vec<IssueLink>) -> Self {
        let link_count = links.len();
        Self { links, link_count }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_empty_has_zero_count_and_no_links() {
        let m = ConversationMetadata::empty();
        assert_eq!(m.link_count, 0);
        assert!(m.links.is_empty());
    }

    #[test]
    fn metadata_from_links_sets_count() {
        let links = vec![
            IssueLink {
                id: "42".into(),
                relation: "blocks".into(),
            },
            IssueLink {
                id: "99".into(),
                relation: "is blocked by".into(),
            },
        ];
        let m = ConversationMetadata::from_links(links);
        assert_eq!(m.link_count, 2);
        assert_eq!(m.links[0].id, "42");
        assert_eq!(m.links[1].relation, "is blocked by");
    }

    #[test]
    fn metadata_serializes_links_and_link_count() {
        let m = ConversationMetadata::from_links(vec![IssueLink {
            id: "1".into(),
            relation: "relates to".into(),
        }]);
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["link_count"], 1);
        assert_eq!(json["links"][0]["id"], "1");
        assert_eq!(json["links"][0]["relation"], "relates to");
    }
}
