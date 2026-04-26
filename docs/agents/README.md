# Agent reference

One file per supported AI coding harness. See
[`../support-matrix.md`](../support-matrix.md) for the release-facing support
contract across all integrations. Each entry documents:

- **ID** — the string passed to `agent_config::by_id`.
- **Hooks** — install path and JSON/script format, broken out by scope.
- **Prompt** — optional rules-markdown surface, if the harness supports one.
- **MCP** and **Skills** — implemented where this crate has a file-backed or
  config-backed contract and tests for the emitted shape.

**Last updated:** 2026-04-26. MCP and skills coverage reflects the current
file-backed locations implemented by this crate.

## Implemented

| Agent | Hooks | Prompt | MCP | Skills |
| ----- | ----- | ------ | --- | ------ |
| [Claude Code](claude.md)         | ✓ (Global + Local) | ✓ | ✓ (Global + Local) | ✓ (Global + Local) |
| [Cursor](cursor.md)              | ✓ (Global + Local) | -   | ✓ (Global + Local) | ✓ (Global + Local) |
| [Gemini CLI](gemini.md)          | ✓ (Global + Local) | ✓ | ✓ (Global + Local) | ✓ (Global + Local) |
| [OpenClaw](openclaw.md)          | - | ✓ (Local) | ✓ (Global, JSON5) | ✓ (Global + Local) |
| [Hermes Agent](hermes.md)        | - | ✓ (Local) | ✓ (Global, YAML) | ✓ (Global) |
| [Codex CLI](codex.md)            | ✓ (Global + Local) | ✓ | ✓ (Global + Local, TOML) | ✓ (Global + Local) |
| [GitHub Copilot](copilot.md)     | ✓ (Local)          | ✓ | ✓ (Global + Local) | ✓ (Global + Local) |
| [OpenCode](opencode.md)          | ✓ (Global + Local) | - | ✓ (Global + Local, object) | ✓ (Global + Local) |
| [Cline](cline.md)                | ✓ (Local, v3.36+ scripts) | ✓ | ✓ (Global) | ✓ (Global + Local) |
| [Roo Code](roo.md)               | -                    | ✓ | ✓ (Global + Local) | -    |
| [Windsurf](windsurf.md)          | ✓ (Local) | ✓ | ✓ (Global + Local) | ✓ (Global + Local) |
| [Kilo Code](kilocode.md)         | -                    | ✓ | ✓ (Global + Local, JSONC) | ✓ (Global + Local) |
| [Google Antigravity](antigravity.md) | -                | ✓ | ✓ (Global + Local) | ✓ (Global + Local) |
| [Amp](amp.md)                    | - | ✓ | ✓ (Global + Local) | ✓ (Global + Local) |
| [CodeBuddy CLI](codebuddy.md)    | ✓ (Global + Local) | ✓ | - | ✓ (Global + Local) |
| [Forge](forge.md)                | - | ✓ | ✓ (Global + Local) | ✓ (Global + Local) |
| [iFlow CLI](iflow.md)            | ✓ (Global + Local) | - | ✓ (Global + Local) | - |
| [JetBrains Junie](junie.md)      | - | ✓ (Local) | ✓ (Global + Local) | - |
| [Qoder CLI](qodercli.md)         | - | ✓ | ✓ (Global + Local) | - |
| [Qwen Code](qwen.md)             | - | ✓ | ✓ (Global + Local) | ✓ (Global + Local) |
| [Tabnine CLI](tabnine.md)        | ✓ (Global + Local) | - | ✓ (Global + Local) | - |
| [Trae](trae.md)                  | - | ✓ (Local) | - | ✓ (Global + Local) |

## Conventions

- A *scope* is either `Global` (the user's home dir) or `Local(<project>)`.
- Hook JSON entries include `"_agent_config_tag": "<your tag>"` so multiple
  consumers can coexist and each can find its own work to remove.
- MCP servers and skills use sidecar ownership ledgers instead of embedding
  agent-config metadata in the harness payload.
- Prompt-markdown injections are wrapped in HTML-comment fences keyed on the
  tag: `<!-- BEGIN AGENT-CONFIG:<tag> --> ... <!-- END AGENT-CONFIG:<tag> -->`.
- Any pre-existing file we modify gets a one-time `<path>.bak` sibling on
  first patch.
