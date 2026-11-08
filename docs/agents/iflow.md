# iFlow CLI

ID: `iflow` — `agent_config::by_id("iflow")`

iFlow combines hooks and MCP servers in a single `settings.json`.

> **Note**: Upstream iFlow CLI announced a shutdown scheduled for April 17, 2026,
> with migration to Qoder (`qodercli`).

## Surfaces

| Surface | Scope          | Notes                                                          |
| ------- | -------------- | -------------------------------------------------------------- |
| Hooks   | Global + Local | Claude-shape JSON envelope                                     |
| Prompt  | -              | Not implemented in this crate (upstream uses `IFLOW.md`)       |
| MCP     | Global + Local | `mcpServers` JSON map (same `settings.json`)                   |
| Skills  | -              | Not implemented in this crate (upstream uses `.iflow/skills/`) |

## Hooks

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.iflow/settings.json` |
| Mechanism | JSON patch |
| Backup | `~/.iflow/settings.json.bak` (first patch only) |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.iflow/settings.json` |
| Mechanism | JSON patch |
| Backup | `<root>/.iflow/settings.json.bak` (first patch only) |

### Format

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "myapp hook iflow" }],
        "_agent_config_tag": "myapp"
      }
    ]
  }
}
```

iFlow exposes special environment variables to hook processes:
- Common variables: `IFLOW_SESSION_ID`, `IFLOW_TRANSCRIPT_PATH`, `IFLOW_CWD`, `IFLOW_HOOK_EVENT_NAME`.
- Tool-specific variables (PreToolUse/PostToolUse): `IFLOW_TOOL_NAME`, `IFLOW_TOOL_ARGS`, `IFLOW_TOOL_ALIASES`.

### Event mapping

iFlow CLI natively defines 9 lifecycle events: `PreToolUse`, `PostToolUse`,
`SetUpEnvironment`, `Stop`, `SubagentStop`, `SessionStart`, `SessionEnd`,
`UserPromptSubmit`, and `Notification`. This crate maps `PreToolUse`,
`PostToolUse`, and arbitrary custom event names:

| `Event::*`     | iFlow string  |
| -------------- | ------------- |
| `PreToolUse`   | `PreToolUse`  |
| `PostToolUse`  | `PostToolUse` |
| `Custom(s)`    | `s`           |

### Matcher mapping

| `Matcher::*`        | iFlow string |
| ------------------- | ------------ |
| `All`               | `*`          |
| `Bash`              | `Bash`       |
| `Exact(s)`          | `s`          |
| `AnyOf([a, b])`     | `a\|b`       |
| `Regex(s)`          | `s` (verbatim) |

## Prompt instructions

### Memory file support

- **Supported format**: Both own file (`IFLOW.md`) and `AGENTS.md`.
- **Primary memory file**: `~/.iflow/IFLOW.md` (global) and `<root>/IFLOW.md` (local).
- **`AGENTS.md` support**: Upstream iFlow CLI supports hierarchical `IFLOW.md` memory files and also parses project-root `AGENTS.md` if present. (Note: `agent-config` currently implements Hooks and MCP surfaces only for iFlow).

## MCP servers

| Scope | File |
| --- | --- |
| Global | `~/.iflow/settings.json` |
| Local | `<root>/.iflow/settings.json` |

```json
{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@example/server"]
    }
  }
}
```

While iFlow also recognizes standalone MCP files (`~/.iflow/mcp/config.json`
and `<root>/.iflow/mcp.json`), `agent-config` uses the unified `settings.json`
configuration file documented under Method 5 in upstream MCP documentation.

Per-server `includeTools` / `excludeTools` arrays are supported by iFlow but
are caller-provided via the standard `McpSpec` plus harness defaults; this
crate does not synthesise either.

Ownership is recorded in a sidecar `<config-dir>/.agent-config-mcp.json` ledger
so MCP and hooks coexist in the same file.

## References

- <https://platform.iflow.cn/en/cli/configuration/settings>
- <https://platform.iflow.cn/en/cli/examples/hooks>
- <https://platform.iflow.cn/en/cli/examples/mcp>
- <https://platform.iflow.cn/en/cli/configuration/iflow>
- <https://platform.iflow.cn/en/cli/examples/skill>

Accessed: 2026-11-08.
