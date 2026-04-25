# ai-hooker

A Rust library that installs hooks and prompt-level integrations into AI
coding harnesses (Claude Code, Cursor, Gemini CLI, Codex, Copilot, OpenCode,
Cline, Roo, Windsurf, Kilo Code, Antigravity).

You supply a `HookSpec` describing the command to run, the event to attach
to, and any rules markdown to inject. The library knows where each harness
keeps its config, what shape that config takes, and how to write to it
safely.

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
- **Reversible.** `uninstall` removes only the tagged content. JSON entries
  carry an `_ai_hooker_tag` marker; markdown blocks are wrapped in
  `<!-- BEGIN AI-HOOKER:<tag> --> ... <!-- END AI-HOOKER:<tag> -->`
  fences. Multiple consumers coexist without stepping on each other.

## What is supported

| Harness                | ID            | Hooks              | Prompt rules | MCP    | Skills |
| ---------------------- | ------------- | ------------------ | ------------ | ------ | ------ |
| Claude Code            | `claude`      | Global + Local     | yes          | TODO   | TODO   |
| Cursor                 | `cursor`      | Global + Local     | -            | TODO   | -      |
| Gemini CLI             | `gemini`      | Global + Local     | yes          | TODO   | -      |
| Codex CLI              | `codex`       | Global + Local     | yes          | TODO   | -      |
| GitHub Copilot         | `copilot`     | Local              | yes          | -      | -      |
| OpenCode               | `opencode`    | Global + Local     | yes          | TODO   | -      |
| Cline                  | `cline`       | -                  | yes          | -      | -      |
| Roo Code               | `roo`         | -                  | yes          | -      | -      |
| Windsurf               | `windsurf`    | -                  | yes          | -      | -      |
| Kilo Code              | `kilocode`    | -                  | yes          | -      | -      |
| Google Antigravity     | `antigravity` | -                  | yes          | -      | TODO   |

Per-harness install paths, JSON shapes, and event/matcher mappings are
documented in [`docs/agents/`](docs/agents/README.md). MCP server
registration and skill installation are confirmed but not yet implemented;
see [`CLAUDE.md`](CLAUDE.md) for the roadmap.

## How to use it in your Rust app

Add the dependency:

```toml
[dependencies]
ai-hooker = "0.1"
```

### Install a hook

```rust
use ai_hooker::{by_id, Event, HookSpec, Matcher, Scope};

fn main() -> ai_hooker::Result<()> {
    let spec = HookSpec::builder("myapp")          // your consumer tag
        .command("myapp hook claude")              // what the harness runs
        .matcher(Matcher::Bash)                    // filter to shell calls
        .event(Event::PreToolUse)                  // before each tool call
        .build();

    let claude = by_id("claude").expect("claude is registered");
    let report = claude.install(&Scope::Global, &spec)?;

    println!("created: {:?}", report.created);
    println!("patched: {:?}", report.patched);
    println!("backed up: {:?}", report.backed_up);
    Ok(())
}
```

`Scope::Global` writes under the user's harness config dir
(`~/.claude/`, `~/.cursor/`, etc.). `Scope::Local(PathBuf)` writes inside
a specific project root.

The `tag` field is **your application's identifier**, not the harness's.
Pick something stable and ASCII alnum / `_` / `-`. It namespaces every
file, JSON entry, and markdown fence the library writes, so multiple tools
built on `ai-hooker` can install side-by-side.

### Inject prompt rules at the same time

```rust
let spec = HookSpec::builder("myapp")
    .command("myapp hook claude")
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

### Iterate every supported harness

```rust
use ai_hooker::{all, Scope};

for integration in all() {
    if integration.is_installed(&Scope::Global, "myapp")? {
        println!("{} has myapp installed", integration.display_name());
    }
}
```

`Integration::supported_scopes()` tells you which scopes a given harness
accepts. Copilot, for example, is `Local` only; calling `install` on it
with `Scope::Global` returns `HookerError::UnsupportedScope`.

### Errors

All operations return `Result<T, HookerError>`. Variants worth handling
explicitly:

- `UnsupportedScope` — the harness does not accept this scope kind.
- `MissingSpecField` — e.g., Gemini's script delegator requires
  `HookSpec::script`, prompt-only agents require `HookSpec::rules`.
- `InvalidTag` — empty or contains characters outside `[A-Za-z0-9_-]`.
- `BackupExists` — a `<path>.bak` is already present; the library refuses
  to overwrite it. Resolve manually before retrying.
- `Io` and `JsonInvalid` carry the offending path.

## License

MIT OR Apache-2.0
