# Adding a new harness

Copy `agent.rs` to `src/agents/<id>.rs`, copy `agent.md` to
`docs/agents/<id>.md`, fill in the placeholders, register in two places, write
a smoke test. Done.

This file walks through the parts that are not obvious. The two reference
implementations are:

- `src/agents/claude.rs` — JSON throughout, all four surfaces.
- `src/agents/codex.rs` — JSON hooks, **TOML** MCP, all four surfaces.

If your harness's MCP shape is JSON, follow Claude. If it's TOML, follow
Codex. Everything else is shared.

## What you are implementing

A "harness" is one AI coding agent (Claude Code, Codex, Cursor, etc.). Each
harness exposes up to four surfaces:

| Surface       | Trait                            | Required? | Skip if...                                  |
| ------------- | -------------------------------- | --------- | ------------------------------------------- |
| Hooks         | `Integration`                    | Yes       | (never skip; this is the trait)             |
| Prompt/rules  | `HookSpec::rules` field          | Optional  | harness has no `<NAME>.md`-style file       |
| MCP servers   | `McpSurface`                     | Optional  | MCP is CLI-managed (e.g., OpenClaw plugins) |
| Skills        | `SkillSurface`                   | Optional  | harness has no skills concept               |

Each implemented surface has six methods to wire up:

- `id`, `supported_*_scopes` (declarative).
- `*_status` — read-only probe used to render install state and drive
  `is_*_installed`.
- `plan_install_*` and `plan_uninstall_*` — side-effect-free preview, must
  not write to disk.
- `install_*` and `uninstall_*` — the actual mutation, atomic + idempotent.

Trait definitions live in `src/integration.rs`.

## Checklist

1. `cp templates/new-harness/agent.rs src/agents/<id>.rs`
2. Replace the placeholders: `Myagent` → `<Yourname>`, `MyagentAgent` →
   `<Yourname>Agent`, `myagent` → `<id>`, `MyAgent` → `<Display Name>`.
3. Decide which surfaces apply. Delete the `impl McpSurface` and/or
   `impl SkillSurface` blocks (and the corresponding helpers + imports) you
   don't need. If your harness has *no* hooks but does have prompt rules,
   replace the hooks-aware `install`/`uninstall` body with the markdown-only
   shape (see `agents::qwen` or `agents::openclaw`).
4. Fill in the path helpers (`hooks_path`, `rules_path`, `mcp_path`,
   `skills_root`) for both `Scope::Global` and `Scope::Local`.
5. Adjust `matcher_to_<id>` and `event_to_string` to your harness's syntax.
6. Confirm every mutating method (`install`, `uninstall`, `install_mcp`,
   `uninstall_mcp`, `install_skill`, `uninstall_skill`) calls
   `scope.ensure_contained(&path)?` before touching disk.
7. Register in `src/agents/mod.rs` (one `pub mod`, one `pub use`) and
   `src/registry.rs` (one entry per surface you implemented). Skill-only
   and MCP-only agents must still implement `Integration` because
   `tests/skill_registry.rs` and `tests/mcp_registry.rs` assert
   subset-of-`all()`.
8. `cp templates/new-harness/agent.md docs/agents/<id>.md` and fill it in
   (date, doc URLs, surfaces, format example).
9. Add a smoke test entry in `tests/registry.rs`, `tests/mcp_registry.rs`,
   `tests/skill_registry.rs`, and/or `tests/plan_api.rs` (only the suites
   you participate in).
10. Regenerate golden fixtures:
    `AGENT_CONFIG_UPDATE_GOLDENS=1 cargo test --test golden`. Inspect the
    diff before committing.
11. Run `cargo test` and `cargo clippy --all-targets -- -D warnings`.
    Update `docs/agents/README.md` matrix and the surface-coverage table
    in `CLAUDE.md`.

## Scoping

Every path helper takes `&Scope` and returns the same path with two arms:

```rust
fn settings_path(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
    Ok(match scope {
        Scope::Global    => paths::claude_home()?.join("settings.json"),
        Scope::Local(p)  => p.join(".claude").join("settings.json"),
    })
}
```

`Scope::Global` resolves to a user-wide path (typically `~/.<id>/`).
`Scope::Local(<root>)` resolves to a project directory.

