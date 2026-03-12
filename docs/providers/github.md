# GitHub Provider

## Capability

- Supports issue and pull request search/fetch.
- Supports issue comments, PR review comments, and links metadata.

## Required Values

- `repo` format: `owner/repo`
- Token env var: `TOKEN_GITHUB` (legacy fallback: `GITHUB_TOKEN`)

## Examples

Search issues:

```bash
99problems get --platform github -q "repo:owner/repo is:issue state:open"
```

Fetch PR by ID with review comments:

```bash
99problems get --platform github --repo owner/repo --id 2402 --type pr --include-review-comments
```

## Notes

- `--url` is optional and usually not needed.
- In `--id` mode, query-only flags like `--state`, `--labels`, `--author` are ignored.
