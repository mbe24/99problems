use serde::Deserialize;
use serde_json::Value;

use crate::model::{Comment, ConversationLink};

#[derive(Deserialize)]
pub(super) struct SearchResponse {
    pub(super) items: Vec<SearchItem>,
}

#[derive(Deserialize)]
pub(super) struct SearchItem {
    pub(super) number: u64,
    pub(super) title: String,
    pub(super) state: String,
    pub(super) body: Option<String>,
    pub(super) repository_url: Option<String>,
    pub(super) pull_request: Option<PullRequestMarker>,
}

#[derive(Deserialize)]
pub(super) struct IssueItem {
    pub(super) number: u64,
    pub(super) title: String,
    pub(super) state: String,
    pub(super) body: Option<String>,
    pub(super) pull_request: Option<PullRequestMarker>,
}

#[derive(Deserialize)]
pub(super) struct PullRequestMarker {}

#[derive(Deserialize)]
pub(super) struct IssueCommentItem {
    pub(super) user: Option<UserItem>,
    pub(super) created_at: String,
    pub(super) body: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ReviewCommentItem {
    pub(super) user: Option<UserItem>,
    pub(super) created_at: String,
    pub(super) body: Option<String>,
    pub(super) path: Option<String>,
    pub(super) line: Option<u64>,
    pub(super) side: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct UserItem {
    pub(super) login: String,
}

pub(super) struct ConversationSeed {
    pub(super) id: u64,
    pub(super) title: String,
    pub(super) state: String,
    pub(super) body: Option<String>,
    pub(super) is_pr: bool,
}

pub(super) fn map_issue_comment(c: IssueCommentItem) -> Comment {
    Comment {
        author: c.user.map(|u| u.login),
        created_at: c.created_at,
        body: c.body,
        kind: Some("issue_comment".into()),
        review_path: None,
        review_line: None,
        review_side: None,
    }
}

pub(super) fn map_review_comment(c: ReviewCommentItem) -> Comment {
    Comment {
        author: c.user.map(|u| u.login),
        created_at: c.created_at,
        body: c.body,
        kind: Some("review_comment".into()),
        review_path: c.path,
        review_line: c.line,
        review_side: c.side,
    }
}

pub(super) fn map_timeline_links(event: &Value) -> Vec<ConversationLink> {
    let event_name = event
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let base_relation = normalize_relation(event_name);
    let mut links = Vec::new();

    if let Some(link) = extract_link(event.pointer("/source/issue"), base_relation) {
        links.push(link);
    }
    if let Some(link) = extract_link(event.get("subject"), base_relation) {
        links.push(link);
    }
    if let Some(link) = extract_link(event.get("blocking_issue"), "blocked_by") {
        links.push(link);
    }
    if let Some(link) = extract_link(event.get("blocked_issue"), "blocks") {
        links.push(link);
    }

    links
}

fn extract_link(node: Option<&Value>, relation: &str) -> Option<ConversationLink> {
    let number = node
        .and_then(|value| value.get("number"))
        .and_then(Value::as_u64)?;
    let kind = if node
        .and_then(|value| value.get("pull_request"))
        .is_some_and(|value| !value.is_null())
    {
        Some("pr".to_string())
    } else {
        Some("issue".to_string())
    };

    Some(ConversationLink {
        id: number.to_string(),
        relation: relation.to_string(),
        kind,
    })
}

fn normalize_relation(raw: &str) -> &str {
    let raw = raw.to_ascii_lowercase();
    if raw.contains("blocked_by") {
        "blocked_by"
    } else if raw.contains("block") || raw.contains("blocking") {
        "blocks"
    } else {
        "relates"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_cross_reference_maps_to_relates() {
        let event: Value = serde_json::json!({
            "event": "cross-referenced",
            "source": {
                "issue": {
                    "number": 42
                }
            }
        });
        let links = map_timeline_links(&event);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].id, "42");
        assert_eq!(links[0].relation, "relates");
        assert_eq!(links[0].kind.as_deref(), Some("issue"));
    }

    #[test]
    fn timeline_blocked_by_maps_relation() {
        let event: Value = serde_json::json!({
            "event": "blocked_by_added",
            "blocking_issue": {
                "number": 7
            }
        });
        let links = map_timeline_links(&event);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].id, "7");
        assert_eq!(links[0].relation, "blocked_by");
    }
}
