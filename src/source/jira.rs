use anyhow::{Result, anyhow};
use reqwest::StatusCode;
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::header::CONTENT_TYPE;
use serde::Deserialize;
use serde_json::Value;

use super::{ContentKind, FetchRequest, FetchTarget, Source};
use crate::model::{Comment, Conversation};

const JIRA_DEFAULT_BASE_URL: &str = "https://jira.atlassian.com";
const PAGE_SIZE: u32 = 100;

pub struct JiraSource {
    client: Client,
    base_url: String,
}

impl JiraSource {
    pub fn new(base_url: Option<String>) -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("99problems-cli/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let base_url = base_url
            .unwrap_or_else(|| JIRA_DEFAULT_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        Ok(Self { client, base_url })
    }

    fn apply_auth(
        req: RequestBuilder,
        token: &Option<String>,
        jira_email: &Option<String>,
    ) -> RequestBuilder {
        let req = req.header("Accept", "application/json");
        match token.as_ref() {
            Some(t) if t.contains(':') => {
                let (user, api_token) = t.split_once(':').unwrap_or_default();
                req.basic_auth(user, Some(api_token))
            }
            Some(t) => match jira_email.as_ref() {
                Some(email) => req.basic_auth(email, Some(t)),
                None => req.bearer_auth(t),
            },
            None => req,
        }
    }

    fn bounded_per_page(per_page: u32) -> u32 {
        per_page.clamp(1, PAGE_SIZE)
    }

    fn fetch_issue(&self, id_or_key: &str, req: &FetchRequest) -> Result<Conversation> {
        let fields = "summary,description,status";
        let url = format!("{}/rest/api/3/issue/{}", self.base_url, id_or_key);
        let http = Self::apply_auth(self.client.get(&url), &req.token, &req.jira_email)
            .query(&[("fields", fields)]);
        let resp = http.send()?;
        if resp.status() == StatusCode::NOT_FOUND {
            let body = resp.text().unwrap_or_default();
            let auth_hint = if req.token.is_some() {
                if req.jira_email.is_none() {
                    " Check Jira permissions or configure Jira email for API-token auth."
                } else {
                    " Check Jira permissions for this issue."
                }
            } else {
                " Jira often returns 404 for unauthorized issues. Set --token, JIRA_TOKEN, or [jira].token."
            };
            return Err(anyhow!(
                "Jira issue '{}' was not found or is not accessible.{} Response: {}",
                id_or_key,
                auth_hint,
                body_snippet(&body)
            ));
        }
        let issue: JiraIssueItem =
            parse_jira_json(resp, &req.token, &req.jira_email, "issue fetch")?;
        let comments = if req.include_comments {
            self.fetch_comments(&issue.key, req)?
        } else {
            vec![]
        };
        Ok(Conversation {
            id: issue.key,
            title: issue.fields.summary,
            state: issue.fields.status.name,
            body: issue
                .fields
                .description
                .as_ref()
                .map(extract_adf_text)
                .filter(|s| !s.is_empty()),
            comments,
        })
    }

    fn fetch_comments(&self, issue_key: &str, req: &FetchRequest) -> Result<Vec<Comment>> {
        let mut start_at = 0u32;
        let per_page = Self::bounded_per_page(req.per_page);
        let mut out = vec![];

        loop {
            let url = format!("{}/rest/api/3/issue/{issue_key}/comment", self.base_url);
            let http =
                Self::apply_auth(self.client.get(&url), &req.token, &req.jira_email).query(&[
                    ("startAt", start_at.to_string()),
                    ("maxResults", per_page.to_string()),
                ]);
            let resp = http.send()?;
            let page: JiraCommentsPage =
                parse_jira_json(resp, &req.token, &req.jira_email, "comment fetch")?;

            for c in page.comments {
                let body = extract_adf_text(&c.body);
                out.push(Comment {
                    author: c.author.map(|a| a.display_name),
                    created_at: c.created,
                    body: if body.is_empty() { None } else { Some(body) },
                    kind: Some("issue_comment".into()),
                    review_path: None,
                    review_line: None,
                    review_side: None,
                });
            }

            let next = page.start_at + page.max_results;
            if next >= page.total {
                break;
            }
            start_at = next;
        }

        Ok(out)
    }

    fn search(&self, req: &FetchRequest, raw_query: &str) -> Result<Vec<Conversation>> {
        let filters = parse_jira_query(raw_query);
        if matches!(filters.kind, ContentKind::Pr) {
            return Err(anyhow!(
                "Platform 'jira' does not support pull requests. Use --type issue."
            ));
        }
        let jql = build_jql(&filters)?;
        let per_page = Self::bounded_per_page(req.per_page);
        let mut start_at = 0u32;
        let mut next_page_token: Option<String> = None;
        let mut results = vec![];

        loop {
            let url = format!("{}/rest/api/3/search/jql", self.base_url);
            let mut query_params: Vec<(String, String)> = vec![
                ("jql".into(), jql.clone()),
                ("maxResults".into(), per_page.to_string()),
                ("fields".into(), "summary,description,status".into()),
            ];
            if let Some(token) = &next_page_token {
                query_params.push(("nextPageToken".into(), token.clone()));
            } else {
                query_params.push(("startAt".into(), start_at.to_string()));
            }

            let http = Self::apply_auth(self.client.get(&url), &req.token, &req.jira_email)
                .query(&query_params);
            let resp = http.send()?;
            let page: JiraSearchResponse =
                parse_jira_json(resp, &req.token, &req.jira_email, "search")?;
            for issue in page.issues {
                let comments = if req.include_comments {
                    self.fetch_comments(&issue.key, req)?
                } else {
                    vec![]
                };
                results.push(Conversation {
                    id: issue.key,
                    title: issue.fields.summary,
                    state: issue.fields.status.name,
                    body: issue
                        .fields
                        .description
                        .as_ref()
                        .map(extract_adf_text)
                        .filter(|s| !s.is_empty()),
                    comments,
                });
            }

            if let Some(token) = page.next_page_token {
                next_page_token = Some(token);
                continue;
            }
            if let (Some(s), Some(m), Some(t)) = (page.start_at, page.max_results, page.total) {
                let next = s + m;
                if next < t {
                    start_at = next;
                    next_page_token = None;
                    continue;
                }
                break;
            }
            if page.is_last == Some(false) {
                return Err(anyhow!(
                    "Jira API search response indicated more pages but provided no pagination cursor."
                ));
            }
            break;
        }

        Ok(results)
    }
}

impl Source for JiraSource {
    fn fetch(&self, req: &FetchRequest) -> Result<Vec<Conversation>> {
        match &req.target {
            FetchTarget::Search { raw_query } => self.search(req, raw_query),
            FetchTarget::Id { id, kind, .. } => {
                if matches!(kind, ContentKind::Pr) {
                    return Err(anyhow!(
                        "Platform 'jira' does not support pull requests. Use --type issue."
                    ));
                }
                Ok(vec![self.fetch_issue(id, req)?])
            }
        }
    }
}

#[derive(Deserialize)]
struct JiraSearchResponse {
    #[serde(rename = "startAt")]
    start_at: Option<u32>,
    #[serde(rename = "maxResults")]
    max_results: Option<u32>,
    total: Option<u32>,
    #[serde(rename = "isLast")]
    is_last: Option<bool>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
    issues: Vec<JiraIssueItem>,
}

#[derive(Deserialize)]
struct JiraIssueItem {
    key: String,
    fields: JiraIssueFields,
}

#[derive(Deserialize)]
struct JiraIssueFields {
    summary: String,
    description: Option<Value>,
    status: JiraStatus,
}

#[derive(Deserialize)]
struct JiraStatus {
    name: String,
}

#[derive(Deserialize)]
struct JiraCommentsPage {
    #[serde(rename = "startAt")]
    start_at: u32,
    #[serde(rename = "maxResults")]
    max_results: u32,
    total: u32,
    comments: Vec<JiraCommentItem>,
}

#[derive(Deserialize)]
struct JiraCommentItem {
    author: Option<JiraAuthor>,
    created: String,
    body: Value,
}

#[derive(Deserialize)]
struct JiraAuthor {
    #[serde(rename = "displayName")]
    display_name: String,
}

#[derive(Debug)]
struct JiraFilters {
    repo: Option<String>,
    kind: ContentKind,
    state: Option<String>,
    labels: Vec<String>,
    author: Option<String>,
    since: Option<String>,
    milestone: Option<String>,
    search_terms: Vec<String>,
}

impl Default for JiraFilters {
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

fn parse_jira_query(raw_query: &str) -> JiraFilters {
    let mut filters = JiraFilters::default();

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
            if kind == "issue" {
                filters.kind = ContentKind::Issue;
                continue;
            }
            if kind == "pr" {
                filters.kind = ContentKind::Pr;
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

fn build_jql(filters: &JiraFilters) -> Result<String> {
    let project = filters
        .repo
        .as_deref()
        .ok_or_else(|| anyhow!("No repo: found in query. Use --repo with Jira project key."))?;

    let mut clauses = vec![format!("project = {}", quote_jql(project))];
    if let Some(state) = filters.state.as_deref() {
        match state.to_ascii_lowercase().as_str() {
            "open" | "opened" => clauses.push("statusCategory != Done".into()),
            "closed" => clauses.push("statusCategory = Done".into()),
            "all" => {}
            _ => clauses.push(format!("status = {}", quote_jql(state))),
        }
    }

    for label in &filters.labels {
        clauses.push(format!("labels = {}", quote_jql(label)));
    }
    if let Some(author) = &filters.author {
        clauses.push(format!("reporter = {}", quote_jql(author)));
    }
    if let Some(since) = &filters.since {
        clauses.push(format!("created >= {}", quote_jql(since)));
    }
    if let Some(milestone) = &filters.milestone {
        clauses.push(format!("fixVersion = {}", quote_jql(milestone)));
    }
    if !filters.search_terms.is_empty() {
        clauses.push(format!(
            "text ~ {}",
            quote_jql(&filters.search_terms.join(" "))
        ));
    }

    Ok(clauses.join(" AND "))
}

fn quote_jql(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\\\""))
}

fn parse_jira_json<T: for<'de> Deserialize<'de>>(
    resp: Response,
    token: &Option<String>,
    jira_email: &Option<String>,
    operation: &str,
) -> Result<T> {
    let status = resp.status();
    let content_type = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = resp.text()?;

    if !status.is_success() {
        let auth_hint = if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            if token.is_some() {
                if jira_email.is_some() {
                    " Jira auth failed. Check token format/scope (email:api_token for Atlassian Cloud)."
                } else {
                    " Jira auth failed. If this is an Atlassian API token, also set Jira email (--jira-email, JIRA_EMAIL, or [jira].email), or pass --token as email:api_token."
                }
            } else {
                " No Jira token detected. Set --token, JIRA_TOKEN, or [jira].token."
            }
        } else {
            ""
        };
        return Err(anyhow!(
            "Jira API {} error {}: {}{}",
            operation,
            status,
            body_snippet(&body),
            auth_hint
        ));
    }

