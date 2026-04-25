# Codex CLI

ID: `codex` — `ai_hooker::by_id("codex")`

## Hooks

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `$CODEX_HOME/hooks.json` (default `~/.codex/hooks.json`) |
| Mechanism | JSON patch |
| Backup | `<file>.bak` (first patch only) |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.codex/hooks.json` |
| Mechanism | JSON patch |
| Backup | `<root>/.codex/hooks.json.bak` (first patch only) |

> Project-scope hooks only load when the project is trusted by Codex.
> Codex also accepts a TOML form (`config.toml` with `[features] codex_hooks = true`);
> v0.1 always writes JSON.

### Format

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "shell",
        "hooks": [
          { "type": "command", "command": "myapp hook codex" }
        ],
        "_ai_hooker_tag": "myapp"
      }
    ]
  }
}
```

### Event mapping

Codex uses `PreToolUse`/`PostToolUse` (PascalCase, like Claude).

| `Event::*`     | Codex string  |
| -------------- | ------------- |
| `PreToolUse`   | `PreToolUse`  |
| `PostToolUse`  | `PostToolUse` |
| `Custom(s)`    | `s`           |

Codex events also include `SessionStart`, `PermissionRequest`, `UserPromptSubmit`, `Stop`.

### Matcher mapping

| `Matcher::*`        | Codex string |
| ------------------- | ------------ |
| `All`               | `*`          |
| `Bash`              | `shell`      |
| `Exact(s)`          | `s`          |
| `AnyOf([a, b])`     | `a\|b`       |
| `Regex(s)`          | `s`          |

## Prompt instructions

| | |
| --- | --- |
| User scope file | `$CODEX_HOME/AGENTS.md` (default `~/.codex/AGENTS.md`) |
| Project scope file | `<root>/AGENTS.md` |
| Format | Tagged HTML-comment fence |

> Codex walks from git root down to cwd, reading `AGENTS.override.md` first,
> then `AGENTS.md`, in each directory. There is **no `@import` directive**
> (an open feature request as of April 2026), so we inject the rules body
> inline rather than emitting an `@RTK.md`-style include.

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `$CODEX_HOME/config.toml` (default `~/.codex/config.toml`) |
| Format | TOML |
| Table | `[mcp_servers.<server-name>]` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.codex/config.toml` |
| Format | TOML |
| Table | `[mcp_servers.<server-name>]` |

> Project MCP config only loads when the project is trusted by Codex.

### Configuration

```toml
[mcp_servers.my_server]
command = "node"
args = ["/path/to/server.js"]

[mcp_servers.my_server.env]
API_KEY = "secret"
```

Supports both stdio (command/args) and http (url/bearer_token_env_var) transports.

## References

- <https://developers.openai.com/codex/hooks>
- <https://developers.openai.com/codex/guides/agents-md>
- <https://developers.openai.com/codex/config-basic>
- <https://developers.openai.com/codex/mcp>
