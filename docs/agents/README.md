# Agent reference

One file per supported AI coding harness. Each entry documents:

- **ID** — the string passed to `ai_hooker::by_id`.
- **Hooks** — install path and JSON/script format, broken out by scope.
- **Prompt** — optional rules-markdown surface, if the harness supports one.
- **MCP** and **Skills** — stubbed for harnesses where these surfaces exist
  upstream but are not yet implemented in `ai-hooker` (see [`CLAUDE.md`](../../CLAUDE.md)).

**Last updated:** 2026-04-25. Research verified against official documentation for all 12 harnesses. PR 1 (MCP for 5 agents) and PR 2 (Skills + Cline/Windsurf hooks + Windsurf MCP) implemented.

## Implemented

| Agent | Hooks | Prompt | MCP | Skills |
| ----- | ----- | ------ | --- | ------ |
| [Claude Code](claude.md)         | ✓ (Global + Local) | ✓ | ✓ (Global + Local) | ✓ (Global + Local) |
| [Cursor](cursor.md)              | ✓ (Global + Local) | -   | ✓ (Global + Local) | - (nightly) |
| [Gemini CLI](gemini.md)          | ✓ (Global + Local) | ✓ | ✓ (Global + Local) | -    |
| [Codex CLI](codex.md)            | ✓ (Global + Local) | ✓ | ✓ (Global + Local, TOML) | -    |
| [GitHub Copilot](copilot.md)     | ✓ (Local)          | ✓ | -    | -    |
| [OpenCode](opencode.md)          | ✓ (Global + Local) | ✓ | ✓ (Global + Local, array) | - (plugin-based) |
| [Cline](cline.md)                | ✓ (Local, v3.36+ scripts) | ✓ | - | - |
| [Roo Code](roo.md)               | -                    | ✓ | -    | -    |
| [Windsurf](windsurf.md)          | ✓ (Local) | ✓ | ✓ (Local) | - |
| [Kilo Code](kilocode.md)         | -                    | ✓ | -    | -    |
| [Google Antigravity](antigravity.md) | -                | ✓ | -    | ✓ (Global + Local) |

## Stubbed

| Agent | Why deferred |
| ----- | ------------ |
| [OpenClaw](openclaw.md) | Plugin contract requires manifest + CLI install (`openclaw plugins install`); doesn't fit the file-drop model used by other integrations. |

## Conventions

- A *scope* is either `Global` (the user's home dir) or `Local(<project>)`.
- All JSON entries we write include `"_ai_hooker_tag": "<your tag>"` so multiple
  consumers can coexist and each can find its own work to remove.
- Prompt-markdown injections are wrapped in HTML-comment fences keyed on the
  tag: `<!-- BEGIN AI-HOOKER:<tag> --> ... <!-- END AI-HOOKER:<tag> -->`.
- Any pre-existing file we modify gets a one-time `<path>.bak` sibling on
  first patch.
