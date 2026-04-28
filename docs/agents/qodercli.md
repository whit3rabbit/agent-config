# Qoder CLI

ID: `qodercli` — `agent_config::by_id("qodercli")`

## Surfaces

| Surface | Scope          | Notes                                  |
| ------- | -------------- | -------------------------------------- |
| Hooks   | -              | Not part of Qoder's documented surface |
| Prompt  | Global + Local | Fenced block in `AGENTS.md`            |
| MCP     | Global + Local | `mcpServers` JSON map; per-scope files |
| Skills  | -              | Not part of Qoder's documented surface |

## Prompt instructions

| | |
| --- | --- |
| User scope file | `~/.qoder/AGENTS.md` |
| Project scope file | `<root>/AGENTS.md` |
| Format | Tagged HTML-comment fence |

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` because Qoder CLI's memory file does not
expose a documented `@import` syntax; the body is injected as a tagged
HTML-comment fenced block in the existing memory file.

### User scope (`Scope::Global`)

| | |
| --- | --- |
| Host file | `~/.qoder/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG:<name> -->`) |
| Ledger | `~/.qoder/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Host file | `<root>/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG:<name> -->`) |
| Ledger | `<root>/.qoder/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP servers

| Scope | File |
| --- | --- |
| Global | `~/.qoder.json` (a flat file in `$HOME`, not under `~/.qoder/`) |
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
Qoder's CLI commands `qodercli mcp add/remove/list` write the same shape
directly.

## References

- <https://docs.qoder.com/cli/using-cli>

Accessed: 2026-04-26.
