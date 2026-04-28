# agent-config

A Rust library that installs hooks, prompt rules, MCP servers, skills, and
standalone instruction files into AI coding harnesses.

Developers don't want to spend time looking up locations for each harness or
tracking where to install tools. 

This library solves that by centralizing the
per-harness file locations and config shapes so downstream tools do not have
to reimplement them.

You supply a `HookSpec`, `McpSpec`, `SkillSpec`, or `InstructionSpec`. The
library knows where each harness keeps its config, what shape that config
takes, and how to write to it safely.

## What this is

A thin, generic installer. It does not ship default content, does not embed
consumer-specific commands, does not include a CLI binary, and does not try
to detect which harnesses are present on the host. The crate focuses on one
thing: take a spec, produce the right files in the right places, and undo
that cleanly later.

Safety guarantees that apply to every integration:

- **Atomic writes.** Write to a temp file, fsync, rename. No torn files.
- **First-touch backups.** Any pre-existing file we modify gets a one-time
  `<path>.bak` sibling. If a `.bak` already exists we refuse to clobber it.
- **Idempotent.** Calling `install` twice with the same spec is a no-op
  after the first.
- **Reversible.** `uninstall` removes only the tagged content. Hook JSON
  entries carry an `_agent_config_tag` marker; markdown blocks are wrapped in
  `<!-- BEGIN AGENT-CONFIG:<tag> --> ... <!-- END AGENT-CONFIG:<tag> -->`
  fences. MCP servers, skills, and instructions use sidecar ledgers. Multiple
  consumers coexist without stepping on each other.

## What is supported

| Harness                | ID            | Hooks              | Prompt rules | MCP              | Skills           | Instructions     |
| ---------------------- | ------------- | ------------------ | ------------ | ---------------- | ---------------- | ---------------- |
| [Claude Code]          | `claude`      | Global + Local     | yes          | Global + Local   | Global + Local   | Global + Local   |
| [Cursor]               | `cursor`      | Global + Local     | -            | Global + Local   | Global + Local   | -                |
| [Gemini CLI]           | `gemini`      | Global + Local     | yes          | Global + Local   | Global + Local   | Global + Local   |
| [Codex CLI]            | `codex`       | Global + Local     | yes          | Global + Local   | Global + Local   | Global + Local   |
| [GitHub Copilot]       | `copilot`     | Local              | yes          | Global + Local   | Global + Local   | Local            |
| [OpenCode]             | `opencode`    | Global + Local     | -            | Global + Local   | Global + Local   | -                |
| [Cline]                | `cline`       | Local              | yes          | Global           | Global + Local   | Local            |
| [Roo Code]             | `roo`         | -                  | yes          | Global + Local   | -                | Local            |
| [Windsurf]             | `windsurf`    | Local              | yes          | Global + Local   | Global + Local   | Local            |
| [Kilo Code]            | `kilocode`    | -                  | yes          | Global + Local   | Global + Local   | Local            |
| [Google Antigravity]   | `antigravity` | -                  | yes          | Global + Local   | Global + Local   | Local            |
| [Amp]                  | `amp`         | -                  | yes          | Global + Local   | Global + Local   | Global + Local   |
| [CodeBuddy CLI]        | `codebuddy`   | Global + Local     | yes          | -                | Global + Local   | Global + Local   |
| [Forge]                | `forge`       | -                  | yes          | Global + Local   | Global + Local   | Global + Local   |
| [iFlow CLI]            | `iflow`       | Global + Local     | -            | Global + Local   | -                | -                |
| [JetBrains Junie]      | `junie`       | -                  | yes (Local)  | Global + Local   | -                | Local            |
| [Qoder CLI]            | `qodercli`    | -                  | yes          | Global + Local   | -                | Global + Local   |
| [Qwen Code]            | `qwen`        | -                  | yes          | Global + Local   | Global + Local   | Global + Local   |
| [Tabnine CLI]          | `tabnine`     | Global + Local     | -            | Global + Local   | -                | -                |
| [Trae]                 | `trae`        | -                  | yes (Local)  | -                | Global + Local   | Local            |
| [OpenClaw]             | `openclaw`    | -                  | yes (Local)  | Global           | Global + Local   | Local            |
| [Hermes Agent]         | `hermes`      | -                  | yes (Local)  | Global           | Global           | Local            |

