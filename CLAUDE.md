# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this crate is

A library that installs hooks, MCP servers, prompt rules, and skills into AI
coding harnesses. Consumers supply a spec (`HookSpec`, `McpSpec`, or
`SkillSpec`); the library knows where and how each harness expects its config.

The crate is intentionally **generic** — no hardcoded commands, no embedded
prose, no consumer-specific identifiers.

## Build & test

```bash
cargo build                    # build the library
cargo test                     # run all tests (unit + integration)
cargo test --lib               # unit tests only
cargo test --test registry     # integration tests only
cargo test <test_name>         # run a single test by name
cargo doc --no-deps            # generate rustdoc
```

No external services or network required. All tests use `tempfile` for isolation.

## Architecture

### Core flow

1. Caller builds a `HookSpec`, `McpSpec`, or `SkillSpec`
2. Looks up an `Integration` by id via `registry::by_id("claude")`
3. Calls the matching surface method: `install`, `install_mcp`, or
   `install_skill`

### Module map

- `lib.rs` — crate root, re-exports public API types
- `spec.rs` — `HookSpec`, `McpSpec`, `SkillSpec`, builders, transports, events,
  matchers
- `integration.rs` — `Integration`, `McpSurface`, `SkillSurface` traits +
  report types
- `scope.rs` — `Scope` enum (Global vs Local), `ScopeKind` discriminant
- `registry.rs` — `all()` and `by_id()` for looking up integrations
- `error.rs` — `HookerError` enum (IO, JSON, path resolution, unsupported scope, missing fields, invalid tag, backup collision)
- `paths.rs` — cross-platform resolution of harness config directories

### Agents (`src/agents/`)

Each file implements `Integration` for one harness. Agents with extra surfaces
also implement `McpSurface` and/or `SkillSurface`. `PromptAgent` remains as a
small reusable prompt-only implementation for future harnesses, but registered
Roo and Kilo agents are dedicated modules because they also support MCP.

### Utility layer (`src/util/`)

Safety-critical primitives shared by all agents:
- `fs_atomic` — write-to-temp + fsync + rename; first-touch `.bak` backups; identical-content no-op
- `json_patch` — tagged insert/remove in JSON arrays (`_ai_hooker_tag` marker); key-order preserving; empty-array pruning
- `mcp_json_map` — ledger-backed MCP insertion into named JSON/JSONC/JSON5 objects
  (engine for all named-object MCP shapes); takes a path, builder fn, and format
- `mcp_json_object` — thin shim over `mcp_json_map` for the standard
  `mcpServers` shape (Claude/Cursor/Gemini/Cline/Roo/Windsurf/Antigravity)
- `yaml_mcp_map` — ledger-backed MCP insertion into named YAML objects
  (Hermes `mcp_servers`)
- `md_block` — HTML-comment fenced markdown blocks; upsert/remove with whitespace normalization

### Adding a new integration

For a copy-paste starting point, see [`templates/new-harness/`](templates/new-harness/README.md).

1. Create `src/agents/<name>.rs` implementing `Integration`
2. If prompt-only (just writes an `.md` file), consider reusing `PromptAgent`;
   if it also supports MCP or skills, create a dedicated module
3. Add `pub mod <name>;` and `pub use` to `src/agents/mod.rs`
4. Add `Box::new(...)` to `registry::all()`
5. If applicable, add it to `registry::mcp_capable()` or
   `registry::skill_capable()`
6. Add or update public smoke tests in `tests/registry.rs`,
   `tests/mcp_registry.rs`, or `tests/skill_registry.rs`
7. Add agent doc to `docs/agents/`

## Surface coverage

Per-agent reference is in [`docs/agents/`](docs/agents/README.md). The matrix:

|                      | Hooks | Prompt | MCP   | Skills |
| -------------------- | ----- | ------ | ----- | ------ |
| claude               | done  | done   | done  | done   |
| cursor               | done  | -      | done  | done   |
| gemini               | done  | done   | done  | done   |
| codex                | done  | done   | done  | done   |
| copilot              | done  | done   | done  | done   |
| opencode             | done  | -      | done  | done   |
| cline                | done  | done   | done  | done   |
| roo                  | -     | done   | done  | -      |
| windsurf             | done  | done   | done  | done   |
| kilocode             | -     | done   | done  | done   |
| antigravity          | -     | done   | done  | done   |
| openclaw             | -     | done   | done  | done   |
| hermes               | -     | done   | done  | done   |

## Future-work checklist

### Phase 2 — MCP server registration — DONE

Implemented as a separate [`McpSurface`](src/integration.rs) trait
(non-breaking; agents without a file-backed MCP contract simply don't
implement it). Use [`registry::mcp_capable`](src/registry.rs) for discovery.

Per-agent locations now wired up:

- Claude: `~/.claude.json` (Global), `<root>/.mcp.json` (Local).
- Cursor: `~/.cursor/mcp.json` (Global), `<root>/.cursor/mcp.json` (Local).
- Gemini: `mcpServers` key in `settings.json` (shared with hooks; coexists
  via the named-object helper).
- Codex: `[mcp_servers.<name>]` tables in `config.toml`. Uses `toml_edit`
  to preserve user comments and key ordering across round trips.
- Copilot: `~/.copilot/mcp-config.json` (Global), `<root>/.mcp.json` (Local),
  `mcpServers` object.
- OpenCode: object-based `mcp` in `~/.config/opencode/opencode.json`
  (Global) or `<root>/opencode.json` (Local). JSONC input is accepted.
- Cline: VS Code globalStorage
  `saoudrizwan.claude-dev/settings/cline_mcp_settings.json` (Global only).
- Roo: VS Code globalStorage
  `rooveterinaryinc.roo-cline/settings/mcp_settings.json` (Global),
  `<root>/.roo/mcp.json` (Local).
- Windsurf: `~/.codeium/windsurf/mcp_config.json` (Global),
  `<root>/.windsurf/mcp_config.json` (Local).
- Kilo Code: `~/.config/kilo/kilo.jsonc` (Global), `<root>/kilo.jsonc`
  or existing `<root>/.kilo/kilo.jsonc` (Local). JSONC input is accepted.
- Antigravity: `~/.gemini/antigravity/mcp_config.json` (Global),
  `<root>/.agent/mcp_config.json` (Local).
- OpenClaw: `~/.openclaw/openclaw.json` (Global only), `mcp.servers`
  object. JSON5 input is accepted and rewritten as strict JSON.
- Hermes: `~/.hermes/config.yaml` (Global only), `mcp_servers` object.

Ownership is recorded in a sidecar `<config-dir>/.ai-hooker-mcp.json` ledger
so multiple consumers coexist; install and uninstall return
[`HookerError::NotOwnedByCaller`] on owner mismatch or hand-installed
entries.

### Phase 3 — Skills — DONE

Implemented as a separate [`SkillSurface`](src/integration.rs) trait. Use
[`registry::skill_capable`](src/registry.rs) for discovery.

Per-agent locations now wired up:

- Claude: `~/.claude/skills/<name>/` (Global), `<root>/.claude/skills/<name>/`
  (Local). `SKILL.md` (with YAML frontmatter), plus optional `scripts/`,
  `references/`, `assets/` subdirs.
- Cursor: `~/.cursor/skills/<name>/` (Global), `<root>/.cursor/skills/<name>/`
  (Local).
- Gemini CLI: `~/.gemini/skills/<name>/` (Global),
  `<root>/.gemini/skills/<name>/` (Local).
- Codex: `~/.agents/skills/<name>/` (Global),
  `<root>/.agents/skills/<name>/` (Local).
- Copilot: `~/.copilot/skills/<name>/` (Global),
  `<root>/.github/skills/<name>/` (Local).
- OpenCode: `~/.config/opencode/skills/<name>/` (Global),
  `<root>/.opencode/skills/<name>/` (Local).
- Cline: `~/.cline/skills/<name>/` (Global),
  `<root>/.cline/skills/<name>/` (Local).
