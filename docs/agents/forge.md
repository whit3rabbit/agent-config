# Forge

ID: `forge` — `agent_config::by_id("forge")`

**Status:** registered for project + global prompt rules in `AGENTS.md`,
MCP servers, skills, and instructions. Forge has no config-file hook surface;
hook installs require a `rules` body and otherwise refuse with `MissingSpecField`.

## Surfaces

| Surface      | Scope          | Notes                                  |
| ------------ | -------------- | -------------------------------------- |
| Hooks        | -              | Not part of Forge's documented surface |
| Prompt       | Global + Local | Fenced block in `AGENTS.md`            |
| MCP          | Global + Local | `mcpServers` JSON map at `.mcp.json`   |
| Skills       | Global + Local | `SKILL.md` directories                 |
| Instructions | Global + Local | `InlineBlock` in `AGENTS.md`           |

Forge is a universal CLI for AI coding agents built on the open Agent Client
Protocol (ACP). It has no config-driven hook surface; this integration accepts
a [`HookSpec`] *only* if `rules` is set.

## Prompt instructions

### Memory file support

- **Supported format**: `AGENTS.md`.
- **Primary memory file**: `~/.forge/AGENTS.md` (global) and `<root>/AGENTS.md` (local).
- **`AGENTS.md` support**: Native; Forge standardizes on `AGENTS.md` as its context instructions file.


| | |
| --- | --- |
| User scope file | `~/.forge/AGENTS.md` |
| Project scope file | `<root>/AGENTS.md` |
| Format | Tagged HTML-comment fence |

Project-local `AGENTS.md` is the standard cross-harness memory file. Forge
checks `AGENTS.md` in the current project (walking up directory trees) as well
as the user-level configuration path. The fenced HTML-comment block keeps our
content scoped so other tools reading the same file do not misinterpret it.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` because Forge's memory file does not
expose a documented `@import` syntax; the body is injected as a tagged
HTML-comment fenced block in the existing memory file.

### User scope (`Scope::Global`)

| | |
| --- | --- |
| Host file | `~/.forge/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `~/.forge/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Host file | `<root>/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `<root>/.forge/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP servers

| Scope  | File                 |
| ------ | -------------------- |
| Global | `~/.forge/.mcp.json` |
| Local  | `<root>/.mcp.json`   |

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

Ownership is recorded in a sidecar `.agent-config-mcp.json` ledger:
- Global: `~/.forge/.agent-config-mcp.json`
- Local: `<root>/.agent-config-mcp.json`

## Skills

| Scope | Path |
| --- | --- |
| Global (User) | `~/.forge/skills/<name>/` |
| Local (Project) | `<root>/.forge/skills/<name>/` |

Each skill is a directory containing a `SKILL.md` file along with any optional
subdirectories (`scripts/`, `references/`, `assets/`).

Ownership is recorded in a sidecar `.agent-config-skills.json` ledger:
- Global: `~/.forge/skills/.agent-config-skills.json`
- Local: `<root>/.forge/skills/.agent-config-skills.json`

## References

- Official website: <https://forgeagents.dev>
- GitHub repository: <https://github.com/forge-agents/forge>
- Architecture overview: <https://github.com/forge-agents/forge/blob/dev/ARCHITECTURE.md>
- Agent Client Protocol (ACP): <https://agentclientprotocol.com>
- Model Context Protocol (MCP): <https://modelcontextprotocol.io>

Accessed: 2026-11-08.
