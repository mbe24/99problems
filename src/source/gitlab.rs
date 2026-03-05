use std::collections::HashSet;
use std::fmt::Write as _;

use anyhow::Result;
use reqwest::StatusCode;
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::Deserialize;
use tracing::{debug, trace, warn};

use super::{ContentKind, FetchRequest, FetchTarget, Source};
use crate::error::{AppError, app_error_from_decode, app_error_from_reqwest};
use crate::model::{Comment, Conversation, ConversationMetadata, IssueLink};

const GITLAB_DEFAULT_BASE_URL: &str = "https://gitlab.com";
const PAGE_SIZE: u32 = 100;

pub struct GitLabSource {
    client: Client,
    base_url: String,
}

impl GitLabSource {
    /// Create a GitLab source client.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(base_url: Option<String>) -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("99problems-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;

        let base_url = base_url
            .unwrap_or_else(|| GITLAB_DEFAULT_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string();

        Ok(Self { client, base_url })
    }

    fn apply_auth(req: RequestBuilder, token: Option<&str>) -> RequestBuilder {
        match token {
            Some(t) => req.header("PRIVATE-TOKEN", t),
            None => req,
        }
    }

    fn bounded_per_page(per_page: u32) -> u32 {
        per_page.clamp(1, PAGE_SIZE)
    }

    fn send(req: RequestBuilder, operation: &str) -> Result<Response> {
        req.send()
            .map_err(|err| app_error_from_reqwest("GitLab", operation, &err).into())
    }

