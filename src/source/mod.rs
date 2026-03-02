use crate::model::Conversation;
use anyhow::Result;

pub mod github_issues;

/// A pluggable data source that fetches issue conversations.
pub trait Source {
    fn fetch(&self, query: &Query) -> Result<Vec<Conversation>>;
    fn fetch_one(&self, repo: &str, issue_id: u64) -> Result<Conversation>;
}

/// Parsed search parameters passed to a Source.
#[derive(Debug, Default, Clone)]
#[allow(dead_code)]
pub struct Query {
    /// Full raw query string (GitHub search syntax), e.g. "is:issue state:closed Event repo:owner/repo"
    pub raw: String,
    pub per_page: u32,
    pub token: Option<String>,
}

impl Query {
    /// Build a query by merging a raw string with convenience shorthands.
    /// Shorthands are only appended if their qualifier isn't already present.
    pub fn build(
        raw: Option<String>,
        repo: Option<String>,
        state: Option<String>,
        labels: Option<String>,
        per_page: u32,
        token: Option<String>,
    ) -> Self {
        let mut parts: Vec<String> = vec![];

        if let Some(r) = raw {
            parts.push(r);
        }

        if let Some(repo) = repo {
            if !parts.iter().any(|p| p.contains("repo:")) {
                parts.push(format!("repo:{repo}"));
            }
        }
        if let Some(state) = state {
            if !parts.iter().any(|p| p.contains("state:")) {
                parts.push(format!("state:{state}"));
            }
        }
        if let Some(labels) = labels {
            for label in labels.split(',') {
                let label = label.trim();
                if !label.is_empty() {
                    parts.push(format!("label:{label}"));
                }
            }
        }

        Query { raw: parts.join(" "), per_page, token }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_query_appends_repo_and_state() {
        let q = Query::build(
            Some("is:issue Event".into()),
            Some("owner/repo".into()),
            Some("closed".into()),
            None,
            100,
            None,
        );
        assert!(q.raw.contains("repo:owner/repo"));
        assert!(q.raw.contains("state:closed"));
        assert!(q.raw.contains("is:issue Event"));
    }

    #[test]
    fn build_query_does_not_duplicate_repo() {
        let q = Query::build(
            Some("is:issue repo:owner/repo".into()),
            Some("other/repo".into()),
            None,
            None,
            100,
            None,
        );
        assert_eq!(q.raw.matches("repo:").count(), 1);
    }

    #[test]
    fn build_query_handles_multiple_labels() {
        let q = Query::build(None, None, None, Some("bug,enhancement".into()), 100, None);
        assert!(q.raw.contains("label:bug"));
        assert!(q.raw.contains("label:enhancement"));
    }

    #[test]
    fn build_query_empty_produces_empty_raw() {
        let q = Query::build(None, None, None, None, 100, None);
        assert_eq!(q.raw.trim(), "");
    }
}
