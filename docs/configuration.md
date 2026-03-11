# Configuration

`99problems` reads instance-based TOML configuration from:

- `~/.99problems`
- `./.99problems`

Minimal example:

```toml
[instances.github]
platform = "github"
repo = "owner/repo"
token = "ghp_your_token"
```

Bitbucket example:

```toml
[instances.bitbucket-cloud]
platform = "bitbucket"
deployment = "cloud"
repo = "workspace/repo_slug"
token = "app_password_or_token"
```

## Configuration behavior

- `--instance` selects an explicit configured instance
- If exactly one instance exists, it is auto-selected
- If multiple instances exist, `default_instance` is used when set

## Useful keys

- `default_instance`
- `instances.<alias>.platform`
- `instances.<alias>.repo`
- `instances.<alias>.url`
- `instances.<alias>.token`
- `instances.<alias>.type` / `instances.<alias>.type_default`
- `instances.<alias>.deployment` (Bitbucket only)
- `instances.<alias>.per_page`
- `telemetry.enabled`
- `telemetry.otlp_endpoint`
- `telemetry.exclude_targets`

See [Config Keys](reference/config-keys.md) for the full key list and env var mappings.
See [Providers](providers/index.md) for repo/project format by platform.

Keep credentials in local config files and avoid committing tokens into version control.