[Claude Code]: docs/agents/claude.md
[Cursor]: docs/agents/cursor.md
[Gemini CLI]: docs/agents/gemini.md
[Codex CLI]: docs/agents/codex.md
[GitHub Copilot]: docs/agents/copilot.md
[OpenCode]: docs/agents/opencode.md
[Cline]: docs/agents/cline.md
[Roo Code]: docs/agents/roo.md
[Windsurf]: docs/agents/windsurf.md
[Kilo Code]: docs/agents/kilocode.md
[Google Antigravity]: docs/agents/antigravity.md
[Amp]: docs/agents/amp.md
[CodeBuddy CLI]: docs/agents/codebuddy.md
[Forge]: docs/agents/forge.md
[iFlow CLI]: docs/agents/iflow.md
[JetBrains Junie]: docs/agents/junie.md
[Qoder CLI]: docs/agents/qodercli.md
[Qwen Code]: docs/agents/qwen.md
[Tabnine CLI]: docs/agents/tabnine.md
[Trae]: docs/agents/trae.md
[OpenClaw]: docs/agents/openclaw.md
[Hermes Agent]: docs/agents/hermes.md

Per-harness install paths, JSON shapes, and event/matcher mappings are
documented in [`docs/agents/`](docs/agents/README.md). The release support
contract is summarized in [`docs/support-matrix.md`](docs/support-matrix.md).
A machine-readable manifest of every agent's surfaces, paths, ledger files,
and marker conventions lives at [`schema/agents.json`](schema/agents.json),
auto-generated by [`examples/gen_schema.rs`](examples/gen_schema.rs) from
the live registry. Runnable end-to-end examples for every surface live in
[`examples/`](examples/README.md). The security model and ownership-ledger
guarantees are described in [`docs/SECURITY.md`](docs/SECURITY.md).
Native OpenClaw hook/plugin installation is still deferred because upstream
exposes that as a CLI-managed plugin lifecycle rather than a stable file-backed
hook contract.

## Supported platforms

The crate runs the same way on every platform it builds for, but the
*environment* it targets is whatever it is launched in:

- **Native macOS / Linux** — fully supported.
- **Native Windows** — supported for harnesses with a documented Windows
  config path (e.g. Claude, Cursor, Codex, Gemini, VS Code-extension MCP
  files for Cline / Roo). `paths::home_dir()` honors `%USERPROFILE%` and
  `paths::config_dir()` honors `%APPDATA%`. Cline's hook surface writes a
  `bash`-shebanged executable script and is therefore **refused on native
  Windows** with [`RefusalReason::UnsupportedPlatform`] / the matching
  [`AgentConfigError::UnsupportedPlatform`]. Cline rules and MCP / skill /
  instruction installs continue to work.
- **WSL** — treated as a Linux environment. A binary running inside WSL
  resolves `$HOME` and writes WSL config; it does not magically reach the
  Windows host profile under `/mnt/c/...`.
- **Targeting Windows host config from inside WSL** — explicitly out of
  scope for v1. Future work would gate this behind an opt-in `GlobalTarget`
  / `PathRoots` API rather than auto-detection.

[`HookCommand::render_shell`] produces POSIX-shell quoting; consumers
targeting a non-POSIX shell (PowerShell / `cmd.exe`) should construct
[`HookCommand::ShellUnchecked`] and quote the command themselves.

## How to use it in your Rust app

Add the dependency:

```toml
[dependencies]
agent-config = "0.1"
```

Each snippet below has a runnable counterpart under
[`examples/`](examples/README.md). Run with `cargo run --example <name>`; every
example writes into a fresh tempdir so it never touches the host's real config.

### Install a hook

```rust
use agent_config::{by_id, Event, HookSpec, Matcher, Scope};

fn main() -> agent_config::Result<()> {
    let spec = HookSpec::builder("myapp")              // your consumer tag
        .command_program("myapp", ["hook", "claude"])  // what the harness runs
        .matcher(Matcher::Bash)                        // filter to shell calls
        .event(Event::PreToolUse)                      // before each tool call
        .try_build()?;

    let claude = by_id("claude").expect("claude is registered");
    let report = claude.install(&Scope::Global, &spec)?;

    println!("created: {:?}", report.created);
    println!("patched: {:?}", report.patched);
    println!("backed up: {:?}", report.backed_up);
    Ok(())
}
```

