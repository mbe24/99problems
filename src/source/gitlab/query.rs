use std::fmt::Write as _;

use crate::source::ContentKind;

#[derive(Debug)]
pub(super) struct GitLabFilters {
    pub(super) repo: Option<String>,
    pub(super) kind: ContentKind,
    pub(super) state: Option<String>,
    pub(super) labels: Vec<String>,
    pub(super) author: Option<String>,
    pub(super) since: Option<String>,
    pub(super) milestone: Option<String>,
    pub(super) search_terms: Vec<String>,
}

impl Default for GitLabFilters {
    fn default() -> Self {
        Self {
            repo: None,
            kind: ContentKind::Issue,
            state: None,
            labels: vec![],
            author: None,
            since: None,
            milestone: None,
            search_terms: vec![],
        }
    }
}

pub(super) fn parse_gitlab_query(raw_query: &str) -> GitLabFilters {
    let mut filters = GitLabFilters::default();

    for token in raw_query.split_whitespace() {
        if token == "is:issue" {
            filters.kind = ContentKind::Issue;
            continue;
        }
        if token == "is:pr" {
            filters.kind = ContentKind::Pr;
            continue;
        }
        if let Some(kind) = token.strip_prefix("type:") {
            if kind == "pr" {
                filters.kind = ContentKind::Pr;
                continue;
            }
            if kind == "issue" {
                filters.kind = ContentKind::Issue;
                continue;
            }
        }
        if let Some(repo) = token.strip_prefix("repo:") {
            filters.repo = Some(repo.to_string());
            continue;
        }
        if let Some(state) = token.strip_prefix("state:") {
            filters.state = Some(state.to_string());
            continue;
        }
        if let Some(label) = token.strip_prefix("label:") {
            filters.labels.push(label.to_string());
            continue;
        }
        if let Some(author) = token.strip_prefix("author:") {
            filters.author = Some(author.to_string());
            continue;
        }
        if let Some(since) = token.strip_prefix("created:>=") {
            filters.since = Some(since.to_string());
            continue;
        }
        if let Some(milestone) = token.strip_prefix("milestone:") {
            filters.milestone = Some(milestone.to_string());
            continue;
        }

        filters.search_terms.push(token.to_string());
    }

    filters
}

pub(super) fn build_search_params(filters: &GitLabFilters) -> Vec<(String, String)> {
    let mut params = Vec::new();

    if let Some(state) = normalize_state(filters.kind, filters.state.as_deref()) {
        params.push(("state".into(), state.to_string()));
    }
    if !filters.labels.is_empty() {
        params.push(("labels".into(), filters.labels.join(",")));
    }
    if let Some(author) = &filters.author {
        params.push(("author_username".into(), author.clone()));
    }
    if let Some(since) = &filters.since {
        params.push(("created_after".into(), since.clone()));
    }
    if let Some(milestone) = &filters.milestone {
        params.push(("milestone".into(), milestone.clone()));
    }
    if !filters.search_terms.is_empty() {
        params.push(("search".into(), filters.search_terms.join(" ")));
    }

    params
}

fn normalize_state(kind: ContentKind, state: Option<&str>) -> Option<&'static str> {
    let s = state?.to_ascii_lowercase();
    match kind {
        ContentKind::Issue => match s.as_str() {
            "open" | "opened" => Some("opened"),
            "closed" => Some("closed"),
            "all" => Some("all"),
            _ => None,
        },
        ContentKind::Pr => match s.as_str() {
            "open" | "opened" => Some("opened"),
            "closed" => Some("closed"),
            "merged" => Some("merged"),
            "locked" => Some("locked"),
            "all" => Some("all"),
            _ => None,
        },
    }
}

pub(super) fn encode_project_path(path: &str) -> String {
    let mut out = String::new();
    for b in path.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(*b, b'-' | b'.' | b'_' | b'~') {
            out.push(*b as char);
        } else {
            write!(out, "%{b:02X}").expect("writing to String should never fail");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gitlab_query_extracts_filters() {
        let q = parse_gitlab_query(
            "is:pr repo:group/project state:closed label:bug author:alice created:>=2024-01-01 milestone:v1 text",
        );
        assert!(matches!(q.kind, ContentKind::Pr));
        assert_eq!(q.repo.as_deref(), Some("group/project"));
        assert_eq!(q.state.as_deref(), Some("closed"));
        assert_eq!(q.labels, vec!["bug"]);
        assert_eq!(q.author.as_deref(), Some("alice"));
        assert_eq!(q.since.as_deref(), Some("2024-01-01"));
        assert_eq!(q.milestone.as_deref(), Some("v1"));
        assert_eq!(q.search_terms, vec!["text"]);
    }

    #[test]
    fn normalize_state_maps_open_to_opened() {
        assert_eq!(
            normalize_state(ContentKind::Issue, Some("open")),
            Some("opened")
        );
        assert_eq!(
            normalize_state(ContentKind::Pr, Some("open")),
            Some("opened")
        );
    }

    #[test]
    fn encode_project_path_encodes_slash() {
        assert_eq!(encode_project_path("group/project"), "group%2Fproject");
    }
}
