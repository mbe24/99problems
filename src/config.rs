use anyhow::{Result, anyhow};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Default, Deserialize, Clone)]
pub struct InstanceConfig {
    pub platform: Option<String>,
    pub token: Option<String>,
    pub account_email: Option<String>,
    pub url: Option<String>,
    pub repo: Option<String>,
    pub state: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub deployment: Option<String>,
    pub per_page: Option<u32>,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct TelemetrySection {
    pub enabled: Option<bool>,
    pub otlp_endpoint: Option<String>,
}

#[derive(Debug, Default, Deserialize, Clone)]
pub struct DotfileConfig {
    pub default_instance: Option<String>,
    pub telemetry: Option<TelemetrySection>,
    #[serde(default)]
    pub instances: HashMap<String, InstanceConfig>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub otlp_endpoint: Option<String>,
}

impl TelemetryConfig {
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.enabled
            && self
                .otlp_endpoint
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
    }
}

#[derive(Debug, Default)]
pub struct Config {
    pub platform: String,
    pub kind: String,
    pub kind_explicit: bool,
    pub token: Option<String>,
    pub account_email: Option<String>,
    pub repo: Option<String>,
    pub state: Option<String>,
    pub deployment: Option<String>,
    pub per_page: u32,
    pub platform_url: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ResolveOptions<'a> {
    pub platform: Option<&'a str>,
    pub instance: Option<&'a str>,
    pub url: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub deployment: Option<&'a str>,
    pub token: Option<&'a str>,
    pub account_email: Option<&'a str>,
    pub repo: Option<&'a str>,
    pub state: Option<&'a str>,
}

impl Config {
    /// Load and resolve config from dotfiles, env vars, and defaults.
    ///
    /// # Errors
    ///
    /// Returns an error if reading/parsing dotfiles fails, if unsupported
    /// dotfile keys are present, or if instance selection/validation fails.
    pub fn load() -> Result<Self> {
        Self::load_with_options(ResolveOptions::default())
    }

    /// Load config with CLI-provided overrides and selectors.
    ///
    /// # Errors
    ///
    /// Returns an error if reading/parsing dotfiles fails, if unsupported
    /// dotfile keys are present, or if instance selection/validation fails.
    pub fn load_with_options(opts: ResolveOptions<'_>) -> Result<Self> {
        let home = load_dotfile(home_dotfile_path())?;
        let local = load_dotfile(local_dotfile_path())?;
        resolve_from_dotfiles(home, local, opts)
    }
}

/// Load and resolve telemetry config from dotfiles.
///
/// # Errors
///
/// Returns an error if reading/parsing dotfiles fails or unsupported keys are
/// present.
pub fn load_telemetry_config() -> Result<TelemetryConfig> {
    let home = load_dotfile(home_dotfile_path())?;
    let local = load_dotfile(local_dotfile_path())?;
    Ok(resolve_telemetry_config(home, local))
}

#[must_use]
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

#[must_use]
fn merge_dotfiles(home: DotfileConfig, local: DotfileConfig) -> DotfileConfig {
    let DotfileConfig {
        default_instance: home_default_instance,
        telemetry: home_telemetry,
        instances: mut merged_instances,
    } = home;
    let DotfileConfig {
        default_instance: local_default_instance,
        telemetry: local_telemetry,
        instances: local_instances,
    } = local;

    for (alias, local_cfg) in local_instances {
        merged_instances
            .entry(alias)
            .and_modify(|home_cfg| *home_cfg = merge_instance(home_cfg, &local_cfg))
            .or_insert(local_cfg);
    }

    DotfileConfig {
        default_instance: local_default_instance.or(home_default_instance),
        telemetry: merge_telemetry(home_telemetry, local_telemetry),
        instances: merged_instances,
    }
}

#[must_use]
fn merge_telemetry(
    home: Option<TelemetrySection>,
    local: Option<TelemetrySection>,
) -> Option<TelemetrySection> {
    match (home, local) {
        (None, None) => None,
        (Some(base), None) => Some(base),
        (None, Some(override_cfg)) => Some(override_cfg),
        (Some(base), Some(override_cfg)) => Some(TelemetrySection {
            enabled: override_cfg.enabled.or(base.enabled),
            otlp_endpoint: override_cfg.otlp_endpoint.or(base.otlp_endpoint),
        }),
    }
}

