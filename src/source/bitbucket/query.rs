use anyhow::Result;

use crate::error::AppError;
use crate::source::ContentKind;

#[derive(Debug, Clone)]
pub(super) struct BitbucketFilters {
    pub(super) repo: Option<String>,
    pub(super) kind: ContentKind,
    pub(super) kind_explicit: bool,
    pub(super) state: Option<String>,
    pub(super) labels: Vec<String>,
    pub(super) author: Option<String>,
    pub(super) since: Option<String>,
    pub(super) milestone: Option<String>,
    pub(super) search_terms: Vec<String>,
}

impl Default for BitbucketFilters {
    fn default() -> Self {
        Self {
            repo: None,
            kind: ContentKind::Pr,
            kind_explicit: false,
            state: None,
            labels: vec![],
            author: None,
            since: None,
            milestone: None,
            search_terms: vec![],
        }
    }
}

pub(super) fn parse_bitbucket_query(raw_query: &str) -> BitbucketFilters {
    let mut filters = BitbucketFilters::default();

    for token in raw_query.split_whitespace() {
        if token == "is:issue" {
            filters.kind = ContentKind::Issue;
            filters.kind_explicit = true;
            continue;
        }
        if token == "is:pr" {
            filters.kind = ContentKind::Pr;
            filters.kind_explicit = true;
            continue;
        }
        if let Some(kind) = token.strip_prefix("type:") {
            if kind == "issue" {
                filters.kind = ContentKind::Issue;
                filters.kind_explicit = true;
                continue;
            }
            if kind == "pr" {
                filters.kind = ContentKind::Pr;
                filters.kind_explicit = true;
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

/// Parse `workspace/repo_slug` from `repo:` input.
///
/// # Errors
///
/// Returns an error when the repository path is missing or malformed.
pub(super) fn parse_workspace_repo(raw_repo: Option<&str>) -> Result<(String, String)> {
    parse_repo_pair(raw_repo, "workspace/repo_slug")
}

/// Parse `project/repo_slug` from `repo:` input.
///
/// # Errors
///
/// Returns an error when the repository path is missing or malformed.
pub(super) fn parse_project_repo(raw_repo: Option<&str>) -> Result<(String, String)> {
    parse_repo_pair(raw_repo, "project/repo_slug")
}

fn parse_repo_pair(raw_repo: Option<&str>, expected_format: &str) -> Result<(String, String)> {
    let repo = raw_repo.ok_or_else(|| {
        AppError::usage(format!(
            "No repo: found in query. Use --repo or include 'repo:{expected_format}' in -q"
        ))
    })?;
    let mut parts = repo.split('/');
    let first = parts.next().unwrap_or_default().trim();
    let second = parts.next().unwrap_or_default().trim();
    let tail = parts.next();
    if first.is_empty() || second.is_empty() || tail.is_some() {
        return Err(AppError::usage(format!(
            "Bitbucket repo must be '{expected_format}', got '{repo}'."
        ))
        .into());
    }
    Ok((first.to_string(), second.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bitbucket_query_extracts_filters() {
        let q = parse_bitbucket_query(
            "is:pr repo:workspace/repo state:closed label:bug author:alice created:>=2025-01-01 milestone:v1 text",
        );
        assert!(matches!(q.kind, ContentKind::Pr));
        assert!(q.kind_explicit);
        assert_eq!(q.repo.as_deref(), Some("workspace/repo"));
        assert_eq!(q.state.as_deref(), Some("closed"));
        assert_eq!(q.labels, vec!["bug"]);
        assert_eq!(q.author.as_deref(), Some("alice"));
        assert_eq!(q.since.as_deref(), Some("2025-01-01"));
        assert_eq!(q.milestone.as_deref(), Some("v1"));
        assert_eq!(q.search_terms, vec!["text"]);
    }

    #[test]
    fn parse_query_defaults_to_pr_without_explicit_kind() {
        let q = parse_bitbucket_query("repo:workspace/repo state:open");
        assert!(matches!(q.kind, ContentKind::Pr));
        assert!(!q.kind_explicit);
    }

    #[test]
    fn parse_workspace_repo_requires_workspace_and_repo_slug() {
        let err = parse_workspace_repo(Some("workspace"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("workspace/repo_slug"));
    }

    #[test]
    fn parse_project_repo_requires_project_and_repo_slug() {
        let err = parse_project_repo(Some("PROJECT")).unwrap_err().to_string();
        assert!(err.contains("project/repo_slug"));
    }
}
