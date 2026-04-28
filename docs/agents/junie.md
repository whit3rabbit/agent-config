# JetBrains Junie

ID: `junie` — `agent_config::by_id("junie")`

**Status:** registered for project-local prompt rules and MCP (Global + Local).
Hook lifecycle support is tracked upstream (JUNIE-1961) but not yet released.

## Surfaces

| Surface | Scope          | Notes                                  |
| ------- | -------------- | -------------------------------------- |
| Hooks   | -              | Pending upstream                       |
| Prompt  | Local          | Fenced block in `<root>/.junie/AGENTS.md` |
| MCP     | Global + Local | `mcpServers` JSON map at `mcp/mcp.json` |
| Skills  | -              | Not part of Junie's documented surface |

## Prompt instructions

| | |
| --- | --- |
| Project scope file | `<root>/.junie/AGENTS.md` |
| Format | Tagged HTML-comment fence |

Global prompt install is unsupported. Junie also reads a custom filename via
the `JUNIE_GUIDELINES_FILENAME` env var; this crate writes the documented
default `AGENTS.md`.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` (project-local only) because JetBrains
Junie's memory file does not expose a documented `@import` syntax; the body
is injected as a tagged HTML-comment fenced block in the existing memory
file.

| | |
| --- | --- |
| Host file | `<root>/.junie/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `<root>/.junie/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP servers

| Scope | File |
| --- | --- |
| Global | `~/.junie/mcp/mcp.json` |
| Local | `<root>/.junie/mcp/mcp.json` |

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

## References

- <https://junie.jetbrains.com/docs/junie-cli-mcp-configuration.html>
- <https://www.jetbrains.com/help/junie/mcp-settings.html>

Accessed: 2026-04-26.
