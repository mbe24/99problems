use anyhow::{Result, anyhow};
use clap::{Args, Subcommand};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::warn;

use crate::error::AppError;

const SKILL_NAME: &str = "99problems";
const SKILL_AUTHOR: &str = "Mikael Beyene";
const SKILL_VERSION: &str = "1.0";
const SKILL_LICENSE: &str = "Apache-2.0";
const MAX_NAME_LEN: usize = 64;
const MAX_DESCRIPTION_LEN: usize = 1024;
const MAX_COMPATIBILITY_LEN: usize = 500;
const MAX_SKILL_LINES: usize = 500;

const SKILL_MD: &str = include_str!("../../templates/skills/99problems/SKILL.md");
const REFERENCE_MD: &str =
    include_str!("../../templates/skills/99problems/references/REFERENCE.md");
const FORMS_MD: &str = include_str!("../../templates/skills/99problems/references/FORMS.md");
const OPENAI_YAML: &str = include_str!("../../templates/skills/99problems/agents/openai.yaml");
const LOGO_SMALL_SVG: &str =
    include_str!("../../templates/skills/99problems/assets/logo-small.svg");
const LOGO_LARGE_SVG: &str =
    include_str!("../../templates/skills/99problems/assets/logo-large.svg");

#[derive(Args, Debug)]
pub(crate) struct SkillArgs {
    #[command(subcommand)]
    command: SkillCommand,
}

#[derive(Subcommand, Debug)]
enum SkillCommand {
    /// Initialize the canonical 99problems skill scaffold
    Init(SkillInitArgs),
}

#[derive(Args, Debug)]
pub(crate) struct SkillInitArgs {
    /// Base directory where skills are stored
    #[arg(long, default_value = ".agents/skills")]
    path: PathBuf,

    /// Overwrite generated files if the target already exists
    #[arg(long)]
    force: bool,
}

/// Run the `skill` command family.
///
/// # Errors
///
/// Returns an error if target validation fails or scaffold files cannot be written.
pub(crate) fn run(args: &SkillArgs) -> Result<()> {
    match &args.command {
        SkillCommand::Init(init_args) => init_skill(init_args),
    }
}

fn init_skill(args: &SkillInitArgs) -> Result<()> {
    validate_skill_name(SKILL_NAME)?;
    validate_skill_markdown(SKILL_MD)?;

    let target_dir = args.path.join(SKILL_NAME);
    if target_dir.exists() && !args.force {
        return Err(AppError::usage(format!(
            "Target '{}' already exists. Use --force to overwrite generated files.",
            target_dir.display()
        ))
        .into());
    }
    if target_dir.is_file() {
        return Err(AppError::usage(format!(
            "Target '{}' is a file. Expected a directory.",
            target_dir.display()
        ))
        .into());
    }

    let skill_file = target_dir.join("SKILL.md");
    let references_dir = target_dir.join("references");
    let reference_file = references_dir.join("REFERENCE.md");
    let forms_file = references_dir.join("FORMS.md");
    let agents_dir = target_dir.join("agents");
    let openai_file = agents_dir.join("openai.yaml");
    let assets_dir = target_dir.join("assets");
    let logo_small_file = assets_dir.join("logo-small.svg");
    let logo_large_file = assets_dir.join("logo-large.svg");

    fs::create_dir_all(&references_dir)?;
    fs::create_dir_all(&agents_dir)?;
    fs::create_dir_all(&assets_dir)?;
    write_file(&skill_file, SKILL_MD)?;
    write_file(&reference_file, REFERENCE_MD)?;
    write_file(&forms_file, FORMS_MD)?;
    write_file(&openai_file, OPENAI_YAML)?;
    write_file(&logo_small_file, LOGO_SMALL_SVG)?;
    write_file(&logo_large_file, LOGO_LARGE_SVG)?;

    let line_count = SKILL_MD.lines().count();
    if line_count > MAX_SKILL_LINES {
        warn!(
            line_count,
            max_lines = MAX_SKILL_LINES,
            path = %skill_file.display(),
            "Generated SKILL.md exceeds recommended line count"
        );
    }

    Ok(())
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
    Ok(())
}

