# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this crate is

A library that installs hooks, MCP servers, prompt rules, skills, and
standalone instruction files into AI coding harnesses. Consumers supply a spec
(`HookSpec`, `McpSpec`, `SkillSpec`, or `InstructionSpec`); the library knows
where and how each harness expects its config.

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
cargo clippy --all-targets     # lints lib + tests + examples
```

No external services or network required. All tests use `tempfile` for isolation.

### Examples

Runnable end-to-end programs live under `examples/`. Each writes into a fresh
tempdir so it never touches the host's real config:

```bash
cargo run --example hooks_install_uninstall
cargo run --example mcp_install
cargo run --example dry_run_plan
cargo run --example gen_schema      # regenerates schema/agents.json
```

See [`examples/README.md`](examples/README.md) for the full list.

### Golden fixtures

Per-agent config-shape fixtures live under `tests/golden/<surface>/<agent>/`.
Regenerate after adding a harness or changing serialization:

```bash
AGENT_CONFIG_UPDATE_GOLDENS=1 cargo test --test golden
```

Inspect the diff before committing.

### Concurrency and Docker notes

Concurrency coverage lives in `tests/concurrency.rs`, with helper-level stress
tests in the utility modules. Shared read-modify-write helpers should hold
`util::file_lock` locks at the helper boundary. Keep lock ordering stable:
config/root lock first, ledger lock second.

Linux container check used during the concurrency work:

```bash
docker run --rm --user "$(id -u):$(id -g)" \
  -e CARGO_HOME=/tmp/cargo \
  -e CARGO_TARGET_DIR=/tmp/agent-config-target \
  -e HOME=/tmp \
  -v "$PWD":/work -w /work \
  rust:latest \
  bash -c 'cargo test --locked --test concurrency'
