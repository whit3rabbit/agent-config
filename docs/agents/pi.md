# Pi

ID: `pi` — `agent_config::by_id("pi")`

**Status:** registered for project + global prompt rules in `AGENTS.md`,
MCP servers (via the `pi-mcp-adapter` extension), skills, and instructions.
Pi has no config-file hook surface; hook installs require a `rules` body and
otherwise refuse with `MissingSpecField`.

## Surfaces

| Surface      | Scope          | Notes                                            |
| ------------ | -------------- | ------------------------------------------------ |
| Hooks        | -              | Pi only exposes hooks via TS extensions          |
| Prompt       | Global + Local | Fenced block in `AGENTS.md`                      |
| MCP          | Global + Local | JSON `mcpServers` map (`pi-mcp-adapter` shape)   |
| Skills       | Global + Local | SKILL.md folders                                 |
| Instructions | Global + Local | `InlineBlock` in `AGENTS.md`                     |

Pi extensions register `pi.on("tool_call", ...)` handlers programmatically
in TypeScript; there is no JSON-config-driven hook surface to install into.
This integration accepts a [`HookSpec`] *only* if `rules` is set.

## Prompt instructions

| | |
| --- | --- |
| User scope file | `~/.pi/agent/AGENTS.md` |
| Project scope file | `<root>/AGENTS.md` |
| Format | Tagged HTML-comment fence |

The project-local `AGENTS.md` is the standard cross-harness memory file (also
read by Codex, Claude Code, and others). The fenced HTML-comment block keeps
our content scoped so other agents reading the same file don't see the fence
as their content. Pi reads the global path on every session and walks up
from cwd to find local `AGENTS.md` / `CLAUDE.md` files.

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
| Global | `~/.pi/agent/mcp.json`        |
| Local  | `<root>/.pi/mcp.json`         |

Pi's MCP support comes from the optional `pi-mcp-adapter` extension. The
adapter reads four files in precedence order:

1. `~/.config/mcp/mcp.json` (cross-host shared)
2. `~/.pi/agent/mcp.json` (Pi global override)
3. `.mcp.json` (project shared)
4. `.pi/mcp.json` (Pi project override)

This crate writes only the Pi-owned files (#2 and #4) to avoid clobbering
cross-host configs that Claude Code, Cursor, or Codex may also read.

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

- <https://github.com/badlogic/pi-mono/tree/main/packages/coding-agent>
- <https://github.com/nicobailon/pi-mcp-adapter>
- <https://deepwiki.com/nicobailon/pi-mcp-adapter/4-configuration-guide>

Accessed: 2026-04-28.