#[must_use]
fn resolve_telemetry_config(home: DotfileConfig, local: DotfileConfig) -> TelemetryConfig {
    let merged = merge_dotfiles(home, local);
    let telemetry = merged.telemetry.unwrap_or_default();
    TelemetryConfig {
        enabled: telemetry.enabled.unwrap_or(false),
        otlp_endpoint: telemetry.otlp_endpoint,
    }
}

fn resolve_from_dotfiles(
    home: DotfileConfig,
    local: DotfileConfig,
    opts: ResolveOptions<'_>,
) -> Result<Config> {
    let merged = merge_dotfiles(home, local);

    if !merged.instances.is_empty() {
        return resolve_instance_mode(&merged, opts);
    }

    if let Some(alias) = opts.instance {
        return Err(anyhow!("Instance '{alias}' not found."));
    }

    if let Some(default_instance) = merged.default_instance {
        return Err(anyhow!(
            "default_instance is set to '{default_instance}', but no instances are configured."
        ));
    }

    resolve_cli_only_mode(opts)
}

fn resolve_instance_mode(merged: &DotfileConfig, opts: ResolveOptions<'_>) -> Result<Config> {
    if let Some(default_instance) = merged.default_instance.as_deref()
        && !merged.instances.contains_key(default_instance)
    {
        return Err(anyhow!(
            "default_instance '{default_instance}' was not found in instances."
        ));
    }

    let selected_alias = if let Some(alias) = opts.instance {
        if merged.instances.contains_key(alias) {
            alias.to_string()
        } else {
            return Err(anyhow!(
                "Instance '{alias}' not found. Available: {}",
                available_instances(&merged.instances)
            ));
        }
    } else if merged.instances.len() == 1 {
        merged
            .instances
            .keys()
            .next()
            .ok_or_else(|| anyhow!("No instances configured."))?
            .clone()
    } else if let Some(default_instance) = merged.default_instance.as_deref() {
        default_instance.to_string()
    } else {
        return Err(anyhow!(
            "Multiple instances configured. Specify --instance or set default_instance. Available: {}",
            available_instances(&merged.instances)
        ));
    };

    let instance = merged
        .instances
        .get(&selected_alias)
        .ok_or_else(|| anyhow!("Instance '{selected_alias}' not found."))?;
    let instance_platform = instance.platform.as_deref().ok_or_else(|| {
        anyhow!("Instance '{selected_alias}' is missing required field 'platform'.")
    })?;

    if let Some(cli_platform) = opts.platform
        && cli_platform != instance_platform
    {
        return Err(anyhow!(
            "Platform mismatch: --instance '{selected_alias}' uses '{instance_platform}', but --platform was '{cli_platform}'."
        ));
    }

    let platform = instance_platform.to_string();
    let deployment = normalize_deployment(
        &platform,
        opts.deployment.or(instance.deployment.as_deref()),
    )?;
    let token = opts
        .token
        .map(std::borrow::ToOwned::to_owned)
        .or_else(|| std::env::var(token_env_var(&platform)).ok())
        .or_else(|| instance.token.clone());
    let account_email = opts
        .account_email
        .map(std::borrow::ToOwned::to_owned)
        .or_else(|| account_email_env_var(&platform).and_then(|var| std::env::var(var).ok()))
        .or_else(|| instance.account_email.clone());
    let (kind, kind_explicit) = if let Some(kind) = opts.kind {
        (kind.to_string(), true)
    } else if let Some(kind) = instance.kind.as_deref() {
        (kind.to_string(), true)
    } else {
        ("issue".to_string(), false)
    };

    Ok(Config {
        platform,
        kind,
        kind_explicit,
        token,
        account_email,
        repo: opts
            .repo
            .map(std::borrow::ToOwned::to_owned)
            .or_else(|| instance.repo.clone()),
        state: opts
            .state
            .map(std::borrow::ToOwned::to_owned)
            .or_else(|| instance.state.clone()),
        deployment,
        per_page: instance.per_page.unwrap_or(100),
        platform_url: opts
            .url
            .map(std::borrow::ToOwned::to_owned)
            .or_else(|| instance.url.clone()),
    })
}

