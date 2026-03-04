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
    subcommand_required = true,
    arg_required_else_help = true,
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

    /// Inspect and edit .99problems configuration
    Config(cmd::config::ConfigArgs),

    /// Generate shell completion script and print it to stdout
    Completions {
        #[arg(value_enum)]
        shell: CompletionShell,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(err) = logging::init(cli.verbose, cli.quiet) {
        render_and_exit(classify_anyhow_error(&err), cli.error_format);
    }

    let result = match cli.command {
        Commands::Get(args) => cmd::get::run(&args),
        Commands::Config(args) => cmd::config::run(&args),
        Commands::Completions { shell } => {
            print_completions(shell.as_clap_shell(), &mut std::io::stdout());
            Ok(())
        }
    };

    if let Err(err) = result {
        render_and_exit(classify_anyhow_error(&err), cli.error_format);
    }
}

fn print_completions<G: Generator>(generator: G, out: &mut dyn Write) {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(generator, &mut cmd, name, out);
}

fn render_and_exit(app_err: AppError, format: ErrorFormat) -> ! {
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
                assert_eq!(args.repo.as_deref(), Some("owner/repo"));
                assert_eq!(args.id.as_deref(), Some("1"));
            }
            Commands::Config(_) | Commands::Completions { .. } => panic!("expected get command"),
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
            Commands::Config(_) | Commands::Completions { .. } => panic!("expected get command"),
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
            Commands::Get(_) | Commands::Completions { .. } => panic!("expected config command"),
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
            Commands::Get(_) | Commands::Config(_) => panic!("expected completions command"),
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
}
