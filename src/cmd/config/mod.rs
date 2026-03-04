mod key;
mod render;
mod store;
mod write;

use anyhow::{Result, anyhow};
use clap::{Args, Subcommand, ValueEnum};

use key::ConfigKey;
use render::render_value;
use store::{ReadScope, WriteScope};

#[derive(Args, Debug)]
pub(crate) struct ConfigArgs {
    #[command(subcommand)]
    pub(crate) command: ConfigSubcommand,
}

#[derive(Subcommand, Debug)]
pub(crate) enum ConfigSubcommand {
    /// Print config file path for a scope
    Path {
        #[arg(long, value_enum, default_value = "local")]
        scope: PathScope,
    },
    /// List all configured keys in a scope
    List {
        #[arg(long, value_enum, default_value = "resolved")]
        scope: ReadScopeArg,
        #[arg(long)]
        show_secrets: bool,
    },
    /// Read one configured key
    Get {
        key: String,
        #[arg(long, value_enum, default_value = "resolved")]
        scope: ReadScopeArg,
        #[arg(long)]
        show_secrets: bool,
    },
    /// Set a configured key
    Set {
        key: String,
        value: String,
        #[arg(long, value_enum, default_value = "local")]
        scope: WriteScopeArg,
    },
    /// Unset a configured key
    Unset {
        key: String,
        #[arg(long, value_enum, default_value = "local")]
        scope: WriteScopeArg,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum PathScope {
    Home,
    Local,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum ReadScopeArg {
    Home,
    Local,
    Resolved,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum WriteScopeArg {
    Home,
    Local,
}

impl From<PathScope> for ReadScope {
    fn from(value: PathScope) -> Self {
        match value {
            PathScope::Home => Self::Home,
            PathScope::Local => Self::Local,
        }
    }
}

impl From<ReadScopeArg> for ReadScope {
    fn from(value: ReadScopeArg) -> Self {
        match value {
            ReadScopeArg::Home => Self::Home,
            ReadScopeArg::Local => Self::Local,
            ReadScopeArg::Resolved => Self::Resolved,
        }
    }
}

impl From<WriteScopeArg> for WriteScope {
    fn from(value: WriteScopeArg) -> Self {
        match value {
            WriteScopeArg::Home => Self::Home,
            WriteScopeArg::Local => Self::Local,
        }
    }
}

/// Run the `config` command family.
///
/// # Errors
///
/// Returns an error if scope loading fails, key parsing/validation fails,
/// or set/unset operations fail to persist.
pub(crate) fn run(args: &ConfigArgs) -> Result<()> {
    match &args.command {
        ConfigSubcommand::Path { scope } => run_path((*scope).into()),
        ConfigSubcommand::List {
            scope,
            show_secrets,
        } => run_list((*scope).into(), *show_secrets),
        ConfigSubcommand::Get {
            key,
            scope,
            show_secrets,
        } => run_get(key, (*scope).into(), *show_secrets),
        ConfigSubcommand::Set { key, value, scope } => run_set(key, value, (*scope).into()),
        ConfigSubcommand::Unset { key, scope } => run_unset(key, (*scope).into()),
    }
}

fn run_path(scope: ReadScope) -> Result<()> {
    let path = store::path_for_read_scope(scope)?;
    println!("{}", path.display());
    Ok(())
}

fn run_list(scope: ReadScope, show_secrets: bool) -> Result<()> {
    let cfg = store::load_dotfile_scope(scope)?;
    let entries = store::list_entries(&cfg);
    for (key, value, is_secret) in entries {
        println!("{key}={}", render_value(&value, is_secret, show_secrets));
    }
    Ok(())
}

fn run_get(key_raw: &str, scope: ReadScope, show_secrets: bool) -> Result<()> {
    let key = ConfigKey::parse(key_raw)?;
    let cfg = store::load_dotfile_scope(scope)?;
    let value = store::get_key_value(&cfg, &key).ok_or_else(|| {
        anyhow!(
            "Key '{key_raw}' is not set for scope '{}'.",
            scope_name(scope)
        )
    })?;

    println!(
        "{key_raw}={}",
        render_value(&value, key.is_secret(), show_secrets)
    );
    Ok(())
}

fn run_set(key_raw: &str, value: &str, scope: WriteScope) -> Result<()> {
    let key = ConfigKey::parse(key_raw)?;
    write::set(scope, &key, value)?;
    let display_value = if key.is_secret() {
        "****".to_string()
    } else {
        value.to_string()
    };
    println!(
        "Set {key_raw}={display_value} in {} scope.",
        scope_name_write(scope)
    );
    Ok(())
}

fn run_unset(key_raw: &str, scope: WriteScope) -> Result<()> {
    let key = ConfigKey::parse(key_raw)?;
    write::unset(scope, &key)?;
    println!("Unset {key_raw} in {} scope.", scope_name_write(scope));
    Ok(())
}

fn scope_name(scope: ReadScope) -> &'static str {
    match scope {
        ReadScope::Home => "home",
        ReadScope::Local => "local",
        ReadScope::Resolved => "resolved",
    }
}

fn scope_name_write(scope: WriteScope) -> &'static str {
    match scope {
        WriteScope::Home => "home",
        WriteScope::Local => "local",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::config::key::InstanceField;

    #[test]
    fn parse_key_paths() {
        assert!(matches!(
            ConfigKey::parse("default_instance").unwrap(),
            ConfigKey::DefaultInstance
        ));
        assert!(matches!(
            ConfigKey::parse("instances.work.platform").unwrap(),
            ConfigKey::InstanceField {
                alias,
                field: InstanceField::Platform
            } if alias == "work"
        ));
    }
}
