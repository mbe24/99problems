# Bitbucket Cloud Provider

## Capability

- Supports pull requests only.
- Supports comments, review comments, and links metadata.

## Required Values

- `platform = "bitbucket"`
- `deployment = "cloud"`
- `repo` format: `workspace/repo_slug`
- Token env var: `BITBUCKET_TOKEN`

## Examples

Search pull requests:

```bash
99problems get --platform bitbucket --deployment cloud -q "repo:workspace/repo_slug is:pr state:open"
```

Fetch PR by ID:

```bash
99problems get --platform bitbucket --deployment cloud --repo workspace/repo_slug --id 12 --type pr
```

## Notes

- Issues are not supported on Bitbucket providers.
- Cloud uses `api.bitbucket.org` by default.
