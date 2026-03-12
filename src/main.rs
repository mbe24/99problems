mod cmd;
mod config;
mod error;
mod format;
mod logging;
mod model;
mod source;

use clap::{ArgAction, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Generator, Shell, generate};
use std::io::Write;
use tracing::error;

use crate::error::{AppError, classify_anyhow_error};

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

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ErrorFormat {
    Text,
    Json,
}

#[derive(Parser, Debug)]
#[command(
    name = "99problems",
    about = "Fetch issue and pull request conversations",
    long_about = "Fetch issue and pull request conversations from GitHub, GitLab, and Jira.",
    subcommand_required = true,
    arg_required_else_help = true,
    next_line_help = true,
    after_help = "Examples:\n  99problems get github --id 1842 --repo schemaorg/schemaorg\n  99problems get -q repo:github/gitignore is:pr 2402 --include-review-comments\n  99problems skill init\n  99problems man --output docs/man",
    version
)]
struct Cli {
    /// Increase diagnostic verbosity (-v, -vv, -vvv)
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count, global = true, conflicts_with = "quiet")]
    verbose: u8,

    /// Suppress non-error diagnostics
    #[arg(short = 'Q', long = "quiet", global = true)]
    quiet: bool,

    /// Error output format
    #[arg(long, value_enum, default_value = "text", global = true)]
    error_format: ErrorFormat,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Fetch issue and pull request conversations
    #[command(visible_alias = "got")]
    Get(Box<cmd::get::GetArgs>),

    /// Initialize a canonical Agent Skill scaffold for 99problems
    Skill(cmd::skill::SkillArgs),

    /// Inspect and edit .99problems configuration
    Config(cmd::config::ConfigArgs),

    /// Generate shell completion script and print it to stdout
    Completions {
        #[arg(value_enum)]
        shell: CompletionShell,
    },

    /// Generate and print/write man pages
    Man(cmd::man::ManArgs),
}

fn main() {
    let cli = Cli::parse();
    let telemetry_config = if matches!(&cli.command, Commands::Get(_)) {
        match config::load_telemetry_config() {
            Ok(cfg) => Some(cfg),
            Err(err) => {
                eprintln!("Warning: telemetry config could not be loaded: {err}");
                None
            }
        }
    } else {
        None
    };
    let telemetry_active = cfg!(feature = "telemetry-otel")
        && telemetry_config
            .as_ref()
            .is_some_and(config::TelemetryConfig::is_active);
    let mut logging_handle = match logging::init(cli.verbose, cli.quiet, telemetry_config.as_ref())
    {
        Ok(handle) => handle,
        Err(err) => {
            let app_err = classify_anyhow_error(&err);
            render_and_exit(&app_err, cli.error_format);
        }
    };
    let result = match cli.command {
        Commands::Get(args) => cmd::get::run(&args, telemetry_active),
        Commands::Skill(args) => cmd::skill::run(&args),
        Commands::Config(args) => cmd::config::run(&args),
        Commands::Completions { shell } => {
            print_completions(shell.as_clap_shell(), &mut std::io::stdout());
            Ok(())
        }
        Commands::Man(args) => cmd::man::run(Cli::command(), &args),
    };

    if let Err(err) = result {
        let app_err = classify_anyhow_error(&err);
        logging_handle.shutdown();
        render_and_exit(&app_err, cli.error_format);
    }
    logging_handle.shutdown();
}

fn print_completions<G: Generator>(generator: G, out: &mut dyn Write) {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(generator, &mut cmd, name, out);
}

