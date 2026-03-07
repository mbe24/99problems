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

pub(super) fn map_issue_collection_links(items: &[Value], relation: &str) -> Vec<ConversationLink> {
    items
        .iter()
        .filter_map(|item| extract_link(Some(item), relation))
        .collect()
}

pub(super) fn map_issue_url_links(issue: &Value) -> Vec<ConversationLink> {
    let mut links = Vec::new();
    if let Some(pr) = issue.get("pull_request") {
        for field in ["html_url", "diff_url", "patch_url"] {
            if let Some(url) = pr.get(field).and_then(Value::as_str) {
                links.push(ConversationLink {
                    id: url.to_string(),
                    relation: "references".to_string(),
                    kind: Some("url".to_string()),
                });
            }
        }
    }

    links
}

pub(super) fn map_graphql_link_nodes(
    nodes: &[Value],
    relation: &str,
    kind: &str,
) -> Vec<ConversationLink> {
    nodes
        .iter()
        .filter_map(|node| {
            let id = node
                .get("number")
                .and_then(Value::as_u64)
                .map(|number| number.to_string())
                .or_else(|| node.get("url").and_then(Value::as_str).map(str::to_string))?;

            let kind = if id.parse::<u64>().is_ok() {
                kind.to_string()
            } else {
                "url".to_string()
            };
            Some(ConversationLink {
                id,
                relation: relation.to_string(),
                kind: Some(kind),
            })
        })
        .collect()
}

fn extract_link(node: Option<&Value>, relation: &str) -> Option<ConversationLink> {
    let id = node
        .and_then(|value| value.get("number"))
        .and_then(Value::as_u64)
        .map(|number| number.to_string())
        .or_else(|| extract_url(node))?;

    let kind = if id.parse::<u64>().is_ok() {
        Some(extract_kind(node).to_string())
    } else {
        Some("url".to_string())
    };

    Some(ConversationLink {
        id,
        relation: relation.to_string(),
        kind,
    })
}

fn extract_url(node: Option<&Value>) -> Option<String> {
    node.and_then(|value| {
        value
            .get("html_url")
            .and_then(Value::as_str)
            .or_else(|| value.get("url").and_then(Value::as_str))
            .map(std::string::ToString::to_string)
    })
}

fn extract_kind(node: Option<&Value>) -> &'static str {
    if node
        .and_then(|value| value.get("pull_request"))
        .is_some_and(|value| !value.is_null())
    {
        return "pr";
    }
    let raw_type = node
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        .or_else(|| {
            node.and_then(|value| value.get("__typename"))
                .and_then(Value::as_str)
        })
        .unwrap_or_default()
        .to_ascii_lowercase();
    if raw_type.contains("pull") {
        "pr"
    } else {
        "issue"
    }
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

    #[test]
    fn timeline_subject_url_without_number_maps_as_url_reference() {
        let event: Value = serde_json::json!({
            "event": "connected",
            "subject": {
                "url": "https://example.com/edge"
            }
        });
        let links = map_timeline_links(&event);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind.as_deref(), Some("url"));
        assert_eq!(links[0].id, "https://example.com/edge");
    }

    #[test]
    fn issue_url_links_include_issue_and_pr_urls() {
        let issue: Value = serde_json::json!({
            "html_url": "https://github.com/o/r/issues/1",
            "pull_request": {
                "html_url": "https://github.com/o/r/pull/1",
                "diff_url": "https://github.com/o/r/pull/1.diff",
                "patch_url": "https://github.com/o/r/pull/1.patch"
            }
        });
        let links = map_issue_url_links(&issue);
        assert!(links.iter().any(|l| l.id.ends_with("/pull/1")));
        assert!(links.iter().any(|l| {
            std::path::Path::new(&l.id)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("diff"))
        }));
        assert!(links.iter().all(|l| l.kind.as_deref() == Some("url")));
        assert!(links.iter().all(|l| l.relation == "references"));
    }

    #[test]
    fn graphql_link_nodes_maps_numeric_to_kind() {
        let nodes: Vec<Value> =
            vec![serde_json::json!({"number": 40, "url": "https://github.com/o/r/pull/40"})];
        let links = map_graphql_link_nodes(&nodes, "closed_by", "pr");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].id, "40");
        assert_eq!(links[0].relation, "closed_by");
        assert_eq!(links[0].kind.as_deref(), Some("pr"));
    }
}
