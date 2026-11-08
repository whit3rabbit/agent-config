# Pi

ID: `pi` — `agent_config::by_id("pi")`

**Status:** registered for project + global prompt rules in `AGENTS.md`,
MCP servers (native in Pi 1.0+; also compatible with `pi-mcp-adapter`), skills,
and instructions. Pi has no config-file hook surface; hook installs require a
`rules` body and otherwise refuse with `MissingSpecField`.

## Surfaces

| Surface      | Scope          | Notes                                            |
| ------------ | -------------- | ------------------------------------------------ |
| Hooks        | -              | Pi only exposes hooks via TS extensions          |
| Prompt       | Global + Local | Fenced block in `AGENTS.md`                      |
| MCP          | Global + Local | JSON `mcpServers` map (native in Pi 1.0+)        |
| Skills       | Global + Local | SKILL.md folders                                 |
| Instructions | Global + Local | `InlineBlock` in `AGENTS.md`                     |

Pi extensions register `pi.on("tool_call", ...)` handlers programmatically
in TypeScript; there is no JSON-config-driven hook surface to install into.
This integration accepts a [`HookSpec`] *only* if `rules` is set.

## Prompt instructions

### Memory file support

- **Supported format**: `AGENTS.md`.
- **Primary memory file**: `~/.pi/agent/AGENTS.md` (global) and `<root>/AGENTS.md` (local).
- **`AGENTS.md` support**: Native; Pi coding agent natively uses `AGENTS.md` for session instructions.


| | |
| --- | --- |
| User scope file | `~/.pi/agent/AGENTS.md` (or `$PI_CODING_AGENT_DIR/AGENTS.md`) |
| Project scope file | `<root>/AGENTS.md` |
| Format | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG:<tag> -->`) |

The project-local `AGENTS.md` is the standard cross-harness memory file (also
read by Codex, Claude Code, and others). The fenced HTML-comment block keeps
our content scoped so other agents reading the same file don't see the fence
as their content. Pi reads the global path on every session and walks up
from cwd to find local `AGENTS.md` / `CLAUDE.md` files.

Global agent path resolution honors the `PI_CODING_AGENT_DIR` environment
variable when set (defaulting to `~/.pi/agent`).

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` sharing the same host file as the prompt
rules surface.

### User scope (`Scope::Global`)

| | |
| --- | --- |
| Host file | `~/.pi/agent/AGENTS.md` (or `$PI_CODING_AGENT_DIR/AGENTS.md`) |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `~/.pi/agent/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

### Project scope (`Scope::Local(<root>)`)

| | |
| --- | --- |
| Host file | `<root>/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `<root>/.pi/.agent-config-instructions.json` |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP servers

| Scope  | File                          | Ledger |
| ------ | ----------------------------- | ------ |
| Global | `~/.pi/agent/mcp.json` (or `$PI_CODING_AGENT_DIR/mcp.json`) | `~/.pi/agent/.agent-config-mcp.json` |
| Local  | `<root>/.pi/mcp.json`         | `<root>/.pi/.agent-config-mcp.json` |

Pi 1.0+ supports MCP natively in core via standard `{"mcpServers": { ... }}`
JSON maps in the user's agent directory (`~/.pi/agent/mcp.json` or
`$PI_CODING_AGENT_DIR/mcp.json`) and project directory (`.pi/mcp.json`). Earlier
Pi setups using the `pi-mcp-adapter` extension use the identical file format.

This crate writes only the Pi-owned files (`~/.pi/agent/mcp.json` and
`<root>/.pi/mcp.json`) to avoid clobbering cross-host configs that Claude Code,
Cursor, or Codex may also read.

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

Ownership is recorded in a sidecar `.agent-config-mcp.json` ledger next to the
config file.

## Skills

| Scope  | Path                          | Ledger |
| ------ | ----------------------------- | ------ |
| Global | `~/.pi/agent/skills/<name>/` (or `$PI_CODING_AGENT_DIR/skills/<name>/`) | `~/.pi/agent/skills/.agent-config-skills.json` |
| Local  | `<root>/.pi/skills/<name>/`   | `<root>/.pi/skills/.agent-config-skills.json` |

Each skill is a folder with `SKILL.md` plus optional `scripts/`,
`references/`, `assets/`. Pi also picks up
`~/.agents/skills/` and `<root>/.agents/skills/` (cross-harness paths); this
crate writes only into the Pi-namespaced root to avoid collisions. Ownership
is recorded in `.agent-config-skills.json` at the root of the skills directory.

## References

### Official Documentation
- <https://pi.dev>
- <https://pi.dev/docs/latest>
- <https://pi.dev/docs/latest/configuration>
- <https://pi.dev/docs/latest/mcp>
- <https://pi.dev/docs/latest/skills>
- <https://pi.dev/docs/latest/extensions>
- <https://pi.dev/docs/latest/environment-variables>

### Repository & Upstream Docs
- <https://github.com/earendil-works/pi>
- <https://github.com/earendil-works/pi/tree/main/packages/coding-agent>
- <https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/configuration.md>
- <https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/mcp.md>
- <https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/skills.md>
- <https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md>
- <https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/environment-variables.md>

### Ecosystem & Specifications
- <https://github.com/nicobailon/pi-mcp-adapter>
- <https://agentskills.io/specification>

Accessed: 2026-11-08.
