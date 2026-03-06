use anyhow::{Result, anyhow};
use std::path::PathBuf;

use crate::cmd::config::key::{ConfigKey, InstanceField};
use crate::config::{DotfileConfig, InstanceConfig, account_email_env_var, token_env_var};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ReadScope {
    Home,
    Local,
    Resolved,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum WriteScope {
    Home,
    Local,
}

pub(crate) fn path_for_read_scope(scope: ReadScope) -> Result<PathBuf> {
    match scope {
        ReadScope::Home => home_dotfile_path(),
        ReadScope::Local | ReadScope::Resolved => local_dotfile_path(),
    }
}

pub(crate) fn path_for_write_scope(scope: WriteScope) -> Result<PathBuf> {
    match scope {
        WriteScope::Home => home_dotfile_path(),
        WriteScope::Local => local_dotfile_path(),
    }
}

pub(crate) fn load_dotfile_scope(scope: ReadScope) -> Result<DotfileConfig> {
    match scope {
        ReadScope::Home => load_single_dotfile(home_dotfile_path()?),
        ReadScope::Local => load_single_dotfile(local_dotfile_path()?),
        ReadScope::Resolved => {
            let home = load_single_dotfile(home_dotfile_path()?)?;
            let local = load_single_dotfile(local_dotfile_path()?)?;
            let mut merged = merge_dotfiles(home, local);
            apply_env_overrides(&mut merged);
            Ok(merged)
        }
    }
}

pub(crate) fn list_entries(cfg: &DotfileConfig) -> Vec<(String, String, bool)> {
    let mut entries = Vec::new();
    if let Some(default_instance) = cfg.default_instance.as_ref() {
        entries.push((
            "default_instance".to_string(),
            default_instance.clone(),
            false,
        ));
    }

    let mut aliases: Vec<&str> = cfg
        .instances
        .keys()
        .map(std::string::String::as_str)
        .collect();
    aliases.sort_unstable();

    for alias in aliases {
        if let Some(inst) = cfg.instances.get(alias) {
            push_field(
                &mut entries,
                alias,
                "platform",
                inst.platform.as_deref(),
                false,
            );
            push_field(&mut entries, alias, "url", inst.url.as_deref(), false);
            push_field(&mut entries, alias, "token", inst.token.as_deref(), true);
            push_field(
                &mut entries,
                alias,
                "account_email",
                inst.account_email.as_deref(),
                false,
            );
            push_field(&mut entries, alias, "repo", inst.repo.as_deref(), false);
            push_field(&mut entries, alias, "state", inst.state.as_deref(), false);
            push_field(&mut entries, alias, "type", inst.kind.as_deref(), false);
            push_field(
                &mut entries,
                alias,
                "deployment",
                inst.deployment.as_deref(),
                false,
            );
            if let Some(per_page) = inst.per_page {
                entries.push((
                    format!("instances.{alias}.per_page"),
                    per_page.to_string(),
                    false,
                ));
            }
        }
    }

    entries
}

pub(crate) fn get_key_value(cfg: &DotfileConfig, key: &ConfigKey) -> Option<String> {
    match key {
        ConfigKey::DefaultInstance => cfg.default_instance.clone(),
        ConfigKey::InstanceField { alias, field } => {
            let inst = cfg.instances.get(alias)?;
            match field {
                InstanceField::Platform => inst.platform.clone(),
                InstanceField::Url => inst.url.clone(),
                InstanceField::Token => inst.token.clone(),
                InstanceField::AccountEmail => inst.account_email.clone(),
                InstanceField::Repo => inst.repo.clone(),
                InstanceField::State => inst.state.clone(),
                InstanceField::Type => inst.kind.clone(),
                InstanceField::Deployment => inst.deployment.clone(),
                InstanceField::PerPage => inst.per_page.map(|v| v.to_string()),
            }
        }
    }
}

fn push_field(
    entries: &mut Vec<(String, String, bool)>,
    alias: &str,
    field: &str,
    value: Option<&str>,
    is_secret: bool,
) {
    if let Some(value) = value {
        entries.push((
            format!("instances.{alias}.{field}"),
            value.to_string(),
            is_secret,
        ));
    }
}

fn home_dotfile_path() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|h| h.join(".99problems"))
        .ok_or_else(|| anyhow!("Could not determine home directory."))
}

fn local_dotfile_path() -> Result<PathBuf> {
    Ok(std::env::current_dir()?.join(".99problems"))
}

fn load_single_dotfile(path: PathBuf) -> Result<DotfileConfig> {
    if !path.exists() {
        return Ok(DotfileConfig::default());
    }
    let content = std::fs::read_to_string(path)?;
    parse_and_validate_dotfile(&content)
}