```

The crate MSRV tracks the current release toolchain used by this repository
(`rust-version = "1.95"`). Keep the CI MSRV job and `Cargo.toml` in sync when
updating toolchains.

### Tests using `Scope::Global` on macOS

`safe_fs::write` for `Scope::Global` walks every path component for symlinks
(via `fs_atomic::reject_symlink_components`). macOS tempdirs live under
`/var/folders/...` where `/var → /private/var` is a symlink, so passing a
raw `tempdir().path()` as a fake `$HOME` fails with
`PathResolution("refusing to write through symlink")`. Canonicalize the
home path before use:

```rust
let home = tempfile::tempdir().unwrap();
let home_path = home.path().canonicalize().unwrap();  // resolves /var → /private/var
```

The existing `IsolatedGlobalEnv` helper in `tests/plan_api.rs` does not yet
canonicalize and currently fails on macOS. New global-scope tests should
canonicalize defensively.

## Architecture

### Core flow

1. Caller builds a `HookSpec`, `McpSpec`, `SkillSpec`, or `InstructionSpec`
2. Looks up an `Integration` / `McpSurface` / `SkillSurface` /
   `InstructionSurface` by id via `registry::by_id("claude")`,
   `registry::mcp_by_id("claude")`, etc.
3. Optionally calls the matching dry-run planner: `plan_install`,
   `plan_install_mcp`, `plan_install_skill`, or `plan_install_instruction`
4. Calls the matching mutation method: `install`, `install_mcp`,
   `install_skill`, or `install_instruction`

### Module map

- `lib.rs` — crate root, re-exports public API types
- `plan.rs` — side-effect-free plan report types, planned changes, statuses,
  warnings, and refusal reasons
- `spec.rs` / `spec/` — `HookSpec`, `McpSpec`, `SkillSpec`, `InstructionSpec`,
  builders, transports, events, matchers, instruction placements
- `integration.rs` — `Integration`, `McpSurface`, `SkillSurface`,
  `InstructionSurface` traits + report types and plan methods
- `scope.rs` — `Scope` enum (Global vs Local), `ScopeKind` discriminant
- `registry.rs` — `all()` and `by_id()` for looking up integrations
- `status.rs` — status reports, drift issues, and current-state probes
- `validation.rs` — `ValidationReport` + `SuggestedAction`; drift validation
  alongside status, answering whether on-disk state is consistent enough to
  repair or mutate safely
- `schema.rs` — live JSON manifest of every registered agent's file layout,
  surfaces, and marker conventions; rendered by `examples/gen_schema.rs` to
  `schema/agents.json`
- `error.rs` — `AgentConfigError` enum (IO, JSON, path resolution, unsupported
  scope, missing fields, invalid tag, backup collision, `ConfigTooLarge`)
- `paths.rs` — cross-platform resolution of harness config directories

### Agents (`src/agents/`)

Each file implements `Integration` for one harness. Agents with extra surfaces
also implement `McpSurface` and/or `SkillSurface`. `PromptAgent` remains as a
small reusable prompt-only implementation for future harnesses, but registered
Roo and Kilo agents are dedicated modules because they also support MCP.
`src/agents/planning.rs` contains adapters from agent path resolution to the
shared dry-run helpers.

### Utility layer (`src/util/`)

Safety-critical primitives shared by all agents:
- `safe_fs` — scope-aware `write` / `remove_file` / `remove_dir_all`; the
  entry point for any helper touching a path the user owns
- `fs_atomic` — write-to-temp + fsync + rename; first-touch `.bak` backups;
  identical-content no-op; bounded reads via `read_capped` (8 MiB)
- `planning` — pure dry-run helpers for file writes/removals, backups,
  permissions, ledger changes, tagged JSON, and markdown blocks
- `ownership` — sidecar ownership ledgers (`.agent-config-mcp.json`,
  `.agent-config-skills.json`, `.agent-config-instructions.json`); v2 schema
  with SHA-256 content hashes for drift detection
- `json_patch/` — tagged insert/remove for JSON arrays/objects
  (`_agent_config_tag` marker); key-order preserving; empty-array pruning;
  shared probe helpers under `status_probe`
- `mcp_json_map` — ledger-backed MCP insertion into named JSON/JSONC/JSON5
  objects (engine for all named-object MCP shapes); takes a path, builder
  fn, and format
- `mcp_json_object` — thin shim over `mcp_json_map` for the standard
  `mcpServers` shape (Claude/Cursor/Gemini/Cline/Roo/Windsurf/Antigravity)
- `json5_patch` — JSON5 input acceptance; rewrites as strict JSON
- `toml_patch` — `toml_edit`-backed insertion into TOML tables (Codex
  `[mcp_servers.<name>]`); preserves comments and key ordering
- `yaml_mcp_map` — ledger-backed MCP insertion into named YAML objects
  (Hermes `mcp_servers`)
- `md_block` — HTML-comment fenced markdown blocks; upsert/remove with
  whitespace normalization

**Scope threading is split by surface.** `instructions_dir::install/uninstall`
take `&Scope` and route writes through `safe_fs` (symlink-aware).
`skills_dir` and the MCP helpers take paths only and rely on the agent layer
to call `scope.ensure_contained()` before delegating. Both patterns are
intentional; match the surrounding surface when extending.

### Dry-run planning

Every public surface has a side-effect-free planner:

- Hooks: `Integration::plan_install` and `Integration::plan_uninstall`
- MCP: `McpSurface::plan_install_mcp` and `McpSurface::plan_uninstall_mcp`
- Skills: `SkillSurface::plan_install_skill` and
  `SkillSurface::plan_uninstall_skill`
- Instructions: `InstructionSurface::plan_install_instruction` and
  `InstructionSurface::plan_uninstall_instruction`

Plans return `InstallPlan` or `UninstallPlan` with a `PlanTarget`, a list of
`PlannedChange` values, a `PlanStatus`, and warnings. The root public
`PlanStatus` name refers to planning, while root `InstallStatus` refers to
actual on-disk status. Status-registry `PlanTarget` is re-exported as
`StatusPlanTarget`.

Planning must read current state, compute desired state in memory, compare the
two, and emit planned changes. Do not implement dry-run by partially running
install or uninstall code. Predictable refusals return `Ok(plan)` with
`PlanStatus::Refused`; path resolution failures, unreadable files, and
invalid caller identifiers remain `Err(AgentConfigError)`.

Ledger effects are represented with `WriteLedger` and `RemoveLedgerEntry`, not
duplicated as generic file writes. Permission changes are represented with
`SetPermissions`.

### Status reporting

- `Integration::status(scope, tag)`,
  `McpSurface::mcp_status(scope, name, expected_owner)`,
  `SkillSurface::skill_status(scope, name, expected_owner)`,
  `InstructionSurface::instruction_status(scope, name, expected_owner)`.
- `expected_owner` is the consumer tag the caller compares against — matching
  ledger entries become `InstalledOwned`, mismatches become
  `InstalledOtherOwner`. Default `is_*_installed` wrappers pass `self.id()` as
  a sentinel so any real consumer owner routes through `OtherOwner` and the
  boolean fold preserves "any owner" semantics.
- Hooks have no separate ledger (the tag IS the owner), so
  `Integration::status` takes only `(scope, tag)`.
- Status probes (`util::*::config_presence`, `tagged_hook_presence`, etc.)
  catch parse failures and return `ConfigPresence::Invalid { reason }` so
  `*_status` surfaces them as `DriftIssue::InvalidConfig`. Never propagate
  `AgentConfigError::JsonInvalid`/`TomlInvalid` from a status probe.

### Adding a new integration

For a copy-paste starting point, see [`templates/new-harness/`](templates/new-harness/README.md).

Every entry in `mcp_capable()` and `skill_capable()` must also appear in
`all()` (enforced by `tests/{mcp,skill}_registry.rs::*_subset_of_all_integrations`).
A harness with skills or MCP but no documented hook/prompt surface cannot
be registered without speculating on a rules file — defer it instead. See
"Deferred harnesses" below.

1. Create `src/agents/<name>.rs` implementing `Integration`
2. If prompt-only (just writes an `.md` file), consider reusing `PromptAgent`;
   if it also supports MCP or skills, create a dedicated module
3. Add `pub mod <name>;` and `pub use` to `src/agents/mod.rs`
4. Add `Box::new(...)` to `registry::all()`
5. If applicable, add it to `registry::mcp_capable()` or
   `registry::skill_capable()`
6. Implement the matching dry-run plan methods for hooks, MCP, and/or skills
7. Add or update public smoke tests in `tests/registry.rs`,
   `tests/mcp_registry.rs`, `tests/skill_registry.rs`, or `tests/plan_api.rs`
8. Add agent doc to `docs/agents/`

## Surface coverage

Per-agent reference is in [`docs/agents/`](docs/agents/README.md). The
release-facing path contract is in
[`docs/support-matrix.md`](docs/support-matrix.md). The surface matrix:

|                      | Hooks | Prompt | MCP   | Skills | Instructions |
| -------------------- | ----- | ------ | ----- | ------ | ------------ |
| claude               | done  | done   | done  | done   | done (ReferencedFile) |
| cursor               | done  | -      | done  | done   | -            |
| gemini               | done  | done   | done  | done   | done (InlineBlock) |
| codex                | done  | done   | done  | done   | done (InlineBlock) |
| copilot              | done  | done   | done  | done   | done (Local, InlineBlock) |
| opencode             | done  | -      | done  | done   | -            |
| cline                | done  | done   | done  | done   | done (Local, StandaloneFile) |
| roo                  | -     | done   | done  | -      | done (Local, StandaloneFile) |
| windsurf             | done  | done   | done  | done   | done (Local, StandaloneFile) |
| kilocode             | -     | done   | done  | done   | done (Local, StandaloneFile) |
| antigravity          | -     | done   | done  | done   | done (Local, StandaloneFile) |
| openclaw             | -     | done   | done  | done   | done (Local, InlineBlock) |
| hermes               | -     | done   | done  | done   | done (Local, InlineBlock) |
| amp                  | -     | done   | done  | done   | done (InlineBlock) |
| codebuddy            | done  | done   | -     | done   | done (InlineBlock) |
| forge                | -     | done   | done  | done   | done (InlineBlock) |
| iflow                | done  | -      | done  | -      | -            |
| junie                | -     | done   | done  | -      | done (Local, InlineBlock) |
| qodercli             | -     | done   | done  | -      | done (InlineBlock) |
| qwen                 | -     | done   | done  | done   | done (InlineBlock) |
| tabnine              | done  | -      | done  | -      | -            |
| trae                 | -     | done   | -     | done   | done (Local, InlineBlock) |

## Instruction surface

Three placement modes ([`InstructionPlacement`](src/spec/instruction.rs)):

- **`ReferencedFile`** — write `<config_dir>/<NAME>.md` and inject a managed
  `@<NAME>.md` include into the harness's memory file (Claude only — has a
  documented `@import` syntax).
- **`InlineBlock`** — inject the body as a tagged HTML-comment fenced block
  inside the harness's existing memory file (Codex, Gemini, Copilot,
  CodeBuddy, Amp, Forge, Qoder, Qwen, Junie, Trae, OpenClaw, Hermes).
- **`StandaloneFile`** — write `<rules-dir>/<NAME>.md` only, no host edit
  (Cline, Roo, Kilo Code, Windsurf, Antigravity — agents whose memory model
  is a per-file rules dir).

**Per-placement shim helpers** in `util::instructions_dir`:
`inline_{status, plan_install, plan_uninstall, install, uninstall}` and
`standalone_*` collapse the `InstructionSurface` impl to ~35 lines per
agent. The agent supplies an `InlineLayout { config_dir, host_file }` or
`StandaloneLayout { config_dir, instruction_dir }`; the shim handles
validation, scope containment, status detection (`md_block::contains` for
inline, file-exists for standalone), and converts `UnsupportedScope` from
layout resolution into `RefusalReason::UnsupportedScope` in plan methods.
Mutating `*_install` / `*_uninstall` propagate the error. Claude's
`ReferencedFile` impl is the only direct caller of the lower-level
`instructions_dir::{install, uninstall, plan_install, plan_uninstall}` —
its dual-file behavior (standalone file + `@import` line in host) didn't
warrant a third shim for one consumer.

## Conventions

- Public errors: `thiserror`-typed (`AgentConfigError`). Internal helpers may use `anyhow`. Never panic on user input.
- Every mutating method (`install`, `uninstall`, `install_mcp`, `install_skill`, etc.) must call `scope.ensure_contained(&path)?` before touching disk. `Scope::Global` rejects symlinked target files; `Scope::Local` rejects symlink components and canonicalized escapes. Skipping it opens a symlink-traversal hole. See `docs/SECURITY.md`.
- Cross-process file locks use `file_lock::with_lock(&path, || { ... Ok::<(), AgentConfigError>(()) })?;` (closure pattern). Drop the closure before locking a different file.
- Integration writes/removals go through `util::safe_fs` (`safe_fs::write`, `safe_fs::remove_file`, `safe_fs::remove_dir_all`); lower-level helpers inside `util` may call `fs_atomic` after their callers validate paths. Never `std::fs::write` directly on a path the user owns.
- File reads of caller-influenced paths go through `fs_atomic::read_capped` (8 MiB cap). Anything larger surfaces as `AgentConfigError::ConfigTooLarge` rather than allocating unbounded memory. New util code must follow this.
- First-touch backups (`<path>.bak`) for any file we modify but did not create. Refuses to clobber an existing `.bak`.
- Idempotency markers:
  - JSON: `_agent_config_tag` field on every object we insert.
  - Markdown: HTML-comment fence `<!-- BEGIN AGENT-CONFIG:<tag> --> ... <!-- END AGENT-CONFIG:<tag> -->`.
- MCP, skills, and instructions use sidecar ownership ledgers instead of
  embedding `_agent_config_tag` into harness-owned server, skill, or
  instruction payloads. Per-surface ledger filenames:
  `.agent-config-mcp.json`, `.agent-config-skills.json`,
  `.agent-config-instructions.json`.
- One file per consumer (no shared array) when the harness allows it (Copilot, OpenCode plugins, prompt-only agents). Avoids JSON-array contention and makes uninstall trivially safe.
- Install/uninstall are idempotent: same tag + same content = no-op. Multiple consumers coexist without conflict.
- Plan generation is side-effect-free. It must not create config files,
  ledgers, backups, directories, or chmod targets.
- v1 shell quoting is POSIX-only. `HookCommand::render_shell()` produces
  `sh`/`bash`-style quoting; integrations storing the rendered string in
  harness JSON (Copilot, Windsurf, Gemini, etc.) inherit POSIX semantics.
  A `ShellKind`/PowerShell rendering abstraction is deferred (see "Deferred
  work" below). Cline writes a `bash`-shebanged script and is refused on
  native Windows with `RefusalReason::UnsupportedPlatform` /
  `AgentConfigError::UnsupportedPlatform`.

## Test discipline

- Every agent module has a `#[cfg(test)]` block exercising install + uninstall + idempotency in a tempdir.
- `tests/registry.rs` is the public-API smoke: every registered id is reachable, basic round-trip works.
- `tests/plan_api.rs` is the dry-run public-API smoke and acceptance suite.
- Run `cargo test` (not just `--lib`) before declaring anything done; the integration tests live in `tests/`.
- `cargo clippy --lib --tests -- -D warnings` should be clean. If it warns, fix it.
- `unsafe_code` is forbidden; `missing_docs` is a warning.
- `#[allow(dead_code)]` markers (e.g. `src/util/mcp_json_array.rs`, the array helpers in `json_patch.rs`) are deliberate scaffolding kept for future surfaces — leave them alone unless explicitly asked to delete.
- If you encounter compile errors in files you didn't touch (e.g., a helper signature changed mid-session), fix only the call-site cascade required to compile. Don't speculatively complete an in-flight refactor.

