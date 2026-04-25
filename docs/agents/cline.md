# Cline

ID: `cline` — `ai_hooker::by_id("cline")`

Cline v3.36+ (January 2026) supports hooks via executable scripts. Earlier versions
support prompt-level rules only.

## Hooks

### User scope (`Scope::Global`)

| | |
| --- | --- |
| Path | `~/Documents/Cline/Hooks/` (macOS/Linux) |
| Format | Executable shell scripts (no extension on Unix) |
| Input/Output | JSON via stdin/stdout |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Path | `.clinerules/hooks/` |
| Format | Executable shell scripts |
| Input/Output | JSON via stdin/stdout |

**Platform support:** macOS and Linux only (no Windows hooks).

### Hook types

Eight hook types supported (v3.36+):

- `TaskStart` — Hook fires at task start
- `TaskResume` — Hook fires when resuming a paused task
- `TaskCancel` — Hook fires when task is cancelled
- `TaskComplete` — Hook fires when task completes successfully
- `PreToolUse` — Hook fires before tool execution
- `PostToolUse` — Hook fires after tool execution
- `UserPromptSubmit` — Hook fires when user submits a prompt
- `PreCompact` — Hook fires before message compaction

### Return fields

Hooks may return JSON with:

```json
{
  "cancel": false,
  "contextModification": "additional context for the task",
  "errorMessage": "optional error message"
}
```

### Example hook script

```bash
#!/bin/bash
# Script receives JSON on stdin
read -r task_json

# Process and output response
echo '{"cancel": false}'
```

## Prompt instructions

### User scope (`Scope::Global`)

Not supported in v0.1. Cline reads global rules from a per-OS Cline Rules
directory (e.g., `~/Documents/Cline/Rules/`). Calling with `Scope::Global`
returns `HookerError::UnsupportedScope`.

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.clinerules/<tag>.md` |
| Mechanism | One markdown file per consumer (the file is owned outright) |
| Format | Plain markdown (the body of `RulesBlock::content`, with a trailing newline) |

Uninstall removes the file and prunes empty parent directories.

> Cline still reads the legacy single-file `<root>/.clinerules`. The directory
> form `<root>/.clinerules/*.md` is preferred and what this integration
> writes. Rules can include YAML frontmatter for conditional activation;
> consumers may pass that as part of `RulesBlock::content`.

## MCP servers — Not supported

Cline does not support MCP servers. Planned for Phase 2.

## Skills — Not supported

Cline does not support skills. Planned for Phase 3.

## References

- <https://docs.cline.bot/customization/cline-rules>
- <https://docs.cline.bot/customization/hooks>
- <https://cline.bot/blog/cline-v3-36-hooks
