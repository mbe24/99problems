use std::collections::HashSet;

use serde::Deserialize;
use serde_json::Value;

use super::super::query::BitbucketFilters;
use crate::model::Comment;

#[derive(Deserialize)]
pub(super) struct BitbucketPage<T> {
    pub(super) values: Vec<T>,
    pub(super) next: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct BitbucketPullRequestItem {
    pub(super) id: u64,
    pub(super) title: String,
    pub(super) state: String,
    pub(super) description: Option<String>,
    pub(super) summary: Option<BitbucketRichText>,
    pub(super) author: Option<BitbucketUser>,
    pub(super) created_on: Option<String>,
    pub(super) links: Option<Value>,
}

#[derive(Deserialize)]
pub(super) struct BitbucketCommentItem {
    pub(super) user: Option<BitbucketUser>,
    pub(super) created_on: Option<String>,
    pub(super) content: Option<BitbucketRichText>,
    pub(super) inline: Option<BitbucketInline>,
    pub(super) deleted: Option<bool>,
}

#[derive(Deserialize)]
pub(super) struct BitbucketInline {
    pub(super) path: Option<String>,
    pub(super) from: Option<u64>,
    pub(super) to: Option<u64>,
}

#[derive(Deserialize)]
pub(super) struct BitbucketRichText {
    pub(super) raw: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct BitbucketUser {
    pub(super) display_name: Option<String>,
    pub(super) nickname: Option<String>,
    pub(super) username: Option<String>,
}

pub(super) fn matches_pr_filters(
    item: &BitbucketPullRequestItem,
    filters: &BitbucketFilters,
) -> bool {
    if !matches_pr_state(item.state.as_str(), filters.state.as_deref()) {
        return false;
    }
    if let Some(author) = filters.author.as_deref()
        && !user_matches(item.author.as_ref(), author)
    {
        return false;
    }
    if let Some(since) = filters.since.as_deref()
        && let Some(created) = item.created_on.as_deref()
        && created < since
    {
        return false;
    }
    matches_terms(
        &[
            item.title.as_str(),
            item.description.as_deref().unwrap_or(""),
            item.summary
                .as_ref()
                .and_then(|c| c.raw.as_deref())
                .unwrap_or(""),
        ],
        filters,
    )
}

fn matches_terms(haystack_parts: &[&str], filters: &BitbucketFilters) -> bool {
    let mut terms = filters.search_terms.clone();
    terms.extend(filters.labels.clone());
    if let Some(milestone) = filters.milestone.as_deref() {
        terms.push(milestone.to_string());
    }
    if terms.is_empty() {
        return true;
    }
    let haystack = haystack_parts.join(" ").to_ascii_lowercase();
    terms
        .iter()
        .all(|term| haystack.contains(&term.to_ascii_lowercase()))
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

fn user_matches(user: Option<&BitbucketUser>, needle: &str) -> bool {
    let Some(user) = user else {
        return false;
    };
    let needle = needle.to_ascii_lowercase();
    user.display_name
        .as_deref()
        .map(str::to_ascii_lowercase)
        .is_some_and(|v| v == needle)
        || user
            .nickname
            .as_deref()
            .map(str::to_ascii_lowercase)
            .is_some_and(|v| v == needle)
        || user
            .username
            .as_deref()
            .map(str::to_ascii_lowercase)
            .is_some_and(|v| v == needle)
}

pub(super) fn map_pr_comment(
    item: BitbucketCommentItem,
    include_review_comments: bool,
) -> Option<Comment> {
    let (kind, review_path, review_line, review_side) = if let Some(inline) = item.inline {
        if !include_review_comments {
            return None;
        }
        let review_line = inline.to.or(inline.from);
        let review_side = if inline.to.is_some() {
            Some("RIGHT".to_string())
        } else if inline.from.is_some() {
            Some("LEFT".to_string())
        } else {
            None
        };
        (
            "review_comment".to_string(),
            inline.path,
            review_line,
            review_side,
        )
    } else {
        ("issue_comment".to_string(), None, None, None)
    };

    Some(Comment {
        author: item.user.and_then(select_author_name),
        created_at: item.created_on.unwrap_or_default(),
        body: item.content.and_then(|c| c.raw),
        kind: Some(kind),
        review_path,
        review_line,
        review_side,
    })
}

fn select_author_name(user: BitbucketUser) -> Option<String> {
    user.nickname.or(user.username).or(user.display_name)
}

pub(super) fn map_url_links(links: Option<&Value>) -> Vec<crate::model::ConversationLink> {
    fn collect(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if let Some(url) = map.get("href").and_then(Value::as_str)
                    && is_human_facing_url(url)
                {
                    out.push(url.to_string());
                }
                for nested in map.values() {
                    collect(nested, out);
                }
            }
            Value::Array(items) => {
                for item in items {
                    collect(item, out);
                }
            }
            _ => {}
        }
    }

