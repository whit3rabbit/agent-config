# agent-config

A Rust library that installs hooks, prompt rules, MCP servers, and skills into
AI coding harnesses.

It centralizes the per-harness file locations and config shapes so downstream
tools do not have to reimplement them.

You supply a `HookSpec`, `McpSpec`, or `SkillSpec`. The library knows where
each harness keeps its config, what shape that config takes, and how to write
to it safely.

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
  fences. MCP servers and skills use sidecar ledgers. Multiple consumers
  coexist without stepping on each other.

## What is supported

| Harness                | ID            | Hooks              | Prompt rules | MCP              | Skills           |
| ---------------------- | ------------- | ------------------ | ------------ | ---------------- | ---------------- |
| [Claude Code]          | `claude`      | Global + Local     | yes          | Global + Local   | Global + Local   |
| [Cursor]               | `cursor`      | Global + Local     | -            | Global + Local   | Global + Local   |
| [Gemini CLI]           | `gemini`      | Global + Local     | yes          | Global + Local   | Global + Local   |
| [Codex CLI]            | `codex`       | Global + Local     | yes          | Global + Local   | Global + Local   |
| [GitHub Copilot]       | `copilot`     | Local              | yes          | Global + Local   | Global + Local   |
| [OpenCode]             | `opencode`    | Global + Local     | -            | Global + Local   | Global + Local   |
| [Cline]                | `cline`       | Local              | yes          | Global           | Global + Local   |
| [Roo Code]             | `roo`         | -                  | yes          | Global + Local   | -                |
| [Windsurf]             | `windsurf`    | Local              | yes          | Global + Local   | Global + Local   |
| [Kilo Code]            | `kilocode`    | -                  | yes          | Global + Local   | Global + Local   |
| [Google Antigravity]   | `antigravity` | -                  | yes          | Global + Local   | Global + Local   |
| [Amp]                  | `amp`         | -                  | yes          | Global + Local   | Global + Local   |
| [CodeBuddy CLI]        | `codebuddy`   | Global + Local     | yes          | -                | Global + Local   |
| [Forge]                | `forge`       | -                  | yes          | Global + Local   | Global + Local   |
| [iFlow CLI]            | `iflow`       | Global + Local     | -            | Global + Local   | -                |
| [JetBrains Junie]      | `junie`       | -                  | yes (Local)  | Global + Local   | -                |
| [Qoder CLI]            | `qodercli`    | -                  | yes          | Global + Local   | -                |
| [Qwen Code]            | `qwen`        | -                  | yes          | Global + Local   | Global + Local   |
| [Tabnine CLI]          | `tabnine`     | Global + Local     | -            | Global + Local   | -                |
| [Trae]                 | `trae`        | -                  | yes (Local)  | -                | Global + Local   |
| [OpenClaw]             | `openclaw`    | -                  | yes (Local)  | Global           | Global + Local   |
| [Hermes Agent]         | `hermes`      | -                  | yes (Local)  | Global           | Global           |

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
Native OpenClaw hook/plugin installation is still deferred because upstream
exposes that as a CLI-managed plugin lifecycle rather than a stable file-backed
hook contract.

## How to use it in your Rust app

Add the dependency:

```toml
[dependencies]
agent-config = "0.1"
```

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

`Scope::Global` writes under the user's harness config dir
(`~/.claude/`, `~/.cursor/`, etc.). `Scope::Local(PathBuf)` writes inside
a specific project root.

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

MCP uninstall is keyed by server name plus owner tag. If another consumer owns
the server, or the server was hand-installed and has no ownership ledger entry,
`uninstall_mcp` returns `AgentConfigError::NotOwnedByCaller`.

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

### Iterate every supported harness

```rust
use agent_config::{all, mcp_capable, skill_capable, Scope};

for integration in all() {
    if integration.is_installed(&Scope::Global, "myapp")? {
        println!("{} has myapp installed", integration.display_name());
    }
}

for mcp in mcp_capable() {
    println!("{} supports MCP", mcp.id());
}

for skills in skill_capable() {
    println!("{} supports skills", skills.id());
}
```

`Integration::supported_scopes()` tells you which scopes a given harness
accepts. Copilot, for example, is `Local` only; calling `install` on it
with `Scope::Global` returns `AgentConfigError::UnsupportedScope`.

Use `mcp_capable()` / `mcp_by_id()` for MCP support and `skill_capable()` /
`skill_by_id()` for skills. Their scope sets can differ from hook support:
Cline hooks are local-only, while Cline MCP is global-only.

### Errors

All operations return `Result<T, AgentConfigError>`. Variants worth handling
explicitly:

- `UnsupportedScope`: the harness does not accept this scope kind.
- `MissingSpecField`: e.g., Gemini's script delegator requires
  `HookSpec::script`, prompt-only agents require `HookSpec::rules`.
- `InvalidTag`: empty or contains characters outside `[A-Za-z0-9_-]`.
- `BackupExists`: a first-touch `<path>.bak` could not be created safely.
  Existing backups are normally preserved and reused.
- `NotOwnedByCaller`: an MCP server or skill is owned by another consumer or
  was hand-installed without an agent-config ledger entry.
- `Io` and `JsonInvalid` carry the offending path.

## License

MIT OR Apache-2.0
