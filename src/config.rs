use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

/// Per-platform credentials and settings.
#[derive(Debug, Default, Deserialize, Clone)]
pub struct PlatformConfig {
    pub token: Option<String>,
    /// Base URL override for self-hosted instances (e.g. GitLab)
    pub url: Option<String>,
}

/// Top-level dotfile structure (.99problems).
#[derive(Debug, Default, Deserialize)]
pub struct DotfileConfig {
    /// Default platform (overridden by --platform flag)
    pub platform: Option<String>,
    /// Default type: "issue" or "pr" (overridden by --type flag)
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub repo: Option<String>,
    pub state: Option<String>,
    pub per_page: Option<u32>,
    pub github: Option<PlatformConfig>,
    pub gitlab: Option<PlatformConfig>,
    pub bitbucket: Option<PlatformConfig>,
}

/// Fully resolved config after merging home + local dotfiles + env vars.
#[derive(Debug, Default)]
pub struct Config {
    pub platform: String,
    pub kind: String,
    pub token: Option<String>,
    pub repo: Option<String>,
    pub state: Option<String>,
    pub per_page: u32,
    /// Base URL for the active platform (for self-hosted instances)
    #[allow(dead_code)]
    pub platform_url: Option<String>,
}

impl Config {
    /// Load and merge: home dotfile (base) → local dotfile (override) → env vars.
    /// CLI flags are applied on top in main.rs.
    pub fn load() -> Result<Self> {
        let home = load_dotfile(home_dotfile_path())?;
        let local = load_dotfile(local_dotfile_path())?;

        let platform = local
            .platform
            .clone()
            .or_else(|| home.platform.clone())
            .unwrap_or_else(|| "github".into());

        let kind = local
            .kind
            .clone()
            .or_else(|| home.kind.clone())
            .unwrap_or_else(|| "issue".into());

        let repo = local.repo.clone().or(home.repo.clone());
        let state = local.state.clone().or(home.state.clone());
        let per_page = local.per_page.or(home.per_page).unwrap_or(100);

        // Resolve token: env var → local dotfile → home dotfile
        let env_var = match platform.as_str() {
            "github" => "GITHUB_TOKEN",
            "gitlab" => "GITLAB_TOKEN",
            "bitbucket" => "BITBUCKET_TOKEN",
            _ => "GITHUB_TOKEN",
        };
        let (dotfile_token, platform_url) = resolve_platform_token(&platform, &local, &home);
        let token = std::env::var(env_var).ok().or(dotfile_token);

        Ok(Self {
            platform,
            kind,
            token,
            repo,
            state,
            per_page,
            platform_url,
        })
    }
}

/// Resolve token and optional URL for the given platform from dotfiles only.
fn resolve_platform_token(
    platform: &str,
    local: &DotfileConfig,
    home: &DotfileConfig,
) -> (Option<String>, Option<String>) {
    let local_platform = platform_section(platform, local);
    let home_platform = platform_section(platform, home);

    let token = local_platform
        .as_ref()
        .and_then(|p| p.token.clone())
        .or_else(|| home_platform.as_ref().and_then(|p| p.token.clone()));

    let url = local_platform
        .as_ref()
        .and_then(|p| p.url.clone())
        .or_else(|| home_platform.as_ref().and_then(|p| p.url.clone()));

    (token, url)
}

fn platform_section<'a>(platform: &str, cfg: &'a DotfileConfig) -> Option<&'a PlatformConfig> {
    match platform {
        "github" => cfg.github.as_ref(),
        "gitlab" => cfg.gitlab.as_ref(),
        "bitbucket" => cfg.bitbucket.as_ref(),
        _ => None,
    }
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
    let cfg: DotfileConfig = toml::from_str(&content)?;
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotfile_config_defaults_when_missing() {
        let cfg = load_dotfile(None).unwrap();
        assert!(cfg.github.is_none());
        assert!(cfg.repo.is_none());
        assert!(cfg.per_page.is_none());
    }

    #[test]
    fn dotfile_config_parses_toml() {
        let toml = r#"
            repo = "owner/repo"
            state = "closed"
            per_page = 50

            [github]
            token = "ghp_test"
        "#;
        let cfg: DotfileConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.repo.as_deref(), Some("owner/repo"));
        assert_eq!(cfg.per_page, Some(50));
        assert_eq!(
            cfg.github.as_ref().and_then(|g| g.token.as_deref()),
            Some("ghp_test")
        );
    }

    #[test]
    fn config_per_page_defaults_to_100() {
        let home = DotfileConfig::default();
        let local = DotfileConfig::default();
        let per_page = local.per_page.or(home.per_page).unwrap_or(100);
        assert_eq!(per_page, 100);
    }

    #[test]
    fn config_platform_defaults_to_github() {
        let home = DotfileConfig::default();
        let local = DotfileConfig::default();
        let platform = local
            .platform
            .or(home.platform)
            .unwrap_or_else(|| "github".into());
        assert_eq!(platform, "github");
    }

    #[test]
    fn resolve_token_uses_platform_section() {
        let home = DotfileConfig::default();
        let local = DotfileConfig {
            github: Some(PlatformConfig {
                token: Some("ghp_section".into()),
                url: None,
            }),
            ..Default::default()
        };
        let (token, _) = resolve_platform_token("github", &local, &home);
        assert_eq!(token.as_deref(), Some("ghp_section"));
    }
}
