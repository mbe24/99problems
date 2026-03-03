mod config;
mod format;
mod model;
mod source;

use anyhow::{Result, anyhow};
use clap::{CommandFactory, Parser, ValueEnum};
use clap_complete::{Generator, Shell, generate};
use std::io::Write;

use config::Config;
use format::{Formatter, json::JsonFormatter, yaml::YamlFormatter};
use source::{ContentKind, FetchRequest, FetchTarget, Query, Source, github_issues::GitHubIssues};

#[derive(Debug, Clone, ValueEnum)]
enum OutputFormat {
    Json,
    Yaml,
}

#[derive(Debug, Clone, ValueEnum, PartialEq)]
enum Platform {
    Github,
    Gitlab,
    Bitbucket,
}

impl Platform {
    fn as_str(&self) -> &str {
        match self {
            Platform::Github => "github",
            Platform::Gitlab => "gitlab",
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
    about = "Fetch GitHub issue conversations",
    version
)]
struct Cli {
    /// Full search query (same syntax as the platform's web UI search bar)
    /// e.g. "state:closed Event repo:owner/repo"
    #[arg(short = 'q', long)]
    query: Option<String>,

    /// Shorthand for adding "repo:owner/repo" to the query
    #[arg(long)]
    repo: Option<String>,

    /// Shorthand for adding "state:open|closed" to the query
    #[arg(long)]
    state: Option<String>,

    /// Shorthand for comma-separated labels, e.g. "bug,enhancement"
    #[arg(long)]
    labels: Option<String>,

    /// Filter by issue/PR author
    #[arg(long)]
    author: Option<String>,

    /// Only include items created on or after this date (YYYY-MM-DD), e.g. "2024-01-01"
    #[arg(long)]
    since: Option<String>,

    /// Filter by milestone title or number
    #[arg(long)]
    milestone: Option<String>,

    /// Fetch a single issue/PR by number (bypasses search)
    #[arg(long = "id", visible_alias = "issue")]
    id: Option<u64>,

    /// Platform to fetch from [default: github]
    #[arg(long, value_enum)]
    platform: Option<Platform>,

    /// Content type to fetch [default: issue]
    #[arg(long = "type", value_enum)]
    kind: Option<ContentType>,

    /// Output format
    #[arg(long, value_enum, default_value = "json")]
    format: OutputFormat,

    /// Include pull request review comments (inline code comments)
    #[arg(long)]
    include_review_comments: bool,

    /// Write output to a file (default: stdout)
    #[arg(short = 'o', long)]
    output: Option<String>,

    /// Personal access token (overrides env var and dotfile)
    #[arg(long)]
    token: Option<String>,

    /// Generate shell completion script and print it to stdout
    #[arg(long, value_enum)]
    completions: Option<CompletionShell>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(shell) = &cli.completions {
        let shell = shell.as_clap_shell();
        print_completions(shell, &mut std::io::stdout());
        return Ok(());
    }
    let mut cfg = Config::load()?;

    // CLI flags override config values
    if let Some(p) = &cli.platform {
        cfg.platform = p.as_str().to_owned();
    }
    if let Some(k) = &cli.kind {
        cfg.kind = k.as_str().to_owned();
    }
    if let Some(t) = cli.token {
        cfg.token = Some(t);
    }

    let repo = cli.repo.clone().or(cfg.repo.clone());
    let state = cli.state.clone().or(cfg.state.clone());

    let formatter: Box<dyn Formatter> = match cli.format {
        OutputFormat::Json => Box::new(JsonFormatter),
        OutputFormat::Yaml => Box::new(YamlFormatter),
    };

    let source: Box<dyn Source> = match cfg.platform.as_str() {
        "github" => Box::new(GitHubIssues::new()?),
        other => return Err(anyhow!("Platform '{other}' is not yet supported")),
    };

    let conversations = if let Some(id) = cli.id {
        let mut ignored_flags = Vec::new();
        if cli.query.is_some() {
            ignored_flags.push("--query");
        }
        if cli.state.is_some() {
            ignored_flags.push("--state");
        }
        if cli.labels.is_some() {
            ignored_flags.push("--labels");
        }
        if cli.author.is_some() {
            ignored_flags.push("--author");
        }
        if cli.since.is_some() {
            ignored_flags.push("--since");
        }
        if cli.milestone.is_some() {
            ignored_flags.push("--milestone");
        }
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
        let kind_explicit = cli.kind.is_some() || cfg.kind == "pr";

        let r = repo
            .as_deref()
            .ok_or_else(|| anyhow!("--repo is required when using --id/--issue"))?;
        let req = FetchRequest {
            target: FetchTarget::Id {
                repo: r.to_string(),
                id,
                kind: id_kind,
                allow_fallback_to_pr: !kind_explicit && matches!(id_kind, ContentKind::Issue),
            },
            per_page: cfg.per_page,
            token: cfg.token.clone(),
            include_review_comments: cli.include_review_comments,
        };
        source.fetch(&req)?
    } else {
        let query = Query::build(
            cli.query.clone(),
            &cfg.kind,
            repo,
            state,
            cli.labels.clone(),
            cli.author.clone(),
            cli.since.clone(),
            cli.milestone.clone(),
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
            include_review_comments: cli.include_review_comments,
        };
        source.fetch(&req)?
    };

    let output = formatter.format(&conversations)?;

    match cli.output {
        Some(path) => {
            let mut file = std::fs::File::create(&path)?;
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
