# Security Model

This document describes the security properties, threat model, and
recommendations for consumers of the `agent-config` library.

## Threat Model

`agent-config` is a **local coordination** library. Its security boundary is
the local filesystem: it prevents one consumer from accidentally clobbering
another consumer's hooks, MCP servers, or skills in shared config files.

It does **not** defend against:

- A malicious local process with write access to config directories.
- Remote attackers (the library never opens network sockets).
- Privilege escalation (all writes happen as the current user).

## Execution Authority

Installing hooks and MCP stdio servers changes the behavior of AI coding
harnesses. Any command registered as a hook or MCP stdio launcher is
executable authority: the harness will run it.

Callers must:

- Only install commands they trust.
- Validate any command components derived from user or project input before
  building a `HookSpec` or `McpSpec`.
- Use [`plan_install`](crate::Integration::plan_install) to preview changes
  before applying them.

## Files Modified

The library writes to the following locations, organized by harness and scope.
Global paths use the user's home or config directory. Local paths are contained
under the project root supplied via `Scope::Local`.

Per-agent file locations are documented in [`docs/agents/`](agents/README.md).

## Ownership Model

### Tags and ledgers

Each consumer identifies itself with a `tag` (ASCII alphanumerics, `_`, `-`).
The library records ownership in sidecar JSON ledger files:

| Ledger file | Tracks ownership of |
|---|---|
| `<config-dir>/.agent-config-mcp.json` | MCP server entries |
| `<skills-root>/.agent-config-skills.json` | Skill directories |
| `.clinerules/hooks/.agent-config-hooks.json` | Cline hook scripts |

### Content hashes (v2 ledgers)

Starting with ledger schema version 2, every install also records a SHA-256
content hash of the owned content. Uninstall refuses to remove an entry
whose recorded hash no longer matches the current on-disk content,
returning `AgentConfigError::ConfigDrifted`.

MCP entries (`mcp_json_map` and `yaml_mcp_map`) record the canonical hash of
the *owned entry* only, so a sibling consumer's installs into the same
shared config do not trip drift. Skills (`skills_dir`) own their own
`SKILL.md` 1:1, so whole-file hashing is sufficient and is what's used.

### Ownership enforcement

- **Hooks** use inline `_agent_config_tag` markers in JSON entries or markdown
  comment fences. Overwriting an entry with a different tag returns
  `NotOwnedByCaller`.
- **MCP servers and skills** use sidecar ledgers. Removing or overwriting an
  entry owned by a different consumer returns `NotOwnedByCaller`.
- **Hand-installed entries** (present in config but absent from the ledger)
  are never modified. `NotOwnedByCaller` is returned with `actual: None`.

### Ledger integrity

Ledgers are plain JSON files, protected from concurrent writes by filesystem
locks. They are **coordination mechanisms**, not security boundaries. A local
process with write access to the ledger file can modify ownership records.

The content hashes recorded in v2 ledgers make undetected tampering harder:
comparing the recorded hash against the current config file content reveals
drift.

## Symlink Defense

For `Scope::Local` writes, the library canonicalizes the project root, walks
the existing components between that root and the target path, rejects symlink
components before following them, and verifies the deepest existing component
remains within the project root. Missing tail components are allowed only after
the existing ancestor chain passes those checks. This prevents:

- Symlinks inside the project directory from escaping to external paths.
- Existing symlink targets from being used as backup or write targets.
- Path traversal via `../` segments in resolved paths.

If the canonicalized path escapes the project root, the operation fails with
`AgentConfigError::PathResolution`.

`Scope::Local(root)` is a caller-supplied trust boundary. The library verifies
that resolved write targets stay under the canonicalized root, but it does not
decide whether that root is the intended project directory. If a caller passes
a symlinked root or a root containing `..`, the canonical destination is treated
as the project root. Consumers should pass a project root they have already
chosen intentionally.

`Scope::Global` writes are not project-contained, but they still reject a
symlinked target file before locking or mutating a config target. Global paths
target the user's home or config directories by design.

## Atomic Writes

Integration file modifications go through `util::safe_fs`, which applies the
scope checks above and then delegates to `write_atomic`, which:

1. Writes to a temporary file in the same directory.
2. Calls `fsync` on the temp file.
3. Renames the temp file to the target (POSIX atomic).

This prevents partial writes from corrupting config files, even during crashes.

### Backup handling

When modifying an existing file for the first time, a `.bak` copy is created
using `O_EXCL` (fails if backup already exists). On uninstall, the backup is
only restored if its content matches the expected post-uninstall state
(`restore_backup_if_matches`). This prevents stale backups from overwriting
user changes.

## Recommendations

1. **Preview before install.** Call `plan_install`, `plan_install_mcp`, or
   `plan_install_skill` before mutating operations. These return a list of
   planned changes without touching disk.
2. **Use typed hook commands.** Prefer
   `HookSpec::builder(tag).command_program(program, args)`. It stores
   program/argument boundaries and renders shell strings with POSIX quoting
   for harnesses that lack argv-native hook APIs. Use
   `command_shell_unchecked` only for trusted, intentional shell syntax.
3. **Avoid local inline MCP secrets.** Project-local MCP configs can be
   committed or shared. Local installs refuse env values whose names look like
   secrets by default. Prefer `McpSpec::builder(name).env_from_host("TOKEN")`
   so the file contains a host-env placeholder, or explicitly call
   `allow_local_inline_secrets()` when writing the value is intentional.
4. **Use unique owner tags.** Each consumer should use a distinctive tag to
   avoid collisions.
5. **Audit regularly.** Call `status()` to check what hooks, MCP servers, and
   skills are installed and whether any drift has occurred.
6. **Pin MCP transports.** Prefer `Stdio` with explicit command paths over
   `Http`/`Sse` with user-supplied URLs.