fn resolve_cli_only_mode(opts: ResolveOptions<'_>) -> Result<Config> {
    let platform = opts.platform.unwrap_or("github").to_string();
    if !matches!(
        platform.as_str(),
        "github" | "gitlab" | "jira" | "bitbucket"
    ) {
        return Err(anyhow!("Platform '{platform}' is not yet supported."));
    }
    let deployment = normalize_deployment(&platform, opts.deployment)?;

    let token = opts
        .token
        .map(std::borrow::ToOwned::to_owned)
        .or_else(|| std::env::var(token_env_var(&platform)).ok());
    let account_email = opts
        .account_email
        .map(std::borrow::ToOwned::to_owned)
        .or_else(|| account_email_env_var(&platform).and_then(|var| std::env::var(var).ok()));
    let (kind, kind_explicit) = if let Some(kind) = opts.kind {
        (kind.to_string(), true)
    } else {
        ("issue".to_string(), false)
    };

    Ok(Config {
        platform,
        kind,
        kind_explicit,
        token,
        account_email,
        repo: opts.repo.map(std::borrow::ToOwned::to_owned),
        state: opts.state.map(std::borrow::ToOwned::to_owned),
        deployment,
        per_page: 100,
        platform_url: opts.url.map(std::borrow::ToOwned::to_owned),
    })
}

fn normalize_deployment(platform: &str, deployment: Option<&str>) -> Result<Option<String>> {
    if platform == "bitbucket" {
        let deployment = deployment.ok_or_else(|| {
            anyhow!(
                "Bitbucket deployment is required. Set [instances.<alias>].deployment or pass --deployment (cloud|selfhosted)."
            )
        })?;
        return match deployment {
            "cloud" | "selfhosted" => Ok(Some(deployment.to_string())),
            other => Err(anyhow!(
                "Invalid bitbucket deployment '{other}'. Supported: cloud, selfhosted."
            )),
        };
    }

    if let Some(value) = deployment {
        return Err(anyhow!(
            "Deployment is only supported for platform 'bitbucket', got '{platform}' with deployment '{value}'."
        ));
    }

    Ok(None)
}

#[must_use]
fn available_instances(instances: &HashMap<String, InstanceConfig>) -> String {
    let mut keys: Vec<&str> = instances.keys().map(std::string::String::as_str).collect();
    keys.sort_unstable();
    keys.join(", ")
}

fn home_dotfile_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".99problems"))
}

fn local_dotfile_path() -> Option<PathBuf> {
    std::env::current_dir().ok().map(|d| d.join(".99problems"))
}

fn load_dotfile(path: Option<PathBuf>) -> Result<DotfileConfig> {
    let path = match path {
        Some(p) if p.exists() => p,
        _ => return Ok(DotfileConfig::default()),
    };
    let content = std::fs::read_to_string(&path)?;
    parse_dotfile_content(&content)
}

fn parse_dotfile_content(content: &str) -> Result<DotfileConfig> {
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
    validate_telemetry_keys(table)?;
    validate_instance_keys(table)?;
    Ok(())
}

fn validate_telemetry_keys(table: &toml::value::Table) -> Result<()> {
    let Some(telemetry) = table.get("telemetry") else {
        return Ok(());
    };
    let telemetry_table = telemetry
        .as_table()
        .ok_or_else(|| anyhow!("Invalid .99problems: 'telemetry' must be a TOML table."))?;
    for key in telemetry_table.keys() {
        if !matches!(key.as_str(), "enabled" | "otlp_endpoint") {
            return Err(anyhow!("Unsupported key 'telemetry.{key}' in .99problems."));
        }
    }
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

#[must_use]
pub fn token_env_var(platform: &str) -> &'static str {
    match platform {
        "github" => "GITHUB_TOKEN",
        "gitlab" => "GITLAB_TOKEN",
        "jira" => "JIRA_TOKEN",
        "bitbucket" => "BITBUCKET_TOKEN",
        _ => "TOKEN",
    }
}

