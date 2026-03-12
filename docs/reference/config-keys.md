# Config Key Reference

## File Locations

- `~/.99problems`
- `./.99problems`

Resolution order merges home + local, then CLI/env overrides.

## Top-Level Keys

- `default_instance`
- `telemetry.enabled`
- `telemetry.otlp_endpoint`
- `telemetry.exclude_targets`

## Instance Keys

Use `instances.<alias>.<field>`:

- `platform` (`github|gitlab|jira|bitbucket`)
- `url`
- `token`
- `account_email` (Jira)
- `repo`
- `state`
- `type`
- `type_default`
- `deployment` (Bitbucket: `cloud|selfhosted`)
- `per_page`

## Environment Variables

- `TOKEN_GITHUB` (legacy fallback: `GITHUB_TOKEN`)
- `TOKEN_GITLAB` (legacy fallback: `GITLAB_TOKEN`)
- `TOKEN_JIRA` (legacy fallback: `JIRA_TOKEN`)
- `JIRA_ACCOUNT_EMAIL`
- `TOKEN_BITBUCKET` (legacy fallback: `BITBUCKET_TOKEN`)

## Selection Behavior

1. Explicit `--instance`
2. Single configured instance (auto)
3. `default_instance`

If multiple instances exist and no default is set, `--instance` is required.
