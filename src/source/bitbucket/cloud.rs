use anyhow::Result;
use reqwest::StatusCode;
use reqwest::blocking::{RequestBuilder, Response};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tracing::{debug, trace, warn};

use super::BitbucketSource;
use super::auth::{apply_auth, auth_hint};
use super::query::{BitbucketFilters, parse_bitbucket_query, parse_repo};
use crate::error::{AppError, app_error_from_decode, app_error_from_reqwest};
use crate::model::{Comment, Conversation};
use crate::source::{ContentKind, FetchRequest, FetchTarget};

const PAGE_SIZE: u32 = 50;

impl BitbucketSource {
    pub(super) fn fetch_cloud_stream(
        &self,
        req: &FetchRequest,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        match &req.target {
            FetchTarget::Search { raw_query } => self.search_stream(req, raw_query, emit),
            FetchTarget::Id {
                repo,
                id,
                kind,
                allow_fallback_to_pr,
            } => self.fetch_by_id_stream(req, repo, id, *kind, *allow_fallback_to_pr, emit),
        }
    }

    fn search_stream(
        &self,
        req: &FetchRequest,
        raw_query: &str,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let filters = parse_bitbucket_query(raw_query);
        let (workspace, repo_slug) = parse_repo(filters.repo.as_deref())?;
        let repo = format!("{workspace}/{repo_slug}");

        match filters.kind {
            ContentKind::Issue => self.search_issues_stream(req, &repo, &filters, emit),
            ContentKind::Pr => self.search_prs_stream(req, &repo, &filters, emit),
        }
    }

    fn search_issues_stream(
        &self,
        req: &FetchRequest,
        repo: &str,
        filters: &BitbucketFilters,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let url = format!("{}/repositories/{repo}/issues", self.base_url);
        let params = vec![("sort".to_string(), "-updated_on".to_string())];
        let mut emitted = 0usize;
        self.get_pages_stream(
            &url,
            &params,
            req.token.as_deref(),
            req.account_email.as_deref(),
            req.per_page,
            &mut |item: BitbucketIssueItem| {
                if !matches_issue_filters(&item, filters) {
                    return Ok(());
                }
                let conversation = self.fetch_issue_conversation(repo, item, req)?;
                emit(conversation)?;
                emitted += 1;
                Ok(())
            },
        )?;
        Ok(emitted)
    }

    fn search_prs_stream(
        &self,
        req: &FetchRequest,
        repo: &str,
        filters: &BitbucketFilters,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let url = format!("{}/repositories/{repo}/pullrequests", self.base_url);
        let params = vec![
            ("sort".to_string(), "-updated_on".to_string()),
            ("state".to_string(), "ALL".to_string()),
        ];
        let mut emitted = 0usize;
        self.get_pages_stream(
            &url,
            &params,
            req.token.as_deref(),
            req.account_email.as_deref(),
            req.per_page,
            &mut |item: BitbucketPullRequestItem| {
                if !matches_pr_filters(&item, filters) {
                    return Ok(());
                }
                let conversation = self.fetch_pr_conversation(repo, item, req)?;
                emit(conversation)?;
                emitted += 1;
                Ok(())
            },
        )?;
        Ok(emitted)
    }

    fn fetch_by_id_stream(
        &self,
        req: &FetchRequest,
        repo: &str,
        id: &str,
        kind: ContentKind,
        allow_fallback_to_pr: bool,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let (workspace, repo_slug) = parse_repo(Some(repo))?;
        let repo = format!("{workspace}/{repo_slug}");
        let id = id.parse::<u64>().map_err(|_| {
            AppError::usage(format!(
                "Bitbucket expects a numeric issue/PR id, got '{id}'."
            ))
        })?;
        match kind {
            ContentKind::Issue => {
                if let Some(issue) = self.fetch_issue_by_id(&repo, id, req)? {
                    emit(self.fetch_issue_conversation(&repo, issue, req)?)?;
                    return Ok(1);
                }
                if allow_fallback_to_pr && let Some(pr) = self.fetch_pr_by_id(&repo, id, req)? {
                    warn!(
                        "Warning: --id defaulted to issue, but found PR #{id}; use --type pr for clarity."
                    );
                    emit(self.fetch_pr_conversation(&repo, pr, req)?)?;
                    return Ok(1);
                }
                Err(AppError::not_found(format!("Issue #{id} not found in repo {repo}.")).into())
            }
            ContentKind::Pr => {
                if let Some(pr) = self.fetch_pr_by_id(&repo, id, req)? {
                    emit(self.fetch_pr_conversation(&repo, pr, req)?)?;
                    return Ok(1);
                }
                if self.fetch_issue_by_id(&repo, id, req)?.is_some() {
                    return Err(AppError::usage(format!(
                        "ID {id} in repo {repo} is an issue, not a pull request."
                    ))
                    .into());
                }
                Err(
                    AppError::not_found(format!("Pull request #{id} not found in repo {repo}."))
                        .into(),
                )
            }
        }
    }