    fn get_pages<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        params: &[(String, String)],
        token: Option<&str>,
        per_page: u32,
        allow_unauthenticated_empty: bool,
    ) -> Result<Vec<T>> {
        let mut results = vec![];
        self.get_pages_stream(
            url,
            params,
            token,
            per_page,
            allow_unauthenticated_empty,
            &mut |item| {
                results.push(item);
                Ok(())
            },
        )?;
        Ok(results)
    }

    #[allow(clippy::too_many_arguments)]
    fn get_pages_stream<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        params: &[(String, String)],
        token: Option<&str>,
        per_page: u32,
        allow_unauthenticated_empty: bool,
        emit: &mut dyn FnMut(T) -> Result<()>,
    ) -> Result<usize> {
        let mut emitted = 0usize;
        let mut page = 1u32;
        let per_page = Self::bounded_per_page(per_page);

        loop {
            let mut query = params.to_vec();
            query.push(("per_page".into(), per_page.to_string()));
            query.push(("page".into(), page.to_string()));
            debug!(url = %url, page, per_page, "fetching GitLab page");

            let req = Self::apply_auth(self.client.get(url).query(&query), token);
            let resp = Self::send(req, "page fetch")?;

            if !resp.status().is_success() {
                let status = resp.status();
                if allow_unauthenticated_empty
                    && token.is_none()
                    && (status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN)
                {
                    return Ok(0);
                }
                if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                    let body = resp.text()?;
                    let hint = if token.is_some() {
                        "GitLab token seems invalid or lacks required scope (use read_api)."
                    } else {
                        "No GitLab token detected. Set --token, GITLAB_TOKEN, or [gitlab].token."
                    };
                    return Err(AppError::auth(format!(
                        "GitLab API auth error {status}: {hint} {body}"
                    ))
                    .with_provider("gitlab")
                    .with_http_status(status)
                    .into());
                }
                let body = resp
                    .text()
                    .map_err(|err| app_error_from_reqwest("GitLab", "error body read", &err))?;
                return Err(AppError::from_http("GitLab", "page fetch", status, &body).into());
            }

            let next_page = resp
                .headers()
                .get("x-next-page")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .trim()
                .to_string();

            let items: Vec<T> = resp
                .json()
                .map_err(|err| app_error_from_decode("GitLab", "page fetch", err))?;
            trace!(count = items.len(), page, "decoded GitLab page");
            for item in items {
                emit(item)?;
                emitted += 1;
            }

            if next_page.is_empty() {
                break;
            }

            page = next_page.parse::<u32>().unwrap_or(page + 1);
        }

        Ok(emitted)
    }

    fn get_one<T: for<'de> Deserialize<'de>>(
        &self,
        url: &str,
        token: Option<&str>,
    ) -> Result<Option<T>> {
        let req = Self::apply_auth(self.client.get(url), token);
        let resp = Self::send(req, "single fetch")?;

        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if resp.status() == StatusCode::UNAUTHORIZED || resp.status() == StatusCode::FORBIDDEN {
            let status = resp.status();
            let hint = if token.is_some() {
                "GitLab token seems invalid or lacks required scope (use read_api)."
            } else {
                "No GitLab token detected. Set --token, GITLAB_TOKEN, or [gitlab].token."
            };
            let body = resp.text()?;
            return Err(
                AppError::auth(format!("GitLab API auth error {status}: {hint} {body}"))
                    .with_provider("gitlab")
                    .with_http_status(status)
                    .into(),
            );
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .map_err(|err| app_error_from_reqwest("GitLab", "error body read", &err))?;
            return Err(AppError::from_http("GitLab", "single fetch", status, &body).into());
        }

        Ok(Some(resp.json().map_err(|err| {
            app_error_from_decode("GitLab", "single fetch", err)
        })?))
    }

    fn fetch_notes(
        &self,
        repo: &str,
        iid: u64,
        is_pr: bool,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let project = encode_project_path(repo);
        let url = if is_pr {
            format!(
                "{}/api/v4/projects/{project}/merge_requests/{iid}/notes",
                self.base_url
            )
        } else {
            format!(
                "{}/api/v4/projects/{project}/issues/{iid}/notes",
                self.base_url
            )
        };

        let notes: Vec<GitLabNote> =
            self.get_pages(&url, &[], req.token.as_deref(), req.per_page, true)?;
        Ok(notes
            .into_iter()
            .filter(|n| !n.system)
            .map(map_note_comment)
            .collect())
    }

    fn fetch_review_comments(
        &self,
        repo: &str,
        iid: u64,
        req: &FetchRequest,
    ) -> Result<Vec<Comment>> {
        let project = encode_project_path(repo);
        let url = format!(
            "{}/api/v4/projects/{project}/merge_requests/{iid}/discussions",
            self.base_url
        );

        let discussions: Vec<GitLabDiscussion> =
            self.get_pages(&url, &[], req.token.as_deref(), req.per_page, true)?;
        let mut seen = HashSet::new();
        let mut comments = vec![];

        for discussion in discussions {
            for note in discussion.notes {
                if note.system || note.position.is_none() || !seen.insert(note.id) {
                    continue;
                }
                comments.push(map_review_comment(note));
            }
        }

        Ok(comments)
    }

    fn fetch_issue_links(&self, repo: &str, iid: u64, token: Option<&str>) -> Vec<IssueLink> {
        let project = encode_project_path(repo);
        let url = format!("{}/api/v4/projects/{project}/issues/{iid}/links", self.base_url);
        debug!(url = %url, "fetching GitLab issue links");
        let items: Vec<GitLabIssueLinkItem> = match self.get_pages(&url, &[], token, PAGE_SIZE, true) {
            Ok(v) => v,
            Err(err) => {
                warn!("GitLab issue link fetch failed for !{iid}: {err}");
                return vec![];
            }
        };
        items
            .into_iter()
            .map(|i| IssueLink {
                id: i.iid.to_string(),
                relation: i.link_type,
            })
            .collect()
    }

    fn fetch_conversation(
        &self,
        repo: &str,
        seed: ConversationSeed,
        req: &FetchRequest,
    ) -> Result<Conversation> {
        let mut comments = Vec::new();
        if req.include_comments {
            comments = self.fetch_notes(repo, seed.id, seed.is_pr, req)?;
            if seed.is_pr && req.include_review_comments {
                comments.extend(self.fetch_review_comments(repo, seed.id, req)?);
                comments.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            }
        }

        let metadata = if seed.is_pr {
            ConversationMetadata::empty()
        } else {
            let links = self.fetch_issue_links(repo, seed.id, req.token.as_deref());
            ConversationMetadata::from_links(links)
        };

        Ok(Conversation {
            id: seed.id.to_string(),
            title: seed.title,
            state: seed.state,
            body: seed.body,
            comments,
            metadata,
        })
    }

    fn search_stream(
        &self,
        req: &FetchRequest,
        raw_query: &str,
        emit: &mut dyn FnMut(Conversation) -> Result<()>,
    ) -> Result<usize> {
        let filters = parse_gitlab_query(raw_query);
        let repo = filters
            .repo
            .as_deref()
            .ok_or_else(|| {
                AppError::usage(
                    "No repo: found in query. Use --repo or include 'repo:group/project' in -q",
                )
            })?
            .to_string();
        let project = encode_project_path(&repo);
        let params = build_search_params(&filters);
        let mut emitted = 0usize;

        match filters.kind {
            ContentKind::Issue => {
                let url = format!("{}/api/v4/projects/{project}/issues", self.base_url);
                self.get_pages_stream(
                    &url,
                    &params,
                    req.token.as_deref(),
                    req.per_page,
                    false,
                    &mut |i: GitLabIssueItem| {
                        let conversation = self.fetch_conversation(
                            &repo,
                            ConversationSeed {
                                id: i.iid,
                                title: i.title,
                                state: i.state,
                                body: i.description,
                                is_pr: false,
                            },
                            req,
                        )?;
                        emit(conversation)?;
                        emitted += 1;
                        Ok(())
                    },
                )?;
            }
            ContentKind::Pr => {
                let url = format!("{}/api/v4/projects/{project}/merge_requests", self.base_url);
                self.get_pages_stream(
                    &url,
                    &params,
                    req.token.as_deref(),
                    req.per_page,
                    false,
                    &mut |mr: GitLabMergeRequestItem| {
                        let conversation = self.fetch_conversation(
                            &repo,
                            ConversationSeed {
                                id: mr.iid,
                                title: mr.title,
                                state: mr.state,
                                body: mr.description,
                                is_pr: true,
                            },
                            req,
                        )?;
                        emit(conversation)?;
                        emitted += 1;
                        Ok(())
                    },
                )?;
            }
        }
        Ok(emitted)
    }

    fn fetch_issue_by_iid(
        &self,
        repo: &str,
        iid: u64,
        token: Option<&str>,
    ) -> Result<Option<GitLabIssueItem>> {
        let project = encode_project_path(repo);
        let url = format!("{}/api/v4/projects/{project}/issues/{iid}", self.base_url);
        self.get_one(&url, token)
    }

    fn fetch_mr_by_iid(
        &self,
        repo: &str,
        iid: u64,
        token: Option<&str>,
    ) -> Result<Option<GitLabMergeRequestItem>> {
        let project = encode_project_path(repo);
        let url = format!(
            "{}/api/v4/projects/{project}/merge_requests/{iid}",
            self.base_url
        );
        self.get_one(&url, token)
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
        let iid = id.parse::<u64>().map_err(|_| {
            AppError::usage(format!("GitLab expects a numeric issue/MR id, got '{id}'."))
        })?;
        match kind {
            ContentKind::Issue => {
                if let Some(issue) = self.fetch_issue_by_iid(repo, iid, req.token.as_deref())? {
                    let conversation = self.fetch_conversation(
                        repo,
                        ConversationSeed {
                            id: issue.iid,
                            title: issue.title,
                            state: issue.state,
                            body: issue.description,
                            is_pr: false,
                        },
                        req,
                    )?;
                    emit(conversation)?;
                    return Ok(1);
                }

                if allow_fallback_to_pr
                    && let Some(mr) = self.fetch_mr_by_iid(repo, iid, req.token.as_deref())?
                {
                    warn!(
                        "Warning: --id defaulted to issue, but found MR !{iid}; use --type pr for clarity."
                    );
                    let conversation = self.fetch_conversation(
                        repo,
                        ConversationSeed {
                            id: mr.iid,
                            title: mr.title,
                            state: mr.state,
                            body: mr.description,
                            is_pr: true,
                        },
                        req,
                    )?;
                    emit(conversation)?;
                    return Ok(1);
                }

                Err(AppError::not_found(format!("Issue #{iid} not found in repo {repo}.")).into())
            }
            ContentKind::Pr => {
                if let Some(mr) = self.fetch_mr_by_iid(repo, iid, req.token.as_deref())? {
                    let conversation = self.fetch_conversation(
                        repo,
                        ConversationSeed {
                            id: mr.iid,
                            title: mr.title,
                            state: mr.state,
                            body: mr.description,
                            is_pr: true,
                        },
                        req,
                    )?;
                    emit(conversation)?;
                    return Ok(1);
                }

                if self
                    .fetch_issue_by_iid(repo, iid, req.token.as_deref())?
                    .is_some()
                {
                    return Err(AppError::usage(format!(
                        "ID {iid} in repo {repo} is an issue, not a merge request."
                    ))
                    .into());
                }

                Err(
                    AppError::not_found(format!("Merge request !{iid} not found in repo {repo}."))
                        .into(),
                )
            }
        }
    }
}