    let mut out: Vec<String> = Vec::new();
    if let Some(links) = links {
        collect(links, &mut out);
    }
    dedupe_urls(out)
        .into_iter()
        .map(|url| crate::model::ConversationLink {
            id: url,
            relation: "references".to_string(),
            kind: Some("url".to_string()),
        })
        .collect()
}

fn is_human_facing_url(url: &str) -> bool {
    !url.to_ascii_lowercase().contains("://api.bitbucket.org/")
}

fn dedupe_urls(urls: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for url in urls {
        if seen.insert(url.clone()) {
            deduped.push(url);
        }
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_state_filter_maps_open_closed_and_merged() {
        assert!(matches_pr_state("OPEN", Some("open")));
        assert!(matches_pr_state("DECLINED", Some("closed")));
        assert!(matches_pr_state("MERGED", Some("merged")));
        assert!(!matches_pr_state("OPEN", Some("merged")));
    }

    #[test]
    fn map_pr_review_comment_sets_review_fields() {
        let item = BitbucketCommentItem {
            user: Some(BitbucketUser {
                display_name: Some("Alice".into()),
                nickname: Some("alice".into()),
                username: None,
            }),
            created_on: Some("2026-01-01T00:00:00.000000+00:00".into()),
            content: Some(BitbucketRichText {
                raw: Some("Looks good".into()),
            }),
            inline: Some(BitbucketInline {
                path: Some("src/lib.rs".into()),
                from: None,
                to: Some(42),
            }),
            deleted: Some(false),
        };
        let mapped = map_pr_comment(item, true).expect("expected comment");
        assert_eq!(mapped.kind.as_deref(), Some("review_comment"));
        assert_eq!(mapped.review_path.as_deref(), Some("src/lib.rs"));
        assert_eq!(mapped.review_line, Some(42));
        assert_eq!(mapped.review_side.as_deref(), Some("RIGHT"));
    }

    #[test]
    fn maps_only_human_facing_links() {
        let links = serde_json::json!({
            "self": {"href": "https://bitbucket.org/workspace/repo/pull-requests/1"},
            "diff": {"href": "https://api.bitbucket.org/2.0/.../diff"},
            "commits": {"href": "https://api.bitbucket.org/2.0/.../commits"},
            "html": {"href": "https://bitbucket.org/workspace/repo/pull-requests/1"}
        });
        let mapped = map_url_links(Some(&links));
        assert_eq!(mapped.len(), 1);
        assert_eq!(
            mapped[0].id,
            "https://bitbucket.org/workspace/repo/pull-requests/1"
        );
        assert!(mapped.iter().all(|l| l.kind.as_deref() == Some("url")));
        assert!(mapped.iter().all(|l| l.relation == "references"));
    }

    #[test]
    fn drops_api_only_links() {
        let links = serde_json::json!({
            "diff": {"href": "https://api.bitbucket.org/2.0/.../diff"},
            "comments": {"href": "https://api.bitbucket.org/2.0/.../comments"}
        });
        let mapped = map_url_links(Some(&links));
        assert!(mapped.is_empty());
    }
}
