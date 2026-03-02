mod config;
mod format;
mod model;
mod source;

use anyhow::{Result, anyhow};
use clap::{Parser, ValueEnum};
use std::io::Write;

use config::Config;
use format::{Formatter, json::JsonFormatter, yaml::YamlFormatter};
use source::{Query, Source, github_issues::GitHubIssues};

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

    /// Fetch a single issue by number (bypasses search)
    #[arg(long)]
    issue: Option<u64>,

    /// Platform to fetch from [default: github]
    #[arg(long, value_enum)]
    platform: Option<Platform>,

    /// Content type to fetch [default: issue]
    #[arg(long = "type", value_enum)]
    kind: Option<ContentType>,

    /// Output format
    #[arg(long, value_enum, default_value = "json")]
    format: OutputFormat,

    /// Write output to a file (default: stdout)
    #[arg(short = 'o', long)]
    output: Option<String>,

    /// Personal access token (overrides env var and dotfile)
    #[arg(long)]
    token: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
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

    let repo = cli.repo.or(cfg.repo.clone());
    let state = cli.state.or(cfg.state.clone());

    let formatter: Box<dyn Formatter> = match cli.format {
        OutputFormat::Json => Box::new(JsonFormatter),
        OutputFormat::Yaml => Box::new(YamlFormatter),
    };

    let source: Box<dyn Source> = match cfg.platform.as_str() {
        "github" => Box::new(GitHubIssues::new()?),
        other => return Err(anyhow!("Platform '{other}' is not yet supported")),
    };

    let conversations = if let Some(issue_id) = cli.issue {
        let r = repo
            .as_deref()
            .ok_or_else(|| anyhow!("--repo is required when using --issue"))?;
        vec![source.fetch_one(r, issue_id)?]
    } else {
        let query = Query::build(
            cli.query,
            &cfg.kind,
            repo,
            state,
            cli.labels,
            cli.author,
            cli.since,
            cli.milestone,
            cfg.per_page,
            cfg.token,
        );
        if query.raw.trim().is_empty() {
            return Err(anyhow!(
                "No query specified. Use -q or provide --repo/--state/--labels."
            ));
        }
        source.fetch(&query)?
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
