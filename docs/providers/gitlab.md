# GitLab Provider

## Capability

- Supports issue and merge-request search/fetch.
- Supports notes (comments), review comments for MRs, and links metadata.

## Required Values

- `repo` format: `group/project`
- Nested groups are supported (for example `group/subgroup/project`).
- Token env var: `GITLAB_TOKEN`

## Examples

Search merge requests:

```bash
99problems get --platform gitlab -q "repo:group/project is:pr state:opened"
```

Fetch issue by ID:

```bash
99problems get --platform gitlab --repo group/project --id 67 --type issue
```

## Notes

- `--url` is optional; set it for self-hosted GitLab.
- GitLab search requires a repo/project scope.
