use anyhow::{Result, anyhow};
use reqwest::blocking::{Client, RequestBuilder};
use serde::Deserialize;

use super::{ContentKind, FetchRequest, FetchTarget, Source};
use crate::model::{Comment, Conversation};

const GITHUB_API_BASE: &str = "https://api.github.com";
const GITHUB_API_VERSION: &str = "2022-11-28";
const PAGE_SIZE: u32 = 100;

pub struct GitHubSource {
    client: Client,
}

impl GitHubSource {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("99problems-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { client })
    }

    /// Adds Authorization + API version headers when a token is present.
    fn apply_auth(req: RequestBuilder, token: &Option<String>) -> RequestBuilder {
        match token.as_ref() {
            Some(t) => req
                .header("Authorization", format!("Bearer {t}"))
                .header("X-GitHub-Api-Version", GITHUB_API_VERSION),
            None => req,
        }
    }

    fn bounded_per_page(per_page: u32) -> u32 {
        per_page.clamp(1, PAGE_SIZE)
    }

    fn get_pages<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        token: &Option<String>,
        per_page: u32,
    ) -> Result<Vec<T>> {
        let mut results = vec![];
        let mut page = 1u32;
        let per_page = Self::bounded_per_page(per_page);

        loop {
            let req = self.client.get(url).query(&[
                ("per_page", per_page.to_string()),
                ("page", page.to_string()),
            ]);
            let req = Self::apply_auth(req, token);
            let resp = req.send()?;

            if !resp.status().is_success() {
                return Err(anyhow!(
                    "GitHub API error {}: {}",
                    resp.status(),
                    resp.text()?
                ));
            }

            let has_next = resp
                .headers()
                .get("link")
                .and_then(|v| v.to_str().ok())
                .map(|l| l.contains(r#"rel="next""#))
                .unwrap_or(false);

            let items: Vec<T> = resp.json()?;
            let done = items.is_empty() || !has_next;
            results.extend(items);
            if done {
                break;
            }
            page += 1;
        }

        Ok(results)
    }

    fn fetch_issue_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let comments_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/comments");
        let raw_comments: Vec<IssueCommentItem> =
            self.get_pages(&comments_url, &req.token, req.per_page)?;
        Ok(raw_comments.into_iter().map(map_issue_comment).collect())
    }

    fn fetch_review_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let comments_url = format!("{GITHUB_API_BASE}/repos/{repo}/pulls/{id}/comments");
        let raw_comments: Vec<ReviewCommentItem> =
            self.get_pages(&comments_url, &req.token, req.per_page)?;
        Ok(raw_comments.into_iter().map(map_review_comment).collect())
    }

    fn fetch_conversation(
        &self,
        repo: &str,
        item: ConversationSeed,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let mut comments = Vec::new();
        if req.include_comments {
            comments = self.fetch_issue_comments(repo, item.id, req)?;
            if item.is_pr && req.include_review_comments {
                comments.extend(self.fetch_review_comments(repo, item.id, req)?);
                comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            }
        }

        Ok(Conversation {
            id: item.id.to_string(),
            title: item.title,
            state: item.state,
            body: item.body,
            comments,
        })
    }

    fn search(&self, req: &FetchRequest, raw_query: &str) -> Result<Vec<Conversation>> {
        let search_url = format!("{GITHUB_API_BASE}/search/issues");
        let mut page = 1u32;
        let mut all_items: Vec<SearchItem> = vec![];
        let per_page = Self::bounded_per_page(req.per_page);

        loop {
            let req_http = self.client.get(&search_url).query(&[
                ("q", raw_query),
                ("per_page", &per_page.to_string()),
                ("page", &page.to_string()),
            ]);
            let req_http = Self::apply_auth(req_http, &req.token);
            let resp = req_http.send()?;

            if !resp.status().is_success() {
                return Err(anyhow!(
                    "GitHub search error {}: {}",
                    resp.status(),
                    resp.text()?
                ));
            }

            let search: SearchResponse = resp.json()?;
            let done = search.items.len() < per_page as usize;
            all_items.extend(search.items);
            if done {
                break;
            }
            page += 1;
        }

        let repo_from_query = extract_repo(raw_query);
        all_items
            .into_iter()
            .map(|item| {
                let repo = item
                    .repository_url
                    .as_deref()
                    .and_then(repo_from_repository_url)
                    .or_else(|| repo_from_query.clone())
                    .ok_or_else(|| {
                        anyhow!(
                            "Could not determine repo for item #{}. Include repo:owner/name in query.",
                            item.number
                        )
                    })?;

                self.fetch_conversation(
                    &repo,
                    ConversationSeed {
                        id: item.number,
                        title: item.title,
                        state: item.state,
                        body: item.body,
                        is_pr: item.pull_request.is_some(),
                    },
                    req,
                )
            })
            .collect()
    }

    fn fetch_by_id(
        &self,
        req: &FetchRequest,
        repo: &str,
        id: &str,
        kind: ContentKind,
        allow_fallback_to_pr: bool,
    ) -> Result<Vec<Conversation>> {
        let issue_id = id
            .parse::<u64>()
            .map_err(|_| anyhow!("GitHub expects a numeric issue/PR id, got '{id}'."))?;
        let issue_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{issue_id}");
        let request = Self::apply_auth(self.client.get(&issue_url), &req.token);
        let resp = request.send()?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "GitHub issue error {}: {}",
                resp.status(),
                resp.text()?
            ));
        }
        let issue: IssueItem = resp.json()?;
        let is_pr = issue.pull_request.is_some();

        match kind {
            ContentKind::Issue if is_pr && !allow_fallback_to_pr => {
                return Err(anyhow!(
                    "ID {issue_id} in repo {repo} is a pull request. Use --type pr or omit --type."
                ));
            }
            ContentKind::Issue if is_pr && allow_fallback_to_pr => {
                eprintln!(
                    "Warning: --id defaulted to issue, but found PR #{issue_id}; use --type pr for clarity."
                );
            }
            ContentKind::Pr if !is_pr => {
                return Err(anyhow!(
                    "ID {issue_id} in repo {repo} is an issue, not a pull request."
                ));
            }
            _ => {}
        }

        Ok(vec![self.fetch_conversation(
            repo,
            ConversationSeed {
                id: issue.number,
                title: issue.title,
                state: issue.state,
                body: issue.body,
                is_pr,
            },
            req,
        )?])
    }
}

