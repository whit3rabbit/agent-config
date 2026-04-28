//! Hook install + idempotent reinstall + uninstall round trip.
//!
//! Demonstrates:
//!
//! - Building a `HookSpec` with `try_build()?`
//! - Installing into `Scope::Local(<tempdir>)` so the host config stays clean
//! - That a second `install` is a no-op (`already_installed == true`)
//! - That `uninstall` removes only this consumer's tagged content
//!
//! Run: `cargo run --example hooks_install_uninstall`

use agent_config::{by_id, Event, HookSpec, Matcher, Result, Scope};

fn main() -> Result<()> {
    // Each example uses a tempdir to avoid touching the user's real config.
    let project = tempfile::tempdir().expect("create tempdir");
    let scope = Scope::Local(project.path().to_path_buf());

    let spec = HookSpec::builder("myapp")
        .command_program("myapp", ["hook", "claude"])
        .matcher(Matcher::Bash)
        .event(Event::PreToolUse)
        .rules("Run `myapp lint` before committing.")
        .try_build()?;

    let claude = by_id("claude").expect("claude is registered");

    // First install: writes settings.json + injects rules into CLAUDE.md.
    let first = claude.install(&scope, &spec)?;
    println!("--- first install ---");
    println!("created : {:?}", first.created);
    println!("patched : {:?}", first.patched);
    println!("backups : {:?}", first.backed_up);
    println!("noop    : {}", first.already_installed);

    // Second install with the same spec: idempotent, nothing changes.
    let second = claude.install(&scope, &spec)?;
    println!("\n--- second install (idempotent) ---");
    println!("noop    : {}", second.already_installed);
    assert!(second.already_installed, "reinstall must be a no-op");

    // Detection.
    assert!(claude.is_installed(&scope, "myapp")?);

    // Uninstall: removes only the tagged JSON entry and the fenced markdown
    // block. Anything else in the user's settings/CLAUDE.md is preserved.
    let removed = claude.uninstall(&scope, "myapp")?;
    println!("\n--- uninstall ---");
    println!("removed : {:?}", removed.removed);
    println!("patched : {:?}", removed.patched);
    println!("restored: {:?}", removed.restored);
    println!("noop    : {}", removed.not_installed);

    assert!(!claude.is_installed(&scope, "myapp")?);
    Ok(())
}
