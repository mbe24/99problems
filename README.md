# 99problems

[![CI](https://github.com/mbe24/99problems/actions/workflows/ci.yml/badge.svg)](https://github.com/mbe24/99problems/actions/workflows/ci.yml)
[![Docs](https://readthedocs.org/projects/99problems/badge/?version=latest)](https://99problems.readthedocs.io/en/latest/)
[![npm](https://img.shields.io/npm/v/@mbe24/99problems?color=7C3AED&label=npm)](https://www.npmjs.com/package/@mbe24/99problems)
[![crates.io](https://img.shields.io/crates/v/problems99?color=7C3AED&label=crates.io)](https://crates.io/crates/problems99)
![platforms](https://img.shields.io/badge/platforms-win%20%7C%20linux%20%7C%20macos-7C3AED)
[![License Info](http://img.shields.io/badge/license-Apache%20License%20v2.0-orange.svg)](https://raw.githubusercontent.com/mbe24/99problems/main/LICENSE)

`99problems` is an AI-native CLI tool for issue and pull-request context retrieval across GitHub, GitLab, Jira, and Bitbucket.
It supports structured output for AI agents and direct human usage, with machine-readable formats (`json`, `yaml`, `jsonl`/`ndjson`) and a human-readable `text` format.

## Why This Tool

Software tasks often depend on decisions made in earlier issues and pull requests.
`99problems` helps recover that history in a consistent shape so current work can be grounded in prior context.

This is useful for Agentic Engineering workflows and for humans doing direct investigation in terminals or scripts.

## Installation

Install the CLI:

```bash
npm install -g @mbe24/99problems
# or
cargo install problems99
```

Initialize the canonical `99problems` Agent Skill scaffold:

```bash
# initialize the canonical 99problems skill in the current project
99problems skill init
# or use the global skill location (shared across projects)
99problems skill init --path ~/.agents/skills
```

## Quick Start

There are two primary ways to use `99problems`. Refer to Agentic Use for AI-assisted workflows, or Manual Use for direct CLI usage.

### Agentic Use

Now that the skill is installed, start your agent session.
Use it to retrieve cross-system context for concrete engineering tasks like topic mapping, bug triage, and progress estimation.

To map work related to a topic across issues and PRs (with explicit skill invocation):

```text
llm-prompt> Use $99problems find related issues and PRs for topic "architectural redesign"
```

To produce a bug-focused status overview for a repository (with implicit skill invocation):

```text
llm-prompt> Create an overview of open bugs and cross-reference them with active PRs in owner/repo.
```

To estimate delivery progress from linked tracker and PR state:

```text
llm-prompt> Estimate progress for topic "build modernization" based on linked issues and PR states.
```

### Manual Use

Use this when you run `99problems` directly in a terminal to fetch context from specific providers.

```bash
# Fetch one GitHub issue
99problems get --repo schemaorg/schemaorg --id 1842

# Fetch one PR with inline review comments
99problems get --repo github/gitignore --id 2402 --type pr --include-review-comments

# Search GitLab issues
99problems get --platform gitlab -q repo:veloren/veloren is:issue state:closed terrain

# Fetch Jira issue by key
99problems get jira --id CLOUD-12817

# Fetch Bitbucket Cloud PR by ID
99problems get --platform bitbucket --deployment cloud --repo workspace/repo_slug --id 1 --type pr

# Fetch Bitbucket Data Center PR by ID
99problems get --platform bitbucket --deployment selfhosted --url https://bitbucket.mycompany.com --repo PROJECT/repo_slug --id 1

# Stream as JSON Lines for pipelines
99problems get -q repo:github/gitignore is:issue state:open --output-mode stream --format jsonl
```

## Commands

```text
99problems get [INSTANCE] [OPTIONS]    Fetch issue and pull request conversations
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
repo = "project"
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
repo = "project/repo_slug"
token = "pat_or_bearer_token"
```

Bitbucket support is pull-request only; when `--type` is omitted, `99problems` defaults to PRs.
For Bitbucket Cloud, use an app-password, repository access token, or workspace-level access token (premium feature) in `token`.

Selection order: positional `INSTANCE`/`--instance` -> single configured instance -> `default_instance`.

### Telemetry

```toml
[telemetry]
enabled = true
otlp_endpoint = "http://localhost:4318/v1/traces"
exclude_targets = ["h2", "hyper", "hyper_util", "rustls"]
```

Telemetry is best-effort and traces `99problems get` without changing normal command behavior or exit codes. For quick local OpenTelemetry backend setup, use Grafana LGTM (Docker image: [`grafana/otel-lgtm`](https://hub.docker.com/r/grafana/otel-lgtm)).

Use `telemetry.exclude_targets` to suppress noisy span-target prefixes (prefix match). Equivalent config command:

```bash
99problems config set telemetry.exclude_targets h2,hyper,hyper_util,rustls
```
Note that telemetry export requires a build with the telemetry-otel feature enabled:
- `telemetry-otel` controls whether OTEL support is compiled in.
- Default builds include it.
- Use `--no-default-features` for telemetry-free release binaries.

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

## Documentation

Project documentation is available at [99problems.readthedocs.io](https://99problems.readthedocs.io/).

Public docs source lives in [`docs/`](docs/). Internal-only notes belong in [`docs/internal/`](docs/internal/) and are excluded from the published site.

### Man Pages

Generate and print root man page:

```bash
99problems man
```

Generate all pages to disk:

```bash
99problems man --output docs/man --section 1
```

## Support this project

If 99problems saves you time, you can support ongoing maintenance via [GitHub Sponsors](https://github.com/sponsors/mbe24) or [Ko-fi](https://ko-fi.com/mbe24).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

See [LICENSE](LICENSE).

Copyright (c) 2026 Mikael Beyene
