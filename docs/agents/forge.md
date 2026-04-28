# Forge

ID: `forge` — `agent_config::by_id("forge")`

## Surfaces

| Surface | Scope          | Notes                                  |
| ------- | -------------- | -------------------------------------- |
| Hooks   | -              | Not part of Forge's documented surface |
| Prompt  | Global + Local | Fenced block in `AGENTS.md`            |
| MCP     | Global + Local | `mcpServers` JSON map at `.mcp.json`   |
| Skills  | Global + Local | `SKILL.md` directories                 |

## Prompt instructions

| | |
| --- | --- |
| User scope file | `~/.forge/AGENTS.md` |
| Project scope file | `<root>/AGENTS.md` |
| Format | Tagged HTML-comment fence |

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` because Forge's memory file does not
expose a documented `@import` syntax; the body is injected as a tagged
HTML-comment fenced block in the existing memory file.

### User scope (`Scope::Global`)

| | |
| --- | --- |
| Host file | `~/.forge/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG:<name> -->`) |
| Ledger | `~/.forge/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Host file | `<root>/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG:<name> -->`) |
| Ledger | `<root>/.forge/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP servers

| Scope | File |
| --- | --- |
| Global | `~/.forge/.mcp.json` |
| Local | `<root>/.mcp.json` |

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

Ownership is recorded in a sidecar `<config-dir>/.agent-config-mcp.json` ledger.

## Skills

| Scope | Path |
| --- | --- |
| User | `~/.forge/skills/<name>/` |
| Project | `<root>/.forge/skills/<name>/` |

## References

- <https://forgecode.dev/docs>
- <https://github.com/forge-agents/forge>

Accessed: 2026-04-26.
