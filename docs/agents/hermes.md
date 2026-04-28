# Hermes Agent

**ID:** `hermes`

**Status:** registered for prompt rules, global MCP, and global skills.

## Prompt

Hermes loads project context from `.hermes.md` / `HERMES.md` before falling
back to other project instruction files. This crate writes `.hermes.md`.

| Scope | File |
| --- | --- |
| Local | `<root>/.hermes.md` |

`agent-config` inserts a fenced markdown block keyed by the consumer tag. Global
prompt install is unsupported, and this crate does not modify `SOUL.md`.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` (project-local only) because Hermes
Agent's memory file does not expose a documented `@import` syntax; the body
is injected as a tagged HTML-comment fenced block in the existing memory
file.

| | |
| --- | --- |
| Host file | `<root>/.hermes.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `<root>/.hermes/.agent-config-instructions.json` (new directory; created on demand to avoid cluttering project root) |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP

Hermes reads MCP server config from `~/.hermes/config.yaml` under
`mcp_servers`.

| Scope | File | Shape |
| --- | --- | --- |
| Global | `~/.hermes/config.yaml` | `mcp_servers.<name>` |

MCP is global-only in this crate. Local MCP install returns
`AgentConfigError::UnsupportedScope`.

Transport mapping:

- `Stdio`: `command`, `args`, optional `env`
- `Http` / `Sse`: `url`, optional `headers`

Ownership is recorded in `.agent-config-mcp.json` beside `config.yaml`.

## Skills

Hermes stores local writable skills under `~/.hermes/skills`, with category
directories. This crate writes a dedicated category:

| Scope | Root |
| --- | --- |
| Global | `~/.hermes/skills/agent-config/<name>/` |

Local skill install is unsupported. External shared skill directories can be
configured in Hermes separately, but this crate does not edit
`skills.external_dirs`.

## References

- <https://hermes-agent.nousresearch.com/docs/user-guide/configuration/>
- <https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp>
- <https://hermes-agent.nousresearch.com/docs/reference/mcp-config-reference/>
- <https://hermes-agent.nousresearch.com/docs/user-guide/features/skills/>

Accessed: 2026-04-25.
