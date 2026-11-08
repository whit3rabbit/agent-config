# Antigravity CLI

ID: `antigravitycli` — `agent_config::by_id("antigravitycli")`

Google's terminal-first Antigravity CLI. This is separate from the
[`antigravity`](antigravity.md) app/IDE integration.

## Hooks

Hook commands are configured inside `hooks.json` under top-level consumer tags.

| Scope | File |
| --- | --- |
| Global | `~/.gemini/antigravity-cli/hooks.json` |
| Local | `<root>/.agents/hooks.json` |

Matches event names like `PreToolUse` and `PostToolUse`. Matcher translates `Matcher::Bash` to `"run_command"`.

If the hook specification includes `rules`, prompt rules are written/injected into local `AGENTS.md` or global `~/.gemini/GEMINI.md`.

## Prompt

### Memory file support

- **Supported format**: Both `AGENTS.md` and `GEMINI.md`.
- **Primary memory files**: `~/.gemini/GEMINI.md` (global user instructions) and `<root>/AGENTS.md` (local repository instructions).
- **`AGENTS.md` support**: Native; Antigravity CLI targets `<root>/AGENTS.md` for project scope.
 Instructions

| | |
| --- | --- |
| User scope file | `~/.gemini/GEMINI.md` |
| Project scope file | `<root>/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence |

Local `AGENTS.md` and global `~/.gemini/GEMINI.md` match the CLI migration
docs: workspace context can stay in `GEMINI.md` or `AGENTS.md`, and global
developer context is read from `~/.gemini/GEMINI.md`.

## Instructions

Installed via `InstructionSurface` using `InstructionPlacement::InlineBlock`.
The instruction body is injected into the same host files used for prompt
rules.

| Scope | Host file | Ledger |
| --- | --- | --- |
| Global | `~/.gemini/GEMINI.md` | `~/.gemini/.agent-config-instructions.json` |
| Local | `<root>/AGENTS.md` | `<root>/.agents/.agent-config-instructions.json` |

## MCP Servers

| Scope | File | Key |
| --- | --- | --- |
| Global | `~/.gemini/antigravity-cli/mcp_config.json` | `mcpServers` |
| Local | `<root>/.agents/mcp_config.json` | `mcpServers` |

Stdio entries use `command`, `args`, and optional `env`.

```json
{
  "mcpServers": {
    "sqlite-explorer": {
      "command": "node",
      "args": ["/usr/local/bin/sqlite-mcp-server.js"],
      "env": {
        "SQLITE_DB_PATH": "/var/data/app.db"
      }
    }
  }
}
```

Remote entries use `serverUrl`, not Gemini CLI's legacy `url` key.

```json
{
  "mcpServers": {
    "remote-indexer": {
      "serverUrl": "https://example.com/mcp",
      "headers": {
        "Authorization": "Bearer TOKEN"
      }
    }
  }
}
```

Google's migration page also mentions `~/.gemini/config/mcp_config.json`;
the CLI plugins page documents `~/.gemini/antigravity-cli/mcp_config.json`,
which is the path this integration writes.

## Skills

Skills are directory-scoped packages containing `SKILL.md` manifest and optional supporting files.

| Scope | File |
| --- | --- |
| Global | `~/.gemini/config/skills/<name>/` |
| Global legacy drain | `~/.gemini/antigravity-cli/skills/<name>/` (status/uninstall only) |
| Local | `<root>/.agents/skills/<name>/` |

`~/.gemini/config/skills/` is the canonical global shared across the
Antigravity app, IDE, and CLI. `~/.gemini/antigravity-cli/skills/` is
CLI-only (the App does not read it); installs there from older releases are
still detected and removed.

## References

- <https://antigravity.google/docs/hooks>
- <https://antigravity.google/docs/skills>
- <https://antigravity.google/docs/mcp>
- <https://antigravity.google/docs/rules>
- <https://antigravity.google/docs/customization-discovery>
- <https://antigravity.google/docs/cli/reference>
- <https://antigravity.google/docs/cli/gcli-migration>
- <https://antigravity.google/docs/settings?tab=cli>
- <https://antigravity.google/docs/plugins?tab=cli>
