# 99problems

[![CI](https://github.com/mbe24/99problems/actions/workflows/ci.yml/badge.svg)](https://github.com/mbe24/99problems/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/@mbe24/99problems?color=7C3AED&label=npm)](https://www.npmjs.com/package/@mbe24/99problems)
[![crates.io](https://img.shields.io/crates/v/problems99?color=7C3AED&label=crates.io)](https://crates.io/crates/problems99)
![platforms](https://img.shields.io/badge/platforms-win%20%7C%20linux%20%7C%20macos-7C3AED)
[![License Info](http://img.shields.io/badge/license-Apache%20License%20v2.0-orange.svg)](https://raw.githubusercontent.com/mbe24/99problems/main/LICENSE)

> AI-friendly access to issue and PR conversations.

`99problems` is a command-line tool that fetches issue and PR conversations — including all comments — and exports them as structured **JSON** or **YAML**. It uses a GitHub-style search syntax (`repo:`, `state:`, `label:`, etc.) and supports GitHub, GitLab, and Jira.

Built in Rust. Distributed as a single binary via npm and crates.io. No runtime dependencies.

## Installation

```bash
npm install -g @mbe24/99problems
```

Or via cargo:

```bash
cargo install problems99
```

Pre-built binaries are available for Windows x64, Linux x64, Linux ARM64, macOS Intel, and macOS Apple Silicon.

## Usage

```bash
# Export all closed issues mentioning "Event" to JSON
99problems get -q "is:issue state:closed Event repo:schemaorg/schemaorg" -o output.json

# Fetch a single issue/PR with all its comments
99problems get --repo schemaorg/schemaorg --id 1842

# Fetch a PR including inline review comments
99problems get --repo github/gitignore --id 2402 --include-review-comments

# Fetch a GitLab issue
99problems get --platform gitlab --repo veloren/veloren --id 6

# Fetch using a configured instance alias
99problems get --instance work-gitlab --id 46 --type pr

# Fetch a Jira issue by key
99problems get --platform jira --id CLOUD-12817

# Search Jira issues without fetching comments (faster)
99problems get --platform jira -q "repo:CPQ state:open" --no-comments

# Export open bug issues as YAML
99problems get -q "state:open label:bug repo:owner/repo" --format yaml

# Pipe into jq for further processing
99problems get -q "state:closed repo:owner/repo" | jq '.[].title'
```

## Output

Each result is a conversation object containing the issue body and all comments:

```json
[
  {
    "id": "1842",
    "title": "Event schema improvements",
    "body": "Issue body text...",
    "url": "https://github.com/schemaorg/schemaorg/issues/1842",
    "state": "closed",
    "created_at": "2019-04-01T12:00:00Z",
    "comments": [
      {
        "author": "octocat",
        "body": "Comment text...",
        "created_at": "2019-04-02T08:00:00Z"
      }
    ]
  }
]
```

## Configuration

`99problems` reads TOML dotfiles so you don't have to repeat flags on every run.

| File | Purpose |
|---|---|
| `~/.99problems` | Global instances |
| `./.99problems` | Per-project instance overrides |

The runtime config is instance-only:

```toml
default_instance = "work-gitlab"

[instances.github]
platform = "github"
repo = "owner/repo"
token = "ghp_your_personal_access_token"

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
email = "user@example.com"
```

Selection order: `--instance` → single configured instance (auto) → `default_instance`.  
If multiple instances are configured and neither `--instance` nor `default_instance` is set, the command errors.
If no instances are configured, `99problems` still works in CLI-only mode (`--platform`, `--url`, `--token`, etc.).

Token is resolved in this order: `--token` flag → `GITHUB_TOKEN`/`GITLAB_TOKEN`/`JIRA_TOKEN`/`BITBUCKET_TOKEN` env var → selected instance token in dotfile.  
For Jira Atlassian Cloud API tokens, also provide an email via `--jira-email`, `JIRA_EMAIL`, or `[instances.<alias>].email` (or pass `--token` as `email:api_token`).

Legacy sections (`[github]`, `[gitlab]`, `[jira]`, `[bitbucket]`) and top-level runtime keys (`platform`, `repo`, `state`, `type`, `per_page`) are not supported.

## Commands

```
Commands:
  get (alias: got)      Fetch issue and pull request conversations
  completions <SHELL>   Generate shell completion script and print it to stdout

Global options:
  -h, --help            Print help
  -V, --version         Print version
```

## Get options

```
99problems get [OPTIONS]

Options:
  -q, --query <QUERY>        Full search query (platform web UI syntax)
  -r, --repo <REPO>          Shorthand for "repo:owner/name" (alias: --project)
  -s, --state <STATE>        Shorthand for "state:open|closed"
  -l, --labels <LABELS>      Comma-separated labels, e.g. "bug,help wanted"
  -a, --author <AUTHOR>      Filter by issue/PR author
  -S, --since <DATE>         Only items created on or after YYYY-MM-DD
  -m, --milestone <NAME>     Filter by milestone title or number
  -i, --id <ID>              Fetch a single issue/PR by identifier
                             Alias: --issue
  -p, --platform <PLATFORM>  Platform adapter: github | gitlab | jira | bitbucket
  -I, --instance <INSTANCE>  Named instance alias from .99problems
  -u, --url <URL>            Override platform base URL for one-off runs
  -t, --type <TYPE>          Content type: issue | pr [default: issue]
                             In --id mode: defaults to issue; explicit --type disables fallback
  -R, --include-review-comments
                             Include pull request review comments (GitHub/GitLab PRs)
      --no-comments         Skip fetching comments (faster, smaller output)
  -f, --format <FORMAT>      Output format: json | yaml [default: json]
  -o, --output <FILE>        Write to file instead of stdout
  -k, --token <TOKEN>        Personal access token
      --jira-email <EMAIL>   Jira account email for API-token basic auth
  -h, --help                 Print help
  -V, --version              Print version
```

## Shell completion

Generate a completion script and source/install it in your shell.

```bash
# If installed via cargo/npm globally:
99problems completions bash

# Via cargo without installing:
cargo run -- completions powershell

# Via npm package:
npx @mbe24/99problems completions zsh
```

When installed via npm, postinstall will try to auto-install completions for
`bash`, `zsh`, and `fish` (best effort), and print a message if shell detection
or auto-install is not possible.

## Use cases

- **LLM context / RAG** — export issue history into a vector store or use as prompt context
- **Issue triage and analysis** — bulk-process issue trackers with Python, JavaScript, or any data tool
- **Training data generation** — build labelled datasets from GitHub discussions and bug reports
- **Changelog and release notes** — extract closed issues for automated release documentation
- **Knowledge base indexing** — crawl project issue trackers for search and retrieval systems

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

See [LICENSE](LICENSE).
