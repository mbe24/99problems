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

#[derive(Debug, Clone, ValueEnum)]
enum SourceKind {
    GithubIssues,
    GithubPrs,
}

#[derive(Parser, Debug)]
#[command(
    name = "99problems",
    about = "Fetch GitHub issue conversations",
    version
)]
struct Cli {
    /// Full GitHub search query (same syntax as the web UI search bar)
    /// e.g. "is:issue state:closed Event repo:owner/repo"
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

    /// Fetch a single issue by number (bypasses search)
    #[arg(long)]
    issue: Option<u64>,

    /// Data source to use
    #[arg(long, value_enum, default_value = "github-issues")]
    source: SourceKind,

    /// Output format
    #[arg(long, value_enum, default_value = "json")]
    format: OutputFormat,

    /// Write output to a file (default: stdout)
    #[arg(short = 'o', long)]
    output: Option<String>,

    /// GitHub personal access token (overrides GITHUB_TOKEN and dotfile)
    #[arg(long)]
    token: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut cfg = Config::load()?;

    // CLI token overrides everything
    if let Some(t) = cli.token {
        cfg.token = Some(t);
    }

    // Override repo/state from CLI if provided
    let repo = cli.repo.or(cfg.repo.clone());
    let state = cli.state.or(cfg.state.clone());

    // Build the formatter
    let formatter: Box<dyn Formatter> = match cli.format {
        OutputFormat::Json => Box::new(JsonFormatter),
        OutputFormat::Yaml => Box::new(YamlFormatter),
    };

    // Build the source
    let source: Box<dyn Source> = match cli.source {
        SourceKind::GithubIssues => Box::new(GitHubIssues::new()?),
        SourceKind::GithubPrs => {
            return Err(anyhow!("github-prs source is not yet implemented"));
        }
    };

    let conversations = if let Some(issue_id) = cli.issue {
        // Single-issue mode
        let r = repo
            .as_deref()
            .ok_or_else(|| anyhow!("--repo is required when using --issue"))?;
        vec![source.fetch_one(r, issue_id)?]
    } else {
        // Search mode
        let query = Query::build(cli.query, repo, state, cli.labels, cfg.per_page, cfg.token);
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