fn validate_skill_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(AppError::usage(format!(
            "Skill name must be between 1 and {MAX_NAME_LEN} characters."
        ))
        .into());
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return Err(AppError::usage(
            "Skill name must not start/end with '-' or contain consecutive hyphens.",
        )
        .into());
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        return Err(AppError::usage(
            "Skill name must use lowercase ASCII letters, digits, or hyphen.",
        )
        .into());
    }
    Ok(())
}

fn validate_skill_markdown(contents: &str) -> Result<()> {
    let (frontmatter, _body) = split_frontmatter(contents)
        .ok_or_else(|| AppError::internal("SKILL.md template missing valid frontmatter"))?;

    let metadata: SkillFrontmatter = serde_yaml::from_str(frontmatter)
        .map_err(|err| anyhow!("Invalid SKILL.md frontmatter: {err}"))?;
    validate_skill_name(&metadata.name)?;

    if metadata.name != SKILL_NAME {
        return Err(
            AppError::internal("SKILL.md template name must match canonical skill name").into(),
        );
    }
    if metadata.description.trim().is_empty() || metadata.description.len() > MAX_DESCRIPTION_LEN {
        return Err(AppError::internal(
            "SKILL.md template description must be non-empty and <= 1024 chars",
        )
        .into());
    }
    if metadata.compatibility.len() > MAX_COMPATIBILITY_LEN {
        return Err(
            AppError::internal("SKILL.md template compatibility must be <= 500 chars").into(),
        );
    }
    if metadata.license != SKILL_LICENSE {
        return Err(AppError::internal("SKILL.md template license must be Apache-2.0").into());
    }
    if metadata
        .metadata
        .get("author")
        .is_none_or(|value| value != SKILL_AUTHOR)
    {
        return Err(AppError::internal("SKILL.md template metadata.author mismatch").into());
    }
    if metadata
        .metadata
        .get("version")
        .is_none_or(|value| value != SKILL_VERSION)
    {
        return Err(AppError::internal("SKILL.md template metadata.version mismatch").into());
    }

    Ok(())
}

fn split_frontmatter(contents: &str) -> Option<(&str, &str)> {
    let raw = contents
        .strip_prefix("---\n")
        .or_else(|| contents.strip_prefix("---\r\n"))?;

    let (end, separator_len) = if let Some(idx) = raw.find("\n---\n") {
        (idx, "\n---\n".len())
    } else {
        let idx = raw.find("\r\n---\r\n")?;
        (idx, "\r\n---\r\n".len())
    };

    let frontmatter = &raw[..end];
    let body = &raw[end + separator_len..];
    Some((frontmatter, body))
}