> **Tip:** Prefer `.try_build()?` over `.build()` in production code.
> The `build()` method panics on invalid specs, which is fine for tests
> but not for a running application.

Runnable: [`examples/hooks_install_uninstall.rs`](examples/hooks_install_uninstall.rs).

`Scope::Global` writes under the user's harness config dir
(`~/.claude/`, `~/.cursor/`, etc.). `Scope::Local(PathBuf)` writes inside
a specific project root. For the scope-gating contract (which harnesses
accept which scopes) see
[`examples/scopes_local_vs_global.rs`](examples/scopes_local_vs_global.rs).

The `tag` field is **your application's identifier**, not the harness's.
Pick something stable and ASCII alnum / `_` / `-`. It namespaces every
file, JSON entry, and markdown fence the library writes, so multiple tools
built on `agent-config` can install side-by-side.

Use `command_program(program, args)` for hook commands by default. It preserves
program/argument boundaries and shell-quotes arguments for harnesses that only
accept command strings. Raw shell remains available as
`command_shell_unchecked(...)` when you intentionally need shell syntax.

### Inject prompt rules at the same time

```rust
let spec = HookSpec::builder("myapp")
    .command_program("myapp", ["hook", "claude"])
    .matcher(Matcher::Bash)
    .event(Event::PreToolUse)
    .rules("Run `myapp lint` before committing.")
    .build();
```

For Claude this lands as a fenced block inside `~/.claude/CLAUDE.md` (or
`<root>/CLAUDE.md` for `Scope::Local`). Each harness uses its own memory
file; the fence shape is identical.

### Uninstall

```rust
let claude = by_id("claude").unwrap();
let report = claude.uninstall(&Scope::Global, "myapp")?;

if report.not_installed {
    println!("nothing to remove");
} else {
    println!("removed: {:?}", report.removed);
    println!("patched: {:?}", report.patched);
    println!("restored backups: {:?}", report.restored);
}
```

Uninstall is keyed only on the tag, so callers do not need to remember
the original `HookSpec`.

### Install an MCP server

```rust
use agent_config::{mcp_by_id, McpSpec, Scope};

fn main() -> agent_config::Result<()> {
    let spec = McpSpec::builder("github")
        .owner("myapp")
        .stdio("npx", ["-y", "@modelcontextprotocol/server-github"])
        .env_from_host("GITHUB_TOKEN")
        .try_build()?;

    let codex = mcp_by_id("codex").expect("codex supports MCP");
    codex.install_mcp(&Scope::Global, &spec)?;
    Ok(())
}
```

Local MCP installs refuse likely inline secrets by default. Use
`env_from_host("GITHUB_TOKEN")` to write a host-env placeholder, or use
`allow_local_inline_secrets()` only for trusted configs where writing the value
into the project file is intentional.

Runnable: [`examples/mcp_install.rs`](examples/mcp_install.rs).

MCP uninstall is keyed by server name plus owner tag. If another consumer owns
the server, or the server was hand-installed and has no ownership ledger entry,
`uninstall_mcp` returns `AgentConfigError::NotOwnedByCaller`. See
[`examples/multi_consumer.rs`](examples/multi_consumer.rs) for two consumer
tags coexisting in the same project.

```rust
use agent_config::{mcp_by_id, AgentConfigError, Scope};

fn remove_github_mcp() -> agent_config::Result<()> {
    let codex = mcp_by_id("codex").expect("codex supports MCP");
    match codex.uninstall_mcp(&Scope::Global, "github", "myapp") {
        Ok(report) if report.not_installed => println!("nothing to remove"),
        Ok(report) => println!("removed: {:?}", report.removed),
        Err(AgentConfigError::NotOwnedByCaller { actual, .. }) => {
            println!("github MCP is owned by {:?}", actual);
        }
        Err(err) => return Err(err),
    }
    Ok(())
}
```

### Install a skill

```rust
use agent_config::{skill_by_id, Scope, SkillSpec};

fn main() -> agent_config::Result<()> {
    let spec = SkillSpec::builder("my-skill")
        .owner("myapp")
        .description("Use when my app needs custom repository context.")
        .body("# My Skill\n\nFollow the local project conventions.")
        .try_build()?;

    let claude = skill_by_id("claude").expect("claude supports skills");
    claude.install_skill(&Scope::Global, &spec)?;
    Ok(())
}
```

