# Tabnine CLI

ID: `tabnine` — `agent_config::by_id("tabnine")`

Tabnine packs hooks and MCP servers into a single `settings.json`.

## Surfaces

| Surface | Scope          | Notes                                  |
| ------- | -------------- | -------------------------------------- |
| Hooks   | Global + Local | Tabnine event names (`BeforeTool`, etc.) |
| Prompt  | -              | No dedicated rules markdown file (guidelines managed remotely/via MCP) |
| MCP     | Global + Local | `mcpServers` JSON map (same `settings.json`) |
| Skills  | -              | Tabnine uses `skills.enabled/disabled` arrays, not directories |

## Hooks

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.tabnine/agent/settings.json` |
| Mechanism | JSON patch |
| Backup | `~/.tabnine/agent/settings.json.bak` (first patch only) |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.tabnine/agent/settings.json` |
| Mechanism | JSON patch |
| Backup | `<root>/.tabnine/agent/settings.json.bak` (first patch only) |

### Format

```json
{
  "hooks": {
    "BeforeTool": [
      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "myapp hook tabnine" }],
        "_agent_config_tag": "myapp"
      }
    ]
  }
}
```

### Event mapping

| `Event::*`     | Tabnine string |
| -------------- | -------------- |
| `PreToolUse`   | `BeforeTool`   |
| `PostToolUse`  | `AfterTool`    |
| `Custom(s)`    | `s`            |

Tabnine documents 11 events: `BeforeTool`, `AfterTool`, `BeforeAgent`,
`AfterAgent`, `Notification`, `SessionStart`, `SessionEnd`, `PreCompress`,
`BeforeModel`, `AfterModel`, `BeforeToolSelection`. Use `Event::Custom`
for events beyond `PreToolUse` / `PostToolUse`.

### Matcher mapping

| `Matcher::*`        | Tabnine string |
| ------------------- | -------------- |
| `All`               | `*`            |
| `Bash`              | `Bash`         |
| `Exact(s)`          | `s`            |
| `AnyOf([a, b])`     | `a\|b`         |
| `Regex(s)`          | `s` (verbatim) |

## Prompt instructions

### Memory file support

- **Supported format**: Neither / Remote MCP only.
- **Native memory**: Tabnine does not read standalone local markdown memory files (`AGENTS.md` or proprietary files).
- **Guideline support**: Guidelines and agent steering are configured through the Tabnine web console or dynamically injected via Tabnine's Coaching Guidelines MCP server.

## MCP servers

| Scope | File |
| --- | --- |
| Global | `~/.tabnine/agent/settings.json` |
| Local | `<root>/.tabnine/agent/settings.json` |

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

Tabnine also recognises top-level `mcp.allowed` and `mcp.excluded` arrays.
This crate does not synthesise either; callers provide them via direct
config edits if needed.

Ownership is recorded in a sidecar `<config-dir>/.agent-config-mcp.json` ledger.

## References

- <https://docs.tabnine.com/main/getting-started/tabnine-cli>
- <https://docs.tabnine.com/main/getting-started/tabnine-cli/features/settings>
- <https://docs.tabnine.com/main/getting-started/tabnine-cli/features/settings/settings-reference>
- <https://docs.tabnine.com/main/getting-started/tabnine-cli/features/hooks>
- <https://docs.tabnine.com/main/getting-started/tabnine-cli/features/hooks/configuration>
- <https://docs.tabnine.com/main/getting-started/tabnine-cli/features/hooks/event-reference>
- <https://docs.tabnine.com/main/getting-started/tabnine-agent/mcp-intro-and-setup/mcp-server-config>
- <https://docs.tabnine.com/main/getting-started/tabnine-cli/features/agent-guidelines-cli>

Accessed: 2026-11-08.