#[derive(Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
    license: String,
    compatibility: String,
    metadata: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("expected valid clock")
            .as_nanos();
        std::env::temp_dir().join(format!("99problems-{prefix}-{}-{now}", std::process::id()))
    }

    fn normalize_lines(text: &str) -> String {
        text.replace("\r\n", "\n")
    }

    #[test]
    fn validates_canonical_skill_name() {
        validate_skill_name(SKILL_NAME).expect("expected canonical skill name to be valid");
    }

    #[test]
    fn validates_skill_template_frontmatter() {
        validate_skill_markdown(SKILL_MD).expect("expected valid SKILL.md template");
    }

    #[test]
    fn init_creates_expected_files() {
        let root = temp_dir("create");
        let args = SkillArgs {
            command: SkillCommand::Init(SkillInitArgs {
                path: root.clone(),
                force: false,
            }),
        };

        run(&args).expect("expected scaffold generation");

        let base = root.join(SKILL_NAME);
        assert!(base.join("SKILL.md").exists());
        assert!(base.join("references").join("REFERENCE.md").exists());
        assert!(base.join("references").join("FORMS.md").exists());
        assert!(base.join("agents").join("openai.yaml").exists());
        assert!(base.join("assets").join("logo-small.svg").exists());
        assert!(base.join("assets").join("logo-large.svg").exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn init_fails_without_force_when_target_exists() {
        let root = temp_dir("exists");
        let base = root.join(SKILL_NAME);
        fs::create_dir_all(&base).expect("expected target directory creation");

        let args = SkillArgs {
            command: SkillCommand::Init(SkillInitArgs {
                path: root.clone(),
                force: false,
            }),
        };

        let err = run(&args).expect_err("expected existing target error");
        assert!(err.to_string().contains("already exists"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn init_with_force_overwrites_generated_files_only() {
        let root = temp_dir("force");
        let base = root.join(SKILL_NAME);
        let refs = base.join("references");
        fs::create_dir_all(&refs).expect("expected references directory");
        fs::create_dir_all(base.join("agents")).expect("expected agents directory");
        fs::create_dir_all(base.join("assets")).expect("expected assets directory");
        fs::write(base.join("SKILL.md"), "old").expect("expected old skill write");
        fs::write(refs.join("REFERENCE.md"), "old").expect("expected old reference write");
        fs::write(refs.join("FORMS.md"), "old").expect("expected old forms write");
        fs::write(base.join("agents").join("openai.yaml"), "old")
            .expect("expected old openai write");
        fs::write(base.join("assets").join("logo-small.svg"), "old")
            .expect("expected old small logo write");
        fs::write(base.join("assets").join("logo-large.svg"), "old")
            .expect("expected old large logo write");
        fs::write(base.join("custom.txt"), "keep").expect("expected custom file write");

        let args = SkillArgs {
            command: SkillCommand::Init(SkillInitArgs {
                path: root.clone(),
                force: true,
            }),
        };
        run(&args).expect("expected force scaffold generation");

        assert_eq!(
            fs::read_to_string(base.join("SKILL.md")).expect("expected skill read"),
            SKILL_MD
        );
        assert_eq!(
            fs::read_to_string(base.join("agents").join("openai.yaml"))
                .expect("expected openai read"),
            OPENAI_YAML
        );
        assert_eq!(
            fs::read_to_string(base.join("assets").join("logo-small.svg"))
                .expect("expected small logo read"),
            LOGO_SMALL_SVG
        );
        assert_eq!(
            fs::read_to_string(base.join("assets").join("logo-large.svg"))
                .expect("expected large logo read"),
            LOGO_LARGE_SVG
        );
        assert_eq!(
            fs::read_to_string(base.join("custom.txt")).expect("expected custom file read"),
            "keep"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn canonical_template_files_match_generator_templates() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let skill = fs::read_to_string(root.join("templates/skills/99problems/SKILL.md"))
            .expect("expected canonical SKILL.md");
        let reference =
            fs::read_to_string(root.join("templates/skills/99problems/references/REFERENCE.md"))
                .expect("expected canonical REFERENCE.md");
        let forms =
            fs::read_to_string(root.join("templates/skills/99problems/references/FORMS.md"))
                .expect("expected canonical FORMS.md");
        let openai =
            fs::read_to_string(root.join("templates/skills/99problems/agents/openai.yaml"))
                .expect("expected canonical openai.yaml");
        let logo_small =
            fs::read_to_string(root.join("templates/skills/99problems/assets/logo-small.svg"))
                .expect("expected canonical logo-small.svg");
        let logo_large =
            fs::read_to_string(root.join("templates/skills/99problems/assets/logo-large.svg"))
                .expect("expected canonical logo-large.svg");

        assert_eq!(normalize_lines(&skill), normalize_lines(SKILL_MD));
        assert_eq!(normalize_lines(&reference), normalize_lines(REFERENCE_MD));
        assert_eq!(normalize_lines(&forms), normalize_lines(FORMS_MD));
        assert_eq!(normalize_lines(&openai), normalize_lines(OPENAI_YAML));
        assert_eq!(
            normalize_lines(&logo_small),
            normalize_lines(LOGO_SMALL_SVG)
        );
        assert_eq!(
            normalize_lines(&logo_large),
            normalize_lines(LOGO_LARGE_SVG)
        );
    }
}