## Things deliberately not in scope

- Auto-detecting which harnesses are installed on the host. Consumer's job.
- Shipping default markdown content. Consumer supplies it.
- A CLI binary. Lives downstream.
- A `ShellKind`/PowerShell rendering abstraction in `HookCommand`. v1 is
  POSIX-only; users targeting non-POSIX shells use `ShellUnchecked`.
- Auto-targeting Windows host config from inside WSL. A binary running in
  WSL writes WSL config; reaching `/mnt/c/Users/.../AppData/...` would need
  an explicit `GlobalTarget` / `PathRoots` API and is intentionally not
  built into the library today.
- Refactoring `paths::*` to take an injected `EnvView` instead of reading
  process env directly. Path tests serialize via `tests/common::env_lock`;
  that's enough until a downstream consumer needs runtime path injection.

## Supported platforms

- **Native macOS / Linux** — full support.
- **Native Windows** — supported wherever the harness has a documented
  Windows config path. `paths::home_dir()` honors `%USERPROFILE%` and
  `paths::config_dir()` honors `%APPDATA%`. Cline hooks are refused
  (POSIX shell required); Cline rules / MCP / skills / instructions still
  install. Coverage lives in `tests/windows_paths.rs` (gated
  `#[cfg(windows)]`).
- **WSL** — treated as a Linux environment.

