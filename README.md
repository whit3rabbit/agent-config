# agent-config

A Rust library for installing skills, hooks, MCP servers, prompt rules, and
instruction files into AI coding harnesses (Claude Code, Cursor, Codex,
Copilot, and 20+ more). Every agent keeps its config in a different place and
in a different shape.

`agent-config` centralizes that knowledge: you supply a `HookSpec`,
`McpSpec`, `SkillSpec`, or `InstructionSpec`, and the library writes the
right files in the right places, then undoes that cleanly later.

The library is deliberately thin. It ships no default content, embeds no
consumer-specific commands, has no CLI binary of its own, and does no host
detection. One spec installs across every supported harness, so downstream
tools don't reimplement per-tool layout.

Just want skills installed into your coding agents? Skip the Rust entirely:
the bundled [`skills` CLI](#install-skills-from-the-command-line) does it from
your terminal, no code required.

[![crates.io](https://img.shields.io/crates/v/agent-config.svg)](https://crates.io/crates/agent-config)
[![docs.rs](https://img.shields.io/docsrs/agent-config)](https://docs.rs/agent-config)
[![CI](https://github.com/whit3rabbit/agent-config/actions/workflows/ci.yml/badge.svg)](https://github.com/whit3rabbit/agent-config/actions/workflows/ci.yml)
[![MIT licensed](https://img.shields.io/crates/l/agent-config.svg)](LICENSE)

[Install skills](#install-skills-from-the-command-line) •
[Library](#use-the-library-from-rust) •
[TUI](#browse-every-surface-in-a-tui) •
[Harnesses](#supported-harnesses) •
[Usage](#usage) •
[License](#license)

**Why it's for developers**

- Stop looking up config locations for each harness.
- Stop tracking where every tool gets installed and how to remove it.

**Safety guarantees** (every integration):

- **Atomic writes.** Write to a temp file, fsync, rename. No torn files.
- **First-touch backups.** Any pre-existing file we modify gets a one-time
  `<path>.bak` sibling. If a `.bak` already exists we refuse to clobber it.
- **Idempotent.** Calling `install` twice with the same spec is a no-op
  after the first.
- **Reversible.** `uninstall` removes only the tagged content. Hook JSON
  entries carry an `_agent_config_tag` marker. Markdown blocks are wrapped in
  `<!-- BEGIN AGENT-CONFIG:<tag> --> ... <!-- END AGENT-CONFIG:<tag> -->`
  fences. MCP servers, skills, and instructions use sidecar ledgers. Multiple
  consumers coexist without stepping on each other.

## Install skills from the command line

The workspace also ships a drop-in replacement for the
[`vercel-labs/skills`](https://github.com/vercel-labs/skills) CLI, built on the
[`skills-store`](skills-store/) engine: install, update, and remove Agent
Skills across every coding agent through one canonical `.agents/skills`
directory plus per-agent symlinks.

### Get the binary

From a clone of this repository:

```bash
cargo install --path skills-cli   # puts `skills` on your PATH
# or, without installing:
cargo run -p skills-cli -- add vercel-labs/agent-skills
```

### Install a skill

`web-design-guidelines` from `vercel-labs/agent-skills` is the most-installed
skill on [skills.sh](https://skills.sh), so it makes a good first run. From
your project:

```bash
skills add vercel-labs/agent-skills
```

The wizard lists the skills in the repo and the agents detected on your
machine (searchable, with universal `.agents/skills` agents shown as an
always-included locked section), then asks for the install scope and method
before writing anything. Non-interactive equivalent:

```bash
# One skill, every detected agent, no prompts
skills add vercel-labs/agent-skills --skill web-design-guidelines -y

# User-level instead of project-level
skills add vercel-labs/agent-skills --skill web-design-guidelines -g -y

# One skill, one agent
skills add vercel-labs/agent-skills --skill web-design-guidelines -a claude-code
```

The `-y` run installs once into the canonical directory and links the agents
that need their own copy:

```text
◇  Installed 1 skill ──────────────────────────────────╮
│                                                      │
│  ✓ ./.agents/skills/web-design-guidelines            │
│    universal: Antigravity, Codex, Gemini CLI,        │
│    GitHub Copilot, OpenCode +16 more                 │
│    symlinked: Claude Code                            │
│                                                      │
╰──────────────────────────────────────────────────────╯

│
└  Done!  Review skills before use; they run with full agent permissions.
```

`universal` agents read `.agents/skills` directly, so one copy serves them
all. Agents with their own skills directory get a symlink into it, and agents
with no directory in this project are skipped rather than created.

### Other commands

| Command                          | Description                      |
| -------------------------------- | -------------------------------- |
| `skills find [query]`            | Search skills.sh                 |
| `skills list` (`ls`)             | List installed skills            |
| `skills use <source>@<skill>`    | Use one skill without installing |
| `skills check`                   | Check for available updates      |
| `skills update [skills...]`      | Update installed skills          |
| `skills remove [skills...]`      | Remove installed skills          |
| `skills experimental_install`    | Restore from `skills-lock.json`  |
| `skills init [name]`             | Create a new skill               |

`skills list` after the install above:

```text
Project Skills

  web-design-guidelines ./.agents/skills/web-design-guidelines
    Agents: Antigravity, Claude Code, Codex, Gemini CLI, GitHub Copilot +4 more  Source: vercel-labs/agent-skills
```

### Compatibility with the upstream CLI

- The same on-disk model: canonical `.agents/skills` (project) and
  `~/.agents/skills` (home), relative symlinks for agents that do not read the
  canonical location, `--copy` mode, and the same
  `missing-agent-project-directory` skip rules.
- The same lock files: `skills-lock.json` (project, schema v1, committed) and
  `.skill-lock.json` (global, schema v3). Skills installed by the upstream CLI
  are listed and removable by this one, and the other way around. Verified
  bidirectionally against `skills@1.7.6`.
- The same command surface (`add`/`a`/`install`/`i`, `use`, `remove`/`rm`/`r`,
  `list`/`ls`, `find`/`search`/`f`/`s`, `update`/`upgrade`, `check`,
  `experimental_install`, `init`, `experimental_sync`), the same flags, the
  same `--json` machine output, and the same interactive wizard (searchable
  multiselect with the locked universal section, an install-method prompt, a
  scope prompt, and installation summary notes).
- The same diagnostics rendering (`[SKILLS_CODE] why` + `fix:` lines) and the
  same agent registry (79 agents including the neutral `universal` target).

Deliberate divergences: no telemetry is sent anywhere, the Notion pack
integration is not implemented, and `experimental_sync` covers the core
node_modules flows (shipped skills, `npm:` entries, and staleness pruning) but
not remote `skills`-field git sources.

### Embed it from Rust

Install through the library, no CLI needed:

```toml
[dependencies]
skills-store = { path = "skills-store" }   # or a crates.io release when published
```

```rust,no_run
use std::path::Path;
use skills_store::{install_skill_dir, InstallOptions, Scope};

let mut warnings = Vec::new();
for agent in skills_store::detect_installed_agents(Path::new(".")) {
    install_skill_dir(
        Path::new("./my-skill"),
        agent,
        &InstallOptions { global: Scope::Project, ..Default::default() },
        &mut warnings,
    );
}
```

`skills-store` is network-free by design: fetch sources yourself (git clone,
HTTPS download, and well-known discovery) and hand the material to
`install_skill_dir` (directory on disk) or `install_skill_files` (in-memory
files). The `skills` binary in `skills-cli/` wires the two together.

## Use the library from Rust

```toml
[dependencies]
agent-config = "0.5"
```

Minimal install: build a spec, look up the harness by id, call `install`.

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

That writes the hook into the harness's config and records it for clean removal.
Per-surface examples (MCP, skills, instructions, dry-run, uninstall) are in
[Usage](#usage) below.

> **Tip:** Prefer `.try_build()?` over `.build()` in production code.
> The `build()` method panics on invalid specs, which is fine for tests
> but not for a running application.

Each snippet has a runnable counterpart under
[`examples/`](examples/README.md). Run with `cargo run --example <name>`. Every
example writes into a fresh tempdir so it never touches the host's real config.

## Browse every surface in a TUI

Want to understand what gets installed where before writing any code? A bundled
ratatui example shows `plan_install_*` outputs across every supported harness
side-by-side, no code required:

```bash
cargo run --example tui_dry_run
```

Four tabs (SKILLS, MCP, HOOKS, INSTRUCTIONS) each list the harnesses that
support that surface. Move with `↑`/`↓` (or `j`/`k`), check rows with
`Space`, flip `Local` / `Global` scope with `g`, and press `Enter` for an
aggregate dry-run across the selection.

Press `?` for the full keymap, `q` to quit. Nothing is written to disk.
Only `plan_install_*` runs. Flipping to `Global` safely previews real
`~/.claude/...`, `~/.gemini/...`, etc. paths without touching them.

Runnable: [`examples/tui_dry_run/`](examples/tui_dry_run/main.rs). For
all bundled examples see [`examples/README.md`](examples/README.md).

## Supported harnesses

| Harness                | ID            | Hooks              | Prompt rules | MCP              | Skills           | Instructions     |
| ---------------------- | ------------- | ------------------ | ------------ | ---------------- | ---------------- | ---------------- |
| [Claude Code](docs/agents/claude.md)          | `claude`      | Global + Local     | yes          | Global + Local   | Global + Local   | Global + Local   |
| [Cursor](docs/agents/cursor.md)               | `cursor`      | Global + Local     | -            | Global + Local   | Global + Local   | -                |
| [Gemini CLI](docs/agents/gemini.md)           | `gemini`      | Global + Local     | yes          | Global + Local   | Global + Local   | Global + Local   |
| [Codex CLI](docs/agents/codex.md)             | `codex`       | Global + Local     | yes          | Global + Local   | Global + Local   | Global + Local   |
| [GitHub Copilot](docs/agents/copilot.md)       | `copilot`     | Local              | yes          | Global + Local   | Global + Local   | Local            |
| [OpenCode](docs/agents/opencode.md)           | `opencode`    | Global + Local     | yes          | Global + Local   | Global + Local   | Global + Local   |
| [Cline](docs/agents/cline.md)                 | `cline`       | Local              | yes          | Global           | Global + Local   | Local            |
| [Roo Code](docs/agents/roo.md)                | `roo`         | -                  | yes          | Global + Local   | -                | Local            |
| [Windsurf](docs/agents/windsurf.md)           | `windsurf`    | Local              | yes          | Global + Local   | Global + Local   | Local            |
| [Kilo Code](docs/agents/kilocode.md)          | `kilocode`    | -                  | yes          | Global + Local   | Global + Local   | Local            |
| [Google Antigravity](docs/agents/antigravity.md) | `antigravity` | -                | yes          | Global + Local   | Global + Local   | Local            |
| [Antigravity CLI](docs/agents/antigravitycli.md) | `antigravitycli` | -             | yes          | Global + Local   | Global + Local   | Global + Local   |
| [Amp](docs/agents/amp.md)                     | `amp`         | -                  | yes          | Global + Local   | Global + Local   | Global + Local   |
| [CodeBuddy CLI](docs/agents/codebuddy.md)     | `codebuddy`   | Global + Local     | yes          | -                | Global + Local   | Global + Local   |
| [Charm Crush](docs/agents/crush.md)           | `crush`       | Global + Local     | yes          | Global + Local   | Global + Local   | Global + Local   |
| [Forge](docs/agents/forge.md)                 | `forge`       | -                  | yes          | Global + Local   | Global + Local   | Global + Local   |
| [iFlow CLI](docs/agents/iflow.md)             | `iflow`       | Global + Local     | -            | Global + Local   | -                | -                |
| [JetBrains Junie](docs/agents/junie.md)       | `junie`       | -                  | yes (Local)  | Global + Local   | -                | Local            |
| [Pi](docs/agents/pi.md)                       | `pi`          | -                  | yes          | Global + Local   | Global + Local   | Global + Local   |
| [Qoder CLI](docs/agents/qodercli.md)          | `qodercli`    | -                  | yes          | Global + Local   | -                | Global + Local   |
| [Qwen Code](docs/agents/qwen.md)              | `qwen`        | -                  | yes          | Global + Local   | Global + Local   | Global + Local   |
| [Tabnine CLI](docs/agents/tabnine.md)         | `tabnine`     | Global + Local     | -            | Global + Local   | -                | -                |
| [Trae](docs/agents/trae.md)                   | `trae`        | -                  | yes (Local)  | -                | Global + Local   | Local            |
| [OpenClaw](docs/agents/openclaw.md)           | `openclaw`    | -                  | yes (Local)  | Global           | Global + Local   | Local            |
| [Hermes Agent](docs/agents/hermes.md)         | `hermes`      | -                  | yes (Local)  | Global           | Global + Local   | Local            |

**Lifecycle notes:**

- **Roo Code** (`roo`) is **retired**. All Roo Code products shut down on
  2026-05-15. The integration is kept for compatibility and migration only.
- **Gemini CLI** (`gemini`) is **legacy / conditional**. Consumer tiers (free,
  AI Pro, AI Ultra) stopped being served on 2026-06-18. Enterprise and paid
  API-key users remain supported. New consumer-tier work belongs on
  `antigravitycli`.

- Per-harness install paths, JSON shapes, and event/matcher mappings:
  [`docs/agents/`](docs/agents/README.md)
- Release support contract: [`docs/support-matrix.md`](docs/support-matrix.md)
- Machine-readable manifest of surfaces, paths, ledger files, and marker
  conventions: [`schema/agents.json`](schema/agents.json), auto-generated by
  [`examples/gen_schema.rs`](examples/gen_schema.rs) from the live registry
- Runnable end-to-end examples for every surface:
  [`examples/`](examples/README.md)
- Security model and ownership-ledger guarantees:
  [`docs/SECURITY.md`](docs/SECURITY.md)

> **Schema platform note.** `schema/agents.json` is the **Linux-canonical**
> view. Several agents (Cline, Roo) resolve their global MCP path through
> `paths::config_dir()`, which differs across macOS, Linux, and Windows, so
> regenerate the schema on Linux for byte-stable output. The companion
> snapshot test (`tests/schema_golden.rs`) only enforces byte equality on
> Linux for the same reason. Downstream tooling that consumes
> `schema/agents.json` should either pin to the Linux view or compute
> per-platform paths client-side.

Native OpenClaw hook/plugin installation is still deferred because upstream
exposes that as a CLI-managed plugin lifecycle rather than a stable file-backed
hook contract.

## Usage

The hook install above is the minimal case. Every other surface follows the
same shape: build a spec, look it up by id, call the matching method.

### Scopes and tags

`Scope::Global` writes under the user's config dir
(`~/.claude/`, `~/.cursor/`, etc.). `Scope::Local(PathBuf)` writes inside
a specific project root. For the scope-gating contract (which tools
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

Runnable: [`examples/hooks_install_uninstall.rs`](examples/hooks_install_uninstall.rs).

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
file. The fence shape is identical.

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
        .disable_model_invocation(true)
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

Three placement modes are available. Pick the one that matches the harness:

```text
  InstructionPlacement
    │
    ├─ ReferencedFile   →  write <name>.md  +  inject @<name>.md fenced block in host
    │                       (Claude — has a documented `@import` syntax)
    │
    ├─ InlineBlock      →  inject body as a fenced block inside the host file (no separate file)
    │                       (Codex, Gemini, Antigravity CLI, Copilot, CodeBuddy,
    │                        Amp, Crush, Forge, Pi, Qoder, Qwen, Junie, Trae,
    │                        OpenClaw, Hermes)
    │
    └─ StandaloneFile   →  write <name>.md only, no host edit
                            (Cline, Roo, Kilo Code, Windsurf, Antigravity —
                             agents whose memory model is a per-file rules dir)
```

Uninstall is keyed on `(name, owner_tag)`. This mirrors MCP and skills:

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
  lifecycle and ledger-tracked ownership. ReferencedFile placements also write
  a separate file that the harness loads via `@import`. Best for long-lived
  toolchain instructions that should remain even when no hooks are installed.

#### Naming caveat

For `ReferencedFile` and `InlineBlock` placements, the instruction's `name` is
also the markdown fence tag (`<!-- BEGIN AGENT-CONFIG:<name> --> ... -->`).
A hook with `tag = "T"` and an instruction with `name = "T"` in the same
memory file collide. The second upsert silently replaces the first.

Pick a name that does not collide with any of your hook tags (prefix it,
e.g. `instr-myapp`, or use `StandaloneFile` placement).

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
accepts. Copilot, for example, is `Local` only. Calling `install` on it
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
`plan_install_instruction` (and the matching uninstall variants). Each returns
an `InstallPlan` / `UninstallPlan` whose `status` is `WillChange`, `NoOp`, or
`Refused`. Its `changes` enumerate every file write, ledger entry, backup, and
permission change the real call would perform, without touching
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

## Supported platforms

The crate runs the same way on every platform it builds for, but the
*environment* it targets is whatever it is launched in:

- **Native macOS / Linux**: fully supported.
- **Native Windows**: supported for harnesses with a documented Windows
  config path (e.g. Claude, Cursor, Codex, Gemini, the `~/.cline/` CLI data
  dir for Cline, the VS Code-extension MCP file for Roo).
  `paths::home_dir()` honors `%USERPROFILE%` and
  `paths::config_dir()` honors `%APPDATA%`. Cline's hook surface writes a
  `bash`-shebanged executable script and is therefore **refused on native
  Windows** with [`RefusalReason::UnsupportedPlatform`] / the matching
  [`AgentConfigError::UnsupportedPlatform`]. Cline rules and MCP / skill /
  instruction installs continue to work.
- **WSL**: treated as a Linux environment. A binary running inside WSL
  resolves `$HOME` and writes WSL config. It does not magically reach the
  Windows host profile under `/mnt/c/...`.
- **Targeting Windows host config from inside WSL**: explicitly out of
  scope for v1. Future work would gate this behind an opt-in `GlobalTarget`
  / `PathRoots` API rather than auto-detection.

[`HookCommand::render_shell`] produces POSIX-shell quoting. Consumers
targeting a non-POSIX shell (PowerShell / `cmd.exe`) should construct
[`HookCommand::ShellUnchecked`] and quote the command themselves.

## License

MIT licensed. See [LICENSE](LICENSE).
</content>
</invoke>
