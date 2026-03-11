# Output and Payload Controls

`get` supports independent output format and output mode controls.

## Output Format

- `text`
- `json`
- `yaml`
- `jsonl`
- `ndjson` (alias of `jsonl`)

## Output Mode

- `auto`
- `batch`
- `stream`

`--stream` is shorthand for `--output-mode stream`.

## Defaults

- TTY stdout: `text` + stream behavior
- Piped/file output: `jsonl` + stream behavior

Use batch mode when you want all-or-nothing output writing at command end.

## Payload Flags

- `--no-comments` skip comments
- `--include-review-comments` include PR inline review comments
- `--no-links` skip relationship metadata
- `--no-body` skip body text (currently intended for Jira usage)
