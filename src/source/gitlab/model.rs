use serde::Deserialize;
use serde_json::Value;

use crate::model::{Comment, ConversationLink};

#[derive(Deserialize)]
pub(super) struct GitLabIssueItem {
    pub(super) iid: u64,
    pub(super) title: String,
    pub(super) state: String,
    pub(super) description: Option<String>,
    pub(super) web_url: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct GitLabMergeRequestItem {
    pub(super) iid: u64,
    pub(super) title: String,
    pub(super) state: String,
    pub(super) description: Option<String>,
    pub(super) web_url: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct GitLabNote {
    pub(super) author: Option<GitLabAuthor>,
    pub(super) created_at: String,
    pub(super) body: String,
    pub(super) system: bool,
}

#[derive(Deserialize)]
pub(super) struct GitLabAuthor {
    pub(super) username: String,
}

#[derive(Deserialize)]
pub(super) struct GitLabDiscussion {
    pub(super) notes: Vec<GitLabDiscussionNote>,
}

#[derive(Deserialize)]
pub(super) struct GitLabDiscussionNote {
    pub(super) id: u64,
    pub(super) author: Option<GitLabAuthor>,
    pub(super) created_at: String,
    pub(super) body: String,
    pub(super) system: bool,
    pub(super) position: Option<GitLabPosition>,
}

#[derive(Deserialize)]
pub(super) struct GitLabPosition {
    pub(super) new_path: Option<String>,
    pub(super) old_path: Option<String>,
    pub(super) new_line: Option<u64>,
    pub(super) old_line: Option<u64>,
}

#[derive(Deserialize)]
pub(super) struct GitLabIssueLinkItem {
    pub(super) link_type: Option<String>,
    pub(super) source_issue: Option<GitLabLinkIssueRef>,
    pub(super) target_issue: Option<GitLabLinkIssueRef>,
}

#[derive(Clone, Deserialize)]
pub(super) struct GitLabLinkIssueRef {
    pub(super) iid: u64,
}

#[derive(Clone, Deserialize)]
pub(super) struct GitLabRelatedIssueRef {
    pub(super) iid: Option<u64>,
    pub(super) id: Option<Value>,
    pub(super) web_url: Option<String>,
}

#[derive(Clone, Deserialize)]
pub(super) struct GitLabMergeRequestRef {
    pub(super) iid: u64,
    pub(super) web_url: Option<String>,
}

pub(super) struct ConversationSeed {
    pub(super) id: u64,
    pub(super) title: String,
    pub(super) state: String,
    pub(super) body: Option<String>,
    pub(super) is_pr: bool,
    pub(super) web_url: Option<String>,
}

pub(super) fn map_note_comment(note: GitLabNote) -> Comment {
    Comment {
        author: note.author.map(|a| a.username),
        created_at: note.created_at,
        body: Some(note.body),
        kind: Some("issue_comment".into()),
        review_path: None,
        review_line: None,
        review_side: None,
    }
}

pub(super) fn map_review_comment(note: GitLabDiscussionNote) -> Comment {
    let position = note.position;
    let review_path = position
        .as_ref()
        .and_then(|p| p.new_path.clone().or_else(|| p.old_path.clone()));
    let review_line = position.as_ref().and_then(|p| p.new_line.or(p.old_line));
    let review_side = position.as_ref().and_then(|p| {
        if p.new_line.is_some() {
            Some("RIGHT".to_string())
        } else if p.old_line.is_some() {
            Some("LEFT".to_string())
        } else {
            None
        }
    });

    Comment {
        author: note.author.map(|a| a.username),
        created_at: note.created_at,
        body: Some(note.body),
        kind: Some("review_comment".into()),
        review_path,
        review_line,
        review_side,
    }
}

pub(super) fn map_issue_link(
    item: &GitLabIssueLinkItem,
    current_iid: u64,
) -> Option<ConversationLink> {
    let source_iid = item.source_issue.as_ref().map(|issue| issue.iid);
    let target_iid = item.target_issue.as_ref().map(|issue| issue.iid);
    let relation = item
        .link_type
        .as_deref()
        .map_or("relates", normalize_relation);

    if source_iid == Some(current_iid) {
        target_iid.map(|iid| ConversationLink {
            id: iid.to_string(),
            relation: relation.to_string(),
            kind: Some("issue".to_string()),
        })
    } else if target_iid == Some(current_iid) {
        source_iid.map(|iid| ConversationLink {
            id: iid.to_string(),
            relation: invert_relation(relation).to_string(),
            kind: Some("issue".to_string()),
        })
    } else {
        None
    }
}

pub(super) fn map_closed_by_link(mr: &GitLabMergeRequestRef) -> ConversationLink {
    ConversationLink {
        id: mr.iid.to_string(),
        relation: "closed_by".to_string(),
        kind: Some("pr".to_string()),
    }
}

pub(super) fn map_closes_related_issue_link(
    issue: &GitLabRelatedIssueRef,
) -> Option<ConversationLink> {
    Some(ConversationLink {
        id: issue_link_id(issue)?,
        relation: "closes".to_string(),
        kind: Some("issue".to_string()),
    })
}

pub(super) fn map_related_issue_link(issue: &GitLabRelatedIssueRef) -> Option<ConversationLink> {
    Some(ConversationLink {
        id: issue_link_id(issue)?,
        relation: "relates".to_string(),
        kind: Some("issue".to_string()),
    })
}

pub(super) fn map_related_mr_link(mr: &GitLabMergeRequestRef) -> ConversationLink {
    ConversationLink {
        id: mr.iid.to_string(),
        relation: "relates".to_string(),
        kind: Some("pr".to_string()),
    }
}

pub(super) fn map_url_reference(url: &str) -> ConversationLink {
    ConversationLink {
        id: url.to_string(),
        relation: "references".to_string(),
        kind: Some("url".to_string()),
    }
}

fn normalize_relation(link_type: &str) -> &'static str {
    match link_type {
        "blocks" => "blocks",
        "is_blocked_by" => "blocked_by",
        _ => "relates",
    }
}

fn invert_relation(relation: &str) -> &'static str {
    match relation {
        "blocks" => "blocked_by",
        "blocked_by" => "blocks",
        _ => "relates",
    }
}