If your harness has a stable `~/.<id>/` location, add a helper to
`src/paths.rs` next to `claude_home()` / `codex_home()` and call it. For
ad-hoc cases, `paths::home_dir()?.join(".<id>")` is fine. Codex respects
`$CODEX_HOME` for relocatable installs (`paths::codex_home()`); follow that
pattern if your harness has the same.

`supported_scopes()` declares which scopes the agent accepts. Returning only
`&[ScopeKind::Local]` makes calls with `Scope::Global` fail with
`AgentConfigError::UnsupportedScope`. Examples: Cline (Global only), Copilot
(Local only), OpenClaw (Local prompt only, Global MCP only).

## Hooks

Both Claude and Codex use the **same JSON envelope**. The only meaningful
difference is the matcher mapping: Claude uses `"Bash"`, Codex uses
`"shell"`.

What ends up on disk (Claude shape, identical for Codex except the matcher
string and the file path):

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "myapp hook claude" }],
        "_agent_config_tag": "myapp"
      }
    ]
  }
}
```

The `_agent_config_tag` field is what lets multiple consumers coexist. Never
omit it.

The `install` body in `agent.rs` is the canonical pattern:

1. `scope.ensure_contained(&path)?` — symlink/path-traversal defense. For
   `Scope::Local`, refuses to mutate any path whose existing components
   include a symlink or canonicalize outside the project root. For
   `Scope::Global`, refuses symlinked target files. Always call this
   BEFORE locking the file or touching it.
2. `file_lock::with_lock(&path, || { ... })` — acquires a cross-process
   file lock for the closure body and drops it on exit. Inside the closure:
3. `json_patch::read_or_empty(&path)` to load (returns empty `Value` if
   missing).
4. `json_patch::upsert_tagged_array_entry(&mut root, &["hooks", &event_key],
   &spec.tag, entry)` — idempotent insert, returns `true` if anything
   changed.
5. `safe_fs::write(scope, &path, &bytes, true)` — atomic rename, writes a
   one-time `.bak` sibling on first patch (the `true` flag).

Uninstall mirrors it: `remove_tagged_array_entries_under` plus
`safe_fs::restore_backup_if_matches` if removing our entry leaves the file
empty and the backup already matches the desired post-uninstall bytes
(stale backups are left in place rather than overwriting user changes).

The matcher/event translators are where the harness-specific knowledge
lives. The full mapping table goes in `docs/agents/<id>.md`.

## Prompt/rules markdown

Same machinery for everyone; only the destination filename differs:

- Claude / CodeBuddy → `CLAUDE.md`
- Codex / OpenClaw / Amp / Forge / Junie / Qoder → `AGENTS.md`
- Gemini → `GEMINI.md`
- Qwen → `QWEN.md`
- Hermes → `.hermes.md`
- Trae → `.trae/project_rules.md`
- ...

If `spec.rules` is `Some(...)`, install upserts a fenced HTML-comment block
into the file:

```markdown
<!-- BEGIN AGENT-CONFIG:myapp -->
Use myapp prefix.
<!-- END AGENT-CONFIG:myapp -->
```

`md_block::upsert(host, tag, content)` produces the new file body;
`md_block::remove(host, tag)` strips it. If the file becomes empty after
removal, `safe_fs::restore_backup_if_matches` can restore a matching backup
without rolling back unrelated edits.

If your harness has no rules/memory file, delete the `if let Some(rules)`
block in `install`, the matching cleanup in `uninstall`, and the
`rules_path` helper.

### Variant: prompt-only Integration (no hooks)

For harnesses with no documented hook surface but a documented prompt-rules
file (e.g., Amp, Forge, Qwen, Qoder, Junie, Trae, OpenClaw, Hermes), the
`Integration` impl reduces to a single-file markdown upsert. Replace the
template's hooks-aware body with the openclaw/qwen/junie shape:

- `install`: require `spec.rules` (return `AgentConfigError::MissingSpecField`
  if absent), then upsert via `md_block::upsert` and `safe_fs::write`.
- `status`: use `StatusReport::for_markdown_block_hook(tag, path)`.
- `plan_install` / `plan_uninstall`: delegate to
  `agent_planning::markdown_install` / `markdown_uninstall`, which handle
  the `MissingRequiredSpecField` and `UnsupportedScope` refusals for you.

If the prompt is project-only (Junie, OpenClaw, Hermes, Trae), declare
`supported_scopes()` as `&[ScopeKind::Local]` and gate `rules_path` with a
`require_local` helper (see `agents::openclaw` and `agents::junie`).

## MCP servers

This is the one surface where Claude and Codex differ.

### Variant A: JSON shape — `{"mcpServers": {...}}` (Claude pattern)

If your harness reads MCP servers from a JSON object map keyed by name,
delegate everything to `mcp_json_object`. The whole `McpSurface` impl is
under 30 lines:

```rust
fn install_mcp(&self, scope: &Scope, spec: &McpSpec) -> Result<InstallReport, AgentConfigError> {
    spec.validate()?;
    let cfg    = Self::mcp_path(scope)?;
    let ledger = ownership::mcp_ledger_for(&cfg);
    mcp_json_object::install(&cfg, &ledger, spec)
}
```

This is what the template ships with. Use it for any harness whose MCP file
looks like:

```json
{ "mcpServers": { "my-server": { "command": "node", "args": [...] } } }
```

(Cursor, Gemini, Cline, Roo, Windsurf, Kilo, Antigravity all use this
shape under different filenames.)

### Variant B: TOML shape — `[mcp_servers.<name>]` (Codex pattern)

If your harness uses TOML, you cannot delegate; you build a `Table` and call
`toml_patch`. The reference is `src/agents/codex.rs`. Key moves:

- `toml_patch::read_or_empty(&cfg)` to load (returns empty doc if missing).
- `toml_patch::contains_named_table(&doc, &["mcp_servers"], &spec.name)`
  to detect prior state.
- `ownership::require_owner(...)` before mutating, so you refuse to clobber
  another consumer's entry.
- `toml_patch::upsert_named_table(&mut doc, &["mcp_servers"], &spec.name,
  build_mcp_table(spec))` for the write.
- `toml_patch::to_string(&doc)` preserves user comments and key ordering.
- `ownership::record_install(&ledger, &spec.name, &spec.owner_tag, hash)`
  updates the sidecar ledger. The 4th arg is an `Option<&str>` content hash
  for drift detection; pass `Some(ownership::content_hash(&serialized))`
  on writes that changed the file, or `ownership::file_content_hash(&cfg)?`
  when reusing the existing on-disk content.

The `build_mcp_table` helper in `codex.rs` handles the three `McpTransport`
variants (Stdio, Http, Sse). Copy it verbatim if you go this route.

### Other shapes

- **Object map under a non-`mcpServers` key** (Copilot's `servers`,
  OpenCode's `mcp`): see `src/agents/copilot.rs` and
  `src/agents/opencode.rs` — they use `mcp_json_map` directly.
- **JSONC** (Kilo): see `src/agents/kilocode.rs`.
- **JSON5** (OpenClaw): see `src/agents/openclaw.rs`.
- **YAML map** (Hermes `mcp_servers`): see `src/agents/hermes.rs`, which
  goes through `yaml_mcp_map`.
- **VS Code globalStorage** paths (Cline, Roo): see `src/agents/cline.rs`.

### Universal MCP rules

- **Always use the ownership ledger.** Never embed `_agent_config_tag` in the
  MCP payload itself; the harness owns that file. The ledger lives at
  `<config-dir>/.agent-config-mcp.json` and is auto-managed by
  `mcp_json_object` / `ownership::record_install`.
- Uninstall returns `AgentConfigError::NotOwnedByCaller` on owner mismatch or
  hand-installed entries. This is the contract; don't work around it.
- **Ledger v2 records a content hash** alongside the owner. The standard
  helpers (`mcp_json_object::install`, `mcp_json_map::install`,
  `yaml_mcp_map::install`, `skills_dir::install`) compute and persist the
  SHA-256 hex digest automatically. Custom impls (TOML, JSONC, JSON5) must
  pass it explicitly via the 4th arg to `ownership::record_install`.
  See `docs/SECURITY.md` for the drift-detection contract.

## Skills

Most agents implement skills. The implementation is a thin wrapper over
`skills_dir`. The only thing your agent decides is `skills_root()`:

```rust
fn skills_root(scope: &Scope) -> Result<PathBuf, AgentConfigError> {
    Ok(match scope {
        Scope::Global   => paths::claude_home()?.join("skills"),
        Scope::Local(p) => p.join(".claude").join("skills"),
    })
}
```

Each skill becomes a directory with a required `SKILL.md` (YAML frontmatter
+ markdown body) and optional `scripts/`, `references/`, `assets/` subdirs.
Asset paths must be relative; absolute paths and `..` segments are rejected
at install time.

Delete the `impl SkillSurface` block (and the `skills_root` helper +
`skills_dir` import) if your harness has no skills concept.

## Status reporting

Every implemented surface needs a `*_status(scope, ...)` method. The
template returns a `StatusReport` (defined in `src/status.rs`) built from
the shared probe helpers:

- **Tagged JSON hooks** (Claude/Codex/Gemini envelope):
  `json_patch::tagged_hook_presence` → `StatusReport::for_tagged_hook`.
- **Markdown-block-only agents** (no hooks, prompt fence is the surface):
  `StatusReport::for_markdown_block_hook(tag, path)` does the whole probe
  in one call — checks the file exists, the fence is well-formed, and the
  tagged block is present.
- **Per-file hooks** (Roo's `.roo/rules/<tag>.md`):
  `StatusReport::for_file_hook`.
- **MCP (`mcpServers` JSON map)**: `mcp_json_object::config_presence` plus
  `ownership::owner_of` → `StatusReport::for_mcp`.
- **Skills**: `skills_dir::paths_for_status` plus `ownership::owner_of` →
  `StatusReport::for_skill`.

Status probes must catch parse failures and surface them as
`ConfigPresence::Invalid { reason }` (the helpers above already do this).
Do not propagate `AgentConfigError::JsonInvalid` from a status method — the
`validate_*` defaults turn that into a structured `DriftIssue` for the
caller.

The default `is_installed` / `is_mcp_installed` / `is_skill_installed`
methods fold the status into a bool, so you usually do not need to override
them.

## Dry-run plan API

Every implemented surface also needs `plan_install_*` and
`plan_uninstall_*`. Plans are side-effect-free: they must not create
config files, ledgers, backups, directories, or chmod targets — only
return a `Vec<PlannedChange>` describing what the mutation *would* do.

For straightforward shapes the template wires the planners through the
shared helpers in `src/util/planning.rs`:

- `plan_tagged_json_upsert` / `plan_tagged_json_remove_under` — tagged
  hook arrays.
- `plan_markdown_upsert` / `plan_markdown_remove` — fenced rules blocks.
- `plan_write_file`, `plan_remove_file`, `plan_restore_backup_or_remove` —
  raw file operations.
- `plan_write_ledger` / `plan_remove_ledger_entry` — sidecar ledger edits.
- `plan_set_permissions` — chmod planning (no-op on non-Unix).

MCP and skill helpers expose pre-baked planners
(`mcp_json_object::plan_install`, `skills_dir::plan_install`, etc.) that
the template already calls.

`src/agents/planning.rs` exposes adapters (`agent_planning::rules_install`,
`markdown_install`, JSON-object/JSON-map/YAML MCP planners, skills) that
build the `PlanTarget` + `RefusalReason` boilerplate for you. Prefer those
over hand-rolling refusal handling when your agent uses one of the standard
shapes.

After all surface planners produce their `Vec<PlannedChange>`, wrap them
with `InstallPlan::from_changes(target, changes)` /
`UninstallPlan::from_changes(target, changes)`. Use the
`plan::has_refusal` predicate to early-return when the first phase already
refused.

## Registering

In `src/agents/mod.rs`:

```rust
pub mod myagent;
pub use myagent::MyagentAgent;
```

In `src/registry.rs`, add one entry to each list your agent participates in:

```rust
pub fn all() -> Vec<Box<dyn Integration>> {
    vec![
        // ...existing entries...
        Box::new(MyagentAgent::new()),
    ]
}

