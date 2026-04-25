# Cursor

ID: `cursor` — `ai_hooker::by_id("cursor")`

## Hooks

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.cursor/hooks.json` |
| Mechanism | JSON patch |
| Backup | `~/.cursor/hooks.json.bak` (first patch only) |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.cursor/hooks.json` |
| Mechanism | JSON patch |
| Backup | `<root>/.cursor/hooks.json.bak` (first patch only) |

### Format

```json
{
  "version": 1,
  "hooks": {
    "preToolUse": [
      {
        "command": "myapp hook cursor",
        "matcher": "Shell",
        "_ai_hooker_tag": "myapp"
      }
    ]
  }
}
```

> Cursor requires the top-level `"version": 1`. The library ensures it.

### Event mapping

Cursor uses **lowerCamelCase** event names (Claude is PascalCase).

| `Event::*`     | Cursor string  |
| -------------- | -------------- |
| `PreToolUse`   | `preToolUse`   |
| `PostToolUse`  | `postToolUse`  |
| `Custom(s)`    | `s`            |

Cursor also supports `beforeShellExecution`, `subagentStart`, `afterAgentResponse`,
`beforeTabFileRead`, etc. — pass via `Event::Custom`.

### Matcher mapping

For tool events, matcher is a tool-type literal (Cursor uses `Shell` rather
than Claude's `Bash`).

| `Matcher::*`        | Cursor string |
| ------------------- | ------------- |
| `All`               | `*`           |
| `Bash`              | `Shell`       |
| `Exact(s)`          | `s`           |
| `AnyOf([a, b])`     | `a\|b`        |
| `Regex(s)`          | `s` (verbatim; intended for `beforeShellExecution`-style events) |

## Prompt instructions

Cursor reads project rules from `<root>/.cursor/rules/*.mdc` and from
`AGENTS.md` at the project root. Not currently wired up; consumers wanting
prompt-level steering can rely on the project-root `AGENTS.md` Codex
integration, which Cursor also reads.

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.cursor/mcp.json` |
| Format | JSON |
| Mechanism | Server config with stdio/SSE/http options |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.cursor/mcp.json` |
| Format | JSON |
| Mechanism | Server config (overrides user config) |

### Configuration

```json
{
  "mcpServers": {
    "my-server": {
      "command": "node",
      "args": ["/path/to/server.js"]
    }
  }
}
```

**Important:** Changes to MCP config require restarting Cursor to take effect.

## Skills

| Scope | Path |
| --- | --- |
| User | `~/.cursor/skills/<name>/` |
| Project | `.cursor/skills/<name>/` |

Each skill is a directory containing `SKILL.md` with required `name` and
`description` frontmatter, plus optional `scripts/`, `references/`, and
`assets/` subdirectories.

## References

- <https://cursor.com/docs/hooks>
- <https://cursor.com/docs/context/mcp>
- <https://cursor.com/docs/context/rules>
- <https://cursor.com/docs/skills>
