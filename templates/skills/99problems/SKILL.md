---
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