impl Source for GitHubSource {
    fn fetch(&self, req: &FetchRequest) -> Result<Vec<Conversation>> {
        match &req.target {
            FetchTarget::Search { raw_query } => self.search(req, raw_query),
            FetchTarget::Id {
                repo,
                id,
                kind,
                allow_fallback_to_pr,
            } => self.fetch_by_id(req, repo, id, *kind, *allow_fallback_to_pr),
        }
    }
}

// --- GitHub API response shapes ---

#[derive(Deserialize)]
struct SearchResponse {
    items: Vec<SearchItem>,
}

#[derive(Deserialize)]
struct SearchItem {
    number: u64,
    title: String,
    state: String,
    body: Option<String>,
    repository_url: Option<String>,
    pull_request: Option<PullRequestMarker>,
}

#[derive(Deserialize)]
struct IssueItem {
    number: u64,
    title: String,
    state: String,
    body: Option<String>,
    pull_request: Option<PullRequestMarker>,
}

#[derive(Deserialize)]
struct PullRequestMarker {}

#[derive(Deserialize)]
struct IssueCommentItem {
    user: Option<UserItem>,
    created_at: String,
    body: Option<String>,
}

#[derive(Deserialize)]
struct ReviewCommentItem {
    user: Option<UserItem>,
    created_at: String,
    body: Option<String>,
    path: Option<String>,
    line: Option<u64>,
    side: Option<String>,
}

#[derive(Deserialize)]
struct UserItem {
    login: String,
}

struct ConversationSeed {
    id: u64,
    title: String,
    state: String,
    body: Option<String>,
    is_pr: bool,
}

fn map_issue_comment(c: IssueCommentItem) -> Comment {
    Comment {
        author: c.user.map(|u| u.login),
        created_at: c.created_at,
        body: c.body,
        kind: Some("issue_comment".into()),
        review_path: None,
        review_line: None,
        review_side: None,
    }
}

fn map_review_comment(c: ReviewCommentItem) -> Comment {
    Comment {
        author: c.user.map(|u| u.login),
        created_at: c.created_at,
        body: c.body,
        kind: Some("review_comment".into()),
        review_path: c.path,
        review_line: c.line,
        review_side: c.side,
    }
}

/// Extract `owner/repo` from a query string containing `repo:owner/repo`.
pub fn extract_repo(query: &str) -> Option<String> {
    query
        .split_whitespace()
        .find(|t| t.starts_with("repo:"))
        .map(|t| t.trim_start_matches("repo:").to_string())
}

fn repo_from_repository_url(url: &str) -> Option<String> {
    let prefix = format!("{GITHUB_API_BASE}/repos/");
    url.strip_prefix(&prefix).map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_repo_finds_token() {
        assert_eq!(
            extract_repo("is:issue state:closed repo:owner/repo Event"),
            Some("owner/repo".into())
        );
    }

    #[test]
    fn extract_repo_returns_none_when_absent() {
        assert_eq!(extract_repo("is:issue state:closed Event"), None);
    }

    #[test]
    fn repo_from_repository_url_parses_repo() {
        assert_eq!(
            repo_from_repository_url("https://api.github.com/repos/owner/repo"),
            Some("owner/repo".into())
        );
    }

    #[test]
    fn repo_from_repository_url_returns_none_for_non_github_api_url() {
        assert_eq!(
            repo_from_repository_url("https://example.com/repos/owner/repo"),
            None
        );
    }
}
