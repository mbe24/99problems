# Query Semantics by Platform

Search mode uses platform-native query behavior plus shorthand flags.

## Shared Shorthand

These flags contribute to search queries where supported:

- `--repo`
- `--state`
- `--labels`
- `--author`
- `--since`
- `--milestone`

## Platform Notes

### GitHub

- Typical qualifiers: `repo:`, `is:issue|is:pr`, `state:`.
- Repo scope format: `owner/repo`.

### GitLab

- Repo scope is required for search.
- Repo format: `group/project`.
- `is:issue|is:pr` controls endpoint selection.

### Jira

- Supports issues only.
- `repo` means Jira project scope in search.
- `text ~` search is used for free text terms.

### Bitbucket Cloud/Data Center

- Supports PRs only.
- Cloud repo format: `workspace/repo_slug`.
- Data Center repo format: `project/repo_slug`.
- Deployment selection is required for Bitbucket.
