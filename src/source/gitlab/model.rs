use serde::Deserialize;

use crate::model::Comment;

#[derive(Deserialize)]
pub(super) struct GitLabIssueItem {
    pub(super) iid: u64,
    pub(super) title: String,
    pub(super) state: String,
    pub(super) description: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct GitLabMergeRequestItem {
    pub(super) iid: u64,
    pub(super) title: String,
    pub(super) state: String,
    pub(super) description: Option<String>,
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

pub(super) struct ConversationSeed {
    pub(super) id: u64,
    pub(super) title: String,
    pub(super) state: String,
    pub(super) body: Option<String>,
    pub(super) is_pr: bool,
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
