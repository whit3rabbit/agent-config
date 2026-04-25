# Kilo Code

ID: `kilocode` — `ai_hooker::by_id("kilocode")`

## Hooks

Not supported. Prompt-level integration only.

## Prompt instructions

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Primary file | `<root>/AGENTS.md` (open standard, case-sensitive uppercase) |
| Rules directory | `.kilo/rules/` (markdown files, alphabetical load order) |
| Configuration | `kilo.jsonc` with `instructions` array |
| Format | Markdown preferred, `.txt` accepted |

### AGENTS.md (Primary)

Kilo Code uses `AGENTS.md` as the primary configuration file (not `KILO.md`).

| | |
| --- | --- |
| File | `<root>/AGENTS.md` or `<root>/AGENT.md` (fallback) |
| Format | Standard Markdown |
| Protection | Write-protected in Kilo UI |

### Load order (priority)

1. Agent-specific prompt (`agent.<name>.prompt`)
2. Project `kilo.jsonc` `instructions` key
3. Project root `AGENTS.md`
4. Global `kilo.jsonc` `instructions` key

### Example kilo.jsonc

```jsonc
{
  "instructions": [
    ".kilo/rules/formatting.md",
    ".kilo/rules/*.md"
  ]
}
```

### User scope (`Scope::Global`)

Not supported in v0.1. Kilo's global rules live at `~/.kilo/rules/`.
Calling with `Scope::Global` returns `HookerError::UnsupportedScope`.

## MCP servers

### User scope (`Scope::Global`)

| | |
| --- | --- |
| File | `~/.config/kilo/kilo.jsonc` |
| Format | JSONC |
| Key | `mcp` (object keyed by server name) |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/kilo.jsonc`, or existing `<root>/.kilo/kilo.jsonc` |
| Format | JSONC |
| Key | `mcp` (object keyed by server name) |

### Configuration

```jsonc
{
  "mcp": {
    "my-server": {
      "type": "local",
      "command": ["node", "/path/to/server.js"],
      "environment": {
        "API_KEY": "secret"
      }
    }
  }
}
```

## Skills

| Scope | Path |
| --- | --- |
| User | `~/.kilo/skills/<name>/` |
| Project | `.kilo/skills/<name>/` |

Kilo Code also supports `.claude/skills` and `.agents/skills` compatibility
directories in the CLI. `ai-hooker` writes the native Kilo path.

## References

- <https://kilo.ai/docs/agent-behavior/agents-md>
- <https://kilo.ai/docs/customize/custom-rules>
- <https://kilo.ai/docs/automate/mcp/using-in-kilo-code>
- <https://kilo.ai/docs/customize/skills>
