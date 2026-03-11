# Agentic Workflow

Use this workflow when an AI agent should retrieve issue/PR context before proposing or implementing changes.

## Setup

Initialize the canonical skill scaffold:

```bash
99problems skill init
```

Optional custom location:

```bash
99problems skill init --path ~/.agents/skills
```

The generated `99problems` skill follows the Agent Skills standard.

## How Agents Use It

- Explicit skill invocation: invoke it directly with `$99problems` in your prompt.

```text
llm-prompt> $99problems summarize related Jira issues and GitHub PRs for topic "document generation redesign"
```

- Implicit skill invocation: assign a task that requires issue/PR context retrieval and let the agent activate the skill automatically.

```text
llm-prompt> Create an overview of open bugs and cross-reference them with active PRs in owner/repo.
```

## Prototypical Tasks

- Build an overview of work on topic X across issues and PRs.
- Cross-reference issues with linked PRs and summarize status.
- Create a bug overview for a repository/project (open vs closed, high-priority focus).
- Estimate progress from issue/PR states and recent updates.
- Surface blockers/blocked-by links and dependency hotspots.
- Summarize key decisions from discussion threads for handoff.

## Next References

- Skill command options: [Commands / skill](commands/skill.md)
- Provider capabilities and constraints: [Providers](providers/index.md)
- Query and output behavior: [Reference](reference/query-semantics.md)
