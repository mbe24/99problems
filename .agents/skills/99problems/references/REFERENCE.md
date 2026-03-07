# 99problems Reference

## Core Retrieval Modes
### Search
Use `-q` for provider query syntax:
```bash
99problems get --instance github -q "repo:owner/repo is:issue state:closed label:security"
```

### ID Fetch
Use `--id` plus explicit `--type` when ambiguity is possible:
```bash
99problems get --instance github --repo owner/repo --id 2402 --type pr
```

## Platform Notes
- GitHub: issues + PRs supported, review comments available.
- GitLab: issues + merge requests supported.
- Jira: issues only.
- Bitbucket: pull requests only.

## Output Guidance
- Streaming pipelines: `--format jsonl --output-mode stream`
- Deterministic files: `--format json --output out.json`
- Smaller payloads: `--no-comments --no-links`

## Configuration Guidance
Prefer instance-based config:
```toml
[instances.github]
platform = "github"
repo = "owner/repo"
token = "ghp_..."
```

Then call:
```bash
99problems get --instance github --id 1842
```