impl Source for GitLabSource {
    fn fetch_stream(
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
}

#[derive(Deserialize)]
struct GitLabIssueItem {
    iid: u64,
    title: String,
    state: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct GitLabMergeRequestItem {
    iid: u64,
    title: String,
    state: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct GitLabNote {
    author: Option<GitLabAuthor>,
    created_at: String,
    body: String,
    system: bool,
}

#[derive(Deserialize)]
struct GitLabAuthor {
    username: String,
}

#[derive(Deserialize)]
struct GitLabDiscussion {
    notes: Vec<GitLabDiscussionNote>,
}

#[derive(Deserialize)]
struct GitLabDiscussionNote {
    id: u64,
    author: Option<GitLabAuthor>,
    created_at: String,
    body: String,
    system: bool,
    position: Option<GitLabPosition>,
}

#[derive(Deserialize)]
struct GitLabPosition {
    new_path: Option<String>,
    old_path: Option<String>,
    new_line: Option<u64>,
    old_line: Option<u64>,
}

#[derive(Deserialize)]
struct GitLabIssueLinkItem {
    iid: u64,
    link_type: String,
}

struct ConversationSeed {
    id: u64,
    title: String,
    state: String,
    body: Option<String>,
    is_pr: bool,
}

fn map_note_comment(note: GitLabNote) -> Comment {
    Comment {
        author: note.author.map(|a| a.username),
        created_at: note.created_at,
        body: Some(note.body),
        kind: Some("issue_comment".into()),
        review_path: None,
        review_line: None,
        review_side: None,
    }
}

fn map_review_comment(note: GitLabDiscussionNote) -> Comment {
    let position = note.position;
    let review_path = position
        .as_ref()
        .and_then(|p| p.new_path.clone().or_else(|| p.old_path.clone()));
    let review_line = position.as_ref().and_then(|p| p.new_line.or(p.old_line));
    let review_side = position.as_ref().and_then(|p| {
        if p.new_line.is_some() {
            Some("RIGHT".to_string())
        } else if p.old_line.is_some() {
            Some("LEFT".to_string())
        } else {
            None
        }
    });

    Comment {
        author: note.author.map(|a| a.username),
        created_at: note.created_at,
        body: Some(note.body),
        kind: Some("review_comment".into()),
        review_path,
        review_line,
        review_side,
    }
}

#[derive(Debug)]
struct GitLabFilters {
    repo: Option<String>,
    kind: ContentKind,
    state: Option<String>,
    labels: Vec<String>,
    author: Option<String>,
    since: Option<String>,
    milestone: Option<String>,
    search_terms: Vec<String>,
}

impl Default for GitLabFilters {
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

fn parse_gitlab_query(raw_query: &str) -> GitLabFilters {
    let mut filters = GitLabFilters::default();

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
            if kind == "pr" {
                filters.kind = ContentKind::Pr;
                continue;
            }
            if kind == "issue" {
                filters.kind = ContentKind::Issue;
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

fn build_search_params(filters: &GitLabFilters) -> Vec<(String, String)> {
    let mut params = Vec::new();

    if let Some(state) = normalize_state(filters.kind, filters.state.as_deref()) {
        params.push(("state".into(), state.to_string()));
    }
    if !filters.labels.is_empty() {
        params.push(("labels".into(), filters.labels.join(",")));
    }
    if let Some(author) = &filters.author {
        params.push(("author_username".into(), author.clone()));
    }
    if let Some(since) = &filters.since {
        params.push(("created_after".into(), since.clone()));
    }
    if let Some(milestone) = &filters.milestone {
        params.push(("milestone".into(), milestone.clone()));
    }
    if !filters.search_terms.is_empty() {
        params.push(("search".into(), filters.search_terms.join(" ")));
    }

    params
}

fn normalize_state(kind: ContentKind, state: Option<&str>) -> Option<&'static str> {
    let s = state?.to_ascii_lowercase();
    match kind {
        ContentKind::Issue => match s.as_str() {
            "open" | "opened" => Some("opened"),
            "closed" => Some("closed"),
            "all" => Some("all"),
            _ => None,
        },
        ContentKind::Pr => match s.as_str() {
            "open" | "opened" => Some("opened"),
            "closed" => Some("closed"),
            "merged" => Some("merged"),
            "locked" => Some("locked"),
            "all" => Some("all"),
            _ => None,
        },
    }
}

fn encode_project_path(path: &str) -> String {
    let mut out = String::new();
    for b in path.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(*b, b'-' | b'.' | b'_' | b'~') {
            out.push(*b as char);
        } else {
            write!(out, "%{b:02X}").expect("writing to String should never fail");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gitlab_query_extracts_filters() {
        let q = parse_gitlab_query(
            "is:pr repo:group/project state:closed label:bug author:alice created:>=2024-01-01 milestone:v1 text",
        );
        assert!(matches!(q.kind, ContentKind::Pr));
        assert_eq!(q.repo.as_deref(), Some("group/project"));
        assert_eq!(q.state.as_deref(), Some("closed"));
        assert_eq!(q.labels, vec!["bug"]);
        assert_eq!(q.author.as_deref(), Some("alice"));
        assert_eq!(q.since.as_deref(), Some("2024-01-01"));
        assert_eq!(q.milestone.as_deref(), Some("v1"));
        assert_eq!(q.search_terms, vec!["text"]);
    }

    #[test]
    fn normalize_state_maps_open_to_opened() {
        assert_eq!(
            normalize_state(ContentKind::Issue, Some("open")),
            Some("opened")
        );
        assert_eq!(
            normalize_state(ContentKind::Pr, Some("open")),
            Some("opened")
        );
    }

    #[test]
    fn encode_project_path_encodes_slash() {
        assert_eq!(encode_project_path("group/project"), "group%2Fproject");
    }
}
