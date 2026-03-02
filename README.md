# 99problems

[![npm](https://img.shields.io/npm/v/@mbe24/99problems?color=7C3AED&label=npm)](https://www.npmjs.com/package/@mbe24/99problems)
[![crates.io](https://img.shields.io/crates/v/problems99?color=7C3AED&label=crates.io)](https://crates.io/crates/problems99)
![platforms](https://img.shields.io/badge/platforms-win%20%7C%20linux%20%7C%20macos-7C3AED)
[![License Info](http://img.shields.io/badge/license-Apache%20License%20v2.0-orange.svg)](https://raw.githubusercontent.com/mbe24/99problems/main/LICENSE)

> AI-friendly access to GitHub issues.

Fetch GitHub issue conversations as structured JSON or YAML — ready for LLM pipelines, RAG, and bulk analysis. Uses the same search syntax as the GitHub web UI.



## Installation

```bash
npm install -g @mbe24/99problems
```

Or via cargo:

```bash
cargo install problems99
```

Pre-built binaries are available for Windows x64, Linux x64, Linux ARM64, macOS Intel, and macOS Apple Silicon. No runtime dependencies.

## Usage

```bash
# Fetch all closed issues mentioning "Event" from a repo
99problems -q "is:issue state:closed Event repo:schemaorg/schemaorg" -o output.json

# Fetch a single issue
99problems --repo schemaorg/schemaorg --issue 1842

# YAML output
99problems -q "state:open label:bug repo:owner/repo" --format yaml

# Pipe into jq
99problems -q "state:closed repo:owner/repo" | jq '.[].title'
```

## Output

Each result is a conversation object with the issue body and all comments:

```json
[
  {
    "id": 1842,
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
| `~/.99problems` | Global defaults (token, preferred repo, etc.) |
| `./.99problems` | Per-project overrides |

Example `~/.99problems`:

```toml
token = "ghp_your_personal_access_token"
```

Example `./.99problems` in a project directory:

```toml
repo  = "owner/my-repo"
state = "closed"
```

Token is resolved in this order: `--token` flag → `GITHUB_TOKEN` env var → `./.99problems` → `~/.99problems`. Without a token the GitHub API rate limit is 60 requests/hour; with one it's 5,000/hour.

## Options

```
Options:
  -q, --query <QUERY>      Full GitHub search query (web UI syntax)
      --repo <REPO>        Shorthand for "repo:owner/name"
      --state <STATE>      Shorthand for "state:open|closed"
      --labels <LABELS>    Comma-separated labels, e.g. "bug,help wanted"
      --issue <ISSUE>      Fetch a single issue by number (requires --repo)
      --source <SOURCE>    Data source [default: github-issues]
      --format <FORMAT>    Output format: json | yaml [default: json]
  -o, --output <FILE>      Write to file instead of stdout
      --token <TOKEN>      GitHub personal access token
  -h, --help               Print help
  -V, --version            Print version
```

## Use cases

- **LLM context / RAG** — load issue history into a vector store or prompt
- **Issue triage** — process closed issues in bulk with Python or JavaScript
- **Dataset generation** — build labelled datasets from GitHub discussions
- **Changelog automation** — extract closed issues for release notes

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

See [LICENSE](LICENSE).