fn parse_and_validate_dotfile(content: &str) -> Result<DotfileConfig> {
    let value: toml::Value = toml::from_str(content)?;
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("Invalid .99problems: expected top-level TOML table."))?;
    validate_dotfile_keys(table)?;
    let cfg: DotfileConfig = toml::from_str(content)?;
    Ok(cfg)
}

fn validate_dotfile_keys(table: &toml::value::Table) -> Result<()> {
    for key in [
        "platform",
        "repo",
        "state",
        "type",
        "per_page",
        "account_email",
        "deployment",
    ] {
        if table.contains_key(key) {
            return Err(anyhow!("Unsupported top-level key '{key}' in .99problems."));
        }
    }
    for key in ["github", "gitlab", "jira", "bitbucket"] {
        if table.contains_key(key) {
            return Err(anyhow!("Legacy section '[{key}]' is not supported."));
        }
    }
    validate_instance_keys(table)?;
    Ok(())
}

fn validate_instance_keys(table: &toml::value::Table) -> Result<()> {
    let Some(instances) = table.get("instances") else {
        return Ok(());
    };
    let instance_entries = instances
        .as_table()
        .ok_or_else(|| anyhow!("Invalid .99problems: 'instances' must be a TOML table."))?;

    for (alias, value) in instance_entries {
        let cfg_table = value.as_table().ok_or_else(|| {
            anyhow!("Invalid .99problems: instances.{alias} must be a TOML table.")
        })?;
        for key in cfg_table.keys() {
            if key == "email" {
                return Err(anyhow!(
                    "Unsupported key 'instances.{alias}.email'. Use 'instances.{alias}.account_email' instead."
                ));
            }
            if !matches!(
                key.as_str(),
                "platform"
                    | "token"
                    | "account_email"
                    | "url"
                    | "repo"
                    | "state"
                    | "type"
                    | "deployment"
                    | "per_page"
            ) {
                return Err(anyhow!(
                    "Unsupported key 'instances.{alias}.{key}' in .99problems."
                ));
            }
        }
    }

    Ok(())
}

fn merge_instance(base: &InstanceConfig, override_cfg: &InstanceConfig) -> InstanceConfig {
    InstanceConfig {
        platform: override_cfg
            .platform
            .clone()
            .or_else(|| base.platform.clone()),
        token: override_cfg.token.clone().or_else(|| base.token.clone()),
        account_email: override_cfg
            .account_email
            .clone()
            .or_else(|| base.account_email.clone()),
        url: override_cfg.url.clone().or_else(|| base.url.clone()),
        repo: override_cfg.repo.clone().or_else(|| base.repo.clone()),
        state: override_cfg.state.clone().or_else(|| base.state.clone()),
        kind: override_cfg.kind.clone().or_else(|| base.kind.clone()),
        deployment: override_cfg
            .deployment
            .clone()
            .or_else(|| base.deployment.clone()),
        per_page: override_cfg.per_page.or(base.per_page),
    }
}

fn merge_dotfiles(home: DotfileConfig, local: DotfileConfig) -> DotfileConfig {
    let mut merged_instances = home.instances;
    for (alias, local_cfg) in local.instances {
        merged_instances
            .entry(alias)
            .and_modify(|home_cfg| *home_cfg = merge_instance(home_cfg, &local_cfg))
            .or_insert(local_cfg);
    }

    DotfileConfig {
        default_instance: local.default_instance.or(home.default_instance),
        instances: merged_instances,
    }
}

fn apply_env_overrides(cfg: &mut DotfileConfig) {
    for instance in cfg.instances.values_mut() {
        if let Some(platform) = instance.platform.as_deref()
            && let Ok(token) = std::env::var(token_env_var(platform))
        {
            instance.token = Some(token);
        }
        if let Some(platform) = instance.platform.as_deref()
            && let Some(env_key) = account_email_env_var(platform)
            && let Ok(account_email) = std::env::var(env_key)
        {
            instance.account_email = Some(account_email);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn list_entries_is_sorted_by_alias() {
        let cfg = DotfileConfig {
            default_instance: None,
            instances: HashMap::from([
                (
                    "zeta".to_string(),
                    InstanceConfig {
                        platform: Some("gitlab".to_string()),
                        ..InstanceConfig::default()
                    },
                ),
                (
                    "alpha".to_string(),
                    InstanceConfig {
                        platform: Some("github".to_string()),
                        ..InstanceConfig::default()
                    },
                ),
            ]),
        };

        let entries = list_entries(&cfg);
        assert_eq!(entries[0].0, "instances.alpha.platform");
        assert_eq!(entries[1].0, "instances.zeta.platform");
    }
}
