# Gemini CLI

ID: `gemini` — `ai_hooker::by_id("gemini")`

## Hooks

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.gemini/settings.json` |
| Mechanism | JSON patch |
| Backup | `~/.gemini/settings.json.bak` (first patch only) |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.gemini/settings.json` |
| Mechanism | JSON patch |
| Backup | `<root>/.gemini/settings.json.bak` (first patch only) |

> There is no `GEMINI_HOME` env var; the user dir is hardcoded to `~/.gemini/`.

### Format

```json
{
  "hooks": {
    "BeforeTool": [
      {
        "matcher": "run_shell_command",
        "hooks": [
          { "type": "command", "command": "myapp hook gemini" }
        ],
        "_ai_hooker_tag": "myapp"
      }
    ]
  }
}
```

### Event mapping

Gemini uses `BeforeTool`/`AfterTool` (Claude/Codex use `PreToolUse`/`PostToolUse`).

| `Event::*`     | Gemini string |
| -------------- | ------------- |
| `PreToolUse`   | `BeforeTool`  |
| `PostToolUse`  | `AfterTool`   |
| `Custom(s)`    | `s`           |

Other Gemini events: `BeforeAgent`, `AfterAgent`, `Notification`, `SessionStart`,
`SessionEnd`, `PreCompress`, `BeforeModel`, `AfterModel`, `BeforeToolSelection`.

### Matcher mapping

Gemini matchers are regex against the tool name. Tool names are snake_case
(`run_shell_command`, `write_file`, `replace`, etc.).

| `Matcher::*`        | Gemini string         |
| ------------------- | --------------------- |
| `All`               | `*`                   |
| `Bash`              | `run_shell_command`   |
| `Exact(s)`          | `s`                   |
| `AnyOf([a, b])`     | `a\|b`                |
| `Regex(s)`          | `s`                   |

### Optional shell-script wrapper — TODO

Gemini also supports a script-delegator pattern: a `~/.gemini/hooks/<tag>.sh`
file plus a `~/.gemini/hooks/.<tag>.sha256` integrity sidecar. v0.1 uses
direct `command` strings (Gemini accepts these). Wrapper-mode lands when a
consumer asks for it; the `chmod` helper in `util::fs_atomic` is already
available.

## Prompt instructions

| | |
| --- | --- |
| User scope file | `~/.gemini/GEMINI.md` |
| Project scope file | `<root>/GEMINI.md` |
| Format | Tagged HTML-comment fence |

Gemini walks ancestor directories and concatenates `GEMINI.md` files. The
filename is overridable via `context.fileName` in `settings.json` (e.g., to
read `AGENTS.md` for cross-tool compatibility).

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.gemini/settings.json` |
| Key | `mcpServers` |
| Format | JSON |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `.gemini/settings.json` |
| Key | `mcpServers` |
| Format | JSON |

### Configuration

```json
{
  "mcpServers": {
    "my-server": {
      "command": "node",
      "args": ["/path/to/server.js"],
      "env": {
        "VAR_NAME": "value"
      }
    }
  }
}
```

Supports both stdio (command/args) and remote (url) servers.

### Environment variables

Interpolation via `$GEMINI_PROJECT_DIR` and `$GEMINI_PLANS_DIR` environment variables.

## Skills

| Scope | Path |
| --- | --- |
| User | `~/.gemini/skills/<name>/` |
| Project | `.gemini/skills/<name>/` |

Gemini CLI also discovers `.agents/skills`, but `ai-hooker` writes the native
Gemini path for this integration.

## References

- <https://geminicli.com/docs/hooks/>
- <https://geminicli.com/docs/cli/gemini-md/>
- <https://geminicli.com/docs/tools/mcp-server/>
