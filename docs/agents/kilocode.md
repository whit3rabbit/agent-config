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

## MCP servers — Not supported

Kilo Code does not support MCP servers.

## Skills — Not supported

Kilo Code does not support skills.

## References

- <https://kilo.ai/docs/agent-behavior/agents-md>
- <https://kilo.ai/docs/customize/custom-rules>
