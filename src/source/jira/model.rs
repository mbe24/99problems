use serde::Deserialize;
use serde_json::Value;

use crate::model::ConversationLink;

#[derive(Deserialize)]
pub(super) struct JiraSearchResponse {
    #[serde(rename = "startAt")]
    pub(super) start_at: Option<u32>,
    #[serde(rename = "maxResults")]
    pub(super) max_results: Option<u32>,
    pub(super) total: Option<u32>,
    #[serde(rename = "isLast")]
    pub(super) is_last: Option<bool>,
    #[serde(rename = "nextPageToken")]
    pub(super) next_page_token: Option<String>,
    pub(super) issues: Vec<JiraIssueItem>,
}

#[derive(Deserialize)]
pub(super) struct JiraIssueItem {
    pub(super) key: String,
    pub(super) fields: JiraIssueFields,
}

#[derive(Deserialize)]
pub(super) struct JiraIssueFields {
    pub(super) summary: String,
    pub(super) description: Option<Value>,
    pub(super) status: JiraStatus,
    #[serde(default)]
    pub(super) issuelinks: Vec<JiraIssueLinkItem>,
}

#[derive(Deserialize)]
pub(super) struct JiraStatus {
    pub(super) name: String,
}

#[derive(Deserialize)]
pub(super) struct JiraCommentsPage {
    #[serde(rename = "startAt")]
    pub(super) start_at: u32,
    #[serde(rename = "maxResults")]
    pub(super) max_results: u32,
    pub(super) total: u32,
    pub(super) comments: Vec<JiraCommentItem>,
}

#[derive(Deserialize)]
pub(super) struct JiraCommentItem {
    pub(super) author: Option<JiraAuthor>,
    pub(super) created: String,
    pub(super) body: Value,
}

#[derive(Deserialize)]
pub(super) struct JiraAuthor {
    #[serde(rename = "displayName")]
    pub(super) display_name: String,
}

#[derive(Clone, Deserialize)]
pub(super) struct JiraIssueLinkItem {
    #[serde(rename = "type")]
    pub(super) link_type: Option<JiraIssueLinkType>,
    #[serde(rename = "inwardIssue")]
    pub(super) inward_issue: Option<JiraLinkedIssue>,
    #[serde(rename = "outwardIssue")]
    pub(super) outward_issue: Option<JiraLinkedIssue>,
}

#[derive(Clone, Deserialize)]
pub(super) struct JiraIssueLinkType {
    pub(super) inward: Option<String>,
    pub(super) outward: Option<String>,
}

#[derive(Clone, Deserialize)]
pub(super) struct JiraLinkedIssue {
    pub(super) key: String,
}

pub(super) fn extract_adf_text(value: &Value) -> String {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(map) => {
                if let Some(Value::String(text)) = map.get("text") {
                    out.push(text.clone());
                }
                if let Some(content) = map.get("content") {
                    walk(content, out);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, out);
                }
            }
            _ => {}
        }
    }

    let mut chunks = Vec::new();
    walk(value, &mut chunks);
    chunks.join(" ").trim().to_string()
}

pub(super) fn map_issue_links(items: Vec<JiraIssueLinkItem>) -> Vec<ConversationLink> {
    let mut links = Vec::new();
    for item in items {
        if let Some(issue) = item.outward_issue {
            let relation = item
                .link_type
                .as_ref()
                .and_then(|kind| kind.outward.as_deref())
                .map_or("relates", normalize_relation);
            links.push(ConversationLink {
                id: issue.key,
                relation: relation.to_string(),
                kind: Some("issue".to_string()),
            });
        }
        if let Some(issue) = item.inward_issue {
            let relation = item
                .link_type
                .as_ref()
                .and_then(|kind| kind.inward.as_deref())
                .map_or("relates", normalize_relation);
            links.push(ConversationLink {
                id: issue.key,
                relation: relation.to_string(),
                kind: Some("issue".to_string()),
            });
        }
    }
    links
}

fn normalize_relation(raw: &str) -> &'static str {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("block") && lower.contains("by") {
        "blocked_by"
    } else if lower.contains("block") {
        "blocks"
    } else {
        "relates"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_adf_text_reads_nested_nodes() {
        let value: Value = serde_json::json!({
            "type": "doc",
            "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Hello"}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "world"}]}
            ]
        });
        assert_eq!(extract_adf_text(&value), "Hello world");
    }

    #[test]
    fn map_issue_links_normalizes_block_relations() {
        let links = map_issue_links(vec![
            JiraIssueLinkItem {
                link_type: Some(JiraIssueLinkType {
                    inward: Some("is blocked by".to_string()),
                    outward: Some("blocks".to_string()),
                }),
                inward_issue: Some(JiraLinkedIssue {
                    key: "ABC-1".to_string(),
                }),
                outward_issue: Some(JiraLinkedIssue {
                    key: "ABC-2".to_string(),
                }),
            },
            JiraIssueLinkItem {
                link_type: Some(JiraIssueLinkType {
                    inward: Some("relates to".to_string()),
                    outward: Some("relates to".to_string()),
                }),
                inward_issue: None,
                outward_issue: Some(JiraLinkedIssue {
                    key: "ABC-3".to_string(),
                }),
            },
        ]);

        assert_eq!(links.len(), 3);
        assert_eq!(links[0].id, "ABC-2");
        assert_eq!(links[0].relation, "blocks");
        assert_eq!(links[1].id, "ABC-1");
        assert_eq!(links[1].relation, "blocked_by");
        assert_eq!(links[2].relation, "relates");
    }
}
