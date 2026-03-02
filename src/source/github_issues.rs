use anyhow::{anyhow, Result};
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::model::{Comment, Conversation};
use super::{Query, Source};

pub struct GitHubIssues {
    client: Client,
}

impl GitHubIssues {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent("99problems-cli/0.1.0")
            .build()?;
        Ok(Self { client })
    }

    fn auth_header(token: &Option<String>) -> Option<String> {
        token.as_ref().map(|t| format!("Bearer {t}"))
    }

    fn get_pages<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        token: &Option<String>,
        per_page: u32,
    ) -> Result<Vec<T>> {
        let mut results = vec![];
        let mut page = 1u32;

        loop {
            let mut req = self.client.get(url)
                .query(&[("per_page", per_page.to_string()), ("page", page.to_string())]);

            if let Some(auth) = Self::auth_header(token) {
                req = req.header("Authorization", auth)
                         .header("X-GitHub-Api-Version", "2022-11-28");
            }

            let resp = req.send()?;

            if !resp.status().is_success() {
                return Err(anyhow!("GitHub API error {}: {}", resp.status(), resp.text()?));
            }

            let has_next = resp.headers()
                .get("link")
                .and_then(|v| v.to_str().ok())
                .map(|l| l.contains(r#"rel="next""#))
                .unwrap_or(false);

            let items: Vec<T> = resp.json()?;
            let done = items.is_empty() || !has_next;
            results.extend(items);
            if done { break; }
            page += 1;
        }

        Ok(results)
    }
}

// --- GitHub API response shapes ---

#[derive(Deserialize)]
struct SearchResponse {
    items: Vec<IssueItem>,
}

#[derive(Deserialize)]
struct IssueItem {
    number: u64,
    title: String,
    state: String,
    body: Option<String>,
}

#[derive(Deserialize)]
struct CommentItem {
    user: Option<UserItem>,
    created_at: String,
    body: Option<String>,
}

#[derive(Deserialize)]
struct UserItem {
    login: String,
}

impl Source for GitHubIssues {
    fn fetch(&self, query: &Query) -> Result<Vec<Conversation>> {
        let search_url = "https://api.github.com/search/issues";
        let mut page = 1u32;
        let mut all_issues: Vec<IssueItem> = vec![];

        loop {
            let mut req = self.client.get(search_url)
                .query(&[
                    ("q", query.raw.as_str()),
                    ("per_page", "100"),
                    ("page", &page.to_string()),
                ]);

            if let Some(auth) = Self::auth_header(&query.token) {
                req = req.header("Authorization", auth)
                         .header("X-GitHub-Api-Version", "2022-11-28");
            }

            let resp = req.send()?;
            if !resp.status().is_success() {
                return Err(anyhow!("GitHub search error {}: {}", resp.status(), resp.text()?));
            }

            let search: SearchResponse = resp.json()?;
            let done = search.items.len() < 100;
            all_issues.extend(search.items);
            if done { break; }
            page += 1;
        }

        // Determine repo from query for comment fetching
        let repo = extract_repo(&query.raw)
            .ok_or_else(|| anyhow!("No repo: found in query. Use --repo or include 'repo:owner/name' in -q"))?;

        let mut conversations = vec![];
        for issue in all_issues {
            let comments_url = format!(
                "https://api.github.com/repos/{repo}/issues/{}/comments",
                issue.number
            );
            let raw_comments: Vec<CommentItem> =
                self.get_pages(&comments_url, &query.token, 100)?;

            conversations.push(Conversation {
                id: issue.number,
                title: issue.title,
                state: issue.state,
                body: issue.body,
                comments: raw_comments.into_iter().map(|c| Comment {
                    author: c.user.map(|u| u.login),
                    created_at: c.created_at,
                    body: c.body,
                }).collect(),
            });
        }

        Ok(conversations)
    }

    fn fetch_one(&self, repo: &str, issue_id: u64) -> Result<Conversation> {
        let issue_url = format!("https://api.github.com/repos/{repo}/issues/{issue_id}");
        let req = self.client.get(&issue_url);
        // token is not stored on struct, so we pass None here — callers set it via Query
        // For fetch_one we skip auth (public repos). Callers who need auth should use fetch().
        let resp = req.send()?;
        if !resp.status().is_success() {
            return Err(anyhow!("GitHub issue error {}: {}", resp.status(), resp.text()?));
        }
        let issue: IssueItem = resp.json()?;

        let comments_url = format!("https://api.github.com/repos/{repo}/issues/{issue_id}/comments");
        let raw_comments: Vec<CommentItem> = self.get_pages(&comments_url, &None, 100)?;

        Ok(Conversation {
            id: issue.number,
            title: issue.title,
            state: issue.state,
            body: issue.body,
            comments: raw_comments.into_iter().map(|c| Comment {
                author: c.user.map(|u| u.login),
                created_at: c.created_at,
                body: c.body,
            }).collect(),
        })
    }
}

/// Extract `owner/repo` from a query string containing `repo:owner/repo`.
pub fn extract_repo(query: &str) -> Option<String> {
    query.split_whitespace()
        .find(|t| t.starts_with("repo:"))
        .map(|t| t.trim_start_matches("repo:").to_string())
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
}