// Add only if you implemented McpSurface:
pub fn mcp_capable() -> Vec<Box<dyn McpSurface>> { /* same shape */ }

// Add only if you implemented SkillSurface:
pub fn skill_capable() -> Vec<Box<dyn SkillSurface>> { /* same shape */ }
```

## Testing

Four layers, in order:

1. **Module unit tests** (in the skeleton's `#[cfg(test)] mod tests`):
   tempdir, `Scope::Local`, install → idempotent re-install → uninstall,
   plus a "plan does not write" check. The skeleton ships with four; flesh
   them out as you go.
2. **Public smoke** in `tests/registry.rs`: add your id to the `for id in
   [...]` loops. Round-trip in a tempdir for the public API.
3. **MCP / skill smoke** in `tests/mcp_registry.rs` /
   `tests/skill_registry.rs`: same, only if you implemented the matching
   surface. Includes idempotency and an `UnsupportedScope` check if your
   agent is single-scope.
4. **Plan-API smoke** in `tests/plan_api.rs`: every registered id must show
   up in the plan-API loops so the no-op/refusal/missing-config previews
   are exercised.

The contract every test enforces:

```
install → is_installed=true
install (again) → already_installed=true
uninstall → is_installed=false
uninstall (again) → not_installed=true
plan_install (no-op) → no on-disk changes after the call
```