fn render_and_exit(app_err: &AppError, format: ErrorFormat) -> ! {
    match format {
        ErrorFormat::Text => eprintln!("Error: {}", app_err.render_text()),
        ErrorFormat::Json => eprintln!("{}", app_err.render_json()),
    }
    error!(
        category = app_err.category().code(),
        exit_code = app_err.exit_code(),
        "command failed"
    );
    std::process::exit(app_err.exit_code());
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
                assert_eq!(args.instance_positional.as_deref(), None);
                assert_eq!(args.repo.as_deref(), Some("owner/repo"));
                assert_eq!(args.id.as_deref(), Some("1"));
            }
            Commands::Skill(_)
            | Commands::Config(_)
            | Commands::Completions { .. }
            | Commands::Man(_) => {
                panic!("expected get command")
            }
        }
    }

    #[test]
    fn parses_got_alias_to_get_subcommand() {
        let cli = Cli::try_parse_from(["99problems", "got", "--repo", "owner/repo", "--id", "2"])
            .expect("expected got alias to parse");
        match cli.command {
            Commands::Get(args) => {
                assert_eq!(args.instance_positional.as_deref(), None);
                assert_eq!(args.repo.as_deref(), Some("owner/repo"));
                assert_eq!(args.id.as_deref(), Some("2"));
            }
            Commands::Skill(_)
            | Commands::Config(_)
            | Commands::Completions { .. }
            | Commands::Man(_) => {
                panic!("expected get command")
            }
        }
    }

    #[test]
    fn parses_config_subcommand() {
        let cli = Cli::try_parse_from([
            "99problems",
            "config",
            "set",
            "instances.work.platform",
            "gitlab",
        ])
        .expect("expected config command to parse");
        match cli.command {
            Commands::Config(_) => {}
            Commands::Get(_)
            | Commands::Skill(_)
            | Commands::Completions { .. }
            | Commands::Man(_) => {
                panic!("expected config command")
            }
        }
    }

    #[test]
    fn parses_skill_init_subcommand() {
        let cli = Cli::try_parse_from(["99problems", "skill", "init"])
            .expect("expected skill init parse");
        match cli.command {
            Commands::Skill(_) => {}
            Commands::Get(_)
            | Commands::Config(_)
            | Commands::Completions { .. }
            | Commands::Man(_) => {
                panic!("expected skill command")
            }
        }
    }

    #[test]
    fn parses_completions_subcommand() {
        let cli = Cli::try_parse_from(["99problems", "completions", "bash"])
            .expect("expected completions command to parse");
        match cli.command {
            Commands::Completions { shell } => match shell {
                CompletionShell::Bash => {}
                CompletionShell::Zsh
                | CompletionShell::Fish
                | CompletionShell::Powershell
                | CompletionShell::Elvish => panic!("expected bash shell"),
            },
            Commands::Get(_) | Commands::Skill(_) | Commands::Config(_) | Commands::Man(_) => {
                panic!("expected completions command")
            }
        }
    }

    #[test]
    fn parses_repeated_verbose_flag() {
        let cli = Cli::try_parse_from([
            "99problems",
            "-vv",
            "get",
            "--repo",
            "owner/repo",
            "--id",
            "1",
        ])
        .expect("expected repeated verbosity flags to parse");
        assert_eq!(cli.verbose, 2);
        assert!(!cli.quiet);
    }

    #[test]
    fn rejects_quiet_and_verbose_together() {
        let err = Cli::try_parse_from([
            "99problems",
            "--quiet",
            "-v",
            "get",
            "--repo",
            "owner/repo",
            "--id",
            "1",
        ])
        .expect_err("expected conflict between --quiet and -v");
        let message = err.to_string();
        assert!(message.contains("--quiet"));
        assert!(message.contains("--verbose"));
    }

    #[test]
    fn parses_error_format_json() {
        let cli = Cli::try_parse_from([
            "99problems",
            "--error-format",
            "json",
            "get",
            "--repo",
            "owner/repo",
            "--id",
            "1",
        ])
        .expect("expected --error-format json to parse");
        match cli.error_format {
            ErrorFormat::Json => {}
            ErrorFormat::Text => panic!("expected json error format"),
        }
    }

    #[test]
    fn parses_man_subcommand() {
        let cli = Cli::try_parse_from(["99problems", "man", "--output", "docs/man"])
            .expect("expected man command to parse");
        match cli.command {
            Commands::Man(_) => {}
            Commands::Get(_)
            | Commands::Skill(_)
            | Commands::Config(_)
            | Commands::Completions { .. } => {
                panic!("expected man command")
            }
        }
    }

    #[test]
    fn parses_get_with_positional_instance_alias() {
        let cli = Cli::try_parse_from(["99problems", "get", "jira", "-i", "25"])
            .expect("expected positional instance alias to parse");
        match cli.command {
            Commands::Get(args) => {
                assert_eq!(args.instance_positional.as_deref(), Some("jira"));
                assert_eq!(args.instance.as_deref(), None);
                assert_eq!(args.id.as_deref(), Some("25"));
            }
            Commands::Skill(_)
            | Commands::Config(_)
            | Commands::Completions { .. }
            | Commands::Man(_) => {
                panic!("expected get command")
            }
        }
    }

    #[test]
    fn parses_get_query_as_unquoted_multi_token_value() {
        let cli = Cli::try_parse_from([
            "99problems",
            "get",
            "-q",
            "is:issue",
            "state:open",
            "architectural",
            "--no-comments",
        ])
        .expect("expected multi-token query to parse");
        match cli.command {
            Commands::Get(args) => {
                assert_eq!(
                    args.query,
                    Some(vec![
                        "is:issue".to_string(),
                        "state:open".to_string(),
                        "architectural".to_string()
                    ])
                );
                assert!(args.no_comments);
            }
            Commands::Skill(_)
            | Commands::Config(_)
            | Commands::Completions { .. }
            | Commands::Man(_) => {
                panic!("expected get command")
            }
        }
    }
}
