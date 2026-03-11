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

See the [README configuration section](https://github.com/mbe24/99problems/blob/main/README.md#configuration) for the complete configuration reference.

## Configuration behavior

- `--instance` selects an explicit configured instance
- If exactly one instance exists, it is auto-selected
- If multiple instances exist, `default_instance` is used when set

Keep credentials in local config files and avoid committing tokens into version control.
