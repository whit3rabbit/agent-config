# Antigravity CLI

ID: `antigravitycli` — `agent_config::by_id("antigravitycli")`

Google's terminal-first Antigravity CLI. This is separate from the
[`antigravity`](antigravity.md) app/IDE integration.

## Hooks

No native hook surface is implemented. `HookSpec::rules` installs prompt
context into the CLI's context file.

## Prompt Instructions

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

Not registered as `SkillSurface` yet. Antigravity CLI skills are flat markdown
files such as `.agents/skills/format-tests.md` and
`~/.gemini/antigravity-cli/skills/format-tests.md`, while this crate's current
skill surface is directory-scoped `SKILL.md` packages.

## References

- <https://antigravity.google/docs/gcli-migration>
- <https://antigravity.google/docs/cli-plugins>
- <https://antigravity.google/assets/docs/cli/gcli-migration.md>
- <https://antigravity.google/assets/docs/cli/cli-plugins.md>
