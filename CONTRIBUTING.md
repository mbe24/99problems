# Contributing to 99problems

Thank you for taking the time to contribute! 🎉

## Ways to contribute

- **Bug reports** — open an issue describing what happened and what you expected
- **Feature requests** — open an issue with the `enhancement` label
- **Pull requests** — see the workflow below

## Development setup

```bash
# Prerequisites: Rust stable (1.85+), cargo
git clone https://github.com/mbe24/99problems
cd 99problems

# Install the pre-commit hook (runs cargo fmt + clippy before each commit)
cp .githooks/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit  # not needed on Windows

# Build
cargo build

# Run unit tests
cargo test

# Run integration tests (requires a GitHub token)
export GITHUB_TOKEN=ghp_your_token
cargo test -- --include-ignored
```

## Pull request workflow

1. Fork the repo and create a branch: `git checkout -b my-feature`
2. Make your changes and add tests where appropriate
3. Run `cargo test` — all tests must pass
4. Run `cargo clippy` and `cargo fmt --check` — no new warnings
5. Open a pull request with a clear description of what and why

## Project structure

```
src/
  main.rs          # CLI entry point (clap)
  config.rs        # Dotfile config loading
  model.rs         # Shared data types (Conversation, Comment)
  source/
    mod.rs         # Source trait + Query builder
    github_issues.rs  # GitHub Issues API client
  format/
    mod.rs         # Formatter trait
    json.rs        # JSON output
    yaml.rs        # YAML output
tests/
  integration.rs   # Live API tests (#[ignore] by default)
```

## Adding a new source

1. Create `src/source/my_source.rs` implementing the `Source` trait
2. Register it in `src/source/mod.rs`
3. Add a variant to `SourceKind` in `src/main.rs`

## Adding a new output format

1. Create `src/format/my_format.rs` implementing the `Formatter` trait
2. Register it in `src/format/mod.rs`
3. Add a variant to `OutputFormat` in `src/main.rs`

## Code of conduct

Be respectful and constructive. We follow the
[Contributor Covenant](https://www.contributor-covenant.org/version/2/1/code_of_conduct/).
