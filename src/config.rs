use anyhow::Result;
use serde::Deserialize;
use std::path::PathBuf;

/// Represents values that can be set in a .99problems TOML dotfile.
#[derive(Debug, Default, Deserialize)]
pub struct DotfileConfig {
    pub token: Option<String>,
    pub repo: Option<String>,
    pub state: Option<String>,
    pub per_page: Option<u32>,
}

/// Fully resolved config after merging home + local dotfiles + env var.
#[derive(Debug, Default)]
pub struct Config {
    pub token: Option<String>,
    pub repo: Option<String>,
    pub state: Option<String>,
    pub per_page: u32,
}

impl Config {
    /// Load and merge: home dotfile (base) → local dotfile (override) → env var.
    /// CLI flags are merged later in main.rs on top of this.
    pub fn load() -> Result<Self> {
        let home = load_dotfile(home_dotfile_path())?;
        let local = load_dotfile(local_dotfile_path())?;

        // Merge: local wins over home
        let token = std::env::var("GITHUB_TOKEN")
            .ok()
            .or_else(|| local.token.clone())
            .or_else(|| home.token.clone());

        let repo = local.repo.or(home.repo);
        let state = local.state.or(home.state);
        let per_page = local.per_page.or(home.per_page).unwrap_or(100);

        Ok(Self { token, repo, state, per_page })
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
        assert!(cfg.token.is_none());
        assert!(cfg.repo.is_none());
        assert!(cfg.per_page.is_none());
    }

    #[test]
    fn dotfile_config_parses_toml() {
        let toml = r#"
            token = "ghp_test"
            repo = "owner/repo"
            state = "closed"
            per_page = 50
        "#;
        let cfg: DotfileConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.token.as_deref(), Some("ghp_test"));
        assert_eq!(cfg.repo.as_deref(), Some("owner/repo"));
        assert_eq!(cfg.per_page, Some(50));
    }

    #[test]
    fn config_per_page_defaults_to_100() {
        // Simulate both dotfiles missing, no env var
        let home = DotfileConfig::default();
        let local = DotfileConfig::default();
        let per_page = local.per_page.or(home.per_page).unwrap_or(100);
        assert_eq!(per_page, 100);
    }
}
