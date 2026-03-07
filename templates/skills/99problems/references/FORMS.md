# 99problems Forms

## Query Form
- Instance:
- Platform (if not using instance):
- Repo/project:
- Type (`issue` or `pr`):
- State:
- Labels:
- Author:
- Since:
- Raw query (`-q`):

## Fetch-by-ID Form
- Instance:
- Repo/project:
- ID/key:
- Type:
- Include review comments? (yes/no):
- Include links? (yes/no):
- Include comments? (yes/no):

## Retrieved Context Snapshot
- Current task or goal:
- Related issue/PR IDs:
- Key decisions from prior work:
- Constraints or invariants to preserve:
- Open questions or unresolved risks:
- Evidence commands run:
- Evidence source IDs and links:

## Output Profile Form
- Format (`text|json|yaml|jsonl|ndjson`):
- Mode (`auto|batch|stream`):
- Output file path (optional):
- Payload reductions (`--no-comments`, `--no-links`):

## Validation Checklist
- Command exits with code 0.
- Response contains expected `id`/`title`.
- Comments/review comments presence matches flags.
- Output format parses successfully in downstream tool.
