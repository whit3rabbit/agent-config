# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
follows [SemVer](https://semver.org/) once 1.0 ships.

## [Unreleased]

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
