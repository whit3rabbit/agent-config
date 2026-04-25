# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this crate is

A library that installs hooks (and, eventually, MCP servers and skills) into
AI coding harnesses. Consumers supply a `HookSpec`; the library knows where
and how each harness expects its config.

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

1. Caller builds a `HookSpec` (command, matcher, event, optional rules/script)
2. Looks up an `Integration` by id via `registry::by_id("claude")`
3. Calls `integration.install(scope, &spec)` or `uninstall(scope, tag)`

### Module map

- `lib.rs` — crate root, re-exports public API types
- `spec.rs` — `HookSpec`, `Matcher`, `Event`, `ScriptTemplate`, builder pattern
- `integration.rs` — `Integration` trait + `InstallReport`/`UninstallReport`/`MigrationReport`
- `scope.rs` — `Scope` enum (Global vs Local), `ScopeKind` discriminant
- `registry.rs` — `all()` and `by_id()` for looking up integrations
- `error.rs` — `HookerError` enum (IO, JSON, path resolution, unsupported scope, missing fields, invalid tag, backup collision)
- `paths.rs` — cross-platform resolution of harness config directories

### Agents (`src/agents/`)

Each file implements `Integration` for one harness. `PromptAgent` is a reusable struct for harnesses that only need project-local `.md` rules files (Cline, Roo, Windsurf, Kilo Code, Antigravity).

### Utility layer (`src/util/`)

Safety-critical primitives shared by all agents:
- `fs_atomic` — write-to-temp + fsync + rename; first-touch `.bak` backups; identical-content no-op
- `json_patch` — tagged insert/remove in JSON arrays (`_ai_hooker_tag` marker); key-order preserving; empty-array pruning
- `md_block` — HTML-comment fenced markdown blocks; upsert/remove with whitespace normalization

### Adding a new integration

1. Create `src/agents/<name>.rs` implementing `Integration`
2. If prompt-only (just writes an `.md` file), reuse `PromptAgent` with a new `rules_dir` constant
3. Add `pub mod <name>;` and `pub use` to `src/agents/mod.rs`
4. Add `Box::new(...)` to `registry::all()`
5. Add the id to the assertion list in `tests/registry.rs`
6. Add agent doc to `docs/agents/`

## Surface coverage

Per-agent reference is in [`docs/agents/`](docs/agents/README.md). The matrix:

|                      | Hooks | Prompt | MCP   | Skills |
| -------------------- | ----- | ------ | ----- | ------ |
| claude               | done  | done   | done  | done   |
| cursor               | done  | -      | done  | -      |
| gemini               | done  | done   | done  | -      |
| codex                | done  | done   | done  | -      |
| copilot              | done  | done   | -     | -      |
| opencode             | done  | -      | done  | -      |
| cline                | done  | done   | -     | -      |
| roo                  | -     | done   | -     | -      |
| windsurf             | done  | done   | done  | -      |
| kilocode             | -     | done   | -     | -      |
| antigravity          | -     | done   | -     | done   |
| openclaw             | TODO  | -      | TODO  | -      |

## Future-work checklist

### Phase 2 — MCP server registration — DONE

Implemented as a separate [`McpSurface`](src/integration.rs) trait
(non-breaking; agents that don't support MCP simply don't implement it). Use
[`registry::mcp_capable`](src/registry.rs) for discovery.

Per-agent locations now wired up:

- Claude: `~/.claude/mcp.json` (Global), `<root>/.mcp.json` (Local) —
  deliberately avoids `~/.claude.json` (the conversation transcript).
- Cursor: `~/.cursor/mcp.json` (Global), `<root>/.cursor/mcp.json` (Local).
- Gemini: `mcpServers` key in `settings.json` (shared with hooks; coexists
  via the named-object helper).
- Codex: `[mcp_servers.<name>]` tables in `config.toml`. Uses `toml_edit`
  to preserve user comments and key ordering across round trips.
- OpenCode: `mcp` array in `opencode.json` (keyed by `name` field per entry).
- Windsurf: `<root>/.windsurf/mcp_config.json`.

Ownership is recorded in a sidecar `<config-dir>/.ai-hooker-mcp.json` ledger
so multiple consumers coexist; uninstall returns
[`HookerError::NotOwnedByCaller`] on owner mismatch or hand-installed
entries.

### Phase 3 — Skills — DONE

Implemented as a separate [`SkillSurface`](src/integration.rs) trait. Use
[`registry::skill_capable`](src/registry.rs) for discovery.

Per-agent locations now wired up:

- Claude: `~/.claude/skills/<name>/` (Global), `<root>/.claude/skills/<name>/`
  (Local). `SKILL.md` (with YAML frontmatter), plus optional `scripts/`,
  `references/`, `assets/` subdirs.
- Antigravity: `~/.gemini/antigravity/skills/<name>/` (Global),
  `<root>/.agent/skills/<name>/` (Local). Same `SKILL.md` layout.

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

### Phase 4 — OpenClaw

See [`docs/agents/openclaw.md`](docs/agents/openclaw.md). Likely implemented
as a shell-out to `openclaw plugins install` rather than direct file
placement, which means it needs a different mechanism category in the trait.

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
- One file per consumer (no shared array) when the harness allows it (Copilot, OpenCode plugins, prompt-only agents). Avoids JSON-array contention and makes uninstall trivially safe.
- Install/uninstall are idempotent: same tag + same content = no-op. Multiple consumers coexist without conflict.

## Test discipline

- Every agent module has a `#[cfg(test)]` block exercising install + uninstall + idempotency in a tempdir.
- `tests/registry.rs` is the public-API smoke: every registered id is reachable, basic round-trip works.
- Run `cargo test` (not just `--lib`) before declaring anything done; the integration tests live in `tests/`.
- `unsafe_code` is forbidden; `missing_docs` is a warning.

## Things deliberately not in scope

- Auto-detecting which harnesses are installed on the host. Consumer's job.
- Shipping default markdown content. Consumer supplies it.
- A CLI binary. Lives downstream.
