# 99problems

[![CI](https://github.com/mbe24/99problems/actions/workflows/ci.yml/badge.svg)](https://github.com/mbe24/99problems/actions/workflows/ci.yml)
[![npm](https://img.shields.io/npm/v/@mbe24/99problems?color=7C3AED&label=npm)](https://www.npmjs.com/package/@mbe24/99problems)
[![crates.io](https://img.shields.io/crates/v/problems99?color=7C3AED&label=crates.io)](https://crates.io/crates/problems99)
![platforms](https://img.shields.io/badge/platforms-win%20%7C%20linux%20%7C%20macos-7C3AED)
[![License Info](http://img.shields.io/badge/license-Apache%20License%20v2.0-orange.svg)](https://raw.githubusercontent.com/mbe24/99problems/main/LICENSE)

> AI-friendly access to GitHub issues.

`99problems` is a command-line tool that fetches GitHub issue conversations — including all comments — and exports them as structured **JSON** or **YAML**. It uses the same search syntax as the GitHub web UI, making it easy to export, analyse, or feed GitHub issues into LLM pipelines, RAG systems, vector stores, and data analysis workflows.

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
99problems -q "is:issue state:closed Event repo:schemaorg/schemaorg" -o output.json

# Fetch a single issue with all its comments
99problems --repo schemaorg/schemaorg --issue 1842

# Export open bug issues as YAML
99problems -q "state:open label:bug repo:owner/repo" --format yaml

# Pipe into jq for further processing
99problems -q "state:closed repo:owner/repo" | jq '.[].title'
```

## Output

Each result is a conversation object containing the issue body and all comments:

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
[github]
token = "ghp_your_personal_access_token"
```

Example `./.99problems` in a project directory:

```toml
repo  = "owner/my-repo"
state = "closed"
```

For self-hosted GitLab:

```toml
platform = "gitlab"

[gitlab]
token = "glpat_your_token"
url   = "https://gitlab.mycompany.com"
```

Token is resolved in this order: `--token` flag → `GITHUB_TOKEN`/`GITLAB_TOKEN`/`BITBUCKET_TOKEN` env var → `./.99problems` → `~/.99problems`. Without a token the GitHub API rate limit is 60 requests/hour; with one it's 5,000/hour.

## Options

```
Options:
  -q, --query <QUERY>        Full search query (platform web UI syntax)
      --repo <REPO>          Shorthand for "repo:owner/name"
      --state <STATE>        Shorthand for "state:open|closed"
      --labels <LABELS>      Comma-separated labels, e.g. "bug,help wanted"
      --author <AUTHOR>      Filter by issue/PR author
      --since <DATE>         Only items created on or after YYYY-MM-DD
      --milestone <NAME>     Filter by milestone title or number
      --issue <ISSUE>        Fetch a single issue by number (requires --repo)
      --platform <PLATFORM>  Platform: github | gitlab | bitbucket [default: github]
      --type <TYPE>          Content type: issue | pr [default: issue]
      --format <FORMAT>      Output format: json | yaml [default: json]
  -o, --output <FILE>        Write to file instead of stdout
      --token <TOKEN>        Personal access token
  -h, --help                 Print help
  -V, --version              Print version
```

## Use cases

- **LLM context / RAG** — export issue history into a vector store or use as prompt context
- **Issue triage and analysis** — bulk-process GitHub issues with Python, JavaScript, or any data tool
- **Training data generation** — build labelled datasets from GitHub discussions and bug reports
- **Changelog and release notes** — extract closed issues for automated release documentation
- **Knowledge base indexing** — crawl project issue trackers for search and retrieval systems

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

See [LICENSE](LICENSE).
