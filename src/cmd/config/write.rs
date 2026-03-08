use anyhow::{Result, anyhow};
use std::fs;
use std::io::Write;
use std::path::Path;
use toml_edit::{DocumentMut, Item, Table, value};

use crate::cmd::config::key::{ConfigKey, InstanceField};
use crate::cmd::config::store::{WriteScope, path_for_write_scope};

/// Set a config key in the selected write scope.
///
/// # Errors
///
/// Returns an error if key/value validation fails, document parsing fails,
/// or writing to disk fails.
pub(crate) fn set(scope: WriteScope, key: &ConfigKey, raw_value: &str) -> Result<()> {
    validate_set_value(key, raw_value)?;

    let path = path_for_write_scope(scope)?;
    let mut doc = load_or_create_doc(&path)?;

    match key {
        ConfigKey::DefaultInstance => {
            doc["default_instance"] = value(raw_value);
        }
        ConfigKey::TelemetryEnabled => {
            let enabled = raw_value.parse::<bool>()?;
            doc["telemetry"]["enabled"] = value(enabled);
        }
        ConfigKey::TelemetryOtlpEndpoint => {
            doc["telemetry"]["otlp_endpoint"] = value(raw_value);
        }
        ConfigKey::InstanceField { alias, field } => {
            let instances_item = doc.entry("instances").or_insert(Item::Table(Table::new()));
            let instances_table = instances_item
                .as_table_mut()
                .ok_or_else(|| anyhow!("Invalid TOML: 'instances' must be a table."))?;

            if !instances_table.contains_key(alias) {
                if *field == InstanceField::Platform {
                    instances_table.insert(alias, Item::Table(Table::new()));
                } else {
                    return Err(anyhow!(
                        "Instance '{alias}' does not exist. Set 'instances.{alias}.platform' first."
                    ));
                }
            }

            let instance_fields = instances_table
                .get_mut(alias)
                .and_then(Item::as_table_mut)
                .ok_or_else(|| anyhow!("Invalid TOML: 'instances.{alias}' must be a table."))?;

            match field {
                InstanceField::PerPage => {
                    let parsed: u32 = raw_value.parse()?;
                    instance_fields["per_page"] = value(i64::from(parsed));
                }
                _ => {
                    instance_fields[field.as_str()] = value(raw_value);
                }
            }
        }
    }

    atomic_write(&path, doc.to_string().as_bytes())
}

/// Unset a config key in the selected write scope.
///
/// # Errors
///
/// Returns an error if the file cannot be loaded or written.
pub(crate) fn unset(scope: WriteScope, key: &ConfigKey) -> Result<()> {
    let path = path_for_write_scope(scope)?;
    let mut doc = load_or_create_doc(&path)?;

    match key {
        ConfigKey::DefaultInstance => {
            let _ = doc.as_table_mut().remove("default_instance");
        }
        ConfigKey::TelemetryEnabled => {
            remove_telemetry_field(&mut doc, "enabled");
        }
        ConfigKey::TelemetryOtlpEndpoint => {
            remove_telemetry_field(&mut doc, "otlp_endpoint");
        }
        ConfigKey::InstanceField { alias, field } => {
            if let Some(instances_table) = doc.get_mut("instances").and_then(Item::as_table_mut) {
                if let Some(instance_item) = instances_table.get_mut(alias)
                    && let Some(instance_table) = instance_item.as_table_mut()
                {
                    let _ = instance_table.remove(field.as_str());
                    if instance_table.is_empty() {
                        let _ = instances_table.remove(alias);
                    }
                }

                if instances_table.is_empty() {
                    let _ = doc.as_table_mut().remove("instances");
                }
            }
        }
    }

    atomic_write(&path, doc.to_string().as_bytes())
}

fn validate_set_value(key: &ConfigKey, raw_value: &str) -> Result<()> {
    if raw_value.trim().is_empty() {
        return Err(anyhow!("Value cannot be empty."));
    }

    match key {
        ConfigKey::DefaultInstance | ConfigKey::TelemetryOtlpEndpoint => {}
        ConfigKey::TelemetryEnabled => {
            let _ = raw_value.parse::<bool>()?;
        }
        ConfigKey::InstanceField { field, .. } => match field {
            InstanceField::Platform => match raw_value {
                "github" | "gitlab" | "jira" | "bitbucket" => {}
                _ => {
                    return Err(anyhow!(
                        "Invalid platform '{raw_value}'. Supported: github, gitlab, jira, bitbucket."
                    ));
                }
            },
            InstanceField::Type => match raw_value {
                "issue" | "pr" => {}
                _ => return Err(anyhow!("Invalid type '{raw_value}'. Supported: issue, pr.")),
            },
            InstanceField::PerPage => {
                let parsed: u32 = raw_value.parse()?;
                if parsed == 0 {
                    return Err(anyhow!("per_page must be >= 1."));
                }
            }
            InstanceField::Deployment => match raw_value {
                "cloud" | "selfhosted" => {}
                _ => {
                    return Err(anyhow!(
                        "Invalid deployment '{raw_value}'. Supported: cloud, selfhosted."
                    ));
                }
            },
            InstanceField::Url
            | InstanceField::Token
            | InstanceField::AccountEmail
            | InstanceField::Repo
            | InstanceField::State => {}
        },
    }

    Ok(())
}

fn remove_telemetry_field(doc: &mut DocumentMut, field: &str) {
    if let Some(telemetry_table) = doc.get_mut("telemetry").and_then(Item::as_table_mut) {
        let _ = telemetry_table.remove(field);
        if telemetry_table.is_empty() {
            let _ = doc.as_table_mut().remove("telemetry");
        }
    }
}

fn load_or_create_doc(path: &Path) -> Result<DocumentMut> {
    if !path.exists() {
        return Ok(DocumentMut::new());
    }
    let raw = std::fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(DocumentMut::new());
    }
    Ok(raw.parse::<DocumentMut>()?)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp = path.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
    }
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}