    fn fetch_issue_by_id(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Option<BitbucketIssueItem>> {
        let url = format!("{}/repositories/{repo}/issues/{id}", self.base_url);
        self.get_one(
            &url,
            req.token.as_deref(),
            req.account_email.as_deref(),
            "issue fetch",
        )
    }

    fn fetch_pr_by_id(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Option<BitbucketPullRequestItem>> {
        let url = format!("{}/repositories/{repo}/pullrequests/{id}", self.base_url);
        self.get_one(
            &url,
            req.token.as_deref(),
            req.account_email.as_deref(),
            "pull request fetch",
        )
    }

    fn fetch_issue_conversation(
        &self,
        repo: &str,
        item: BitbucketIssueItem,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let comments = if req.include_comments {
            self.fetch_issue_comments(repo, item.id, req)?
        } else {
            Vec::new()
        };
        Ok(Conversation {
            id: item.id.to_string(),
            title: item.title,
            state: item.state,
            body: item.content.and_then(|c| c.raw).filter(|b| !b.is_empty()),
            comments,
        })
    }

    fn fetch_pr_conversation(
        &self,
        repo: &str,
        item: BitbucketPullRequestItem,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let comments = if req.include_comments {
            self.fetch_pr_comments(repo, item.id, req)?
        } else {
            Vec::new()
        };
        let body = item
            .description
            .or_else(|| item.summary.and_then(|s| s.raw))
            .filter(|b| !b.is_empty());
        Ok(Conversation {
            id: item.id.to_string(),
            title: item.title,
            state: item.state,
            body,
            comments,
        })
    }

    fn fetch_issue_comments(
        &self,
        repo: &str,
        id: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let url = format!("{}/repositories/{repo}/issues/{id}/comments", self.base_url);
        let mut comments = Vec::new();
        self.get_pages_stream(
            &url,
            &[],
            req.token.as_deref(),
            req.account_email.as_deref(),
            req.per_page,
            &mut |item: BitbucketCommentItem| {
                if item.deleted.unwrap_or(false) {
                    return Ok(());
                }
                comments.push(map_issue_comment(item));
                Ok(())
            },
        )?;
        Ok(comments)
    }

    fn fetch_pr_comments(&self, repo: &str, id: u64, req: &FetchRequest) -> Result<Vec<Comment>> {
        let url = format!(
            "{}/repositories/{repo}/pullrequests/{id}/comments",
            self.base_url
        );
        let mut comments = Vec::new();
        self.get_pages_stream(
            &url,
            &[],
            req.token.as_deref(),
            req.account_email.as_deref(),
            req.per_page,
            &mut |item: BitbucketCommentItem| {
                if item.deleted.unwrap_or(false) {
                    return Ok(());
                }
                if let Some(mapped) = map_pr_comment(item, req.include_review_comments) {
                    comments.push(mapped);
                }
                Ok(())
            },
        )?;
        comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(comments)
    }

    fn bounded_per_page(per_page: u32) -> u32 {
        per_page.clamp(1, PAGE_SIZE)
    }

    fn send(req: RequestBuilder, operation: &str) -> Result<Response> {
        req.send()
            .map_err(|err| app_error_from_reqwest("Bitbucket", operation, &err).into())
    }

    fn get_one<T: DeserializeOwned>(
        &self,
        url: &str,
        token: Option<&str>,
        account_email: Option<&str>,
        operation: &str,
    ) -> Result<Option<T>> {
        let request = apply_auth(self.client.get(url), token, account_email)
            .header("Accept", "application/json");
        let response = Self::send(request, operation)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let item = parse_bitbucket_json(response, token, account_email, operation)?;
        Ok(Some(item))
    }

    #[allow(clippy::too_many_arguments)]
    fn get_pages_stream<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(String, String)],
        token: Option<&str>,
        account_email: Option<&str>,
        per_page: u32,
        emit: &mut dyn FnMut(T) -> Result<()>,
    ) -> Result<usize> {
        let per_page = Self::bounded_per_page(per_page);
        let mut emitted = 0usize;
        let mut next_url = Some(url.to_string());
        let mut first = true;

        while let Some(current_url) = next_url {
            debug!(url = %current_url, per_page, "fetching Bitbucket page");
            let mut request = apply_auth(self.client.get(&current_url), token, account_email)
                .header("Accept", "application/json");
            if first {
                let mut merged_params = params.to_vec();
                merged_params.push(("pagelen".to_string(), per_page.to_string()));
                request = request.query(&merged_params);
                first = false;
            }

            let response = Self::send(request, "page fetch")?;
            let page: BitbucketPage<T> =
                parse_bitbucket_json(response, token, account_email, "page fetch")?;
            trace!(count = page.values.len(), "decoded Bitbucket page");
            for item in page.values {
                emit(item)?;
                emitted += 1;
            }
            next_url = page.next;
        }

        Ok(emitted)
    }
}

