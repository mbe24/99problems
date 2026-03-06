use anyhow::Result;
use clap::{Args, ValueEnum};
use std::io::{IsTerminal, Write};
use tracing::{debug, info, warn};

use crate::config::{Config, ResolveOptions, token_env_var};
use crate::error::AppError;
use crate::format::{
    StreamFormatter, json::JsonStreamFormatter, jsonl::JsonLinesFormatter, text::TextFormatter,
    yaml::YamlStreamFormatter,
};
use crate::source::{
    ContentKind, FetchRequest, FetchTarget, Query, Source, bitbucket::BitbucketSource,
    github::GitHubSource, gitlab::GitLabSource, jira::JiraSource,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum OutputFormat {
    Json,
    Yaml,
    Jsonl,
    Ndjson,
    Text,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub(crate) enum OutputMode {
    Auto,
    Batch,
    Stream,
}

#[derive(Debug, Clone, ValueEnum, PartialEq)]
pub(crate) enum Platform {
    Github,
    Gitlab,
    Jira,
    Bitbucket,
}

impl Platform {
    pub(crate) fn as_str(&self) -> &str {
        match self {
            Platform::Github => "github",
            Platform::Gitlab => "gitlab",
            Platform::Jira => "jira",
            Platform::Bitbucket => "bitbucket",
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
pub(crate) enum ContentType {
    Issue,
    Pr,
}

impl ContentType {
    fn as_str(&self) -> &str {
        match self {
            ContentType::Issue => "issue",
            ContentType::Pr => "pr",
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
pub(crate) enum DeploymentType {
    Cloud,
    Selfhosted,
}

impl DeploymentType {
    fn as_str(&self) -> &str {
        match self {
            DeploymentType::Cloud => "cloud",
            DeploymentType::Selfhosted => "selfhosted",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ResolvedOutputMode {
    Batch,
    Stream,
}

#[derive(Debug, Clone, Copy)]
enum ResolvedOutputFormat {
    Json,
    Yaml,
    Jsonl,
    Text,
}

#[derive(Debug, Clone, Copy)]
struct OutputPlan {
    mode: ResolvedOutputMode,
    format: ResolvedOutputFormat,
}

#[derive(Args, Debug)]
#[allow(clippy::struct_excessive_bools)]
#[command(
    next_line_help = true,
    after_help = "Examples:\n  99problems get --repo schemaorg/schemaorg --id 1842\n  99problems get --repo github/gitignore --id 2402 --type pr --include-review-comments\n  99problems get -q \"repo:owner/repo state:open label:bug\" --output-mode stream --format jsonl"
)]
pub(crate) struct GetArgs {
    /// Full search query (same syntax as the platform's web UI search bar)
    /// e.g. "state:closed Event repo:owner/repo"
    #[arg(short = 'q', long)]
    pub(crate) query: Option<String>,

    /// Shorthand for adding "repo:owner/repo" to the query (alias: --project)
    #[arg(short = 'r', long, visible_alias = "project")]
    pub(crate) repo: Option<String>,

    /// Shorthand for adding "state:open|closed" to the query
    #[arg(short = 's', long)]
    pub(crate) state: Option<String>,

    /// Shorthand for comma-separated labels, e.g. "bug,enhancement"
    #[arg(short = 'l', long)]
    pub(crate) labels: Option<String>,

    /// Filter by issue/PR author
    #[arg(short = 'a', long)]
    pub(crate) author: Option<String>,

    /// Only include items created on or after this date (YYYY-MM-DD), e.g. "2024-01-01"
    #[arg(short = 'S', long)]
    pub(crate) since: Option<String>,

    /// Filter by milestone title or number
    #[arg(short = 'm', long)]
    pub(crate) milestone: Option<String>,

    /// Fetch a single issue/PR by identifier (bypasses search)
    #[arg(short = 'i', long = "id", visible_alias = "issue")]
    pub(crate) id: Option<String>,

    /// Platform adapter to fetch from (used directly in CLI-only mode)
    #[arg(short = 'p', long, value_enum)]
    pub(crate) platform: Option<Platform>,

    /// Named instance alias from .99problems ([instances.<alias>])
    #[arg(short = 'I', long)]
    pub(crate) instance: Option<String>,

    /// Override platform base URL for one-off runs
    #[arg(short = 'u', long)]
    pub(crate) url: Option<String>,

    /// Bitbucket deployment type (required for --platform bitbucket)
    #[arg(long, value_enum)]
    pub(crate) deployment: Option<DeploymentType>,

    /// Content type to fetch (Bitbucket supports pull requests only; omitted type defaults to pr)
    #[arg(short = 't', long = "type", value_enum)]
    pub(crate) kind: Option<ContentType>,

    /// Output format (default: text for TTY, jsonl for piped/file output)
    #[arg(short = 'f', long, value_enum)]
    pub(crate) format: Option<OutputFormat>,

    /// Output behavior mode
    #[arg(long, value_enum)]
    pub(crate) output_mode: Option<OutputMode>,

    /// Shorthand for --output-mode stream
    #[arg(long, conflicts_with = "output_mode")]
    pub(crate) stream: bool,

    /// Include pull request review comments (inline code comments)
    #[arg(short = 'R', long)]
    pub(crate) include_review_comments: bool,

    /// Skip fetching comments (faster, smaller output)
    #[arg(long)]
    pub(crate) no_comments: bool,

    /// Skip fetching related links metadata (faster, smaller output)
    #[arg(long)]
    pub(crate) no_links: bool,

    /// Write output to a file (default: stdout)
    #[arg(short = 'o', long)]
    pub(crate) output: Option<String>,

    /// Personal access token (overrides env var and dotfile)
    #[arg(short = 'k', long)]
    pub(crate) token: Option<String>,

    /// Account email used with Jira API-token basic auth
    #[arg(long)]
    pub(crate) account_email: Option<String>,
}

/// Run the `get` command.
///
/// # Errors
///
/// Returns an error if config resolution, request building, remote fetching,
/// or output writing fails.
pub(crate) fn run(args: &GetArgs) -> Result<()> {
    let cfg = load_config_for_get(args)?;
    emit_get_warnings(&cfg, args)?;

    let source = build_source_for_platform(&cfg)?;
    let req = build_fetch_request(&cfg, args)?;
    let output_plan = resolve_output_plan(args);
    debug!(
        platform = %cfg.platform,
        kind = %cfg.kind,
        include_comments = !args.no_comments,
        include_review_comments = args.include_review_comments,
        include_links = !args.no_links,
        output_mode = ?output_plan.mode,
        output_format = ?output_plan.format,
        "resolved get configuration"
    );

    match output_plan.mode {
        ResolvedOutputMode::Batch => write_batch_output(
            source.as_ref(),
            &req,
            output_plan.format,
            args.output.as_deref(),
        ),
        ResolvedOutputMode::Stream => write_stream_output(
            source.as_ref(),
            &req,
            output_plan.format,
            args.output.as_deref(),
        ),
    }
}

fn load_config_for_get(args: &GetArgs) -> Result<Config> {
    if args.platform.is_none()
        && args.instance.is_none()
        && args.url.is_none()
        && args.deployment.is_none()
        && args.kind.is_none()
        && args.token.is_none()
        && args.account_email.is_none()
        && args.repo.is_none()
        && args.state.is_none()
    {
        return Config::load()
            .map_err(|err| AppError::usage(format!("Config error: {err}")).into());
    }

    Config::load_with_options(ResolveOptions {
        platform: args.platform.as_ref().map(Platform::as_str),
        instance: args.instance.as_deref(),
        url: args.url.as_deref(),
        kind: args.kind.as_ref().map(ContentType::as_str),
        deployment: args.deployment.as_ref().map(DeploymentType::as_str),
        token: args.token.as_deref(),
        account_email: args.account_email.as_deref(),
        repo: args.repo.as_deref(),
        state: args.state.as_deref(),
    })
    .map_err(|err| AppError::usage(format!("Config error: {err}")).into())
}

fn emit_get_warnings(cfg: &Config, args: &GetArgs) -> Result<()> {
    if cfg.token.is_none() {
        let env_var = token_env_var(&cfg.platform);
        warn!(
            "Warning: no token detected for {}. You may be subject to API rate limiting. Set --token, {}, or configure it in .99problems.",
            cfg.platform, env_var
        );
    }
    if cfg.platform == "jira"
        && let Some(token) = cfg.token.as_deref()
        && looks_like_atlassian_api_token(token)
        && cfg.account_email.is_none()
    {
        warn!(
            "Warning: Jira token looks like an Atlassian API token. Configure --account-email, JIRA_ACCOUNT_EMAIL, or [instances.<alias>].account_email, or provide --token as email:api_token."
        );
    }
    if args.no_comments && args.include_review_comments {
        warn!("Warning: --include-review-comments is ignored when --no-comments is set.");
    }
    if cfg.platform == "jira" && cfg.kind == "pr" {
        return Err(AppError::usage(
            "Platform 'jira' does not support pull requests. Use --type issue.",
        )
        .into());
    }
    if cfg.platform == "bitbucket" && cfg.kind == "issue" && cfg.kind_explicit {
        return Err(AppError::usage(
            "Platform 'bitbucket' supports pull requests only. Use --type pr or omit --type.",
        )
        .into());
    }
    Ok(())
}

fn build_source_for_platform(cfg: &Config) -> Result<Box<dyn Source>> {
    match cfg.platform.as_str() {
        "github" => Ok(Box::new(GitHubSource::new()?)),
        "gitlab" => Ok(Box::new(GitLabSource::new(cfg.platform_url.clone())?)),
        "jira" => Ok(Box::new(JiraSource::new(cfg.platform_url.clone())?)),
        "bitbucket" => Ok(Box::new(BitbucketSource::new(
            cfg.platform_url.clone(),
            cfg.deployment.clone(),
        )?)),
        other => Err(AppError::usage(format!("Platform '{other}' is not yet supported")).into()),
    }
}

fn build_fetch_request(cfg: &Config, args: &GetArgs) -> Result<FetchRequest> {
    let repo = cfg.repo.clone();
    let state = cfg.state.clone();
    let is_bitbucket = cfg.platform == "bitbucket";
    let effective_kind = if is_bitbucket && cfg.kind == "issue" && !cfg.kind_explicit {
        "pr"
    } else {
        cfg.kind.as_str()
    };

    if let Some(id) = &args.id {
        let ignored_flags = ignored_flags_in_id_mode(args);
        if !ignored_flags.is_empty() {
            warn!(
                "Warning: when using --id/--issue, these flags are ignored: {}",
                ignored_flags.join(", ")
            );
        }

        let id_kind = if effective_kind == "pr" {
            ContentKind::Pr
        } else {
            ContentKind::Issue
        };

        let repo_for_id = if cfg.platform == "jira" {
            repo.unwrap_or_default()
        } else {
            repo.ok_or_else(|| AppError::usage("--repo is required when using --id/--issue"))?
        };

        return Ok(FetchRequest {
            target: FetchTarget::Id {
                repo: repo_for_id,
                id: id.clone(),
                kind: id_kind,
                allow_fallback_to_pr: if is_bitbucket {
                    false
                } else {
                    !cfg.kind_explicit && matches!(id_kind, ContentKind::Issue)
                },
            },
            per_page: cfg.per_page,
            token: cfg.token.clone(),
            account_email: cfg.account_email.clone(),
            include_comments: !args.no_comments,
            include_review_comments: args.include_review_comments,
            include_links: !args.no_links,
        });
    }

    let query = Query::build(
        args.query.clone(),
        effective_kind,
        repo,
        state,
        args.labels.clone(),
        args.author.clone(),
        args.since.clone(),
        args.milestone.clone(),
        cfg.per_page,
        cfg.token.clone(),
    );
    if query.raw.trim().is_empty() {
        return Err(AppError::usage(
            "No query specified. Use -q or provide --repo/--state/--labels.",
        )
        .into());
    }

    Ok(FetchRequest {
        target: FetchTarget::Search {
            raw_query: query.raw,
        },
        per_page: query.per_page,
        token: query.token,
        account_email: cfg.account_email.clone(),
        include_comments: !args.no_comments,
        include_review_comments: args.include_review_comments,
        include_links: !args.no_links,
    })
}

fn ignored_flags_in_id_mode(args: &GetArgs) -> Vec<&'static str> {
    let mut ignored_flags = Vec::new();
    if args.query.is_some() {
        ignored_flags.push("--query");
    }
    if args.state.is_some() {
        ignored_flags.push("--state");
    }
    if args.labels.is_some() {
        ignored_flags.push("--labels");
    }
    if args.author.is_some() {
        ignored_flags.push("--author");
    }
    if args.since.is_some() {
        ignored_flags.push("--since");
    }
    if args.milestone.is_some() {
        ignored_flags.push("--milestone");
    }
    ignored_flags
}

fn resolve_output_plan(args: &GetArgs) -> OutputPlan {
    let stdout_is_tty = args.output.is_none() && std::io::stdout().is_terminal();
    resolve_output_plan_with_tty(args, stdout_is_tty)
}

fn resolve_output_plan_with_tty(args: &GetArgs, stdout_is_tty: bool) -> OutputPlan {
    let mode = if args.stream {
        OutputMode::Stream
    } else {
        args.output_mode.unwrap_or(OutputMode::Auto)
    };
    let resolved_mode = match mode {
        OutputMode::Batch => ResolvedOutputMode::Batch,
        OutputMode::Auto | OutputMode::Stream => ResolvedOutputMode::Stream,
    };

    let selected_format = args.format.unwrap_or({
        if stdout_is_tty {
            OutputFormat::Text
        } else {
            OutputFormat::Jsonl
        }
    });
    let resolved_format = match selected_format {
        OutputFormat::Json => ResolvedOutputFormat::Json,
        OutputFormat::Yaml => ResolvedOutputFormat::Yaml,
        OutputFormat::Jsonl | OutputFormat::Ndjson => ResolvedOutputFormat::Jsonl,
        OutputFormat::Text => ResolvedOutputFormat::Text,
    };

    OutputPlan {
        mode: resolved_mode,
        format: resolved_format,
    }
}

fn build_formatter(format: ResolvedOutputFormat) -> Box<dyn StreamFormatter> {
    match format {
        ResolvedOutputFormat::Json => Box::new(JsonStreamFormatter::new()),
        ResolvedOutputFormat::Yaml => Box::new(YamlStreamFormatter::new()),
        ResolvedOutputFormat::Jsonl => Box::new(JsonLinesFormatter),
        ResolvedOutputFormat::Text => Box::new(TextFormatter::new()),
    }
}

fn write_batch_output(
    source: &dyn Source,
    req: &FetchRequest,
    format: ResolvedOutputFormat,
    output_path: Option<&str>,
) -> Result<()> {
    let conversations = source.fetch(req)?;
    let mut formatter = build_formatter(format);
    let mut rendered = Vec::new();
    formatter.begin(&mut rendered)?;
    for conversation in &conversations {
        formatter.write_item(&mut rendered, conversation)?;
    }
    formatter.finish(&mut rendered)?;

    if let Some(path) = output_path {
        let mut file = std::fs::File::create(path)?;
        file.write_all(&rendered)?;
        info!(count = conversations.len(), path = %path, "wrote conversations to file");
    } else {
        let mut out = std::io::stdout();
        out.write_all(&rendered)?;
        out.flush()?;
        info!(count = conversations.len(), "wrote conversations to stdout");
    }

    Ok(())
}

fn write_stream_output(
    source: &dyn Source,
    req: &FetchRequest,
    format: ResolvedOutputFormat,
    output_path: Option<&str>,
) -> Result<()> {
    let mut formatter = build_formatter(format);
    let mut writer: Box<dyn Write> = match output_path {
        Some(path) => Box::new(std::fs::File::create(path)?),
        None => Box::new(std::io::stdout()),
    };
    formatter.begin(&mut writer)?;

    let mut emitted = 0usize;
    let fetch_result = source.fetch_stream(req, &mut |conversation| {
        formatter.write_item(&mut writer, &conversation)?;
        emitted += 1;
        Ok(())
    });
    if let Err(err) = fetch_result {
        return Err(AppError::provider(format!(
            "Fetch failed after writing {emitted} conversations: {err}"
        ))
        .into());
    }

    formatter.finish(&mut writer)?;
    writer.flush()?;
    if let Some(path) = output_path {
        info!(count = emitted, path = %path, "stream-wrote conversations to file");
    } else {
        info!(count = emitted, "stream-wrote conversations to stdout");
    }
    Ok(())
}

fn looks_like_atlassian_api_token(token: &str) -> bool {
    token.starts_with("AT") && !token.contains(':') && !token.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::source::FetchTarget;

    fn args() -> GetArgs {
        GetArgs {
            query: None,
            repo: Some("owner/repo".into()),
            state: None,
            labels: None,
            author: None,
            since: None,
            milestone: None,
            id: Some("1".into()),
            platform: None,
            instance: None,
            url: None,
            deployment: None,
            kind: None,
            format: None,
            output_mode: None,
            stream: false,
            include_review_comments: false,
            no_comments: false,
            no_links: false,
            output: None,
            token: None,
            account_email: None,
        }
    }

    fn bitbucket_config(deployment: &str, kind: &str, kind_explicit: bool) -> Config {
        Config {
            platform: "bitbucket".into(),
            kind: kind.into(),
            kind_explicit,
            token: None,
            account_email: None,
            repo: Some("PROJECT/repo".into()),
            state: None,
            deployment: Some(deployment.into()),
            per_page: 100,
            platform_url: Some("https://bitbucket.example.com".into()),
        }
    }

    #[test]
    fn resolve_output_plan_defaults_to_text_for_tty() {
        let plan = resolve_output_plan_with_tty(&args(), true);
        assert!(matches!(plan.mode, ResolvedOutputMode::Stream));
        assert!(matches!(plan.format, ResolvedOutputFormat::Text));
    }

    #[test]
    fn resolve_output_plan_defaults_to_jsonl_for_non_tty() {
        let plan = resolve_output_plan_with_tty(&args(), false);
        assert!(matches!(plan.mode, ResolvedOutputMode::Stream));
        assert!(matches!(plan.format, ResolvedOutputFormat::Jsonl));
    }

    #[test]
    fn resolve_output_plan_honors_batch_mode() {
        let mut args = args();
        args.output_mode = Some(OutputMode::Batch);
        let plan = resolve_output_plan_with_tty(&args, false);
        assert!(matches!(plan.mode, ResolvedOutputMode::Batch));
    }

    #[test]
    fn resolve_output_plan_stream_shorthand_wins() {
        let mut args = args();
        args.stream = true;
        let plan = resolve_output_plan_with_tty(&args, false);
        assert!(matches!(plan.mode, ResolvedOutputMode::Stream));
    }

    #[test]
    fn resolve_output_plan_maps_ndjson_to_jsonl() {
        let mut args = args();
        args.format = Some(OutputFormat::Ndjson);
        let plan = resolve_output_plan_with_tty(&args, false);
        assert!(matches!(plan.format, ResolvedOutputFormat::Jsonl));
    }

    #[test]
    fn bitbucket_id_defaults_to_pr_when_kind_is_implicit() {
        let cfg = bitbucket_config("cloud", "issue", false);
        let req = build_fetch_request(&cfg, &args()).unwrap();
        match req.target {
            FetchTarget::Id {
                kind,
                allow_fallback_to_pr,
                ..
            } => {
                assert!(matches!(kind, ContentKind::Pr));
                assert!(!allow_fallback_to_pr);
            }
            FetchTarget::Search { .. } => panic!("expected id target"),
        }
    }

    #[test]
    fn bitbucket_cloud_explicit_issue_is_rejected() {
        let cfg = bitbucket_config("cloud", "issue", true);
        let err = emit_get_warnings(&cfg, &args()).unwrap_err().to_string();
        assert!(err.contains("supports pull requests only"));
    }

    #[test]
    fn bitbucket_selfhosted_explicit_issue_is_rejected() {
        let cfg = bitbucket_config("selfhosted", "issue", true);
        let err = emit_get_warnings(&cfg, &args()).unwrap_err().to_string();
        assert!(err.contains("supports pull requests only"));
    }

    #[test]
    fn bitbucket_search_defaults_to_pr_when_kind_is_implicit() {
        let cfg = bitbucket_config("cloud", "issue", false);
        let mut args = args();
        args.id = None;
        let req = build_fetch_request(&cfg, &args).unwrap();
        match req.target {
            FetchTarget::Search { raw_query } => {
                assert!(raw_query.contains("is:pr"));
                assert!(!raw_query.contains("is:issue"));
            }
            FetchTarget::Id { .. } => panic!("expected search target"),
        }
    }

    #[test]
    fn build_fetch_request_respects_no_links_flag() {
        let cfg = bitbucket_config("cloud", "issue", false);
        let mut args = args();
        args.id = None;
        args.no_links = true;
        let req = build_fetch_request(&cfg, &args).unwrap();
        assert!(!req.include_links);
    }
}
