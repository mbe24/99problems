# `get` Command

Fetch issue or pull-request conversations from configured providers.

## Basic Forms

Search mode:

```bash
99problems get -q "repo:owner/repo is:issue state:open"
```

ID mode:

```bash
99problems get --repo owner/repo --id 1842 --type issue
```

## Core Inputs

- `-q, --query`: raw provider query string.
- `-i, --id`: fetch one issue/PR directly.
- `-r, --repo`: provider repo/project shorthand.
- `-t, --type`: `issue` or `pr`.
- `-p, --platform`: direct platform selection.
- `-I, --instance`: select configured instance alias.

## Query Shorthand Flags

- `-s, --state`
- `-l, --labels`
- `-a, --author`
- `-S, --since`
- `-m, --milestone`

These are merged into the query for search mode.

## Output Controls

- `-f, --format`: `text|json|yaml|jsonl|ndjson`
- `--output-mode`: `auto|batch|stream`
- `--stream`: shorthand for stream mode
- `-o, --output`: write to file instead of stdout

See [Output and Payload Controls](../reference/output-and-payload.md).

## Payload Controls

- `--no-comments`
- `--include-review-comments`
- `--no-links`
- `--no-body` (currently Jira-focused optimization)

## Auth and Provider Routing

- `-k, --token`
- `--account-email` (Jira API-token basic auth)
- `-u, --url`
- `--deployment` (Bitbucket only: `cloud|selfhosted`)

## ID Mode Notes

When `--id` is used, these query filters are ignored:

- `--query`
- `--state`
- `--labels`
- `--author`
- `--since`
- `--milestone`
