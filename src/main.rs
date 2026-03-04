mod cmd;
mod config;
mod format;
mod model;
mod source;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Generator, Shell, generate};
use std::io::Write;

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
    Get(Box<cmd::get::GetArgs>),

    /// Inspect and edit .99problems configuration
    Config(cmd::config::ConfigArgs),

    /// Generate shell completion script and print it to stdout
    Completions {
        #[arg(value_enum)]
        shell: CompletionShell,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Get(args) => cmd::get::run(&args),
        Commands::Config(args) => cmd::config::run(&args),
        Commands::Completions { shell } => {
            print_completions(shell.as_clap_shell(), &mut std::io::stdout());
            Ok(())
        }
    }
}

fn print_completions<G: Generator>(generator: G, out: &mut dyn Write) {
    let mut cmd = Cli::command();
    let name = cmd.get_name().to_string();
    generate(generator, &mut cmd, name, out);
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
}
