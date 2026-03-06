use serde::Deserialize;

use crate::model::Comment;

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
