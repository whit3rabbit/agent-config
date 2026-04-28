# OpenClaw

**ID:** `openclaw`

**Status:** registered. Native OpenClaw plugin and hook-pack installation is
deferred because upstream documents it as `openclaw plugins install` lifecycle
work, not a stable file-backed hook contract.

## Prompt

OpenClaw project instructions use the workspace `AGENTS.md` file.

| Scope | File |
| --- | --- |
| Local | `<root>/AGENTS.md` |

`agent-config` inserts a fenced markdown block keyed by the consumer tag. Global
prompt install is unsupported.

## Instructions

Standalone instruction files installed via `InstructionSurface`. Uses
`InstructionPlacement::InlineBlock` (project-local only) because OpenClaw's
memory file does not expose a documented `@import` syntax; the body is
injected as a tagged HTML-comment fenced block in the existing memory file.

| | |
| --- | --- |
| Host file | `<root>/AGENTS.md` |
| Mechanism | Tagged HTML-comment fence (`<!-- BEGIN AGENT-CONFIG-INSTR:<name> -->`) |
| Ledger | `<root>/.agents/.agent-config-instructions.json` (reuses the existing `.agents/` skills directory) |
| Placement | `InstructionPlacement::InlineBlock` |

## MCP

OpenClaw stores outbound MCP server definitions in its JSON5 config under
`mcp.servers`.

| Scope | File | Shape |
| --- | --- | --- |
| Global | `~/.openclaw/openclaw.json` | `mcp.servers.<name>` |

MCP is global-only in this crate. Local MCP install returns
`AgentConfigError::UnsupportedScope`.

Transport mapping:

- `Stdio`: `command`, `args`, optional `env`
- `Http`: `url`, optional `headers`, `transport: "streamable-http"`
- `Sse`: `url`, optional `headers`, `transport: "sse"`

OpenClaw also has `openclaw mcp set/unset`; this crate writes the documented
config shape directly and records ownership in `.agent-config-mcp.json` beside
the config.

## Skills

OpenClaw loads AgentSkills-compatible skill folders from several roots. The
crate writes these roots:

| Scope | Root |
| --- | --- |
| Global | `~/.openclaw/skills/<name>/` |
| Local | `<root>/.agents/skills/<name>/` |

Each skill directory contains `SKILL.md` plus optional caller-supplied
`scripts/`, `references/`, and `assets/`.

## Deferred Hooks And Plugins

OpenClaw native plugins require `openclaw.plugin.json`, `package.json`, and a
runtime entrypoint, and are normally installed with `openclaw plugins install`.
Hook packs participate in that plugin install surface. `agent-config` does not
shell out to the OpenClaw CLI in this pass.

## References

- <https://docs.openclaw.ai/tools/plugin>
- <https://docs.openclaw.ai/plugins/manifest>
- <https://docs.openclaw.ai/cli/mcp>
- <https://docs.openclaw.ai/tools/skills>
- <https://docs.openclaw.ai/tools/creating-skills>
- <https://docs.openclaw.ai/reference/templates/AGENTS>

Accessed: 2026-04-25.
