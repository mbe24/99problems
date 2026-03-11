# Provider Overview

`99problems` supports four source platforms:

- GitHub
- GitLab
- Jira
- Bitbucket (Cloud and Data Center deployments)

Use provider pages in this section for required repo/project formats, auth expectations, and provider-specific constraints.

## Fast Comparison

| Provider | Supported Types | `repo` Meaning | URL Required | Notes |
| --- | --- | --- | --- | --- |
| GitHub | issues + pull requests | `owner/repo` | No (default API host) | GitHub query syntax |
| GitLab | issues + merge requests | `group/project` (nested groups supported) | Optional | GitLab query syntax |
| Jira | issues only | project key/name for search | Optional | PRs not supported |
| Bitbucket Cloud | pull requests only | `workspace/repo_slug` | No (`api.bitbucket.org`) | deployment must be `cloud` |
| Bitbucket Data Center | pull requests only | `project/repo_slug` | Yes | deployment must be `selfhosted` |
