/// Integration tests — require a live GitHub API token.
/// Run with: GITHUB_TOKEN=ghp_... cargo test -- --include-ignored

#[cfg(test)]
mod tests {
    use problems99::source::{
        ContentKind, FetchRequest, FetchTarget, Source, github_issues::GitHubIssues,
    };

    fn token() -> Option<String> {
        // Prefer env var, fall back to dotfile config (same resolution as the binary)
        std::env::var("GITHUB_TOKEN").ok().or_else(|| {
            problems99::config::Config::load()
                .ok()
                .and_then(|c| c.token)
        })
    }

    fn req_id(repo: &str, id: u64, include_review_comments: bool) -> FetchRequest {
        FetchRequest {
            target: FetchTarget::Id {
                repo: repo.to_string(),
                id,
                kind: ContentKind::Issue,
                allow_fallback_to_pr: true,
            },
            per_page: 100,
            token: token(),
            include_review_comments,
        }
    }

    fn req_id_with_kind(
        repo: &str,
        id: u64,
        kind: ContentKind,
        allow_fallback_to_pr: bool,
    ) -> FetchRequest {
        FetchRequest {
            target: FetchTarget::Id {
                repo: repo.to_string(),
                id,
                kind,
                allow_fallback_to_pr,
            },
            per_page: 100,
            token: token(),
            include_review_comments: false,
        }
    }

    #[test]
    #[ignore = "requires GITHUB_TOKEN and live network"]
    fn fetch_known_issue_1842() {
        let source = GitHubIssues::new().unwrap();
        let req = req_id("schemaorg/schemaorg", 1842, false);
        let conv = source.fetch(&req).unwrap().into_iter().next().unwrap();
        assert_eq!(conv.id, 1842);
        assert_eq!(conv.title, "Online-only events");
        assert_eq!(conv.state, "closed");
        assert!(conv.body.is_some());
        assert!(!conv.comments.is_empty());
    }

    #[test]
    #[ignore = "requires GITHUB_TOKEN and live network"]
    fn search_returns_results() {
        let source = GitHubIssues::new().unwrap();
        let req = FetchRequest {
            target: FetchTarget::Search {
                raw_query: "is:issue state:closed EventSeries repo:schemaorg/schemaorg".into(),
            },
            per_page: 10,
            token: token(),
            include_review_comments: false,
        };
        let results = source.fetch(&req).unwrap();
        assert!(!results.is_empty());
        for conv in &results {
            assert!(!conv.title.is_empty());
            assert_eq!(conv.state, "closed");
        }
    }

    #[test]
    #[ignore = "requires GITHUB_TOKEN and live network"]
    fn fetch_one_comment_has_author_and_body() {
        let source = GitHubIssues::new().unwrap();
        let req = req_id("schemaorg/schemaorg", 1842, false);
        let conv = source.fetch(&req).unwrap().into_iter().next().unwrap();
        let first = conv
            .comments
            .first()
            .expect("expected at least one comment");
        assert!(first.author.is_some());
        assert!(!first.created_at.is_empty());
    }

    #[test]
    #[ignore = "requires GITHUB_TOKEN and live network"]
    fn fetch_pr_2402_default_issue_comments_only() {
        let source = GitHubIssues::new().unwrap();
        let req = req_id("github/gitignore", 2402, false);
        let conv = source.fetch(&req).unwrap().into_iter().next().unwrap();

        assert_eq!(conv.id, 2402);
        assert!(!conv.title.is_empty());
        assert!(!conv.state.is_empty());
        assert!(!conv.comments.is_empty());
        assert!(!conv.comments.iter().any(|c| {
            c.kind.as_deref() == Some("review_comment")
                || c.review_path.is_some()
                || c.review_line.is_some()
                || c.review_side.is_some()
        }));
    }

    #[test]
    #[ignore = "requires GITHUB_TOKEN and live network"]
    fn fetch_pr_2402_with_review_comments() {
        let source = GitHubIssues::new().unwrap();
        let req = req_id("github/gitignore", 2402, true);
        let conv = source.fetch(&req).unwrap().into_iter().next().unwrap();

        assert_eq!(conv.id, 2402);
        assert!(
            conv.comments
                .iter()
                .any(|c| c.kind.as_deref() == Some("review_comment"))
        );
        assert!(conv.comments.iter().any(|c| c.review_path.is_some()));
    }

    #[test]
    #[ignore = "requires GITHUB_TOKEN and live network"]
    fn search_pr_query_includes_2402() {
        let source = GitHubIssues::new().unwrap();
        let req = FetchRequest {
            target: FetchTarget::Search {
                raw_query: "repo:github/gitignore is:pr 2402".into(),
            },
            per_page: 10,
            token: token(),
            include_review_comments: false,
        };
        let results = source.fetch(&req).unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|c| c.id == 2402));
    }

    #[test]
    #[ignore = "requires GITHUB_TOKEN and live network"]
    fn fetch_issue_as_pr_errors_when_kind_is_explicit() {
        let source = GitHubIssues::new().unwrap();
        let req = req_id_with_kind("schemaorg/schemaorg", 1842, ContentKind::Pr, false);
        let err = source.fetch(&req).unwrap_err().to_string();
        assert!(err.contains("not a pull request"));
    }

    #[test]
    #[ignore = "requires GITHUB_TOKEN and live network"]
    fn fetch_pr_as_issue_errors_when_fallback_is_disabled() {
        let source = GitHubIssues::new().unwrap();
        let req = req_id_with_kind("github/gitignore", 2402, ContentKind::Issue, false);
        let err = source.fetch(&req).unwrap_err().to_string();
        assert!(err.contains("is a pull request"));
    }
}
