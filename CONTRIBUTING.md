# Contributing to 99problems

## Development Setup

```bash
git clone https://github.com/mbe24/99problems
cd 99problems

# Optional: install local pre-commit hook (fmt + clippy)
cp .githooks/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit

cargo build
cargo test
```

Ignored integration tests (live APIs):

```bash
cargo test -- --include-ignored --skip jira_
```

## Local Quality Gates

Run these before committing:

```bash
cargo fmt
cargo clippy --all-targets --no-deps -- -D warnings
cargo clippy --all-targets --no-deps -- -W clippy::pedantic
cargo test
```

## Help and Man Pages

The CLI help and man pages are generated from the clap command model.

Regenerate man pages after CLI/help changes:

```bash
cargo run -- man --output docs/man --section 1
```

Verify no drift:

```bash
git diff -- docs/man
```

## Agent Skill Scaffold

The canonical skill lives under `.agents/skills/99problems/`.

If you change `skill init` template output, re-run:

```bash
cargo run -- skill init --force
```

## Command Module Layout

Hybrid command-module convention:

- simple commands: `src/cmd/<name>.rs`
- complex commands: `src/cmd/<name>/mod.rs` with submodules

## Pull Requests

1. Create a feature branch.
2. Keep commits focused and compile-safe.
3. Run local quality gates.
4. Open a PR with problem statement + design summary.
