use crate::model::Conversation;
use anyhow::Result;

pub mod github;
pub mod gitlab;
pub mod jira;

/// A pluggable data source that fetches issue/PR conversations.
pub trait Source {
    /// Fetch issue or pull request conversations for a request target.
    ///
    /// # Errors
    ///
    /// Returns an error when request validation fails, authentication fails,
    /// or the remote platform returns a non-success/invalid response.
    fn fetch(&self, req: &FetchRequest) -> Result<Vec<Conversation>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Issue,
    Pr,
}

#[derive(Debug, Clone)]
pub enum FetchTarget {
    Search {
        raw_query: String,
    },
    Id {
        repo: String,
        id: String,
        kind: ContentKind,
        allow_fallback_to_pr: bool,
    },
}

#[derive(Debug, Clone)]
pub struct FetchRequest {
    pub target: FetchTarget,
    pub per_page: u32,
    pub token: Option<String>,
    pub jira_email: Option<String>,
    pub include_comments: bool,
    pub include_review_comments: bool,
}

/// Parsed search parameters passed to a Source.
#[derive(Debug, Default, Clone)]
#[allow(dead_code)]
pub struct Query {
    /// Full raw query string (platform search syntax), e.g. "state:closed Event repo:owner/repo"
    pub raw: String,
    pub per_page: u32,
    pub token: Option<String>,
}

impl Query {
    /// Build a query by merging a raw string with convenience shorthands.
    /// Shorthands are only appended if their qualifier isn't already present in the raw string.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        raw: Option<String>,
        kind: &str,
        repo: Option<String>,
        state: Option<String>,
        labels: Option<String>,
        author: Option<String>,
        since: Option<String>,
        milestone: Option<String>,
        per_page: u32,
        token: Option<String>,
    ) -> Self {
        let mut parts: Vec<String> = vec![];

        if let Some(r) = raw {
            parts.push(r);
        }

        // Inject type qualifier unless already present
        if !parts
            .iter()
            .any(|p| p.contains("is:issue") || p.contains("is:pr") || p.contains("type:"))
        {
            match kind {
                "pr" => parts.push("is:pr".into()),
                _ => parts.push("is:issue".into()),
            }
        }

        if let Some(repo) = repo
            && !parts.iter().any(|p| p.contains("repo:"))
        {
            parts.push(format!("repo:{repo}"));
        }
        if let Some(state) = state
            && !parts.iter().any(|p| p.contains("state:"))
        {
            parts.push(format!("state:{state}"));
        }
        if let Some(labels) = labels {
            for label in labels.split(',') {
                let label = label.trim();
                if !label.is_empty() {
                    parts.push(format!("label:{label}"));
                }
            }
        }
        if let Some(author) = author
            && !parts.iter().any(|p| p.contains("author:"))
        {
            parts.push(format!("author:{author}"));
        }
        if let Some(since) = since
            && !parts.iter().any(|p| p.contains("created:"))
        {
            parts.push(format!("created:>={since}"));
        }
        if let Some(milestone) = milestone
            && !parts.iter().any(|p| p.contains("milestone:"))
        {
            parts.push(format!("milestone:{milestone}"));
        }

        Query {
            raw: parts.join(" "),
            per_page,
            token,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(raw: Option<&str>, kind: &str, repo: Option<&str>, state: Option<&str>) -> Query {
        Query::build(
            raw.map(std::convert::Into::into),
            kind,
            repo.map(std::convert::Into::into),
            state.map(std::convert::Into::into),
            None,
            None,
            None,
            None,
            100,
            None,
        )
    }

    #[test]
    fn build_query_appends_repo_and_state() {
        let q = build(Some("Event"), "issue", Some("owner/repo"), Some("closed"));
        assert!(q.raw.contains("repo:owner/repo"));
        assert!(q.raw.contains("state:closed"));
        assert!(q.raw.contains("Event"));
        assert!(q.raw.contains("is:issue"));
    }

    #[test]
    fn build_query_does_not_duplicate_repo() {
        let q = build(
            Some("is:issue repo:owner/repo"),
            "issue",
            Some("other/repo"),
            None,
        );
        assert_eq!(q.raw.matches("repo:").count(), 1);
    }

    #[test]
    fn build_query_handles_multiple_labels() {
        let q = Query::build(
            None,
            "issue",
            None,
            None,
            Some("bug,enhancement".into()),
            None,
            None,
            None,
            100,
            None,
        );
        assert!(q.raw.contains("label:bug"));
        assert!(q.raw.contains("label:enhancement"));
    }

    #[test]
    fn build_query_empty_produces_type_qualifier() {
        let q = build(None, "issue", None, None);
        assert!(q.raw.contains("is:issue"));
    }

    #[test]
    fn build_query_pr_type_injects_is_pr() {
        let q = build(None, "pr", Some("owner/repo"), None);
        assert!(q.raw.contains("is:pr"));
        assert!(!q.raw.contains("is:issue"));
    }

    #[test]
    fn build_query_does_not_duplicate_type() {
        let q = build(Some("is:pr repo:owner/repo"), "pr", None, None);
        assert_eq!(q.raw.matches("is:pr").count(), 1);
    }

    #[test]
    fn build_query_author_and_since() {
        let q = Query::build(
            None,
            "issue",
            None,
            None,
            None,
            Some("octocat".into()),
            Some("2024-01-01".into()),
            None,
            100,
            None,
        );
        assert!(q.raw.contains("author:octocat"));
        assert!(q.raw.contains("created:>=2024-01-01"));
    }

    #[test]
    fn build_query_milestone() {
        let q = Query::build(
            None,
            "issue",
            None,
            None,
            None,
            None,
            None,
            Some("v1.0".into()),
            100,
            None,
        );
        assert!(q.raw.contains("milestone:v1.0"));
    }
}
