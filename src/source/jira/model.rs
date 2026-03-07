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

#[derive(Clone, Deserialize)]
pub(super) struct JiraIssueFields {
    pub(super) summary: String,
    pub(super) description: Option<Value>,
    pub(super) status: JiraStatus,
    pub(super) parent: Option<JiraLinkedIssue>,
    #[serde(default)]
    pub(super) subtasks: Vec<JiraLinkedIssue>,
    #[serde(default)]
    pub(super) issuelinks: Vec<JiraIssueLinkItem>,
    #[serde(default)]
    pub(super) attachment: Vec<JiraAttachmentItem>,
}

#[derive(Deserialize)]
pub(super) struct JiraKeySearchResponse {
    #[serde(rename = "startAt")]
    pub(super) start_at: Option<u32>,
    #[serde(rename = "maxResults")]
    pub(super) max_results: Option<u32>,
    pub(super) total: Option<u32>,
    #[serde(rename = "isLast")]
    pub(super) is_last: Option<bool>,
    #[serde(rename = "nextPageToken")]
    pub(super) next_page_token: Option<String>,
    pub(super) issues: Vec<JiraKeyIssue>,
}

#[derive(Deserialize)]
pub(super) struct JiraKeyIssue {
    pub(super) key: String,
}

#[derive(Clone, Deserialize)]
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

#[derive(Clone, Deserialize)]
pub(super) struct JiraAttachmentItem {
    pub(super) content: Option<String>,
}

#[derive(Clone, Deserialize)]
pub(super) struct JiraRemoteLinkItem {
    pub(super) relationship: Option<String>,
    pub(super) object: Option<JiraRemoteObject>,
}

#[derive(Clone, Deserialize)]
pub(super) struct JiraRemoteObject {
    pub(super) url: Option<String>,
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

pub(super) fn map_attachment_links(items: Vec<JiraAttachmentItem>) -> Vec<ConversationLink> {
    items
        .into_iter()
        .filter_map(|attachment| {
            attachment.content.map(|url| ConversationLink {
                id: url,
                relation: "attachment".to_string(),
                kind: Some("file".to_string()),
            })
        })
        .collect()
}

pub(super) fn map_remote_links(items: Vec<JiraRemoteLinkItem>) -> Vec<ConversationLink> {
    items
        .into_iter()
        .filter_map(|link| {
            let url = link.object.and_then(|object| object.url)?;
            let relation = link
                .relationship
                .as_deref()
                .map_or("references", normalize_remote_relation);
            Some(ConversationLink {
                kind: Some(infer_remote_kind(&url).to_string()),
                id: url,
                relation: relation.to_string(),
            })
        })
        .collect()
}

pub(super) fn map_parent_child_links(fields: &JiraIssueFields) -> Vec<ConversationLink> {
    let mut links = Vec::new();
    if let Some(parent) = &fields.parent {
        links.push(ConversationLink {
            id: parent.key.clone(),
            relation: "parent".to_string(),
            kind: Some("issue".to_string()),
        });
    }
    for child in &fields.subtasks {
        links.push(ConversationLink {
            id: child.key.clone(),
            relation: "child".to_string(),
            kind: Some("issue".to_string()),
        });
    }
    links
}

fn normalize_remote_relation(raw: &str) -> &'static str {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("block") && lower.contains("by") {
        "blocked_by"
    } else if lower.contains("block") {
        "blocks"
    } else if lower.contains("close") {
        "closes"
    } else {
        "references"
    }
}

fn infer_remote_kind(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("/-/merge_requests/")
        || lower.contains("/merge_requests/")
        || lower.contains("/pull/")
        || lower.contains("/pull-requests/")
    {
        "pr"
    } else {
        "url"
    }
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

    #[test]
    fn map_attachment_links_marks_kind_file() {
        let links = map_attachment_links(vec![JiraAttachmentItem {
            content: Some("https://example.com/file.txt".to_string()),
        }]);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind.as_deref(), Some("file"));
        assert_eq!(links[0].relation, "attachment");
    }

    #[test]
    fn map_remote_links_defaults_to_references() {
        let links = map_remote_links(vec![JiraRemoteLinkItem {
            relationship: Some("mentioned in".to_string()),
            object: Some(JiraRemoteObject {
                url: Some("https://example.com/pr/123".to_string()),
            }),
        }]);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind.as_deref(), Some("url"));
        assert_eq!(links[0].relation, "references");
    }

    #[test]
    fn map_remote_links_infers_pr_kind_for_merge_request_url() {
        let links = map_remote_links(vec![JiraRemoteLinkItem {
            relationship: Some("mentioned on".to_string()),
            object: Some(JiraRemoteObject {
                url: Some(
                    "https://gitlab.example.com/group/project/-/merge_requests/42".to_string(),
                ),
            }),
        }]);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind.as_deref(), Some("pr"));
        assert_eq!(links[0].relation, "references");
    }

    #[test]
    fn maps_parent_and_subtask_links() {
        let fields = JiraIssueFields {
            summary: "x".to_string(),
            description: None,
            status: JiraStatus {
                name: "Open".to_string(),
            },
            parent: Some(JiraLinkedIssue {
                key: "ABC-100".to_string(),
            }),
            subtasks: vec![JiraLinkedIssue {
                key: "ABC-101".to_string(),
            }],
            issuelinks: vec![],
            attachment: vec![],
        };
        let links = map_parent_child_links(&fields);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].relation, "parent");
        assert_eq!(links[1].relation, "child");
    }
}
