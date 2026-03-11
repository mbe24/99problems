# Getting Started

Install `99problems`:

```bash
npm install -g @mbe24/99problems
# or
cargo install problems99
```

Fetch a GitHub issue:

```bash
99problems get --repo schemaorg/schemaorg --id 1842
```

Search GitLab issues:

```bash
99problems get --platform gitlab -q "repo:veloren/veloren is:issue state:closed terrain"
```
