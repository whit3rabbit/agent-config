# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
follows [SemVer](https://semver.org/) once 1.0 ships.

## [Unreleased]

### Added

- **`skills-store` crate** — a network-free Rust engine for the canonical
  Agent Skills store: installs skills once into `.agents/skills` (project) or
  `~/.agents/skills` (home) and symlinks the agents that do not read those
  directories natively. Ships the 79-agent registry from vercel-labs/skills
  (detection included), both lock files (`skills-lock.json` v1,
  `.skill-lock.json` v3), name sanitization, SKILL.md frontmatter parsing,
  recursive skill discovery (priority containers, `skills.sh.json` paths,
  plugin manifests), folder hashing, and safe copy semantics (symlink
  escape/cycle rejection, `metadata.json`/`.git`/`__pycache__` exclusion).
- **`skills-cli` crate** — a `skills` binary wizard-compatible with
  vercel-labs/skills@`d3701c1`: `add`/`install`/`i` (interactive wizard with
  searchable multiselect, locked universal section, scope + install-method
  prompts, installation summary, `--json`/`--all`/`--copy`/`-g`/`-y`/`-l`
  flags), `remove`/`rm` (`--all`, `--global`, agent filters, ghost-symlink
  cleanup), `list`/`ls` (project/global, agent filter, `--json`),
  `find`/`search` (skills.sh API + interactive fzf-style prompt),
  `use` (prompt generation, `--agent` launch), `update`/`upgrade`/`check`
  (GitHub Trees API fast path, clone-based relocation), `experimental_install`
  (restore from `skills-lock.json` with frozen-lockfile semantics), `init`,
  and `experimental_sync` (core node_modules flows). Blob fast path
  (skills.sh snapshot downloads), well-known discovery (RFC 8615, v0.1.0 +
  v0.2.0 digests), direct SKILL.md/archive downloads with strict zip/tar
  validation, and git clone fallbacks (commit-SHA fetch, `gh` retry, SSH).
- **Workspace layout**: the repo is now a Cargo workspace
  (`agent-config` + `skills-store` + `skills-cli`); the library crate is
  unchanged.

### Compatibility

- State-level interop with the upstream `skills` CLI is verified
  bidirectionally against `skills@1.7.6`: skills installed by either tool are
  listed and removed by the other, and both tools read and extend the same
  lock files without clobbering each other's entries.
- Deliberate divergences: no telemetry is sent anywhere; the Notion pack
  integration is not implemented; `experimental_sync` does not cover remote
  `skills`-field git sources.

## [0.5.0] - 2026-11-07

### Changed

- **Amp**: global skills moved from `~/.config/amp/skills/` to Amp's current
  canonical `~/.config/agents/skills/` (built from the home directory, not
  the XDG config dir). `~/.config/amp/` keeps `settings.json` (MCP) and the
  instruction ledger; legacy `~/.config/amp/skills/` installs remain visible
  to status and drain on uninstall. Project scope (`.agents/skills`) is
  unchanged.
- **Antigravity**: global skills moved from the legacy
  `~/.gemini/antigravity/skills/` to the canonical `~/.gemini/config/skills/`
  shared across the Antigravity app, IDE, and CLI. The legacy root is kept as
  a status/uninstall drain.
- **Antigravity CLI**: global skills moved from the CLI-only
  `~/.gemini/antigravity-cli/skills/` to the canonical
  `~/.gemini/config/skills/`. The CLI-only root is kept as a drain. The
  shared per-root skills ledger keeps both Antigravity integrations
  consistent.
- **Cline**: skills now default to the standard Agent Skills locations
  `.agents/skills/` (project) and `~/.agents/skills/` (global) per
  cline/cline#9074. Legacy `.cline/skills/` installs still drain on status
  and uninstall.
- **Kilo Code**: project skills moved from `.kilo/skills/` to the standard
  `.agents/skills/`, which current Kilo builds load first. `.kilo/skills/`
  is kept as a status/uninstall drain. Global stays `~/.kilo/skills/`.
- **Claude**: `paths::claude_home()` now honors `CLAUDE_CONFIG_DIR` (same
  shape as `CODEX_HOME`). Global hooks, prompts, MCP, skills, and
  instruction writes follow Claude Code's documented config-directory
  override.
- **Hermes**: `paths::hermes_home()` now honors `HERMES_HOME`. Skills gained
  project scope at `<root>/.hermes/skills/agent-config/<name>/` alongside the
  global `$HERMES_HOME/skills/agent-config/<name>/` root
  (nested category directories are an official Hermes layout).

### Documentation

- Refreshed `path-contract-audit.md` with a 2026-11-07 verification pass
  driven by the vercel-labs/skills audit. Confirmed the
  verified-correct paths for claude, cursor, gemini, copilot, opencode, codex,
  openclaw, pi, windsurf, crush, codebuddy, qwen, trae, and forge.
- Updated `docs/support-matrix.md` and per-agent docs for every moved root;
  corrected the README harness-table Skills cell for `antigravitycli`, which
  had drifted.
- Re-dated the deferred-harness rationale in AGENTS.md for kimi, bob, goose,
  kiro-cli, and vibe against vercel's now-public skills contracts.

### Migration notes

- Path moves change where new skills are written; pre-0.5 skills at legacy
  locations are not auto-migrated. Reinstall to populate the new roots;
  legacy installs are picked up by `skill_status` and removed by
  `uninstall_skill`.

## [0.4.0] - 2026-06-21

### Changed

- **Amp**: moved global config paths to Amp's documented locations. User MCP
  settings now write `~/.config/amp/settings.json` (was `~/.amp/settings.json`).
  Global skills write `~/.config/amp/skills/` (was `~/.amp/skills/`), global
  rules/instructions write `~/.config/AGENTS.md` (was `~/.amp/AGENTS.md`), and
  native project skills write `<root>/.agents/skills/` (was `<root>/.amp/skills/`).
  Local MCP (`<root>/.amp/settings.json`) and project rules (`<root>/AGENTS.md`)
  are unchanged. Added `paths::amp_config_dir()`.
- **Kilo Code**: rules and standalone instructions now write the current
  `.kilo/rules` layout instead of the legacy `.kilocode/rules`. MCP
  (`kilo.jsonc`) is unchanged.

### Documentation

- Marked **Roo Code** retired/EOL (all products shut down 2026-05-15) in the
  support matrix, README, and per-agent doc. Integration kept for compatibility.
- Marked **Gemini CLI** legacy/conditional across the same docs. Consumer-tier
  serving stopped 2026-06-18; enterprise and paid API users remain supported.
- Corrected the **Cline** support-matrix/doc rows to the standalone-CLI global
  MCP path `~/.cline/data/settings/cline_mcp_settings.json`. The code was
  already migrated; the docs were stale.
- Refreshed `path-contract-audit.md` with a 2026-06-21 verification pass.
  Re-verified Roo, Gemini, Amp, Cline, Kilo. Disproved a Junie claim
  (`.junie/AGENTS.md` is the current default, not `.junie/guidelines.md`).
  Logged Qoder global MCP (`~/.qoder.json`) as an open re-audit item.

### Migration notes

- Amp and Kilo path moves change where new config is written. Pre-0.4 files at
  the old locations are left in place (not auto-migrated). Reinstall to populate
  the new paths; remove old files manually if desired.

## [0.3.3] - 2026-06-07

### Fixed

- Fixed Windows path resolution tests following the Cline directory migration.
- Hardened global test environments to recover from poisoned mutex locks.

## [0.3.2] - 2026-06-07

### Fixed

- Regenerated JSON schema to fix the out-of-date `schema/agents.json` verification test.

## [0.3.1] - 2026-06-07

### Changed

- Upgraded shared hook lifecycle events.
- Hardened Gemini hooks, instructions, and MCP.
- Migrated Cline paths from global storage to `.cline/data`.
- Broadened Codex integration support.
- Updated Windows Antigravity path test.

## [0.3.0] - 2026-05-30

### Added

- Support dynamic TS plugin hooking in OpenCode.

### Changed

- Refactored agent configuration and hooks storage from `.agent` to `.agents` directory.

## [0.2.0] - 2026-05-29

### Added

- Added `antigravitycli` as a distinct Antigravity CLI integration with prompt
  context, instructions, and MCP support. CLI skills are intentionally deferred
  because Antigravity CLI uses flat `.md` skill files while this crate's
  `SkillSurface` models directory-scoped `SKILL.md` packages.
- Added OpenCode prompt rules and instruction-surface support through
  documented `AGENTS.md` files.

### Fixed

- Antigravity global MCP installs now resolve the documented
  `~/.gemini/antigravity/mcp_config.json` leaf symlink when it points to a
  shared config file under `~/.gemini`. This matches Antigravity's
  app-created layout without weakening the generic global symlink safety
  policy.
- Codex hook `Matcher::Bash` now emits Codex's documented `Bash` matcher.
- Codex streamable HTTP MCP entries now write `http_headers` and omit the
  unsupported `type = "http"` field. SSE transport is refused for Codex MCP.
- OpenCode's generated default plugin now uses the current
  `tool.execute.before` plugin callback signature.

### Changed

- Antigravity app/IDE local prompt rules, instructions, and skills now write
  to `.agents/*` paths. Existing `.agent/*` installs are still detected for
  status and uninstall, but are not auto-migrated.
- OpenCode `HookSpec::rules` now injects into OpenCode's documented
  `AGENTS.md` prompt file instead of only writing the plugin file.

### Changed (breaking, instruction surface)

- The instruction surface (`InstructionPlacement::InlineBlock` and
  `InstructionPlacement::ReferencedFile`) now writes its managed markdown
  fence with a distinct prefix:
  `<!-- BEGIN AGENT-CONFIG-INSTR:<name> --> ... <!-- END AGENT-CONFIG-INSTR:<name> -->`.
  Hook rules continue to use `<!-- BEGIN AGENT-CONFIG:<tag> -->`. Before,
  a hook with `tag = T` and an instruction with `name = T` installed into
  the same memory file (e.g. `~/.claude/CLAUDE.md`, `AGENTS.md`,
  `GEMINI.md`) would silently overwrite each other.
- Status detection and uninstall on the instruction surface accept the
  legacy `AGENT-CONFIG:<name>` prefix as a fallback. Consumers upgrading
  from a pre-rename build will see existing installs detected and removed
  cleanly. Re-installing an instruction whose host carries the legacy
  fence prunes the legacy block before writing the new one. Pruning is
  gated on the instructions ledger already having an entry for the name.
  A hook block sharing the same identifier is never erased.

### Documentation

- README now notes that `schema/agents.json` is the Linux-canonical view of
  agent path layouts. macOS and Windows views differ for any agent whose
  config dir flows through `paths::config_dir()` (Cline, Roo). Regenerate
  on Linux for byte-stable output; the companion snapshot test
  (`tests/schema_golden.rs`) only enforces equality on Linux.
- `CLAUDE.md` records that the Crush hook integration relies on Crush's
  Go decoder ignoring unknown JSON fields, even though Crush's `HookConfig`
  schema declares `additionalProperties: false`. A strict-decoding switch
  upstream would push the inline `_agent_config_tag` marker into a sidecar
  ledger like the existing MCP / skills / instructions ones. No hook ledger
  infrastructure exists today; the migration is deferred until needed.

### Migration notes

- Outstanding installs from a pre-rename build do not require manual
  cleanup. Uninstall detects the legacy `AGENT-CONFIG:<name>` fence and
  removes it.
- Re-installing the same instruction (matching `name`) replaces the
  legacy block with the new-prefix block in a single operation.
- Consumers parsing memory files for installed agent-config blocks should
  match either prefix (`AGENT-CONFIG:` for hooks,
  `AGENT-CONFIG-INSTR:` for instructions).
