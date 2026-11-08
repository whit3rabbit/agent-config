# Cline

ID: `cline` — `agent_config::by_id("cline")`

Cline v3.36+ (January 2026) supports hooks via executable scripts. Earlier versions
support prompt-level rules only.

## Hooks

### User scope (`Scope::Global`)

Not supported in v0.1. Cline exposes user-level hooks directories on some
platforms (e.g. `~/.cline/hooks/` or `~/Documents/Cline/Hooks/`), but this
crate only writes project-local hooks because the global path is
platform-specific and less stable. Calling with `Scope::Global` returns
`AgentConfigError::UnsupportedScope`.

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Primary path | `<root>/.cline/hooks/<event>` |
| Legacy drain | `<root>/.clinerules/hooks/<event>` (status and uninstall only) |
| Ledger | `<root>/.cline/hooks/.agent-config-hooks.json` (legacy: `<root>/.clinerules/hooks/.agent-config-hooks.json`) |
| Format | Executable shell scripts (chmod 0o755; shebang `#!/usr/bin/env bash` prefixed if missing) |
| Input/Output | JSON via stdin/stdout |

**Platform support:** macOS and Linux only (`AgentConfigError::UnsupportedPlatform` / `RefusalReason::UnsupportedPlatform` on Windows).

### Hook types

Eight standard hook types supported (v3.36+):

- `TaskStart` — Hook fires at task start
- `TaskResume` — Hook fires when resuming a paused task
- `TaskCancel` — Hook fires when task is cancelled
- `TaskComplete` — Hook fires when task completes successfully
- `PreToolUse` — Hook fires before tool execution
- `PostToolUse` — Hook fires after tool execution
- `UserPromptSubmit` — Hook fires when user submits a prompt
- `PreCompact` — Hook fires before message compaction

Custom event names matching `^[a-zA-Z0-9_-]+$` are also accepted as executable script names.

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

### Memory file support

- **Supported format**: Both own rules (`.cline/rules/`) and `AGENTS.md`.
- **Primary rules directory**: `<root>/.cline/rules/<name>.md` (and legacy `<root>/.clinerules/`).
- **`AGENTS.md` support**: Cline discovers and loads global `~/.agents/AGENTS.md` and project `AGENTS.md`.


### User scope (`Scope::Global`)

Not supported in v0.1. Cline reads global rules from `~/.cline/rules/` and
per-OS Cline Rules directories (e.g., `~/Documents/Cline/Rules/`). Calling with
`Scope::Global` returns `AgentConfigError::UnsupportedScope`.

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Primary file | `<root>/.cline/rules/<tag>.md` |
| Legacy fallback | `<root>/.clinerules/<tag>.md` (status and uninstall only) |
| Mechanism | One markdown file per consumer (the file is owned outright) |
| Format | Plain markdown (the body of `RulesBlock::content`, with a trailing newline) |

Uninstall removes the file and prunes empty parent directories.

> Cline still reads the legacy single-file `<root>/.clinerules` and legacy
> directory `<root>/.clinerules/*.md`. The canonical directory form
> `<root>/.cline/rules/*.md` is preferred and what this integration writes.
> Rules can include YAML frontmatter for conditional activation; consumers may
> pass that as part of `RulesBlock::content`.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::StandaloneFile` because Cline has a per-file rules
directory; each instruction is one file in that directory, with no host
include needed.

| | |
| --- | --- |
| Primary file | `<root>/.cline/rules/<name>.md` |
| Legacy drain | `<root>/.clinerules/<name>.md` (status and uninstall only) |
| Mechanism | One file per instruction — no host file needed |
| Primary ledger | `<root>/.cline/rules/.agent-config-instructions.json` |
| Legacy ledger | `<root>/.clinerules/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::StandaloneFile` |

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.cline/data/settings/cline_mcp_settings.json` |
| Ledger | `~/.cline/data/settings/.agent-config-mcp.json` |
| Format | JSON |
| Key | `mcpServers` |
| Env override | `CLINE_DATA_DIR` (→ `<dir>/settings/...`) or `CLINE_DIR` (→ `<dir>/data/settings/...`) |

This is the standalone Cline CLI settings location, documented at
[docs.cline.bot/getting-started/config](https://docs.cline.bot/getting-started/config)
and [docs.cline.bot/mcp/mcp-overview](https://docs.cline.bot/mcp/mcp-overview).
It is deliberately distinct from the VS Code extension's globalStorage path
(`.../globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json`);
this crate targets the CLI. The legacy extension path is still resolvable via
`paths::legacy_cline_mcp_global_file()` for status checks.

### Project scope (`Scope::Local(<root>)`)

Not supported. Cline's global MCP settings file is shared across the CLI, IDE,
and SDK. Calling with `Scope::Local` returns `AgentConfigError::UnsupportedScope`.

### Configuration

```json
{
  "mcpServers": {
    "my-server": {
      "command": "npx",
      "args": ["-y", "@example/server"]
    }
  }
}
```

## Skills

| Scope | Path | Ledger |
| --- | --- | --- |
| User | `~/.agents/skills/<name>/` | `~/.agents/skills/.agent-config-skills.json` |
| Project | `.agents/skills/<name>/` | `<root>/.agents/skills/.agent-config-skills.json` |
| Legacy drain | `~/.cline/skills/<name>/`, `.cline/skills/<name>/` | `.agent-config-skills.json` in respective dir |

Each skill is a directory containing `SKILL.md`. Cline natively loads the
standard Agent Skills locations (`.agents/skills`, `~/.agents/skills`) since
cline/cline#9074, so `agent-config` writes those. `.cline/skills/` still loads
but is legacy; installs there from older releases are still detected and
removed. Note `~/.agents/skills` is shared with Codex, Cursor, Pi, Copilot,
and other standard-conforming agents — the per-root ownership ledger keeps
multi-consumer installs safe.

## References

- <https://docs.cline.bot/getting-started/config>
- <https://docs.cline.bot/customization/cline-rules>
- <https://docs.cline.bot/customization/hooks>
- <https://docs.cline.bot/customization/skills>
- <https://docs.cline.bot/mcp/mcp-overview>
- <https://docs.cline.bot/sdk/plugins>
- <https://docs.cline.bot/usage/cli-overview>
- <https://cline.bot/blog/cline-v3-36-hooks>
- <https://github.com/cline/cline/pull/9074>

Accessed: 2026-11-08.