See `README.md` "Supported platforms" and `docs/SECURITY.md` "Platform
notes" for the user-facing contract.

## Deferred work

### Variants of supported harnesses

Reachable via existing types but no first-party shim ships:

- **Gemini shell-script delegator** — write `~/.gemini/hooks/<tag>.sh` plus
  a `.<tag>.sha256` integrity sidecar instead of an inline `command`.
  `util::fs_atomic::chmod` is already in place for this.
- **Copilot PowerShell variant** — pair `bash` with `powershell` for
  Windows-only consumers.
- **Cursor `beforeShellExecution`** — Cursor's regex-against-command event,
  more precise than `preToolUse` + `Shell` matcher. Already reachable via
  `Event::Custom("beforeShellExecution")` + `Matcher::Regex(...)`.
- **OpenClaw native hook/plugin install** — likely needs a shell-out to
  `openclaw plugins install` rather than direct file placement.

### Harnesses

Researched 2026-04-26 against spec-kit's supported list. The following
harnesses are intentionally not registered. The architecture requires every
agent in `registry::all()` to implement `Integration` (hooks or prompt rules),
so harnesses with neither documented surface — even if they have skills or
MCP — cannot be registered without speculating about file paths.

| ID       | Reason                                                                 |
| -------- | ---------------------------------------------------------------------- |
| auggie   | MCP and prompt-rules file contracts not in public docs                 |
| bob      | IDE-primary; only MCP partially documented; no skills/hooks contract   |
| goose    | YAML recipes don't fit the `SkillSurface` model; needs separate design |
| kimi     | TOML MCP and skills documented, but no rules/hook surface; would need a speculative `Integration` impl |
| kiro-cli | Hooks live in per-event JSON files and MCP in agent-config files; both shapes need bespoke ledgers |
| pi       | MCP only via the optional `pi-mcp-adapter` extension package           |
| shai     | Per-agent config-file format not publicly documented                   |
| vibe     | Skills documented, but no rules/hook surface                           |
