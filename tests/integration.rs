/// Integration tests — live network tests for GitHub + GitLab + Jira.
/// Run with: cargo test -- --include-ignored
/// Optional env vars for higher-rate/authenticated calls:
/// - `TOKEN_GITHUB`=... (legacy fallback: `GITHUB_TOKEN`)
/// - `TOKEN_GITLAB`=... (legacy fallback: `GITLAB_TOKEN`)
/// - `TOKEN_JIRA`=... (legacy fallback: `JIRA_TOKEN`)
/// - `TOKEN_BITBUCKET`=... (legacy fallback: `BITBUCKET_TOKEN`)
/// - `BITBUCKET_REPO`=`workspace/repo_slug`
/// - `BITBUCKET_PR_ID`=numeric pull request id
#[cfg(test)]
mod tests {
    use problems99::source::{
        ContentKind, FetchRequest, FetchTarget, Source, bitbucket::BitbucketSource,
        github::GitHubSource, gitlab::GitLabSource, jira::JiraSource,
    };

    fn fetch_all<S: Source + ?Sized>(
        source: &S,
        req: &FetchRequest,
    ) -> anyhow::Result<Vec<problems99::model::Conversation>> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build Tokio runtime for integration test");
        runtime.block_on(source.fetch(req))
    }

    fn github_token() -> Option<String> {
        std::env::var("TOKEN_GITHUB")
            .ok()
            .or_else(|| std::env::var("GITHUB_TOKEN").ok())
            .or_else(|| {
                problems99::config::Config::load_with_options(problems99::config::ResolveOptions {
                    instance: Some("github"),
                    ..problems99::config::ResolveOptions::default()
                })
                .ok()
                .and_then(|c| c.token)
            })
    }

    fn gitlab_token() -> Option<String> {
        std::env::var("TOKEN_GITLAB")
            .ok()
            .or_else(|| std::env::var("GITLAB_TOKEN").ok())
    }

    fn jira_token() -> Option<String> {
        std::env::var("TOKEN_JIRA")
            .ok()
            .or_else(|| std::env::var("JIRA_TOKEN").ok())
    }

    fn bitbucket_token() -> Option<String> {
        std::env::var("TOKEN_BITBUCKET")
            .ok()
            .or_else(|| std::env::var("BITBUCKET_TOKEN").ok())
    }

    fn required_env(var: &str) -> String {
        std::env::var(var).unwrap_or_else(|_| panic!("missing required env var: {var}"))
    }

    fn is_public_jira_login_wall(err: &str) -> bool {
        err.contains("non-JSON content-type 'text/html'")
            && (err.contains("auth/login page")
                || err.contains("login.jsp?permissionViolation")
                || err.contains("id-frontend.prod-east.frontend.public.atl-paas.net"))
    }

    fn fail_public_jira_login_wall(test_name: &str, msg: &str) -> ! {
        panic!(
            "{test_name}: public Jira endpoint returned a login/auth wall instead of JSON. \
             This indicates external endpoint/auth drift (not an adapter parsing bug). \
             Response details: {msg}"
        )
    }

    fn is_transient_gitlab_upstream_error(err: &str) -> bool {
        let msg = err.to_ascii_lowercase();
        msg.contains("gitlab api page fetch error 502")
            || msg.contains("gitlab api page fetch error 503")
            || msg.contains("gitlab api page fetch error 504")
            || msg.contains("bad gateway")
            || msg.contains("gateway timeout")
            || msg.contains("service unavailable")
            || msg.contains("operation timed out")
            || msg.contains("timed out")
    }

    fn maybe_skip_transient_gitlab_error(test_name: &str, err: &str) -> bool {
        if is_transient_gitlab_upstream_error(err) {
            eprintln!(
                "{test_name}: skipping due to transient GitLab upstream/network failure: {err}"
            );
            return true;
        }
        false
    }

    fn req_id(repo: &str, id: &str, include_review_comments: bool) -> FetchRequest {
        FetchRequest {
            target: FetchTarget::Id {
                repo: repo.to_string(),
                id: id.to_string(),
                kind: ContentKind::Issue,
            },
            per_page: 100,
            token: github_token(),
            account_email: None,
            include_body: true,
            include_comments: true,
            include_review_comments,
            include_links: true,
        }
    }

    fn req_id_with_kind(repo: &str, id: &str, kind: ContentKind) -> FetchRequest {
        FetchRequest {
            target: FetchTarget::Id {
                repo: repo.to_string(),
                id: id.to_string(),
                kind,
            },
            per_page: 100,
            token: github_token(),
            account_email: None,
            include_body: true,
            include_comments: true,
            include_review_comments: false,
            include_links: true,
        }
    }

    #[test]
    #[ignore = "requires TOKEN_GITHUB (or GITHUB_TOKEN) and live network"]
    fn github_fetch_known_issue_1842() {
        let source = GitHubSource::new(false).unwrap();
        let req = req_id("schemaorg/schemaorg", "1842", false);
        let conv = fetch_all(&source, &req)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(conv.id, "1842");
        assert_eq!(conv.title, "Online-only events");
        assert_eq!(conv.state, "closed");
        assert!(conv.body.is_some());
        assert!(!conv.comments.is_empty());
    }

    #[test]
    #[ignore = "requires TOKEN_GITHUB (or GITHUB_TOKEN) and live network"]
    fn github_search_returns_results() {
        let source = GitHubSource::new(false).unwrap();
        let req = FetchRequest {
            target: FetchTarget::Search {
                raw_query: "is:issue state:closed EventSeries repo:schemaorg/schemaorg".into(),
            },
            per_page: 3,
            token: github_token(),
            account_email: None,
            include_body: true,
            include_comments: false,
            include_review_comments: false,
            include_links: false,
        };
        let results = fetch_all(&source, &req).unwrap();
        assert!(!results.is_empty());
        for conv in &results {
            assert!(!conv.title.is_empty());
            assert_eq!(conv.state, "closed");
        }
    }

    #[test]
    #[ignore = "requires TOKEN_GITHUB (or GITHUB_TOKEN) and live network"]
    fn github_fetch_one_comment_has_author_and_body() {
        let source = GitHubSource::new(false).unwrap();
        let req = req_id("schemaorg/schemaorg", "1842", false);
        let conv = fetch_all(&source, &req)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let first = conv
            .comments
            .first()
            .expect("expected at least one comment");
        assert!(first.author.is_some());
        assert!(!first.created_at.is_empty());
    }

    #[test]
    #[ignore = "requires TOKEN_GITHUB (or GITHUB_TOKEN) and live network"]
    fn github_fetch_pr_2402_default_issue_comments_only() {
        let source = GitHubSource::new(false).unwrap();
        let req = req_id_with_kind("github/gitignore", "2402", ContentKind::Pr);
        let conv = fetch_all(&source, &req)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();

        assert_eq!(conv.id, "2402");
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
    #[ignore = "requires TOKEN_GITHUB (or GITHUB_TOKEN) and live network"]
    fn github_fetch_pr_2402_with_review_comments() {
        let source = GitHubSource::new(false).unwrap();
        let mut req = req_id_with_kind("github/gitignore", "2402", ContentKind::Pr);
        req.include_review_comments = true;
        let conv = fetch_all(&source, &req)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();

        assert_eq!(conv.id, "2402");
        assert!(
            conv.comments
                .iter()
                .any(|c| c.kind.as_deref() == Some("review_comment"))
        );
        assert!(conv.comments.iter().any(|c| c.review_path.is_some()));
    }

    #[test]
    #[ignore = "requires TOKEN_GITHUB (or GITHUB_TOKEN) and live network"]
    fn github_search_pr_query_includes_2402() {
        let source = GitHubSource::new(false).unwrap();
        let req = FetchRequest {
            target: FetchTarget::Search {
                raw_query: "repo:github/gitignore is:pr 2402".into(),
            },
            per_page: 3,
            token: github_token(),
            account_email: None,
            include_body: true,
            include_comments: false,
            include_review_comments: false,
            include_links: false,
        };
        let results = fetch_all(&source, &req).unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().any(|c| c.id == "2402"));
    }

    #[test]
    #[ignore = "requires TOKEN_GITHUB (or GITHUB_TOKEN) and live network"]
    fn github_fetch_issue_as_pr_errors_when_kind_is_explicit() {
        let source = GitHubSource::new(false).unwrap();
        let req = req_id_with_kind("schemaorg/schemaorg", "1842", ContentKind::Pr);
        let err = fetch_all(&source, &req).unwrap_err().to_string();
        assert!(err.contains("not a pull request"));
    }

    #[test]
    #[ignore = "requires TOKEN_GITHUB (or GITHUB_TOKEN) and live network"]
    fn github_fetch_pr_as_issue_errors_when_fallback_is_disabled() {
        let source = GitHubSource::new(false).unwrap();
        let req = req_id_with_kind("github/gitignore", "2402", ContentKind::Issue);
        let err = fetch_all(&source, &req).unwrap_err().to_string();
        assert!(err.contains("is a pull request"));
    }

    #[test]
    #[ignore = "requires live network (TOKEN_GITLAB/GITLAB_TOKEN recommended for comments)"]
    fn gitlab_fetch_issue_6() {
        let source = GitLabSource::new(None, false).unwrap();
        let req = FetchRequest {
            target: FetchTarget::Id {
                repo: "veloren/veloren".into(),
                id: "6".into(),
                kind: ContentKind::Issue,
            },
            per_page: 50,
            token: gitlab_token(),
            account_email: None,
            include_body: true,
            include_comments: true,
            include_review_comments: false,
            include_links: true,
        };
        let conv = match fetch_all(&source, &req) {
            Ok(results) => results.into_iter().next().unwrap(),
            Err(err) => {
                let msg = err.to_string();
                if maybe_skip_transient_gitlab_error("gitlab_fetch_issue_6", &msg) {
                    return;
                }
                panic!("unexpected GitLab issue fetch error: {msg}");
            }
        };
        assert_eq!(conv.id, "6");
        assert!(!conv.title.is_empty());
    }

    #[test]
    #[ignore = "requires live network (TOKEN_GITLAB/GITLAB_TOKEN recommended for comments)"]
    fn gitlab_fetch_mr_6() {
        let source = GitLabSource::new(None, false).unwrap();
        let req = FetchRequest {
            target: FetchTarget::Id {
                repo: "veloren/veloren".into(),
                id: "6".into(),
                kind: ContentKind::Pr,
            },
            per_page: 50,
            token: gitlab_token(),
            account_email: None,
            include_body: true,
            include_comments: true,
            include_review_comments: true,
            include_links: true,
        };
        let conv = match fetch_all(&source, &req) {
            Ok(results) => results.into_iter().next().unwrap(),
            Err(err) => {
                let msg = err.to_string();
                if maybe_skip_transient_gitlab_error("gitlab_fetch_mr_6", &msg) {
                    return;
                }
                panic!("unexpected GitLab MR fetch error: {msg}");
            }
        };
        assert_eq!(conv.id, "6");
        assert!(!conv.title.is_empty());
    }

    #[test]
    #[ignore = "requires live network (TOKEN_GITLAB/GITLAB_TOKEN recommended for comments)"]
    fn gitlab_search_issue_results() {
        let source = GitLabSource::new(None, false).unwrap();
        let req = FetchRequest {
            target: FetchTarget::Search {
                raw_query: "repo:veloren/veloren is:issue state:closed terrain".into(),
            },
            per_page: 10,
            token: gitlab_token(),
            account_email: None,
            include_body: true,
            include_comments: true,
            include_review_comments: false,
            include_links: true,
        };
        let results = match fetch_all(&source, &req) {
            Ok(results) => results,
            Err(err) => {
                let msg = err.to_string();
                if maybe_skip_transient_gitlab_error("gitlab_search_issue_results", &msg) {
                    return;
                }
                panic!("unexpected GitLab issue search error: {msg}");
            }
        };
        assert!(!results.is_empty());
    }

    #[test]
    #[ignore = "requires live network (TOKEN_GITLAB/GITLAB_TOKEN recommended for comments)"]
    fn gitlab_search_mr_results() {
        let source = GitLabSource::new(None, false).unwrap();
        let req = FetchRequest {
            target: FetchTarget::Search {
                raw_query: "repo:veloren/veloren is:pr state:closed netcode".into(),
            },
            per_page: 10,
            token: gitlab_token(),
            account_email: None,
            include_body: true,
            include_comments: true,
            include_review_comments: true,
            include_links: true,
        };
        let results = match fetch_all(&source, &req) {
            Ok(results) => results,
            Err(err) => {
                let msg = err.to_string();
                if maybe_skip_transient_gitlab_error("gitlab_search_mr_results", &msg) {
                    return;
                }
                panic!("unexpected GitLab MR search error: {msg}");
            }
        };
        assert!(!results.is_empty());
    }

    #[test]
    #[ignore = "requires live network (public Jira endpoint)"]
    fn jira_fetch_public_issue_cloud_12817() {
        let source = JiraSource::new(Some("https://jira.atlassian.com".into()), false).unwrap();
        let req = FetchRequest {
            target: FetchTarget::Id {
                repo: String::new(),
                id: "CLOUD-12817".into(),
                kind: ContentKind::Issue,
            },
            per_page: 50,
            token: jira_token(),
            account_email: None,
            include_body: true,
            include_comments: true,
            include_review_comments: false,
            include_links: true,
        };
        let conv = match fetch_all(&source, &req) {
            Ok(results) => results.into_iter().next().unwrap(),
            Err(err) => {
                let msg = err.to_string();
                if is_public_jira_login_wall(&msg) {
                    fail_public_jira_login_wall("jira_fetch_public_issue_cloud_12817", &msg);
                }
                panic!("unexpected Jira issue fetch error: {msg}");
            }
        };
        assert_eq!(conv.id, "CLOUD-12817");
        assert!(!conv.title.is_empty());
    }

    #[test]
    #[ignore = "requires live network (public Jira endpoint)"]
    fn jira_search_public_project() {
        let source = JiraSource::new(Some("https://jira.atlassian.com".into()), false).unwrap();
        let req = FetchRequest {
            target: FetchTarget::Search {
                raw_query: "repo:CLOUD state:closed CLOUD-12817".into(),
            },
            per_page: 5,
            token: jira_token(),
            account_email: None,
            include_body: true,
            include_comments: true,
            include_review_comments: false,
            include_links: true,
        };
        let results = match fetch_all(&source, &req) {
            Ok(results) => results,
            Err(err) => {
                let msg = err.to_string();
                if is_public_jira_login_wall(&msg) {
                    fail_public_jira_login_wall("jira_search_public_project", &msg);
                }
                panic!("unexpected Jira search error: {msg}");
            }
        };
        assert!(!results.is_empty());
        assert!(results.iter().any(|c| c.id == "CLOUD-12817"));
    }

    #[test]
    fn jira_rejects_pr_kind() {
        let source = JiraSource::new(Some("https://jira.atlassian.com".into()), false).unwrap();
        let req = FetchRequest {
            target: FetchTarget::Id {
                repo: String::new(),
                id: "CLOUD-12817".into(),
                kind: ContentKind::Pr,
            },
            per_page: 5,
            token: None,
            account_email: None,
            include_body: true,
            include_comments: true,
            include_review_comments: false,
            include_links: true,
        };
        let err = fetch_all(&source, &req).unwrap_err().to_string();
        assert!(err.contains("does not support pull requests"));
    }

    #[test]
    #[ignore = "requires live network and BITBUCKET_REPO/BITBUCKET_PR_ID env vars"]
    fn bitbucket_cloud_fetch_pr_by_id() {
        let source = BitbucketSource::new(None, Some("cloud".into()), false).unwrap();
        let repo = required_env("BITBUCKET_REPO");
        let pr_id = required_env("BITBUCKET_PR_ID");
        let req = FetchRequest {
            target: FetchTarget::Id {
                repo,
                id: pr_id.clone(),
                kind: ContentKind::Pr,
            },
            per_page: 50,
            token: bitbucket_token(),
            account_email: None,
            include_body: true,
            include_comments: true,
            include_review_comments: true,
            include_links: true,
        };
        let conv = fetch_all(&source, &req)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(conv.id, pr_id);
        assert!(!conv.title.is_empty());
    }

    #[test]
    #[ignore = "requires live network and BITBUCKET_REPO env var"]
    fn bitbucket_cloud_search_pr_results() {
        let source = BitbucketSource::new(None, Some("cloud".into()), false).unwrap();
        let repo = required_env("BITBUCKET_REPO");
        let req = FetchRequest {
            target: FetchTarget::Search {
                raw_query: format!("repo:{repo} is:pr state:all"),
            },
            per_page: 10,
            token: bitbucket_token(),
            account_email: None,
            include_body: true,
            include_comments: false,
            include_review_comments: false,
            include_links: true,
        };
        let results = fetch_all(&source, &req).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn bitbucket_cloud_rejects_issue_kind() {
        let source = BitbucketSource::new(None, Some("cloud".into()), false).unwrap();
        let req = FetchRequest {
            target: FetchTarget::Id {
                repo: "workspace/repo".into(),
                id: "1".into(),
                kind: ContentKind::Issue,
            },
            per_page: 10,
            token: None,
            account_email: None,
            include_body: true,
            include_comments: false,
            include_review_comments: false,
            include_links: true,
        };
        let err = fetch_all(&source, &req).unwrap_err().to_string();
        assert!(err.contains("supports pull requests only"));
    }
}
