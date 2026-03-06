use anyhow::Result;

use crate::error::AppError;
use crate::source::ContentKind;

#[derive(Debug)]
pub(super) struct JiraFilters {
    pub(super) repo: Option<String>,
    pub(super) kind: ContentKind,
    pub(super) state: Option<String>,
    pub(super) labels: Vec<String>,
    pub(super) author: Option<String>,
    pub(super) since: Option<String>,
    pub(super) milestone: Option<String>,
    pub(super) search_terms: Vec<String>,
}

impl Default for JiraFilters {
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

pub(super) fn parse_jira_query(raw_query: &str) -> JiraFilters {
    let mut filters = JiraFilters::default();

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
            if kind == "issue" {
                filters.kind = ContentKind::Issue;
                continue;
            }
            if kind == "pr" {
                filters.kind = ContentKind::Pr;
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

pub(super) fn build_jql(filters: &JiraFilters) -> Result<String> {
    let project = filters.repo.as_deref().ok_or_else(|| {
        AppError::usage("No repo: found in query. Use --repo with Jira project key.")
    })?;

    let mut clauses = vec![format!("project = {}", quote_jql(project))];
    if let Some(state) = filters.state.as_deref() {
        match state.to_ascii_lowercase().as_str() {
            "open" | "opened" => clauses.push("statusCategory != Done".into()),
            "closed" => clauses.push("statusCategory = Done".into()),
            "all" => {}
            _ => clauses.push(format!("status = {}", quote_jql(state))),
        }
    }

    for label in &filters.labels {
        clauses.push(format!("labels = {}", quote_jql(label)));
    }
    if let Some(author) = &filters.author {
        clauses.push(format!("reporter = {}", quote_jql(author)));
    }
    if let Some(since) = &filters.since {
        clauses.push(format!("created >= {}", quote_jql(since)));
    }
    if let Some(milestone) = &filters.milestone {
        clauses.push(format!("fixVersion = {}", quote_jql(milestone)));
    }
    if !filters.search_terms.is_empty() {
        clauses.push(format!(
            "text ~ {}",
            quote_jql(&filters.search_terms.join(" "))
        ));
    }

    Ok(clauses.join(" AND "))
}

fn quote_jql(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_jira_query_extracts_filters() {
        let q = parse_jira_query(
            "repo:CLOUD state:closed label:api author:alice created:>=2024-01-01 milestone:v1 text",
        );
        assert_eq!(q.repo.as_deref(), Some("CLOUD"));
        assert!(matches!(q.kind, ContentKind::Issue));
        assert_eq!(q.state.as_deref(), Some("closed"));
        assert_eq!(q.labels, vec!["api"]);
        assert_eq!(q.author.as_deref(), Some("alice"));
        assert_eq!(q.since.as_deref(), Some("2024-01-01"));
        assert_eq!(q.milestone.as_deref(), Some("v1"));
        assert_eq!(q.search_terms, vec!["text"]);
    }

    #[test]
    fn build_jql_maps_closed_to_done_category() {
        let q = parse_jira_query("repo:CLOUD state:closed");
        let jql = build_jql(&q).unwrap();
        assert!(jql.contains("project = \"CLOUD\""));
        assert!(jql.contains("statusCategory = Done"));
    }
}
