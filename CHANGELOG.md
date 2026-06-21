# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
follows [SemVer](https://semver.org/) once 1.0 ships.

## [Unreleased]

## [0.4.0] - 2026-06-21

### Changed

- **Amp**: moved global config paths to Amp's documented locations. User MCP
  settings now write `~/.config/amp/settings.json` (was `~/.amp/settings.json`),
  global skills write `~/.config/amp/skills/` (was `~/.amp/skills/`), global
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
- Marked **Gemini CLI** legacy/conditional (consumer-tier serving stopped
  2026-06-18; enterprise and paid API-key users supported) across the same docs.
- Corrected the **Cline** support-matrix/doc rows to the standalone-CLI global
  MCP path `~/.cline/data/settings/cline_mcp_settings.json` (the code was already
  migrated; the docs were stale).
- Refreshed `path-contract-audit.md` with a 2026-06-21 verification pass:
  re-verified Roo, Gemini, Amp, Cline, Kilo; disproved a Junie claim
  (`.junie/AGENTS.md` is the current default, not `.junie/guidelines.md`);
  logged Qoder global MCP (`~/.qoder.json`) as an open re-audit item.

### Migration notes

- Amp and Kilo path moves change where new config is written; pre-0.4 files at
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
  shared config file under `~/.gemini`, matching Antigravity's app-created
  layout without weakening the generic global symlink safety policy.
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
  Hook rules continue to use `<!-- BEGIN AGENT-CONFIG:<tag> -->`. This
  eliminates a silent collision where a hook with `tag = T` and an
  instruction with `name = T` installed into the same memory file (e.g.
  `~/.claude/CLAUDE.md`, `AGENTS.md`, `GEMINI.md`) would overwrite each
  other.
- Status detection and uninstall on the instruction surface accept the
  legacy `AGENT-CONFIG:<name>` prefix as a fallback, so consumers upgrading
  from a pre-rename build will see existing installs detected and removed
  cleanly. Re-installing an instruction whose host carries the legacy
  fence prunes the legacy block before writing the new one. Pruning is
  gated on the instructions ledger already having an entry for the name,
  so a hook block sharing the same identifier is never erased.

### Documentation

- README now notes that `schema/agents.json` is the Linux-canonical view of
  agent path layouts. macOS and Windows views differ for any agent whose
  config dir flows through `paths::config_dir()` (Cline, Roo). Regenerate
  on Linux for byte-stable output; the companion snapshot test
  (`tests/schema_golden.rs`) only enforces equality on Linux.
- `CLAUDE.md` records that the Crush hook integration relies on Crush's
  Go decoder ignoring unknown JSON fields. Crush's `HookConfig` schema
  declares `additionalProperties: false`; if upstream switches to strict
  decoding, the inline `_agent_config_tag` marker will need to migrate to
  a sidecar `.agent-config-hooks.json` ledger paralleling the existing
  MCP / skills / instructions ledgers. No hook ledger infrastructure
  exists today; the migration is deferred until needed.

### Migration notes

- Outstanding installs from a pre-rename build do not require manual
  cleanup. Uninstall detects the legacy `AGENT-CONFIG:<name>` fence and
  removes it.
- Re-installing the same instruction (matching `name`) replaces the
  legacy block with the new-prefix block in a single operation.
- Consumers parsing memory files for installed agent-config blocks should
  match either prefix (`AGENT-CONFIG:` for hooks,
  `AGENT-CONFIG-INSTR:` for instructions).
