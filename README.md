# 99problems

[![CI](https://github.com/mbe24/99problems/actions/workflows/ci.yml/badge.svg)](https://github.com/mbe24/99problems/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/@mbe24/99problems?color=7C3AED&label=npm)](https://www.npmjs.com/package/@mbe24/99problems)
[![crates.io](https://img.shields.io/crates/v/problems99?color=7C3AED&label=crates.io)](https://crates.io/crates/problems99)
![platforms](https://img.shields.io/badge/platforms-win%20%7C%20linux%20%7C%20macos-7C3AED)
[![License Info](http://img.shields.io/badge/license-Apache%20License%20v2.0-orange.svg)](https://raw.githubusercontent.com/mbe24/99problems/main/LICENSE)

`99problems` fetches issue and pull request conversations (including comments) from GitHub, GitLab, Jira, and Bitbucket.
It supports machine-readable output (`json`, `yaml`, `jsonl`/`ndjson`) and a human-readable `text` format.

## Installation

```bash
npm install -g @mbe24/99problems
# or
cargo install problems99
```

## Quick Start

```bash
# Fetch one GitHub issue
99problems get --repo schemaorg/schemaorg --id 1842

# Fetch one PR with inline review comments
99problems get --repo github/gitignore --id 2402 --type pr --include-review-comments

# Search GitLab issues
99problems get --platform gitlab -q "repo:veloren/veloren is:issue state:closed terrain"

# Fetch Jira issue by key
99problems get --platform jira --id CLOUD-12817

# Fetch Bitbucket Cloud PR by ID
99problems get --platform bitbucket --deployment cloud --repo workspace/repo_slug --id 1 --type pr

# Fetch Bitbucket Data Center PR by ID
99problems get --platform bitbucket --deployment selfhosted --url https://bitbucket.mycompany.com --repo PROJECT/repo_slug --id 1

# Stream as JSON Lines for pipelines
99problems get -q "repo:github/gitignore is:issue state:open" --output-mode stream --format jsonl

# Scaffold the canonical Agent Skill
99problems skill init
```

## Commands

```text
99problems get [OPTIONS]               Fetch issue and pull request conversations
99problems skill init [OPTIONS]        Scaffold the canonical Agent Skill
99problems config <SUBCOMMAND>         Inspect and edit .99problems configuration
99problems completions <SHELL>         Generate shell completion scripts
99problems man [OPTIONS]               Generate man pages (stdout or files)
```

Global options:

```text
-v, --verbose              Increase diagnostics (-v, -vv, -vvv)
-Q, --quiet                Show errors only
    --error-format <FMT>   Error output: text|json (default: text)
-h, --help                 Print help
-V, --version              Print version
```

## Configuration

`99problems` uses instance-based TOML config from:

- `~/.99problems`
- `./.99problems`

Example:

```toml
default_instance = "work-gitlab"

[instances.github]
platform = "github"
repo = "owner/repo"
token = "ghp_your_token"

[instances.work-gitlab]
platform = "gitlab"
url = "https://gitlab.mycompany.com"
repo = "group/project"
token = "glpat_your_token"

[instances.work-jira]
platform = "jira"
url = "https://jira.mycompany.com"
repo = "CPQ"
token = "atlassian_api_token"
account_email = "user@example.com"

[instances.bitbucket-cloud]
platform = "bitbucket"
deployment = "cloud"
repo = "workspace/repo_slug"
token = "username:app_password"

[instances.bitbucket-dc]
platform = "bitbucket"
deployment = "selfhosted"
url = "https://bitbucket.mycompany.com"
repo = "PROJECT/repo_slug"
token = "pat_or_bearer_token"
```

Bitbucket support is pull-request only; when `--type` is omitted, `99problems` defaults to PRs.
For Bitbucket Cloud, use an app-password, repository access token, or workspace-level access token (premium feature) in `token`.

Selection order: `--instance` -> single configured instance -> `default_instance`.

## Man Pages

Generate and print root man page:

```bash
99problems man
```

Generate all pages to disk:

```bash
99problems man --output docs/man --section 1
```

## Output Modes

`get` supports two orthogonal controls:

- `--format`: `json`, `yaml`, `jsonl`, `ndjson` (alias of `jsonl`), `text`
- `--output-mode`: `auto`, `batch`, `stream` (or `--stream`)

Payload controls:

- `--no-comments`: skip issue/PR comments
- `--include-review-comments`: include inline review comments (for PRs)
- `--no-links`: skip linked-issue/PR metadata

Defaults:

- TTY stdout: `--format text`, `--output-mode auto` (resolved to streaming)
- piped stdout / file output: `--format jsonl`, `--output-mode auto` (resolved to streaming)

Use `--output-mode batch` when you want all-or-nothing output at the end.

## Shell Completions

```bash
99problems completions bash
99problems completions zsh
99problems completions powershell
```

## Agent Skill Scaffold

Canonical editable skill sources live in `templates/skills/99problems`.
Generate runtime skill files under `.agents/skills/99problems` with:

```bash
99problems skill init
```

Use user scope by overriding path:

```bash
99problems skill init --path ~/.agents/skills
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

See [LICENSE](LICENSE).

Copyright (c) 2026 Mikael Beyene
