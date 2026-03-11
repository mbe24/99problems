# `man` Command

Generate man pages for `99problems` and its subcommands.

## Usage

Print root page to stdout:

```bash
99problems man
```

Write all generated pages to a directory:

```bash
99problems man --output docs/man --section 1
```

## Options

- `-o, --output <DIR>`: output directory (if omitted, print root page)
- `--section <N>`: man section number (default `1`)
