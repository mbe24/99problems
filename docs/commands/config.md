# `config` Command

Inspect and edit `.99problems` configuration values.

## Subcommands

### `config path`

Print config file path for a scope.

```bash
99problems config path --scope local
99problems config path --scope home
```

### `config list`

List keys in `home`, `local`, or `resolved` scope.

```bash
99problems config list --scope resolved
99problems config list --scope local --show-secrets
```

### `config get`

Read one key by path.

```bash
99problems config get default_instance
99problems config get instances.work.repo --scope resolved
```

### `config set`

Set one key in `home` or `local` scope.

```bash
99problems config set default_instance work-gitlab
99problems config set instances.work.platform gitlab
```

### `config unset`

Remove one key from `home` or `local` scope.

```bash
99problems config unset instances.work.token
```

## Key Paths

- Top-level:
  - `default_instance`
  - `telemetry.enabled`
  - `telemetry.otlp_endpoint`
  - `telemetry.exclude_targets`
- Instance keys:
  - `instances.<alias>.platform`
  - `instances.<alias>.url`
  - `instances.<alias>.token`
  - `instances.<alias>.account_email`
  - `instances.<alias>.repo`
  - `instances.<alias>.state`
  - `instances.<alias>.type`
  - `instances.<alias>.type_default`
  - `instances.<alias>.deployment`
  - `instances.<alias>.per_page`

See [Config Key Reference](../reference/config-keys.md).
