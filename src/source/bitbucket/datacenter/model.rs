use serde::Deserialize;

use super::super::query::BitbucketFilters;
use crate::model::Comment;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BitbucketDcPage<T> {
    pub(super) values: Vec<T>,
    pub(super) is_last_page: bool,
    pub(super) next_page_start: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BitbucketDcPullRequestItem {
    pub(super) id: u64,
    pub(super) title: String,
    pub(super) state: String,
    pub(super) description: Option<String>,
    pub(super) author: Option<BitbucketDcParticipant>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BitbucketDcParticipant {
    pub(super) user: Option<BitbucketDcUser>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BitbucketDcCommentItem {
    pub(super) text: Option<String>,
    pub(super) author: Option<BitbucketDcUser>,
    pub(super) created_date: Option<i64>,
    pub(super) anchor: Option<BitbucketDcAnchor>,
    #[serde(default)]
    pub(super) comments: Vec<BitbucketDcCommentItem>,
    pub(super) deleted: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BitbucketDcActivityItem {
    pub(super) action: Option<String>,
    pub(super) comment: Option<BitbucketDcCommentItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BitbucketDcAnchor {
    pub(super) path: Option<String>,
    pub(super) src_path: Option<String>,
    pub(super) line: Option<u64>,
    pub(super) line_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct BitbucketDcUser {
    pub(super) display_name: Option<String>,
    pub(super) name: Option<String>,
    pub(super) slug: Option<String>,
}

pub(super) fn matches_pr_filters(
    item: &BitbucketDcPullRequestItem,
    filters: &BitbucketFilters,
) -> bool {
    if !matches_pr_state(item.state.as_str(), filters.state.as_deref()) {
        return false;
    }
    if let Some(author) = filters.author.as_deref()
        && !participant_matches(item.author.as_ref(), author)
    {
        return false;
    }

    let mut terms = filters.search_terms.clone();
    terms.extend(filters.labels.clone());
    if let Some(milestone) = filters.milestone.as_deref() {
        terms.push(milestone.to_string());
    }
    if terms.is_empty() {
        return true;
    }

    let haystack = [
        item.title.as_str(),
        item.description.as_deref().unwrap_or(""),
    ]
    .join(" ")
    .to_ascii_lowercase();
    terms
        .iter()
        .all(|term| haystack.contains(&term.to_ascii_lowercase()))
}

fn participant_matches(participant: Option<&BitbucketDcParticipant>, needle: &str) -> bool {
    let Some(user) = participant.and_then(|p| p.user.as_ref()) else {
        return false;
    };
    user_matches(user, needle)
}

fn user_matches(user: &BitbucketDcUser, needle: &str) -> bool {
    let needle = needle.to_ascii_lowercase();
    user.display_name
        .as_deref()
        .map(str::to_ascii_lowercase)
        .is_some_and(|v| v == needle)
        || user
            .name
            .as_deref()
            .map(str::to_ascii_lowercase)
            .is_some_and(|v| v == needle)
        || user
            .slug
            .as_deref()
            .map(str::to_ascii_lowercase)
            .is_some_and(|v| v == needle)
}

fn matches_pr_state(state: &str, filter_state: Option<&str>) -> bool {
    let state = state.to_ascii_lowercase();
    let Some(filter) = filter_state.map(str::to_ascii_lowercase) else {
        return true;
    };
    match filter.as_str() {
        "open" | "opened" => state == "open",
        "closed" => matches!(state.as_str(), "merged" | "declined" | "superseded"),
        "merged" => state == "merged",
        "declined" => state == "declined",
        "all" => true,
        other => state == other,
    }
}

pub(super) fn collect_pr_comment(
    item: BitbucketDcCommentItem,
    include_review_comments: bool,
    out: &mut Vec<Comment>,
) {
    if item.deleted.unwrap_or(false) {
        return;
    }

    if let Some(mapped) = map_pr_comment(&item, include_review_comments) {
        out.push(mapped);
    }

    for reply in item.comments {
        collect_pr_comment(reply, include_review_comments, out);
    }
}

pub(super) fn collect_comments_from_activity(
    activity: BitbucketDcActivityItem,
    include_review_comments: bool,
    out: &mut Vec<Comment>,
) {
    if !activity
        .action
        .as_deref()
        .is_some_and(|action| action.eq_ignore_ascii_case("COMMENTED"))
    {
        return;
    }
    if let Some(comment) = activity.comment {
        collect_pr_comment(comment, include_review_comments, out);
    }
}

fn map_pr_comment(item: &BitbucketDcCommentItem, include_review_comments: bool) -> Option<Comment> {
    let anchor = item.anchor.as_ref();
    let is_review =
        anchor.is_some_and(|a| a.line.is_some() || a.path.is_some() || a.src_path.is_some());
    if is_review && !include_review_comments {
        return None;
    }

    let kind = if is_review {
        "review_comment"
    } else {
        "issue_comment"
    }
    .to_string();

    let review_path = anchor.and_then(|a| a.path.clone().or_else(|| a.src_path.clone()));
    let review_side = anchor.and_then(|a| match a.line_type.as_deref() {
        Some("REMOVED") => Some("LEFT".to_string()),
        Some("ADDED") => Some("RIGHT".to_string()),
        _ => None,
    });

    Some(Comment {
        author: item.author.as_ref().and_then(select_author_name),
        created_at: item
            .created_date
            .map(|value| value.to_string())
            .unwrap_or_default(),
        body: item.text.clone(),
        kind: Some(kind),
        review_path,
        review_line: anchor.and_then(|a| a.line),
        review_side,
    })
}

fn select_author_name(user: &BitbucketDcUser) -> Option<String> {
    user.display_name
        .clone()
        .or_else(|| user.name.clone())
        .or_else(|| user.slug.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_nested_review_comment_metadata() {
        let mut out = Vec::new();
        collect_pr_comment(
            BitbucketDcCommentItem {
                text: Some("root".into()),
                author: Some(BitbucketDcUser {
                    display_name: Some("Alice".into()),
                    name: Some("alice".into()),
                    slug: None,
                }),
                created_date: Some(1),
                anchor: Some(BitbucketDcAnchor {
                    path: Some("src/lib.rs".into()),
                    src_path: None,
                    line: Some(12),
                    line_type: Some("ADDED".into()),
                }),
                comments: vec![BitbucketDcCommentItem {
                    text: Some("reply".into()),
                    author: None,
                    created_date: Some(2),
                    anchor: None,
                    comments: vec![],
                    deleted: Some(false),
                }],
                deleted: Some(false),
            },
            true,
            &mut out,
        );

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].kind.as_deref(), Some("review_comment"));
        assert_eq!(out[0].review_path.as_deref(), Some("src/lib.rs"));
        assert_eq!(out[0].review_line, Some(12));
        assert_eq!(out[0].review_side.as_deref(), Some("RIGHT"));
        assert_eq!(out[1].kind.as_deref(), Some("issue_comment"));
    }

    #[test]
    fn skips_review_comments_when_disabled() {
        let mut out = Vec::new();
        collect_pr_comment(
            BitbucketDcCommentItem {
                text: Some("root".into()),
                author: None,
                created_date: Some(1),
                anchor: Some(BitbucketDcAnchor {
                    path: Some("src/lib.rs".into()),
                    src_path: None,
                    line: Some(12),
                    line_type: Some("ADDED".into()),
                }),
                comments: vec![],
                deleted: Some(false),
            },
            false,
            &mut out,
        );

        assert!(out.is_empty());
    }

    #[test]
    fn activity_filter_ignores_non_comment_actions() {
        let mut out = Vec::new();
        collect_comments_from_activity(
            BitbucketDcActivityItem {
                action: Some("OPENED".into()),
                comment: Some(BitbucketDcCommentItem {
                    text: Some("x".into()),
                    author: None,
                    created_date: Some(1),
                    anchor: None,
                    comments: vec![],
                    deleted: Some(false),
                }),
            },
            true,
            &mut out,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn activity_filter_collects_commented_action() {
        let mut out = Vec::new();
        collect_comments_from_activity(
            BitbucketDcActivityItem {
                action: Some("COMMENTED".into()),
                comment: Some(BitbucketDcCommentItem {
                    text: Some("hello".into()),
                    author: None,
                    created_date: Some(1),
                    anchor: None,
                    comments: vec![],
                    deleted: Some(false),
                }),
            },
            true,
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind.as_deref(), Some("issue_comment"));
    }
}
