# OpenClaw

**ID:** `openclaw`

**Status:** registered. Native OpenClaw plugin and hook-pack installation is
deferred because upstream documents it as `openclaw plugins install` lifecycle
work, not a stable file-backed hook contract.

## Prompt

### Memory file support

- **Supported format**: `AGENTS.md`.
- **Primary memory file**: `<root>/AGENTS.md`.
- **`AGENTS.md` support**: Native; OpenClaw agent workspace templates standardize on `<root>/AGENTS.md` for workspace operating instructions.


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

OpenClaw stores outbound MCP server definitions in its JSON5 config at
`~/.openclaw/openclaw.json` under `mcp.servers`.

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

OpenClaw loads AgentSkills-compatible skill folders from several roots in a
defined precedence order (workspace skills, project agent skills
`<workspace>/.agents/skills`, personal agent skills `~/.agents/skills`, managed
skills `~/.openclaw/skills`, workshop skills, bundled skills, and extraDirs).
The crate writes these roots:

| Scope | Root | Precedence |
| --- | --- | --- |
| Global | `~/.openclaw/skills/<name>/` | Managed / local skills |
| Local | `<root>/.agents/skills/<name>/` | Project agent skills |

Each skill directory contains `SKILL.md` plus optional caller-supplied
`scripts/`, `references/`, and `assets/`.

## Deferred Hooks And Plugins

OpenClaw native plugins require `openclaw.plugin.json`, `package.json`, and a
runtime entrypoint, and are normally installed with `openclaw plugins install`.
Hook packs participate in that plugin install surface. `agent-config` does not
shell out to the OpenClaw CLI in this pass.

## References

- Configuration:
  - <https://docs.openclaw.ai/gateway/configuration> (JSON5 config at `~/.openclaw/openclaw.json`)
  - <https://docs.openclaw.ai/gateway/config-extensions> (`mcp.servers`, `skills`, and `plugins` schema)
- Prompt / Instructions / Workspace:
  - <https://docs.openclaw.ai/reference/templates/AGENTS> (`AGENTS.md` template)
  - <https://docs.openclaw.ai/concepts/agent-workspace> (Agent workspace layout and operating instructions)
- MCP:
  - <https://docs.openclaw.ai/cli/mcp> (`openclaw mcp` CLI index)
  - <https://docs.openclaw.ai/cli/mcp/registry> (Outbound MCP server client registry)
  - <https://docs.openclaw.ai/cli/mcp/transports> (Transport specifications for Stdio, Streamable HTTP, and SSE)
  - <https://docs.openclaw.ai/tools/mcp> (MCP capabilities and server configuration)
- Skills:
  - <https://docs.openclaw.ai/tools/skills> (Skills overview, directory roots, and loading order)
  - <https://docs.openclaw.ai/tools/skills-config> (Skills configuration reference)
  - <https://docs.openclaw.ai/tools/creating-skills> (AgentSkills authoring with `SKILL.md`)
- Plugins & CLI Lifecycle:
  - <https://docs.openclaw.ai/tools/plugin> (Plugin overview and runtime capabilities)
  - <https://docs.openclaw.ai/plugins/manifest> (Native plugin manifest `openclaw.plugin.json`)
  - <https://docs.openclaw.ai/cli/plugins/install> (`openclaw plugins install` command reference)

Accessed: 2026-11-08.