Skill uninstall uses the same ownership model as MCP:

```rust
let claude = agent_config::skill_by_id("claude").unwrap();
claude.uninstall_skill(&agent_config::Scope::Global, "my-skill", "myapp")?;
```

Runnable: [`examples/skill_install.rs`](examples/skill_install.rs) (also
demonstrates attaching an executable asset under `scripts/`).

### Install an instruction

Instructions are standalone, named markdown files that the harness loads on
every session. For example: write `~/.claude/MYAPP.md` and add a managed
`@MYAPP.md` include to `~/.claude/CLAUDE.md`. The library tracks ownership
in a sidecar ledger so multiple consumers coexist.

```rust
use agent_config::{instruction_by_id, InstructionPlacement, InstructionSpec, Scope};

fn main() -> agent_config::Result<()> {
    let spec = InstructionSpec::builder("MYAPP")
        .owner("myapp")
        .placement(InstructionPlacement::ReferencedFile)
        .body("# MyApp\n\nProject-specific guidance loaded into the agent every session.\n")
        .try_build()?;

    let claude = instruction_by_id("claude").expect("claude instruction-capable");
    let report = claude.install_instruction(&Scope::Global, &spec)?;
    println!("created: {:?}", report.created);
    println!("patched: {:?}", report.patched);
    Ok(())
}
```

What that writes:

```text
                            ┌─────────────────────────────────────────┐
  consumer code   ─────▶    │  InstructionSpec::builder("MYAPP")      │
                            │    .owner("myapp")                      │
                            │    .placement(ReferencedFile)           │
                            │    .body("...")                         │
                            │    .try_build()?                        │
                            └────────────────┬────────────────────────┘
                                             │
                                             ▼
                            instruction_by_id("claude")
                                             │
                                             ▼
                            install_instruction(&Scope::Global, &spec)
                                             │
                          ┌──────────────────┼──────────────────────┐
                          ▼                  ▼                      ▼
              ~/.claude/MYAPP.md     ~/.claude/CLAUDE.md      ~/.claude/
              (instruction file)     (host file: managed       .agent-config-
                                     @MYAPP.md fenced block)   instructions.json
                                                               (ownership ledger)
```

Three placement modes are available; pick the one that matches the harness:

```text
  InstructionPlacement
    │
    ├─ ReferencedFile   →  write <name>.md  +  inject @<name>.md fenced block in host
    │                       (Claude — has a documented `@import` syntax)
    │
    ├─ InlineBlock      →  inject body as a fenced block inside the host file (no separate file)
    │                       (Codex, Gemini, CodeBuddy, Amp, Forge, Qoder, Qwen,
    │                        Copilot, Junie, Trae, OpenClaw, Hermes)
    │
    └─ StandaloneFile   →  write <name>.md only, no host edit
                            (Cline, Roo, Kilo Code, Windsurf, Antigravity —
                             agents whose memory model is a per-file rules dir)
```

Uninstall is keyed on `(name, owner_tag)`, mirroring MCP and skills:

```rust
let claude = agent_config::instruction_by_id("claude").unwrap();
claude.uninstall_instruction(&agent_config::Scope::Global, "MYAPP", "myapp")?;
```

Runnable: [`examples/instruction_install.rs`](examples/instruction_install.rs)
(walks all three placement modes against Claude, Codex, and Cline).

#### Instructions vs prompt rules

Instructions are a separate surface from `HookSpec::rules`:

- **`HookSpec::rules`**: a fenced markdown block injected as part of installing a
  hook. Tied to the hook's lifecycle (uninstall the hook, the block goes away).
  Best for short usage notes that travel with a tool's hook integration.
- **`InstructionSpec`**: a standalone, named instruction with an independent
  lifecycle, ledger-tracked ownership, and (for ReferencedFile placements) a
  separate file that the harness loads via `@import`. Best for long-lived
  toolchain instructions that should remain even when no hooks are installed.

#### Naming caveat

