/// Integration tests — require a live GitHub API token.
/// Run with: GITHUB_TOKEN=ghp_... cargo test -- --include-ignored
///
/// These tests use schemaorg/schemaorg#1842 as a known stable fixture.

#[cfg(test)]
mod tests {
    use problems99::source::{Query, Source, github_issues::GitHubIssues};

    fn token() -> Option<String> {
        std::env::var("GITHUB_TOKEN").ok()
    }

    #[test]
    #[ignore = "requires GITHUB_TOKEN and live network"]
    fn fetch_known_issue_1842() {
        let source = GitHubIssues::new().unwrap();
        let conv = source.fetch_one("schemaorg/schemaorg", 1842).unwrap();
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
        let query = Query::build(
            Some("is:issue state:closed EventSeries repo:schemaorg/schemaorg".into()),
            None,
            None,
            None,
            10,
            token(),
        );
        let results = source.fetch(&query).unwrap();
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
        let conv = source.fetch_one("schemaorg/schemaorg", 1842).unwrap();
        let first = conv
            .comments
            .first()
            .expect("expected at least one comment");
        assert!(first.author.is_some());
        assert!(!first.created_at.is_empty());
    }
}
