mod config;
mod format;
mod model;
mod source;

use anyhow::{Result, anyhow};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Generator, Shell, generate};
use std::io::Write;

use config::{Config, ResolveOptions, token_env_var};
use format::{Formatter, json::JsonFormatter, yaml::YamlFormatter};
use model::Conversation;
use source::{
    ContentKind, FetchRequest, FetchTarget, Query, Source, github::GitHubSource,
    gitlab::GitLabSource, jira::JiraSource,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Json,
    Yaml,
}

#[derive(Debug, Clone, ValueEnum, PartialEq)]
enum Platform {
    Github,
    Gitlab,
    Jira,
    Bitbucket,
}

impl Platform {
    fn as_str(&self) -> &str {
        match self {
            Platform::Github => "github",
            Platform::Gitlab => "gitlab",
            Platform::Jira => "jira",
            Platform::Bitbucket => "bitbucket",
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
enum ContentType {
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
enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Powershell,
    Elvish,
}

impl CompletionShell {
    fn as_clap_shell(&self) -> Shell {
        match self {
            CompletionShell::Bash => Shell::Bash,
            CompletionShell::Zsh => Shell::Zsh,
            CompletionShell::Fish => Shell::Fish,
            CompletionShell::Powershell => Shell::PowerShell,
            CompletionShell::Elvish => Shell::Elvish,
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "99problems",
    about = "Fetch issue and pull request conversations",
    subcommand_required = true,
    arg_required_else_help = true,
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Fetch issue and pull request conversations
    #[command(visible_alias = "got")]
    Get(Box<GetArgs>),

    /// Generate shell completion script and print it to stdout
    Completions {
        #[arg(value_enum)]
        shell: CompletionShell,
    },
}

#[derive(Args, Debug)]
struct GetArgs {
    /// Full search query (same syntax as the platform's web UI search bar)
    /// e.g. "state:closed Event repo:owner/repo"
    #[arg(short = 'q', long)]
    query: Option<String>,

    /// Shorthand for adding "repo:owner/repo" to the query (alias: --project)
    #[arg(short = 'r', long, visible_alias = "project")]
    repo: Option<String>,

    /// Shorthand for adding "state:open|closed" to the query
    #[arg(short = 's', long)]
    state: Option<String>,

    /// Shorthand for comma-separated labels, e.g. "bug,enhancement"
    #[arg(short = 'l', long)]
    labels: Option<String>,

    /// Filter by issue/PR author
    #[arg(short = 'a', long)]
    author: Option<String>,

    /// Only include items created on or after this date (YYYY-MM-DD), e.g. "2024-01-01"
    #[arg(short = 'S', long)]
    since: Option<String>,

    /// Filter by milestone title or number
    #[arg(short = 'm', long)]
    milestone: Option<String>,

    /// Fetch a single issue/PR by identifier (bypasses search)
    #[arg(short = 'i', long = "id", visible_alias = "issue")]
    id: Option<String>,

    /// Platform adapter to fetch from (used directly in CLI-only mode)
    #[arg(short = 'p', long, value_enum)]
    platform: Option<Platform>,

    /// Named instance alias from .99problems ([instances.<alias>])
    #[arg(short = 'I', long)]
    instance: Option<String>,

    /// Override platform base URL for one-off runs
    #[arg(short = 'u', long)]
    url: Option<String>,

    /// Content type to fetch [default: issue]
    #[arg(short = 't', long = "type", value_enum)]
    kind: Option<ContentType>,

    /// Output format
    #[arg(short = 'f', long, value_enum, default_value = "json")]
    format: OutputFormat,

    /// Include pull request review comments (inline code comments)
    #[arg(short = 'R', long)]
    include_review_comments: bool,

    /// Skip fetching comments (faster, smaller output)
    #[arg(long)]
    no_comments: bool,

    /// Write output to a file (default: stdout)
    #[arg(short = 'o', long)]
    output: Option<String>,

    /// Personal access token (overrides env var and dotfile)
    #[arg(short = 'k', long)]
    token: Option<String>,

    /// Jira account email used with API tokens (for Atlassian Cloud basic auth)
    #[arg(long)]
    jira_email: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Get(args) => run_get(&args),
        Commands::Completions { shell } => {
            print_completions(shell.as_clap_shell(), &mut std::io::stdout());
            Ok(())
        }
    }
}

fn run_get(args: &GetArgs) -> Result<()> {
    let cfg = load_config_for_get(args)?;
    emit_get_warnings(&cfg, args)?;

    let source = build_source_for_platform(&cfg)?;
    let conversations = fetch_get_conversations(source.as_ref(), &cfg, args)?;
    write_formatted_output(args.format, args.output.as_deref(), &conversations)
}

fn load_config_for_get(args: &GetArgs) -> Result<Config> {
    if args.platform.is_none()
        && args.instance.is_none()
        && args.url.is_none()
        && args.kind.is_none()
        && args.token.is_none()
        && args.jira_email.is_none()
        && args.repo.is_none()
        && args.state.is_none()
    {
        return Config::load();
    }

    Config::load_with_options(ResolveOptions {
        platform: args.platform.as_ref().map(Platform::as_str),
        instance: args.instance.as_deref(),
        url: args.url.as_deref(),
        kind: args.kind.as_ref().map(ContentType::as_str),
        token: args.token.as_deref(),
        jira_email: args.jira_email.as_deref(),
        repo: args.repo.as_deref(),
        state: args.state.as_deref(),
    })
}

fn emit_get_warnings(cfg: &Config, args: &GetArgs) -> Result<()> {
    if cfg.token.is_none() {
        let env_var = token_env_var(&cfg.platform);
        eprintln!(
            "Warning: no token detected for {}. You may be subject to API rate limiting. Set --token, {}, or configure it in .99problems.",
            cfg.platform, env_var
        );
    }
    if cfg.platform == "jira"
        && let Some(token) = cfg.token.as_deref()
        && looks_like_atlassian_api_token(token)
        && cfg.jira_email.is_none()
    {
        eprintln!(
            "Warning: Jira token looks like an Atlassian API token. Configure --jira-email, JIRA_EMAIL, or [instances.<alias>].email, or provide --token as email:api_token."
        );
    }
    if args.no_comments && args.include_review_comments {
        eprintln!("Warning: --include-review-comments is ignored when --no-comments is set.");
    }
    if cfg.platform == "jira" && cfg.kind == "pr" {
        return Err(anyhow!(
            "Platform 'jira' does not support pull requests. Use --type issue."
        ));
    }
    Ok(())
}

fn build_source_for_platform(cfg: &Config) -> Result<Box<dyn Source>> {
    match cfg.platform.as_str() {
        "github" => Ok(Box::new(GitHubSource::new()?)),
        "gitlab" => Ok(Box::new(GitLabSource::new(cfg.platform_url.clone())?)),
        "jira" => Ok(Box::new(JiraSource::new(cfg.platform_url.clone())?)),
        other => Err(anyhow!("Platform '{other}' is not yet supported")),
    }
}

fn fetch_get_conversations(
    source: &dyn Source,
    cfg: &Config,
    args: &GetArgs,
) -> Result<Vec<Conversation>> {
    let repo = cfg.repo.clone();
    let state = cfg.state.clone();

    if let Some(id) = &args.id {
        return fetch_get_by_id(source, cfg, args, repo, id);
    }

    fetch_get_by_search(source, cfg, args, repo, state)
}

fn fetch_get_by_id(
    source: &dyn Source,
    cfg: &Config,
    args: &GetArgs,
    repo: Option<String>,
    id: &str,
) -> Result<Vec<Conversation>> {
    let ignored_flags = ignored_flags_in_id_mode(args);
    if !ignored_flags.is_empty() {
        eprintln!(
            "Warning: when using --id/--issue, these flags are ignored: {}",
            ignored_flags.join(", ")
        );
    }

    let id_kind = if cfg.kind == "pr" {
        ContentKind::Pr
    } else {
        ContentKind::Issue
    };
    let kind_explicit = args.kind.is_some() || cfg.kind == "pr";

    let repo_for_id = if cfg.platform == "jira" {
        repo.unwrap_or_default()
    } else {
        repo.ok_or_else(|| anyhow!("--repo is required when using --id/--issue"))?
    };
    let req = FetchRequest {
        target: FetchTarget::Id {
            repo: repo_for_id,
            id: id.to_string(),
            kind: id_kind,
            allow_fallback_to_pr: !kind_explicit && matches!(id_kind, ContentKind::Issue),
        },
        per_page: cfg.per_page,
        token: cfg.token.clone(),
        jira_email: cfg.jira_email.clone(),
        include_comments: !args.no_comments,
        include_review_comments: args.include_review_comments,
    };
    source.fetch(&req)
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

fn fetch_get_by_search(
    source: &dyn Source,
    cfg: &Config,
    args: &GetArgs,
    repo: Option<String>,
    state: Option<String>,
) -> Result<Vec<Conversation>> {
    let query = Query::build(
        args.query.clone(),
        &cfg.kind,
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
        return Err(anyhow!(
            "No query specified. Use -q or provide --repo/--state/--labels."
        ));
    }
    let req = FetchRequest {
        target: FetchTarget::Search {
            raw_query: query.raw,
        },
        per_page: query.per_page,
        token: query.token,
        jira_email: cfg.jira_email.clone(),
        include_comments: !args.no_comments,
        include_review_comments: args.include_review_comments,
    };
    source.fetch(&req)
}

fn build_formatter(format: OutputFormat) -> Box<dyn Formatter> {
    match format {
        OutputFormat::Json => Box::new(JsonFormatter),
        OutputFormat::Yaml => Box::new(YamlFormatter),
    }
}

fn write_formatted_output(
    format: OutputFormat,
    output_path: Option<&str>,
    conversations: &[Conversation],
) -> Result<()> {
    let formatter = build_formatter(format);
    let output = formatter.format(conversations)?;

    match output_path {
        Some(path) => {
            let mut file = std::fs::File::create(path)?;
            file.write_all(output.as_bytes())?;
            eprintln!("Wrote {} conversations to {path}", conversations.len());
        }
        None => println!("{output}"),
    }

    Ok(())
}

fn print_completions<G: Generator>(generator: G, out: &mut dyn Write) {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(generator, &mut cmd, name, out);
}

fn looks_like_atlassian_api_token(token: &str) -> bool {
    token.starts_with("AT") && !token.contains(':') && !token.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_get_subcommand() {
        let cli = Cli::try_parse_from(["99problems", "get", "--repo", "owner/repo", "--id", "1"])
            .expect("expected get subcommand to parse");
        match cli.command {
            Commands::Get(args) => {
                assert_eq!(args.repo.as_deref(), Some("owner/repo"));
                assert_eq!(args.id.as_deref(), Some("1"));
            }
            Commands::Completions { .. } => panic!("expected get command"),
        }
    }

    #[test]
    fn parses_got_alias_to_get_subcommand() {
        let cli = Cli::try_parse_from(["99problems", "got", "--repo", "owner/repo", "--id", "2"])
            .expect("expected got alias to parse");
        match cli.command {
            Commands::Get(args) => {
                assert_eq!(args.repo.as_deref(), Some("owner/repo"));
                assert_eq!(args.id.as_deref(), Some("2"));
            }
            Commands::Completions { .. } => panic!("expected get command"),
        }
    }

    #[test]
    fn parses_completions_subcommand() {
        let cli = Cli::try_parse_from(["99problems", "completions", "bash"])
            .expect("expected completions command to parse");
        match cli.command {
            Commands::Completions { shell } => match shell {
                CompletionShell::Bash => {}
                _ => panic!("expected bash shell"),
            },
            Commands::Get(_) => panic!("expected completions command"),
        }
    }
}
