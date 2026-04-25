# Agent reference

One file per supported AI coding harness. Each entry documents:

- **ID** — the string passed to `ai_hooker::by_id`.
- **Hooks** — install path and JSON/script format, broken out by scope.
- **Prompt** — optional rules-markdown surface, if the harness supports one.
- **MCP** and **Skills** — implemented where there is a confirmed file-backed
  or documented config-backed contract.

**Last updated:** 2026-04-25. MCP and skills coverage reflects the current
file-backed locations documented by each harness.

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

## Conventions

- A *scope* is either `Global` (the user's home dir) or `Local(<project>)`.
- Hook JSON entries include `"_ai_hooker_tag": "<your tag>"` so multiple
  consumers can coexist and each can find its own work to remove.
- MCP servers and skills use sidecar ownership ledgers instead of embedding
  ai-hooker metadata in the harness payload.
- Prompt-markdown injections are wrapped in HTML-comment fences keyed on the
  tag: `<!-- BEGIN AI-HOOKER:<tag> --> ... <!-- END AI-HOOKER:<tag> -->`.
- Any pre-existing file we modify gets a one-time `<path>.bak` sibling on
  first patch.