For `ReferencedFile` and `InlineBlock` placements, the instruction's `name` is
also the markdown fence tag (`<!-- BEGIN AGENT-CONFIG:<name> --> ... -->`).
If you install both a hook with `tag = "T"` and an instruction with
`name = "T"` into the same memory file, the second upsert silently replaces
the first. Pick a name that does not collide with any of your hook tags
(prefix it, e.g. `instr-myapp`, or use `StandaloneFile` placement).

### Iterate every supported harness

```rust
use agent_config::{all, instruction_capable, mcp_capable, skill_capable, Scope};

// Guard against `UnsupportedScope` by checking `supported_scopes()` first.
// Calling `install` or `is_installed` on a scope the harness rejects returns
// `AgentConfigError::UnsupportedScope` rather than writing anywhere.
for integration in all() {
    if integration.supported_scopes().contains(&Scope::Global.kind())
        && integration.is_installed(&Scope::Global, "myapp").unwrap_or(false)
    {
        println!("{} has myapp installed", integration.display_name());
    }
}

for mcp in mcp_capable() {
    println!("{} supports MCP", mcp.id());
}

for skills in skill_capable() {
    println!("{} supports skills", skills.id());
}

for instr in instruction_capable() {
    println!("{} supports instructions", instr.id());
}
```

`Integration::supported_scopes()` tells you which scopes a given harness
accepts. Copilot, for example, is `Local` only; calling `install` on it
with `Scope::Global` returns `AgentConfigError::UnsupportedScope`.

Use `mcp_capable()` / `mcp_by_id()` for MCP, `skill_capable()` /
`skill_by_id()` for skills, and `instruction_capable()` /
`instruction_by_id()` for instructions. Their scope sets can differ from
hook support: Cline hooks are local-only, while Cline MCP is global-only.

For a runnable version of the above, see
[`examples/discover_capable_agents.rs`](examples/discover_capable_agents.rs).

### Preview changes with a dry-run plan

Every install/uninstall has a side-effect-free planner: `plan_install`,
`plan_uninstall`, `plan_install_mcp`, `plan_install_skill`,
`plan_install_instruction` (and the matching uninstall variants). They return
an `InstallPlan` / `UninstallPlan` whose `status` is `WillChange`, `NoOp`, or
`Refused`, and whose `changes` enumerate every file write, ledger entry,
backup, and permission change the real call would perform, without touching
the filesystem.

```rust
use agent_config::{by_id, PlanStatus};

let plan = by_id("claude").unwrap().plan_install(&scope, &spec)?;
match plan.status {
    PlanStatus::WillChange => println!("{} change(s) planned", plan.changes.len()),
    PlanStatus::NoOp => println!("nothing to do"),
    PlanStatus::Refused => println!("refused: {:?}", plan.changes),
}
```

Runnable: [`examples/dry_run_plan.rs`](examples/dry_run_plan.rs).

### Browse every surface in a TUI

A bundled ratatui example shows `plan_install_*` outputs across every
supported harness side-by-side, no code required:

```bash
cargo run --example tui_dry_run
```

Four tabs (SKILLS, MCP, HOOKS, INSTRUCTIONS) each list the harnesses that
support that surface. Move with `↑`/`↓` (or `j`/`k`), check rows with
`Space`, flip `Local` / `Global` scope with `g`, and press `Enter` for an
aggregate dry-run across the selection. Press `?` for the full keymap,
`q` to quit. Nothing is written to disk; only `plan_install_*` runs, so
flipping to `Global` safely previews real `~/.claude/...`,
`~/.gemini/...`, etc. paths without touching them.

Runnable: [`examples/tui_dry_run/`](examples/tui_dry_run/main.rs). For
all bundled examples see [`examples/README.md`](examples/README.md).

### Errors

All operations return `Result<T, AgentConfigError>`. Variants worth handling
explicitly:

- `UnsupportedScope`: the harness does not accept this scope kind.
- `MissingSpecField`: e.g., Gemini's script delegator requires
  `HookSpec::script`, prompt-only agents require `HookSpec::rules`.
- `InvalidTag`: empty or contains characters outside `[A-Za-z0-9_-]`.
- `BackupExists`: a first-touch `<path>.bak` could not be created safely.
  Existing backups are normally preserved and reused.
- `NotOwnedByCaller`: an MCP server, skill, or instruction is owned by another
  consumer or was hand-installed without an agent-config ledger entry.
- `Io` and `JsonInvalid` carry the offending path.

## License

MIT