Run `cargo test` (not `--lib`) before declaring done; the integration
tests live in `tests/`.

## Conventions cheat-sheet

- A *scope* is `Scope::Global` (user home) or `Scope::Local(<root>)` (a
  project).
- Hook JSON entries always carry `"_agent_config_tag": "<your tag>"` so
  multiple consumers coexist.
- MCP servers and skills track ownership via a sidecar ledger, never via a
  marker in the harness payload.
- Markdown injections use the `<!-- BEGIN AGENT-CONFIG:<tag> --> ... <!-- END
  AGENT-CONFIG:<tag> -->` fence format.
- Any pre-existing file we modify gets a one-time `<path>.bak` sibling on
  first patch (`safe_fs::write(scope, _, _, true)`).
- `safe_fs::restore_backup_if_matches(scope, path, desired_bytes)` is the only way to
  restore a backup on uninstall: it refuses if the backup no longer
  matches the desired post-uninstall state. Stale backups stay on disk.
- Atomic writes only. Never call `std::fs::write` on a path the user owns.
- **`scope.ensure_contained(&path)?` before every mutation.** `Scope::Local`
  refuses paths whose existing components include a symlink or canonicalize
  outside the project root; `Scope::Global` refuses symlinked target files.
  Skipping this opens a symlink-traversal hole.
