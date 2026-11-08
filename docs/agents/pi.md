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

| | |
| --- | --- |
| User scope file | `~/.pi/agent/AGENTS.md` (or `$PI_CODING_AGENT_DIR/AGENTS.md`) |
| Project scope file | `<root>/AGENTS.md` |
| Format | Tagged HTML-comment fence |

The project-local `AGENTS.md` is the standard cross-harness memory file (also
read by Codex, Claude Code, and others). The fenced HTML-comment block keeps
our content scoped so other agents reading the same file don't see the fence
as their content. Pi reads the global path on every session and walks up
from cwd to find local `AGENTS.md` / `CLAUDE.md` files.

Global agent path resolution honors the `PI_CODING_AGENT_DIR` environment
variable when set (defaulting to `~/.pi/agent`).

## Instructions

Inline-block placement, sharing the same host file as the prompt rules
surface.

| | |
| --- | --- |
| User scope ledger | `~/.pi/agent/.agent-config-instructions.json` |
| Project scope ledger | `<root>/.pi/.agent-config-instructions.json` |
| Host file | Same as prompt (`AGENTS.md`) |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP servers

| Scope  | File                          |
| ------ | ----------------------------- |
| Global | `~/.pi/agent/mcp.json` (or `$PI_CODING_AGENT_DIR/mcp.json`) |
| Local  | `<root>/.pi/mcp.json`         |

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

Ownership is recorded in a sidecar `<config-dir>/.agent-config-mcp.json`
ledger.

## Skills

| Scope  | Path                          |
| ------ | ----------------------------- |
| Global | `~/.pi/agent/skills/<name>/`  |
| Local  | `<root>/.pi/skills/<name>/`   |

Each skill is a folder with `SKILL.md` plus optional `scripts/`,
`references/`, `assets/`. Pi also picks up
`~/.agents/skills/` and `<root>/.agents/skills/` (cross-harness paths); this
crate writes only into the Pi-namespaced root to avoid collisions.

## References

- <https://pi.dev>
- <https://github.com/earendil-works/pi>
- <https://github.com/earendil-works/pi/tree/main/packages/coding-agent>
- <https://github.com/nicobailon/pi-mcp-adapter>

Accessed: 2026-11-08.
