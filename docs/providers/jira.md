# Jira Provider

## Capability

- Supports issue search/fetch only.
- Pull requests are not supported.
- Supports issue comments and links metadata.

## Required Values

- `repo` is Jira project scope for search (for example `CPQ`).
- Token env var: `TOKEN_JIRA` (legacy fallback: `JIRA_TOKEN`)
- Optional account email env var: `JIRA_ACCOUNT_EMAIL`

## Examples

Search issues in one project:

```bash
99problems get --platform jira --repo CPQ -q "architectural redesign" --type issue
```

Fetch issue by key:

```bash
99problems get --platform jira --id CPQ-19831 --type issue
```

## Notes

- `--no-body` is currently supported for Jira and can reduce payload size.
- For API-token basic auth, set `account_email` + token.
