use anyhow::Result;
use reqwest::blocking::{RequestBuilder, Response};
use serde::Deserialize;
use tracing::{debug, trace, warn};

use super::model::{
    ConversationSeed, IssueCommentItem, ReviewCommentItem, map_graphql_link_nodes,
    map_issue_collection_links, map_issue_comment, map_issue_url_links, map_review_comment,
    map_timeline_links,
};
use super::{GITHUB_API_BASE, GITHUB_API_VERSION, GitHubSource, PAGE_SIZE};
use crate::error::{AppError, app_error_from_decode, app_error_from_reqwest};
use crate::model::{Comment, Conversation, ConversationLink, ConversationMetadata};
use crate::source::FetchRequest;

impl GitHubSource {
    pub(super) fn apply_auth(req: RequestBuilder, token: Option<&str>) -> RequestBuilder {
        let req = req
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION);
        if let Some(t) = token {
            req.header("Authorization", format!("Bearer {t}"))
        } else {
            req
        }
    }

    pub(super) fn bounded_per_page(per_page: u32) -> u32 {
        per_page.clamp(1, PAGE_SIZE)
    }

    pub(super) fn send(req: RequestBuilder, operation: &str) -> Result<Response> {
        req.send()
            .map_err(|err| app_error_from_reqwest("GitHub", operation, &err).into())
    }

    pub(super) fn get_pages<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        token: Option<&str>,
        per_page: u32,
    ) -> Result<Vec<T>> {
        let mut results = vec![];
        let mut page = 1u32;
        let per_page = Self::bounded_per_page(per_page);

        loop {
            debug!(url = %url, page, per_page, "fetching GitHub page");
            let req = self.client.get(url).query(&[
                ("per_page", per_page.to_string()),
                ("page", page.to_string()),
            ]);
            let req = Self::apply_auth(req, token);
            let resp = Self::send(req, "page fetch")?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp
                    .text()
                    .map_err(|err| app_error_from_reqwest("GitHub", "error body read", &err))?;
                return Err(AppError::from_http("GitHub", "page fetch", status, &body).into());
            }

            let has_next = resp
                .headers()
                .get("link")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|l| l.contains(r#"rel="next""#));

            let items: Vec<T> = resp
                .json()
                .map_err(|err| app_error_from_decode("GitHub", "page fetch", err))?;
            trace!(count = items.len(), page, "decoded GitHub page");
            let done = items.is_empty() || !has_next;
            results.extend(items);
            if done {
                break;
            }
            page += 1;
        }

        Ok(results)
    }

    pub(super) fn fetch_issue_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let comments_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/comments");
        let raw_comments: Vec<IssueCommentItem> =
            self.get_pages(&comments_url, req.token.as_deref(), req.per_page)?;
        Ok(raw_comments.into_iter().map(map_issue_comment).collect())
    }

    pub(super) fn fetch_review_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let comments_url = format!("{GITHUB_API_BASE}/repos/{repo}/pulls/{id}/comments");
        let raw_comments: Vec<ReviewCommentItem> =
            self.get_pages(&comments_url, req.token.as_deref(), req.per_page)?;
        Ok(raw_comments.into_iter().map(map_review_comment).collect())
    }

    pub(super) fn fetch_links(
        &self,
        repo: &str,
        id: u64,
        is_pr: bool,
        req: &FetchRequest,
    ) -> ConversationMetadata {
        if !req.include_links {
            return ConversationMetadata::empty();
        }

        let mut links: Vec<ConversationLink> = Vec::new();
        let timeline_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/timeline");
        match self.get_pages::<serde_json::Value>(&timeline_url, req.token.as_deref(), req.per_page)
        {
            Ok(events) => {
                for event in events {
                    links.extend(map_timeline_links(&event));
                }
            }
            Err(err) => warn!(repo, id, error = %err, "GitHub timeline link fetch failed"),
        }

        let issue_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}");
        match self.get_one::<serde_json::Value>(&issue_url, req.token.as_deref()) {
            Ok(Some(issue)) => links.extend(map_issue_url_links(&issue)),
            Ok(None) => {}
            Err(err) => warn!(repo, id, error = %err, "GitHub issue detail link fetch failed"),
        }

        let blocked_by_url =
            format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/dependencies/blocked_by");
        match self.get_pages::<serde_json::Value>(
            &blocked_by_url,
            req.token.as_deref(),
            req.per_page,
        ) {
            Ok(blocked_by) => links.extend(map_issue_collection_links(&blocked_by, "blocked_by")),
            Err(err) => warn!(repo, id, error = %err, "GitHub blocked_by dependency fetch failed"),
        }

        let blocking_url =
            format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/dependencies/blocking");
        match self.get_pages::<serde_json::Value>(&blocking_url, req.token.as_deref(), req.per_page)
        {
            Ok(blocking) => links.extend(map_issue_collection_links(&blocking, "blocks")),
            Err(err) => warn!(repo, id, error = %err, "GitHub blocking dependency fetch failed"),
        }

        let sub_issues_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/sub_issues");
        match self.get_pages::<serde_json::Value>(
            &sub_issues_url,
            req.token.as_deref(),
            req.per_page,
        ) {
            Ok(sub_issues) => links.extend(map_issue_collection_links(&sub_issues, "child")),
            Err(err) => warn!(repo, id, error = %err, "GitHub sub-issues fetch failed"),
        }

        let parent_url = format!("{GITHUB_API_BASE}/repos/{repo}/issues/{id}/parent");
        match self.get_one::<serde_json::Value>(&parent_url, req.token.as_deref()) {
            Ok(Some(parent)) => {
                links.extend(map_issue_collection_links(
                    std::slice::from_ref(&parent),
                    "parent",
                ));
            }
            Ok(None) => {}
            Err(err) => warn!(repo, id, error = %err, "GitHub parent issue fetch failed"),
        }

        match self.fetch_graphql_links(repo, id, is_pr, req.token.as_deref()) {
            Ok(graph_links) => links.extend(graph_links),
            Err(err) => warn!(repo, id, error = %err, "GitHub GraphQL links fetch failed"),
        }

        ConversationMetadata::with_links(links)
    }

    pub(super) fn get_one<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        token: Option<&str>,
    ) -> Result<Option<T>> {
        let req = Self::apply_auth(self.client.get(url), token);
        let resp = Self::send(req, "single fetch")?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .map_err(|err| app_error_from_reqwest("GitHub", "error body read", &err))?;
            return Err(AppError::from_http("GitHub", "single fetch", status, &body).into());
        }
        let item = resp
            .json()
            .map_err(|err| app_error_from_decode("GitHub", "single fetch", err))?;
        Ok(Some(item))
    }

    fn fetch_graphql_links(
        &self,
        repo: &str,
        id: u64,
        is_pr: bool,
        token: Option<&str>,
    ) -> Result<Vec<ConversationLink>> {
        let Some((owner, name)) = repo.split_once('/') else {
            return Ok(Vec::new());
        };
        let (query, relation, kind, path) = if is_pr {
            (
                "query($owner:String!,$name:String!,$n:Int!){ repository(owner:$owner,name:$name){ pullRequest(number:$n){ closingIssuesReferences(first:100){nodes{number url}} } } }",
                "closes",
                "issue",
                "/data/repository/pullRequest/closingIssuesReferences/nodes",
            )
        } else {
            (
                "query($owner:String!,$name:String!,$n:Int!){ repository(owner:$owner,name:$name){ issue(number:$n){ closedByPullRequestsReferences(first:100){nodes{number url}} } } }",
                "closed_by",
                "pr",
                "/data/repository/issue/closedByPullRequestsReferences/nodes",
            )
        };

        let body = serde_json::json!({
            "query": query,
            "variables": {
                "owner": owner,
                "name": name,
                "n": id
            }
        });

        let request = Self::apply_auth(self.client.post("https://api.github.com/graphql"), token)
            .header("Content-Type", "application/json")
            .json(&body);
        let response = Self::send(request, "graphql fetch")?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .map_err(|err| app_error_from_reqwest("GitHub", "error body read", &err))?;
            return Err(AppError::from_http("GitHub", "graphql fetch", status, &body).into());
        }
        let payload: serde_json::Value = response
            .json()
            .map_err(|err| app_error_from_decode("GitHub", "graphql fetch", err))?;
        if let Some(errors) = payload.get("errors") {
            return Err(
                AppError::provider(format!("GitHub GraphQL returned errors: {errors}")).into(),
            );
        }
        let nodes = payload
            .pointer(path)
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(map_graphql_link_nodes(&nodes, relation, kind))
    }

    pub(super) fn fetch_conversation(
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
        let metadata = if req.include_links {
            self.fetch_links(repo, item.id, item.is_pr, req)
        } else {
            ConversationMetadata::empty()
        };

        Ok(Conversation {
            id: item.id.to_string(),
            title: item.title,
            state: item.state,
            body: item.body,
            comments,
            metadata,
        })
    }
}
