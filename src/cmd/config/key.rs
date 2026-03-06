use anyhow::{Result, anyhow};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigKey {
    DefaultInstance,
    InstanceField { alias: String, field: InstanceField },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstanceField {
    Platform,
    Url,
    Token,
    AccountEmail,
    Repo,
    State,
    Type,
    Deployment,
    PerPage,
}

impl InstanceField {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            InstanceField::Platform => "platform",
            InstanceField::Url => "url",
            InstanceField::Token => "token",
            InstanceField::AccountEmail => "account_email",
            InstanceField::Repo => "repo",
            InstanceField::State => "state",
            InstanceField::Type => "type",
            InstanceField::Deployment => "deployment",
            InstanceField::PerPage => "per_page",
        }
    }
}

impl ConfigKey {
    /// Parse a key path used by `config get/set/unset`.
    ///
    /// # Errors
    ///
    /// Returns an error when the key path is malformed or unsupported.
    pub(crate) fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed == "default_instance" {
            return Ok(Self::DefaultInstance);
        }

        let parts: Vec<&str> = trimmed.split('.').collect();
        if parts.len() != 3 || parts.first() != Some(&"instances") {
            return Err(anyhow!(
                "Invalid key path '{raw}'. Use 'default_instance' or 'instances.<alias>.<field>'."
            ));
        }
        let alias = parts[1].trim();
        if alias.is_empty() {
            return Err(anyhow!(
                "Invalid key path '{raw}': instance alias cannot be empty."
            ));
        }

        let field = match parts[2] {
            "platform" => InstanceField::Platform,
            "url" => InstanceField::Url,
            "token" => InstanceField::Token,
            "account_email" => InstanceField::AccountEmail,
            "repo" => InstanceField::Repo,
            "state" => InstanceField::State,
            "type" => InstanceField::Type,
            "deployment" => InstanceField::Deployment,
            "per_page" => InstanceField::PerPage,
            other => {
                return Err(anyhow!(
                    "Unsupported instance field '{other}'. Supported: platform, url, token, account_email, repo, state, type, deployment, per_page."
                ));
            }
        };

        Ok(Self::InstanceField {
            alias: alias.to_string(),
            field,
        })
    }

    pub(crate) fn is_secret(&self) -> bool {
        matches!(
            self,
            Self::InstanceField {
                field: InstanceField::Token,
                ..
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_instance() {
        assert_eq!(
            ConfigKey::parse("default_instance").unwrap(),
            ConfigKey::DefaultInstance
        );
    }

    #[test]
    fn parses_instance_field() {
        let key = ConfigKey::parse("instances.work.platform").unwrap();
        assert_eq!(
            key,
            ConfigKey::InstanceField {
                alias: "work".to_string(),
                field: InstanceField::Platform
            }
        );
    }

    #[test]
    fn rejects_invalid_path() {
        let err = ConfigKey::parse("instances.work").unwrap_err().to_string();
        assert!(err.contains("Invalid key path"));
    }
}
