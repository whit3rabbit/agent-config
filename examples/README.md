# Examples

Runnable programs that exercise the public `agent-config` API. Every example
writes into a fresh `tempfile::tempdir()` (project-local scope) so it never
touches the host's real `~/.claude/`, `~/.cursor/`, etc. Read each file's top
comment for what it demonstrates.

Run any example with:

```bash
cargo run --example <name>
```

| Example                                              | Surface       | Topic                                                      |
| ---------------------------------------------------- | ------------- | ---------------------------------------------------------- |
| [`hooks_install_uninstall`](hooks_install_uninstall.rs) | Hooks         | Install + idempotent reinstall + uninstall round trip      |
| [`scopes_local_vs_global`](scopes_local_vs_global.rs)   | Hooks         | `Scope::Global` vs `Scope::Local`, `supported_scopes` gate |
| [`mcp_install`](mcp_install.rs)                       | MCP           | stdio MCP install with `env_from_host` secret handling     |
| [`skill_install`](skill_install.rs)                   | Skills        | Skill install with frontmatter + an executable asset       |
| [`instruction_install`](instruction_install.rs)       | Instructions  | `ReferencedFile`, `InlineBlock`, and `StandaloneFile`      |
| [`dry_run_plan`](dry_run_plan.rs)                     | All           | `plan_install` / `plan_uninstall` previews, no mutation    |
| [`multi_consumer`](multi_consumer.rs)                 | MCP           | Two owner tags coexist, `NotOwnedByCaller` enforcement     |
| [`discover_capable_agents`](discover_capable_agents.rs) | All         | Iterate `all()`, `mcp_capable()`, `skill_capable()`, etc.  |
| [`tui_dry_run`](tui_dry_run/main.rs)                  | All         | ratatui TUI: per-surface tabs, multi-select, live plan preview, `Local`/`Global` scope toggle |

## How the examples are organised

Every program follows the same shape:

1. Build a tempdir and turn it into `Scope::Local`.
2. Construct a spec via the builder, using `try_build()?` (production style).
3. Look up the integration via `by_id` / `mcp_by_id` / `skill_by_id` /
   `instruction_by_id`, or use a concrete type like `ClaudeAgent::new()`.
4. Call the matching `install_*` / `uninstall_*` method.
5. Print the relevant report fields and the on-disk effect.

The examples deliberately do not run against `Scope::Global`. Global writes
land in the user's home directory; the test suite isolates that behind an
env-mocked harness (see `tests/plan_api.rs::IsolatedGlobalEnv`), but examples
should not pollute a developer machine on `cargo run`. The `scopes_local_vs_global`
example shows how the API differs without performing the global write.

The one exception is `tui_dry_run`: it lets you flip to `Scope::Global`
because it is dry-run-only (`plan_install_*` is pure read), so the global
preview shows real `~/.claude/...`-style paths without writing them.

## See also

- [`docs/agents/README.md`](../docs/agents/README.md) for per-harness
  install paths and config shapes.
- [`docs/SECURITY.md`](../docs/SECURITY.md) for the security model behind
  ownership ledgers, secret policies, and atomic writes.
- [`templates/new-harness/README.md`](../templates/new-harness/README.md)
  for adding support for a new AI coding harness.
