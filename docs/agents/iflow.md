# iFlow CLI

ID: `iflow` — `agent_config::by_id("iflow")`

iFlow combines hooks and MCP servers in a single `settings.json`.

## Surfaces

| Surface | Scope          | Notes                                  |
| ------- | -------------- | -------------------------------------- |
| Hooks   | Global + Local | Claude-shape JSON envelope             |
| Prompt  | -              | Not part of iFlow's documented surface |
| MCP     | Global + Local | `mcpServers` JSON map (same `settings.json`) |
| Skills  | -              | Not part of iFlow's documented surface |

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

iFlow exposes `IFLOW_TOOL_NAME` and `IFLOW_TOOL_ARGS` env vars to hook
processes.

### Event mapping

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

Per-server `includeTools` / `excludeTools` arrays are supported by iFlow but
are caller-provided via the standard `McpSpec` plus harness defaults; this
crate does not synthesise either.

Ownership is recorded in a sidecar `<config-dir>/.agent-config-mcp.json` ledger
so MCP and hooks coexist in the same file.

## References

- <https://platform.iflow.cn/en/cli/configuration/settings>
- <https://platform.iflow.cn/en/cli/examples/hooks>

Accessed: 2026-04-26.