fn issue_link_id(issue: &GitLabRelatedIssueRef) -> Option<String> {
    if let Some(iid) = issue.iid {
        return Some(iid.to_string());
    }
    let id = issue.id.as_ref()?;
    id.as_str()
        .map(str::to_string)
        .or_else(|| id.as_u64().map(|value| value.to_string()))
        .or_else(|| id.as_i64().map(|value| value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_issue_link_inverts_relation_for_target_side() {
        let link = map_issue_link(
            &GitLabIssueLinkItem {
                link_type: Some("blocks".to_string()),
                source_issue: Some(GitLabLinkIssueRef { iid: 10 }),
                target_issue: Some(GitLabLinkIssueRef { iid: 20 }),
            },
            20,
        )
        .expect("expected mapped link");
        assert_eq!(link.id, "10");
        assert_eq!(link.relation, "blocked_by");
        assert_eq!(link.kind.as_deref(), Some("issue"));
    }

    #[test]
    fn map_issue_link_preserves_relation_for_source_side() {
        let link = map_issue_link(
            &GitLabIssueLinkItem {
                link_type: Some("is_blocked_by".to_string()),
                source_issue: Some(GitLabLinkIssueRef { iid: 11 }),
                target_issue: Some(GitLabLinkIssueRef { iid: 22 }),
            },
            11,
        )
        .expect("expected mapped link");
        assert_eq!(link.id, "22");
        assert_eq!(link.relation, "blocked_by");
    }

    #[test]
    fn map_related_issue_link_uses_external_string_id() {
        let issue = GitLabRelatedIssueRef {
            iid: None,
            id: Some(serde_json::json!("CPQ-20376")),
            web_url: None,
        };
        let link = map_related_issue_link(&issue).expect("expected related link");
        assert_eq!(link.id, "CPQ-20376");
        assert_eq!(link.relation, "relates");
        assert_eq!(link.kind.as_deref(), Some("issue"));
    }

    #[test]
    fn map_closes_related_issue_link_prefers_iid() {
        let issue = GitLabRelatedIssueRef {
            iid: Some(42),
            id: Some(serde_json::json!("CPQ-20376")),
            web_url: None,
        };
        let link = map_closes_related_issue_link(&issue).expect("expected closes link");
        assert_eq!(link.id, "42");
        assert_eq!(link.relation, "closes");
    }
}
