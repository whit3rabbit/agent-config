# Cursor

ID: `cursor` — `agent_config::by_id("cursor")`

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
        "_agent_config_tag": "myapp"
      }
    ]
  }
}
```

> Cursor requires the top-level `"version": 1`. The library ensures it.
> Cursor watches `hooks.json` files and reloads them on save.

### Event mapping

Cursor uses **lowerCamelCase** event names (Claude is PascalCase).

| `Event::*`     | Cursor string  |
| -------------- | -------------- |
| `PreToolUse`   | `preToolUse`   |
| `PostToolUse`  | `postToolUse`  |
| `Custom(s)`    | `s`            |

Cursor officially supports additional events that can be configured via `Event::Custom`:
- **Tool execution:** `preToolUse`, `postToolUse`, `postToolUseFailure`
- **Shell & MCP execution:** `beforeShellExecution`, `afterShellExecution`, `beforeMCPExecution`, `afterMCPExecution`
- **Subagents:** `subagentStart`, `subagentStop`
- **File operations:** `afterFileEdit`, `beforeReadFile`, `beforeTabFileRead`, `afterTabFileEdit`
- **Prompt & model responses:** `beforeSubmitPrompt`, `afterAgentResponse`, `afterAgentThought`
- **Session & lifecycle:** `stop`, `sessionStart`, `sessionEnd`, `preCompact`, `workspaceOpen`

### Matcher mapping

For tool events, matcher is a tool-type literal (Cursor uses `Shell` rather
than Claude's `Bash`). Valid tool matchers include `Shell`, `Read`, `Write`, `Edit`,
`Grep`, `Delete`, `Task`, or `MCP:<tool_name>`. For shell commands (e.g. in
`beforeShellExecution`), regex matching on the command string is supported.

| `Matcher::*`        | Cursor string |
| ------------------- | ------------- |
| `All`               | `*`           |
| `Bash`              | `Shell`       |
| `Exact(s)`          | `s`           |
| `AnyOf([a, b])`     | `a\|b`        |
| `Regex(s)`          | `s` (verbatim; intended for `beforeShellExecution`-style events) |

## Prompt instructions

### Memory file support

- **Supported format**: Both own rules (`.cursor/rules/*.mdc`) and `AGENTS.md`.
- **Primary memory files**: `<root>/.cursor/rules/*.mdc` (and legacy `.cursorrules`).
- **`AGENTS.md` support**: Native; Cursor natively discovers and loads `AGENTS.md` at the project root and nested subdirectories.


Cursor reads project rules from `<root>/.cursor/rules/*.mdc` (requiring
frontmatter with `description`, `globs`, and `alwaysApply`) and from
`AGENTS.md` at the project root or subdirectories (nested `AGENTS.md` support).
Cursor also supports global User Rules configured in `Customize` → `Rules`.

The prompt/instruction surface is not currently wired up directly for `cursor`
in `agent-config`; consumers wanting prompt-level steering can rely on the
project-root `AGENTS.md` Codex integration, which Cursor also reads.

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.cursor/mcp.json` |
| Ledger | `~/.cursor/.agent-config-mcp.json` |
| Format | JSON |
| Mechanism | Server config with stdio/SSE/http options |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.cursor/mcp.json` |
| Ledger | `<root>/.cursor/.agent-config-mcp.json` |
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

Cursor supports stdio, SSE, and HTTP transports, environment variable passing,
and configuration variable interpolation (e.g., `${env:VAR_NAME}`).

**Important:** Changes to MCP config require restarting Cursor to take effect.

## Skills

| Scope | Path |
| --- | --- |
| User | `~/.cursor/skills/<name>/` |
| Project | `.cursor/skills/<name>/` |

Cursor also natively discovers skills in `.agents/skills/` and `~/.agents/skills/`;
`agent-config` targets `.cursor/skills/` specifically for this harness.

Each skill is a directory containing `SKILL.md` with required `name` and
`description` frontmatter, plus optional `scripts/`, `references/`, and
`assets/` subdirectories. Ownership and drift tracking are managed via the
sidecar ledger `.agent-config-skills.json`.

## References

- <https://cursor.com/docs/hooks>
- <https://cursor.com/docs/mcp>
- <https://cursor.com/docs/rules>
- <https://cursor.com/docs/skills>
- <https://cursor.com/docs/subagents>
- <https://cursor.com/docs/plugins>
- <https://cursor.com/docs/customize-cursor>
- <https://cursor.com/docs/cli/overview>
