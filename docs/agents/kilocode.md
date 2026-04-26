# Kilo Code

ID: `kilocode` — `agent_config::by_id("kilocode")`

## Hooks

Not supported. Prompt-level integration only.

## Prompt instructions

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| File | `<root>/.kilocode/rules/<tag>.md` |
| Mechanism | One markdown file per consumer |
| Format | Plain markdown |

Kilo Code can also read root-level `AGENTS.md` and `kilo.jsonc`
instruction paths, but v0.1 writes only the per-consumer rules directory above.

### User scope (`Scope::Global`)

Not supported in v0.1. Kilo's global rules live at `~/.kilo/rules/`.
Calling with `Scope::Global` returns `AgentConfigError::UnsupportedScope`.

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
        "API_KEY": "${API_KEY}"
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
directories in the CLI. `agent-config` writes the native Kilo path.

## References

- <https://kilo.ai/docs/agent-behavior/agents-md>
- <https://kilo.ai/docs/customize/custom-rules>
- <https://kilo.ai/docs/automate/mcp/using-in-kilo-code>
- <https://kilo.ai/docs/customize/skills>
