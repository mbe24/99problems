# Bitbucket Data Center Provider

## Capability

- Supports pull requests only.
- Supports comments/review comments and links metadata.

## Required Values

- `platform = "bitbucket"`
- `deployment = "selfhosted"`
- `url` must be provided.
- `repo` format: `project/repo_slug`
- Token env var: `BITBUCKET_TOKEN`

## Examples

Search pull requests:

```bash
99problems get --platform bitbucket --deployment selfhosted --url https://bitbucket.example.com -q "repo:TEST/base is:pr"
```

Fetch PR by ID:

```bash
99problems get --platform bitbucket --deployment selfhosted --url https://bitbucket.example.com --repo TEST/base --id 33 --type pr
```

## Notes

- Personal repositories can appear under user namespaces in clone URLs, but `99problems` expects `project/repo_slug` for Data Center requests.
- Configure deployment per instance to avoid repeating flags.
