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

const SKILL_MD: &str = r#"---
name: 99problems
description: Fetch and analyze issue and pull request conversations when you need structured engineering context from trackers and code hosts.
license: Apache-2.0
compatibility: Requires the `99problems` CLI in PATH. Install or update with npm (`npm install -g @mbe24/99problems`). Cargo installation is also supported.
metadata:
  author: Mikael Beyene
  version: "1.0"
---

# 99problems Skill

## When To Use This Skill
Use this skill when you need consistent issue or pull-request retrieval from GitHub, GitLab, Jira, or Bitbucket via the `99problems` CLI.

## Required Inputs
- Provider context via `--instance` or explicit flags (`--platform`, `--repo`, `--url`, `--deployment`)
- Search query (`-q`) or single identifier (`--id`)
- Optional token configured in `.99problems` or via environment variables

## Workflow
1. Resolve the target platform and repository/project.
2. Choose search mode (`-q`) or direct fetch mode (`--id`).
3. Select output shape (`--format`, `--output-mode`) and payload controls (`--no-comments`, `--no-links`).
4. Run `99problems get ...`.
5. Validate output and hand off to downstream tooling.

## Command Patterns
### Issue Search
```bash
99problems get --instance github -q "repo:owner/repo is:issue state:open label:bug"
```

### Pull Request Search
```bash
99problems get --instance github -q "repo:owner/repo is:pr state:open" --type pr
```

### Fetch Issue by ID
```bash
99problems get --instance github --repo owner/repo --id 1842 --type issue
```

### Fetch Pull Request by ID
```bash
99problems get --instance github --repo owner/repo --id 2402 --type pr --include-review-comments
```

### Fetch Jira Issue by Key
```bash
99problems get --instance jira-work --id CPQ-19831 --type issue
```

## Output Handling
- For machine pipelines, prefer `--format jsonl` and stream mode.
- For human inspection, use default TTY text output or `--format yaml`.
- Use `--no-links` and `--no-comments` when payload size should be minimized.

## Boundaries
- Jira supports issues only (no PRs).
- Bitbucket support is PR-only.
- This skill does not parse free-text links; use provider APIs and supported flags.

## Troubleshooting
- Authentication errors: configure token in `.99problems` or env vars.
- Empty output: verify query qualifiers (`repo:`, `is:issue`, `is:pr`, `state:`).
- Schema drift checks: regenerate man pages after CLI changes with `99problems man --output docs/man --section 1`.

## Progressive Disclosure
Keep this file concise. Move detailed recipes to `references/REFERENCE.md` and reusable forms to `references/FORMS.md`.

Optional directories for expansion:
- `scripts/` for executable helpers
- `assets/` for static templates/data
"#;

const REFERENCE_MD: &str = r#"# 99problems Reference

## Core Retrieval Modes
### Search
Use `-q` for provider query syntax:
```bash
99problems get --instance github -q "repo:owner/repo is:issue state:closed label:security"
```

### ID Fetch
Use `--id` plus explicit `--type` when ambiguity is possible:
```bash
99problems get --instance github --repo owner/repo --id 2402 --type pr
```

## Platform Notes
- GitHub: issues + PRs supported, review comments available.
- GitLab: issues + merge requests supported.
- Jira: issues only.
- Bitbucket: pull requests only.

## Output Guidance
- Streaming pipelines: `--format jsonl --output-mode stream`
- Deterministic files: `--format json --output out.json`
- Smaller payloads: `--no-comments --no-links`

## Configuration Guidance
Prefer instance-based config:
```toml
[instances.github]
platform = "github"
repo = "owner/repo"
token = "ghp_..."
```

Then call:
```bash
99problems get --instance github --id 1842
```
"#;

const FORMS_MD: &str = r"# 99problems Forms

## Query Form
- Instance:
- Platform (if not using instance):
- Repo/project:
- Type (`issue` or `pr`):
- State:
- Labels:
- Author:
- Since:
- Raw query (`-q`):

## Fetch-by-ID Form
- Instance:
- Repo/project:
- ID/key:
- Type:
- Include review comments? (yes/no):
- Include links? (yes/no):
- Include comments? (yes/no):

## Output Profile Form
- Format (`text|json|yaml|jsonl|ndjson`):
- Mode (`auto|batch|stream`):
- Output file path (optional):
- Payload reductions (`--no-comments`, `--no-links`):

## Validation Checklist
- Command exits with code 0.
- Response contains expected `id`/`title`.
- Comments/review comments presence matches flags.
- Output format parses successfully in downstream tool.
";

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

    fs::create_dir_all(&references_dir)?;
    write_file(&skill_file, SKILL_MD)?;
    write_file(&reference_file, REFERENCE_MD)?;
    write_file(&forms_file, FORMS_MD)?;

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
    let raw = contents.strip_prefix("---\n")?;
    let end = raw.find("\n---\n")?;
    let frontmatter = &raw[..end];
    let body = &raw[end + 5..];
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
        fs::write(base.join("SKILL.md"), "old").expect("expected old skill write");
        fs::write(refs.join("REFERENCE.md"), "old").expect("expected old reference write");
        fs::write(refs.join("FORMS.md"), "old").expect("expected old forms write");
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
            fs::read_to_string(base.join("custom.txt")).expect("expected custom file read"),
            "keep"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn canonical_repo_skill_files_match_generator_templates() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let skill = fs::read_to_string(root.join(".agents/skills/99problems/SKILL.md"))
            .expect("expected canonical SKILL.md");
        let reference =
            fs::read_to_string(root.join(".agents/skills/99problems/references/REFERENCE.md"))
                .expect("expected canonical REFERENCE.md");
        let forms = fs::read_to_string(root.join(".agents/skills/99problems/references/FORMS.md"))
            .expect("expected canonical FORMS.md");

        assert_eq!(normalize_lines(&skill), normalize_lines(SKILL_MD));
        assert_eq!(normalize_lines(&reference), normalize_lines(REFERENCE_MD));
        assert_eq!(normalize_lines(&forms), normalize_lines(FORMS_MD));
    }
}
