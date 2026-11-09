# Agent reference

One file per supported AI coding harness. See
[`../support-matrix.md`](../support-matrix.md) for the release-facing support
contract across all integrations. Each entry documents:

- **ID** — the string passed to `agent_config::by_id`.
- **Hooks** — install path and JSON/script format, broken out by scope.
- **Prompt** — optional rules-markdown surface, if the harness supports one.
- **MCP**, **Skills**, and **Instructions** — implemented where this crate has
  a file-backed or config-backed contract and tests for the emitted shape.

**Last updated:** 2026-11-08. MCP, skills, and instruction coverage reflects
the current file-backed locations implemented by this crate.

## Implemented

| Agent | Hooks | Prompt | MCP | Skills | Instructions | Memory File Support |
| ----- | ----- | ------ | --- | ------ | ------------ | ------------------- |
| [Claude Code](claude.md)         | ✓ (Global + Local) | ✓ | ✓ (Global + Local) | ✓ (Global + Local) | ✓ (Global + Local, ReferencedFile) | Own (`CLAUDE.md`) |
| [Cursor](cursor.md)              | ✓ (Global + Local) | -   | ✓ (Global + Local) | ✓ (Global + Local) | - | Both (`AGENTS.md` + `.cursor/rules/`) |
| [Gemini CLI](gemini.md)          | ✓ (Global + Local) | ✓ | ✓ (Global + Local) | ✓ (Global + Local) | ✓ (Global + Local, InlineBlock) | Own (`GEMINI.md`) |
| [OpenClaw](openclaw.md)          | - | ✓ (Local) | ✓ (Global, JSON5) | ✓ (Global + Local) | ✓ (Local, InlineBlock) | `AGENTS.md` |
| [Hermes Agent](hermes.md)        | - | ✓ (Local) | ✓ (Global, YAML) | ✓ (Global) | ✓ (Local, InlineBlock) | Both (`.hermes.md` + `AGENTS.md`) |
| [Codex CLI](codex.md)            | ✓ (Global + Local) | ✓ | ✓ (Global + Local, TOML) | ✓ (Global + Local) | ✓ (Global + Local, InlineBlock) | `AGENTS.md` |
| [GitHub Copilot](copilot.md)     | ✓ (Local)          | ✓ | ✓ (Global + Local) | ✓ (Global + Local) | ✓ (Local, InlineBlock) | Both (`copilot-instructions.md` + `AGENTS.md`) |
| [OpenCode](opencode.md)          | ✓ (Global + Local) | ✓ | ✓ (Global + Local, object) | ✓ (Global + Local) | ✓ (Global + Local, InlineBlock) | `AGENTS.md` |
| [Cline](cline.md)                | ✓ (Local, v3.36+ scripts) | ✓ | ✓ (Global) | ✓ (Global + Local) | ✓ (Local, StandaloneFile) | Both (`.cline/rules/` + `AGENTS.md`) |
| [Roo Code](roo.md)               | -                    | ✓ | ✓ (Global + Local) | -    | ✓ (Local, StandaloneFile) | Both (`.roo/rules/` + `AGENTS.md`) |
| [Windsurf](windsurf.md)          | ✓ (Local) | ✓ | ✓ (Global + Local) | ✓ (Global + Local) | ✓ (Local, StandaloneFile) | Both (`.windsurf/rules/` + `AGENTS.md`) |
| [Kilo Code](kilocode.md)         | -                    | ✓ | ✓ (Global + Local, JSONC) | ✓ (Global + Local) | ✓ (Local, StandaloneFile) | Both (`.kilo/rules/` + `AGENTS.md`) |
| [Google Antigravity](antigravity.md) | -                | ✓ | ✓ (Global + Local) | ✓ (Global + Local) | ✓ (Local, StandaloneFile) | Both (`.agents/rules/` + `AGENTS.md`) |
| [Antigravity CLI](antigravitycli.md) | -                | ✓ | ✓ (Global + Local) | - | ✓ (Global + Local, InlineBlock) | Both (`AGENTS.md` + `GEMINI.md`) |
| [Amp](amp.md)                    | - | ✓ | ✓ (Global + Local) | ✓ (Global + Local) | ✓ (Global + Local, InlineBlock) | Both (`AGENTS.md` + `CLAUDE.md` fallback) |
| [CodeBuddy CLI](codebuddy.md)    | ✓ (Global + Local) | ✓ | - | ✓ (Global + Local) | ✓ (Global + Local, InlineBlock) | Both / All (`CODEBUDDY.md` + `AGENTS.md` + `CLAUDE.md`) |
| [Charm Crush](crush.md)          | ✓ (Global + Local) | ✓ | ✓ (Global + Local, JSONC) | ✓ (Global + Local) | ✓ (Global + Local, InlineBlock) | Both (`AGENTS.md` + `CRUSH.md`) |
| [Forge](forge.md)                | - | ✓ | ✓ (Global + Local) | ✓ (Global + Local) | ✓ (Global + Local, InlineBlock) | `AGENTS.md` |
| [iFlow CLI](iflow.md)            | ✓ (Global + Local) | - | ✓ (Global + Local) | - | - | Both (`IFLOW.md` + `AGENTS.md`) |
| [JetBrains Junie](junie.md)      | - | ✓ (Local) | ✓ (Global + Local) | - | ✓ (Local, InlineBlock) | `AGENTS.md` (`.junie/AGENTS.md` + root) |
| [Pi](pi.md)                      | - | ✓ | ✓ (Global + Local) | ✓ (Global + Local) | ✓ (Global + Local, InlineBlock) | `AGENTS.md` |
| [Qoder CLI](qodercli.md)         | - | ✓ | ✓ (Global + Local) | - | ✓ (Global + Local, InlineBlock) | `AGENTS.md` (and `.qoder/rules/`) |
| [Qwen Code](qwen.md)             | - | ✓ | ✓ (Global + Local) | ✓ (Global + Local) | ✓ (Global + Local, InlineBlock) | Both (`QWEN.md` + `AGENTS.md`) |
| [Tabnine CLI](tabnine.md)        | ✓ (Global + Local) | - | ✓ (Global + Local) | - | - | MCP only (Coaching Guidelines) |
| [Trae](trae.md)                  | - | ✓ (Local) | - | ✓ (Global + Local) | ✓ (Local, InlineBlock) | Both (`.trae/project_rules.md` + `AGENTS.md`) |
| [DeepSeek Harness](dsh.md)       | - | ✓ (Global + Local) | - | ✓ (Global + Local) | ✓ (Global + Local, InlineBlock) | `AGENTS.md` |

## Conventions

- A *scope* is either `Global` (the user's home dir) or `Local(<project>)`.
- Hook JSON entries include `"_agent_config_tag": "<your tag>"` so multiple
  consumers can coexist and each can find its own work to remove.
- MCP servers and skills use sidecar ownership ledgers instead of embedding
  agent-config metadata in the harness payload.
- Prompt-markdown injections are wrapped in HTML-comment fences keyed on the
  identifier. Hooks (and prompt-only rules) use
  `<!-- BEGIN AGENT-CONFIG:<tag> --> ... <!-- END AGENT-CONFIG:<tag> -->`;
  the instruction surface uses a distinct prefix
  `<!-- BEGIN AGENT-CONFIG-INSTR:<name> --> ... <!-- END AGENT-CONFIG-INSTR:<name> -->`
  so a hook tag and an instruction name with the same string cannot
  collide.
- Any pre-existing file we modify gets a one-time `<path>.bak` sibling on
  first patch.