- Windsurf: `~/.codeium/windsurf/skills/<name>/` (Global),
  `<root>/.windsurf/skills/<name>/` (Local).
- Kilo Code: `~/.kilo/skills/<name>/` (Global),
  `<root>/.kilo/skills/<name>/` (Local).
- Antigravity: `~/.gemini/antigravity/skills/<name>/` (Global),
  `<root>/.agent/skills/<name>/` (Local). Same `SKILL.md` layout.
- OpenClaw: `~/.openclaw/skills/<name>/` (Global),
  `<root>/.agents/skills/<name>/` (Local).
- Hermes: `~/.hermes/skills/ai-hooker/<name>/` (Global only).

Sidecar ledger lives at `<skills_root>/.ai-hooker-skills.json`. Asset paths
must be relative — absolute paths and `..` segments are rejected at install
time to prevent directory escape.

### Newly implemented hook surfaces

- Cline v3.36+: per-event executable scripts at `.clinerules/hooks/<event>`,
  with ledger-protected ownership so multiple consumers don't overwrite each
  other's hooks.
- Windsurf: `<root>/.windsurf/hooks.json` with snake_case event keys
  (`pre_run_command`, `post_cascade_response`, etc.) and `_ai_hooker_tag`
  marked entries for multi-consumer coexistence.

### Phase 4 — OpenClaw And Hermes — DONE

See [`docs/agents/openclaw.md`](docs/agents/openclaw.md) and
[`docs/agents/hermes.md`](docs/agents/hermes.md). OpenClaw native
hook/plugin install remains deferred because it likely needs a shell-out to
`openclaw plugins install` rather than direct file placement.

### Phase 5 — Variants we deferred

- **Gemini shell-script delegator** — write `~/.gemini/hooks/<tag>.sh` plus
  a `.<tag>.sha256` integrity sidecar instead of an inline `command`.
  `util::fs_atomic::chmod` is already in place for this.
- **Copilot PowerShell variant** — pair `bash` with `powershell` for
  Windows-only consumers.
- **Cursor `beforeShellExecution`** — Cursor's regex-against-command event,
  more precise than `preToolUse` + `Shell` matcher. Already reachable via
  `Event::Custom("beforeShellExecution")` + `Matcher::Regex(...)`.

## Conventions

- Public errors: `thiserror`-typed (`HookerError`). Internal helpers may use `anyhow`. Never panic on user input.
- Atomic writes only (`util::fs_atomic::write_atomic`); never `std::fs::write` directly on a path the user owns.
- First-touch backups (`<path>.bak`) for any file we modify but did not create. Refuses to clobber an existing `.bak`.
- Idempotency markers:
  - JSON: `_ai_hooker_tag` field on every object we insert.
  - Markdown: HTML-comment fence `<!-- BEGIN AI-HOOKER:<tag> --> ... <!-- END AI-HOOKER:<tag> -->`.
- MCP and skills use sidecar ownership ledgers instead of embedding
  `_ai_hooker_tag` into harness-owned server or skill payloads.
- One file per consumer (no shared array) when the harness allows it (Copilot, OpenCode plugins, prompt-only agents). Avoids JSON-array contention and makes uninstall trivially safe.
- Install/uninstall are idempotent: same tag + same content = no-op. Multiple consumers coexist without conflict.

## Test discipline

- Every agent module has a `#[cfg(test)]` block exercising install + uninstall + idempotency in a tempdir.
- `tests/registry.rs` is the public-API smoke: every registered id is reachable, basic round-trip works.
- Run `cargo test` (not just `--lib`) before declaring anything done; the integration tests live in `tests/`.
- `unsafe_code` is forbidden; `missing_docs` is a warning.
- `#[allow(dead_code)]` markers (e.g. `src/util/mcp_json_array.rs`, the array helpers in `json_patch.rs`) are deliberate scaffolding kept for future surfaces — leave them alone unless explicitly asked to delete.

## Things deliberately not in scope

- Auto-detecting which harnesses are installed on the host. Consumer's job.
- Shipping default markdown content. Consumer supplies it.
- A CLI binary. Lives downstream.