- Cross-process file locks (`file_lock::with_lock(&path, || { ... })`)
  wrap every install/uninstall block that touches a shared file. Drop the
  guard (return from the closure) before locking a different file.
- Install and uninstall are idempotent: same input, same on-disk end state,
  no spurious diffs.
- Plan generation is side-effect-free. It must not create config files,
  ledgers, backups, directories, or chmod targets.

## Files referenced

- Trait definitions: `src/integration.rs`
- Spec types: `src/spec.rs` (HookSpec / McpSpec / SkillSpec)
- Plan types: `src/plan.rs` (InstallPlan / UninstallPlan / PlannedChange)
- Status types: `src/status.rs` (StatusReport / InstallStatus / DriftIssue)
- Scope and containment check: `src/scope.rs`
  (`Scope::ensure_contained`, symlink-aware target check for Global,
  symlink-aware containment for Local)
- Path helpers: `src/paths.rs`
- File locks: `src/util/file_lock.rs` (`with_lock` closure pattern)
- Safe integration mutations: `src/util/safe_fs.rs`
  (`write`, `remove_file`, `remove_dir_all`, `restore_backup_if_matches`)
- Atomic writes / backup restore internals: `src/util/fs_atomic.rs`
- Ownership ledger (v2 with content hashes): `src/util/ownership.rs`
- Plan helpers: `src/util/planning.rs`
- Plan adapters: `src/agents/planning.rs`
- Utility layer: `src/util/` (don't reinvent these)
- Security model: `docs/SECURITY.md`
- JSON-shape MCP example (prompt + MCP + skills): `src/agents/claude.rs`
- TOML-shape MCP example: `src/agents/codex.rs`
- Object-map MCP variant: `src/agents/opencode.rs`, `src/agents/copilot.rs`
- JSONC MCP variant: `src/agents/kilocode.rs`
- JSON5 MCP variant: `src/agents/openclaw.rs`
- YAML MCP variant: `src/agents/hermes.rs`
- Markdown-block-only Integration: `src/agents/qwen.rs`,
  `src/agents/amp.rs`, `src/agents/forge.rs`
- Local-only Integration with require-rules guard: `src/agents/openclaw.rs`,
  `src/agents/junie.rs`, `src/agents/trae.rs`
- Test patterns: `tests/registry.rs`, `tests/mcp_registry.rs`,
  `tests/skill_registry.rs`, `tests/plan_api.rs`, `tests/golden.rs`
  (regenerate with `AGENT_CONFIG_UPDATE_GOLDENS=1 cargo test --test golden`)
