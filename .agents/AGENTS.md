# Agent Policy

This document defines execution rules for automation agents contributing to this repository.
Wording is intentional and consistent with [CONTRIBUTING.md](../CONTRIBUTING.md) and existing CI guidance.

## Rust Post-Change Commands

After any Rust source change, run exactly the following two commands before opening or updating a pull request:

```bash
cargo fmt
cargo clippy --all-targets --no-deps --features telemetry-otel -- -W clippy::pedantic
```

No additional Rust commands are required in this iteration.

## Commit Message Requirement

For every completed task with repo changes, include a Conventional Commit message draft with a scope (`type(scope): summary`). This includes interactive sessions and autonomous agents that are allowed to create commits directly.

## Docs Sync Requirements

When a change affects observable behavior, CLI usage, or workflow, update the following in the same pull request:

1. `README.md` — keep usage examples and command reference current.
2. `docs/` — update any affected content pages (getting-started, configuration, etc.).
3. `templates/skills/99problems/SKILL.md` — update when skill usage or workflow steps change.

If a docs page does not exist yet for new behavior, create it under `docs/` and register it in `mkdocs.yml`.

## Scope Boundaries

- This policy covers only agent-driven contributions; human contributors follow [CONTRIBUTING.md](../CONTRIBUTING.md).
- No runtime code changes may be introduced solely by this policy file.
- No new CI jobs or workflow changes are implied by this document.
- Do not expand the mandatory Rust command list without a separate policy revision.