fn parse_bitbucket_json<T: DeserializeOwned>(
    resp: Response,
    token: Option<&str>,
    account_email: Option<&str>,
    operation: &str,
) -> Result<T> {
    let status = resp.status();
    let body = resp.text()?;
    if !status.is_success() {
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(AppError::auth(format!(
                "Bitbucket API {operation} error {status}: {}. {}",
                body_snippet(&body),
                auth_hint(token, account_email)
            ))
            .with_provider("bitbucket")
            .with_http_status(status)
            .into());
        }
        return Err(AppError::from_http("Bitbucket", operation, status, &body)
            .with_provider("bitbucket")
            .into());
    }

    serde_json::from_str(&body).map_err(|err| {
        app_error_from_decode(
            "Bitbucket",
            operation,
            format!("{err} (body starts with: {})", body_snippet(&body)),
        )
        .into()
    })
}

fn body_snippet(body: &str) -> String {
    body.chars()
        .take(200)
        .collect::<String>()
        .replace('\n', " ")
}

fn matches_issue_filters(item: &BitbucketIssueItem, filters: &BitbucketFilters) -> bool {
    if !matches_issue_state(item.state.as_str(), filters.state.as_deref()) {
        return false;
    }
    if let Some(author) = filters.author.as_deref()
        && !user_matches(item.reporter.as_ref(), author)
    {
        return false;
    }
    if let Some(since) = filters.since.as_deref()
        && let Some(created) = item.created_on.as_deref()
        && created < since
    {
        return false;
    }
    matches_terms(
        &[
            item.title.as_str(),
            item.content
                .as_ref()
                .and_then(|c| c.raw.as_deref())
                .unwrap_or(""),
        ],
        filters,
    )
}

fn matches_pr_filters(item: &BitbucketPullRequestItem, filters: &BitbucketFilters) -> bool {
    if !matches_pr_state(item.state.as_str(), filters.state.as_deref()) {
        return false;
    }
    if let Some(author) = filters.author.as_deref()
        && !user_matches(item.author.as_ref(), author)
    {
        return false;
    }
    if let Some(since) = filters.since.as_deref()
        && let Some(created) = item.created_on.as_deref()
        && created < since
    {
        return false;
    }
    matches_terms(
        &[
            item.title.as_str(),
            item.description.as_deref().unwrap_or(""),
            item.summary
                .as_ref()
                .and_then(|c| c.raw.as_deref())
                .unwrap_or(""),
        ],
        filters,
    )
}

fn matches_terms(haystack_parts: &[&str], filters: &BitbucketFilters) -> bool {
    let mut terms = filters.search_terms.clone();
    terms.extend(filters.labels.clone());
    if let Some(milestone) = filters.milestone.as_deref() {
        terms.push(milestone.to_string());
    }
    if terms.is_empty() {
        return true;
    }
    let haystack = haystack_parts.join(" ").to_ascii_lowercase();
    terms
        .iter()
        .all(|term| haystack.contains(&term.to_ascii_lowercase()))
}

fn matches_issue_state(state: &str, filter_state: Option<&str>) -> bool {
    let state = state.to_ascii_lowercase();
    let Some(filter) = filter_state.map(str::to_ascii_lowercase) else {
        return true;
    };
    match filter.as_str() {
        "open" | "opened" => !matches!(state.as_str(), "resolved" | "closed"),
        "closed" => matches!(state.as_str(), "resolved" | "closed"),
        "all" => true,
        other => state == other,
    }
}

fn matches_pr_state(state: &str, filter_state: Option<&str>) -> bool {
    let state = state.to_ascii_lowercase();
    let Some(filter) = filter_state.map(str::to_ascii_lowercase) else {
        return true;
    };
    match filter.as_str() {
        "open" | "opened" => state == "open",
        "closed" => matches!(state.as_str(), "merged" | "declined" | "superseded"),
        "merged" => state == "merged",
        "declined" => state == "declined",
        "all" => true,
        other => state == other,
    }
}

