# Getting Started

Install `99problems`:

```bash
npm install -g @mbe24/99problems
# or
cargo install problems99
```

## Manual Usage

Fetch a GitHub issue:

```bash
99problems get --repo schemaorg/schemaorg --id 1842
```

Search GitLab issues:

```bash
99problems get --platform gitlab -q "repo:veloren/veloren is:issue state:closed terrain"
```

Fetch Jira issue by key:

```bash
99problems get --platform jira --id CPQ-19831 --type issue
```

Fetch Bitbucket Cloud PR by ID:

```bash
99problems get --platform bitbucket --deployment cloud --repo workspace/repo_slug --id 12 --type pr
```

Manual next steps:

- Configure a default instance in `~/.99problems` for fewer CLI flags
- Use `--format jsonl --output-mode stream` for automation pipelines
- Review all providers in [Providers](providers/index.md)
- Review every command/subcommand in [Commands](commands/index.md)

## Agentic Usage

Initialize the canonical skill:

```bash
99problems skill init
```

Agents can use the skill automatically when relevant, or explicitly via `$99problems`.

Example:

```text
$99problems find related issues and PRs for topic "architectural redesign"
```

For task patterns and examples, see [Agentic Workflow](agentic-workflow.md).