    if !content_type.contains("application/json") {
        return Err(anyhow!(
            "Jira API {} returned non-JSON content-type '{}' (body starts with: {}). This often means an auth/login page.",
            operation,
            content_type,
            body_snippet(&body)
        ));
    }

    serde_json::from_str(&body).map_err(|e| {
        anyhow!(
            "Jira API {} response JSON decode failed: {} (body starts with: {})",
            operation,
            e,
            body_snippet(&body)
        )
    })
}

fn body_snippet(body: &str) -> String {
    body.chars()
        .take(200)
        .collect::<String>()
        .replace('\n', " ")
}

fn extract_adf_text(value: &Value) -> String {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(map) => {
                if let Some(Value::String(text)) = map.get("text") {
                    out.push(text.to_string());
                }
                if let Some(content) = map.get("content") {
                    walk(content, out);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, out);
                }
            }
            _ => {}
        }
    }

    let mut chunks = Vec::new();
    walk(value, &mut chunks);
    chunks.join(" ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_jira_query_extracts_filters() {
        let q = parse_jira_query(
            "repo:CLOUD state:closed label:api author:alice created:>=2024-01-01 milestone:v1 text",
        );
        assert_eq!(q.repo.as_deref(), Some("CLOUD"));
        assert!(matches!(q.kind, ContentKind::Issue));
        assert_eq!(q.state.as_deref(), Some("closed"));
        assert_eq!(q.labels, vec!["api"]);
        assert_eq!(q.author.as_deref(), Some("alice"));
        assert_eq!(q.since.as_deref(), Some("2024-01-01"));
        assert_eq!(q.milestone.as_deref(), Some("v1"));
        assert_eq!(q.search_terms, vec!["text"]);
    }

    #[test]
    fn build_jql_maps_closed_to_done_category() {
        let q = parse_jira_query("repo:CLOUD state:closed");
        let jql = build_jql(&q).unwrap();
        assert!(jql.contains("project = \"CLOUD\""));
        assert!(jql.contains("statusCategory = Done"));
    }

    #[test]
    fn extract_adf_text_reads_nested_nodes() {
        let value: Value = serde_json::json!({
            "type": "doc",
            "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "Hello"}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "world"}]}
            ]
        });
        assert_eq!(extract_adf_text(&value), "Hello world");
    }
}