fn user_matches(user: Option<&BitbucketUser>, needle: &str) -> bool {
    let Some(user) = user else {
        return false;
    };
    let needle = needle.to_ascii_lowercase();
    user.display_name
        .as_deref()
        .map(str::to_ascii_lowercase)
        .is_some_and(|v| v == needle)
        || user
            .nickname
            .as_deref()
            .map(str::to_ascii_lowercase)
            .is_some_and(|v| v == needle)
        || user
            .username
            .as_deref()
            .map(str::to_ascii_lowercase)
            .is_some_and(|v| v == needle)
}

fn map_issue_comment(item: BitbucketCommentItem) -> Comment {
    Comment {
        author: item.user.and_then(select_author_name),
        created_at: item.created_on.unwrap_or_default(),
        body: item.content.and_then(|c| c.raw),
        kind: Some("issue_comment".to_string()),
        review_path: None,
        review_line: None,
        review_side: None,
    }
}

fn map_pr_comment(item: BitbucketCommentItem, include_review_comments: bool) -> Option<Comment> {
    let (kind, review_path, review_line, review_side) = if let Some(inline) = item.inline {
        if !include_review_comments {
            return None;
        }
        let review_line = inline.to.or(inline.from);
        let review_side = if inline.to.is_some() {
            Some("RIGHT".to_string())
        } else if inline.from.is_some() {
            Some("LEFT".to_string())
        } else {
            None
        };
        (
            "review_comment".to_string(),
            inline.path,
            review_line,
            review_side,
        )
    } else {
        ("issue_comment".to_string(), None, None, None)
    };

    Some(Comment {
        author: item.user.and_then(select_author_name),
        created_at: item.created_on.unwrap_or_default(),
        body: item.content.and_then(|c| c.raw),
        kind: Some(kind),
        review_path,
        review_line,
        review_side,
    })
}

fn select_author_name(user: BitbucketUser) -> Option<String> {
    user.nickname.or(user.username).or(user.display_name)
}

#[derive(Deserialize)]
struct BitbucketPage<T> {
    values: Vec<T>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct BitbucketIssueItem {
    id: u64,
    title: String,
    state: String,
    content: Option<BitbucketRichText>,
    reporter: Option<BitbucketUser>,
    created_on: Option<String>,
}

#[derive(Deserialize)]
struct BitbucketPullRequestItem {
    id: u64,
    title: String,
    state: String,
    description: Option<String>,
    summary: Option<BitbucketRichText>,
    author: Option<BitbucketUser>,
    created_on: Option<String>,
}

#[derive(Deserialize)]
struct BitbucketCommentItem {
    user: Option<BitbucketUser>,
    created_on: Option<String>,
    content: Option<BitbucketRichText>,
    inline: Option<BitbucketInline>,
    deleted: Option<bool>,
}

#[derive(Deserialize)]
struct BitbucketInline {
    path: Option<String>,
    from: Option<u64>,
    to: Option<u64>,
}

#[derive(Deserialize)]
struct BitbucketRichText {
    raw: Option<String>,
}

#[derive(Deserialize)]
struct BitbucketUser {
    display_name: Option<String>,
    nickname: Option<String>,
    username: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_state_filter_maps_open_and_closed() {
        assert!(matches_issue_state("new", Some("open")));
        assert!(!matches_issue_state("closed", Some("open")));
        assert!(matches_issue_state("resolved", Some("closed")));
    }

    #[test]
    fn pr_state_filter_maps_open_closed_and_merged() {
        assert!(matches_pr_state("OPEN", Some("open")));
        assert!(matches_pr_state("DECLINED", Some("closed")));
        assert!(matches_pr_state("MERGED", Some("merged")));
        assert!(!matches_pr_state("OPEN", Some("merged")));
    }

    #[test]
    fn map_pr_review_comment_sets_review_fields() {
        let item = BitbucketCommentItem {
            user: Some(BitbucketUser {
                display_name: Some("Alice".into()),
                nickname: Some("alice".into()),
                username: None,
            }),
            created_on: Some("2026-01-01T00:00:00.000000+00:00".into()),
            content: Some(BitbucketRichText {
                raw: Some("Looks good".into()),
            }),
            inline: Some(BitbucketInline {
                path: Some("src/lib.rs".into()),
                from: None,
                to: Some(42),
            }),
            deleted: Some(false),
        };
        let mapped = map_pr_comment(item, true).expect("expected comment");
        assert_eq!(mapped.kind.as_deref(), Some("review_comment"));
        assert_eq!(mapped.review_path.as_deref(), Some("src/lib.rs"));
        assert_eq!(mapped.review_line, Some(42));
        assert_eq!(mapped.review_side.as_deref(), Some("RIGHT"));
    }
}