#[must_use]
pub fn account_email_env_var(platform: &str) -> Option<&'static str> {
    match platform {
        "jira" => Some("JIRA_ACCOUNT_EMAIL"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve_with_opts(dotfile: &str, opts: ResolveOptions<'_>) -> Result<Config> {
        let home = DotfileConfig::default();
        let local = parse_dotfile_content(dotfile)?;
        resolve_from_dotfiles(home, local, opts)
    }

    #[test]
    fn parse_instance_only_dotfile() {
        let cfg = parse_dotfile_content(
            r#"
            default_instance = "work"
            [instances.work]
            platform = "gitlab"
            repo = "group/project"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.default_instance.as_deref(), Some("work"));
        assert!(cfg.instances.contains_key("work"));
    }

    #[test]
    fn rejects_legacy_section() {
        let err = parse_dotfile_content(
            r#"
            [github]
            token = "x"
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("Legacy section '[github]'"));
    }

    #[test]
    fn rejects_top_level_runtime_key() {
        let err = parse_dotfile_content(r#"platform = "github""#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Unsupported top-level key 'platform'"));
    }

    #[test]
    fn rejects_legacy_instance_email_key() {
        let err = parse_dotfile_content(
            r#"
            [instances.work]
            platform = "jira"
            email = "user@example.com"
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("instances.work.email"));
        assert!(err.contains("account_email"));
    }

    #[test]
    fn rejects_unknown_instance_key() {
        let err = parse_dotfile_content(
            r#"
            [instances.work]
            platform = "github"
            foo = "bar"
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("instances.work.foo"));
    }

    #[test]
    fn auto_selects_single_instance() {
        let cfg = resolve_with_opts(
            r#"
            [instances.only]
            platform = "gitlab"
            repo = "group/project"
            "#,
            ResolveOptions::default(),
        )
        .unwrap();
        assert_eq!(cfg.platform, "gitlab");
        assert_eq!(cfg.repo.as_deref(), Some("group/project"));
        assert_eq!(cfg.kind, "issue");
        assert!(!cfg.kind_explicit);
    }

    #[test]
    fn uses_default_instance_when_multiple() {
        let cfg = resolve_with_opts(
            r#"
            default_instance = "work"
            [instances.work]
            platform = "gitlab"
            repo = "group/work"

            [instances.public]
            platform = "github"
            repo = "owner/repo"
            "#,
            ResolveOptions::default(),
        )
        .unwrap();
        assert_eq!(cfg.platform, "gitlab");
        assert_eq!(cfg.repo.as_deref(), Some("group/work"));
    }

    #[test]
    fn errors_on_ambiguous_instances_without_default() {
        let err = resolve_with_opts(
            r#"
            [instances.work]
            platform = "gitlab"

            [instances.public]
            platform = "github"
            "#,
            ResolveOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("Multiple instances configured"));
    }

    #[test]
    fn errors_on_unknown_instance() {
        let err = resolve_with_opts(
            r#"
            [instances.work]
            platform = "gitlab"
            "#,
            ResolveOptions {
                instance: Some("missing"),
                ..ResolveOptions::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("Instance 'missing' not found"));
    }

    #[test]
    fn errors_on_platform_mismatch() {
        let err = resolve_with_opts(
            r#"
            [instances.work]
            platform = "gitlab"
            "#,
            ResolveOptions {
                instance: Some("work"),
                platform: Some("github"),
                ..ResolveOptions::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("Platform mismatch"));
    }

    #[test]
    fn cli_overrides_instance_fields() {
        let cfg = resolve_with_opts(
            r#"
            [instances.work]
            platform = "gitlab"
            repo = "group/project"
            state = "opened"
            type = "issue"
            "#,
            ResolveOptions {
                repo: Some("override/repo"),
                state: Some("closed"),
                kind: Some("pr"),
                ..ResolveOptions::default()
            },
        )
        .unwrap();
        assert_eq!(cfg.repo.as_deref(), Some("override/repo"));
        assert_eq!(cfg.state.as_deref(), Some("closed"));
        assert_eq!(cfg.kind, "pr");
        assert!(cfg.kind_explicit);
    }

    #[test]
    fn cli_only_mode_without_instances_works() {
        let cfg = resolve_from_dotfiles(
            DotfileConfig::default(),
            DotfileConfig::default(),
            ResolveOptions {
                platform: Some("github"),
                repo: Some("owner/repo"),
                ..ResolveOptions::default()
            },
        )
        .unwrap();
        assert_eq!(cfg.platform, "github");
        assert_eq!(cfg.repo.as_deref(), Some("owner/repo"));
        assert_eq!(cfg.kind, "issue");
        assert!(!cfg.kind_explicit);
    }

    #[test]
    fn merge_instances_deep_merges_fields() {
        let home = DotfileConfig {
            default_instance: None,
            telemetry: None,
            instances: HashMap::from([(
                "work".to_string(),
                InstanceConfig {
                    platform: Some("gitlab".to_string()),
                    token: Some("home-token".to_string()),
                    account_email: None,
                    url: Some("https://home.example".to_string()),
                    repo: Some("group/home".to_string()),
                    state: None,
                    kind: None,
                    deployment: None,
                    per_page: Some(20),
                },
            )]),
        };
        let local = DotfileConfig {
            default_instance: Some("work".to_string()),
            telemetry: None,
            instances: HashMap::from([(
                "work".to_string(),
                InstanceConfig {
                    platform: None,
                    token: Some("local-token".to_string()),
                    account_email: None,
                    url: None,
                    repo: Some("group/local".to_string()),
                    state: Some("opened".to_string()),
                    kind: Some("pr".to_string()),
                    deployment: None,
                    per_page: None,
                },
            )]),
        };
        let merged = merge_dotfiles(home, local);
        let work = merged.instances.get("work").unwrap();
        assert_eq!(work.platform.as_deref(), Some("gitlab"));
        assert_eq!(work.token.as_deref(), Some("local-token"));
        assert_eq!(work.url.as_deref(), Some("https://home.example"));
        assert_eq!(work.repo.as_deref(), Some("group/local"));
        assert_eq!(work.state.as_deref(), Some("opened"));
        assert_eq!(work.kind.as_deref(), Some("pr"));
        assert_eq!(work.per_page, Some(20));
        assert_eq!(merged.default_instance.as_deref(), Some("work"));
    }

    #[test]
    fn bitbucket_requires_deployment() {
        let err = resolve_with_opts(
            r#"
            [instances.work]
            platform = "bitbucket"
            repo = "workspace/repo"
            "#,
            ResolveOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("Bitbucket deployment is required"));
    }

    #[test]
    fn deployment_rejected_for_non_bitbucket_platforms() {
        let err = resolve_from_dotfiles(
            DotfileConfig::default(),
            DotfileConfig::default(),
            ResolveOptions {
                platform: Some("github"),
                deployment: Some("cloud"),
                ..ResolveOptions::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("Deployment is only supported"));
    }

    #[test]
    fn telemetry_is_parsed_and_resolved_from_dotfiles() {
        let home = parse_dotfile_content(
            r#"
            [telemetry]
            enabled = false
            otlp_endpoint = "http://home:4318/v1/traces"
            "#,
        )
        .unwrap();
        let local = parse_dotfile_content(
            r"
            [telemetry]
            enabled = true
            ",
        )
        .unwrap();

        let telemetry = resolve_telemetry_config(home, local);
        assert!(telemetry.enabled);
        assert_eq!(
            telemetry.otlp_endpoint.as_deref(),
            Some("http://home:4318/v1/traces")
        );
    }

    #[test]
    fn rejects_unknown_telemetry_key() {
        let err = parse_dotfile_content(
            r#"
            [telemetry]
            foo = "bar"
            "#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("telemetry.foo"));
    }
}
