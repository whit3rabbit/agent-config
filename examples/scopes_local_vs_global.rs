//! `Scope::Global` vs `Scope::Local`, and how `supported_scopes` gates them.
//!
//! Some harnesses accept both scopes, some only `Local` (Copilot, Cline,
//! Windsurf hooks; OpenClaw / Hermes / Trae instructions), some only `Global`
//! (Cline MCP). Calling an unsupported scope returns `UnsupportedScope`
//! instead of writing anywhere.
//!
//! This example does not touch the user's home directory. It shows the
//! `Scope::Global` *intent* via the API but only mutates a tempdir under
//! `Scope::Local`.
//!
//! Run: `cargo run --example scopes_local_vs_global`

use agent_config::{by_id, AgentConfigError, Event, HookSpec, Matcher, Result, Scope, ScopeKind};

fn main() -> Result<()> {
    let project = tempfile::tempdir().expect("create tempdir");
    let local = Scope::Local(project.path().to_path_buf());

    let spec = HookSpec::builder("myapp")
        .command_program("myapp", ["hook"])
        .matcher(Matcher::All)
        .event(Event::PreToolUse)
        .try_build()?;

    // Example A: an integration that supports Global and Local. We only
    // exercise the Local path so the host config stays clean.
    let claude = by_id("claude").expect("claude is registered");
    print_supported_scopes("claude", claude.supported_scopes());
    let report = claude.install(&local, &spec)?;
    println!("  installed locally to {:?}", report.created);
    let _ = claude.uninstall(&local, "myapp")?;

    // Example B: a Local-only integration. Calling Global would fail before
    // any write happens.
    let copilot = by_id("copilot").expect("copilot is registered");
    print_supported_scopes("copilot", copilot.supported_scopes());
    let global_attempt = copilot.install(&Scope::Global, &spec);
    match global_attempt {
        Err(AgentConfigError::UnsupportedScope { id, scope }) => {
            println!("  copilot refused Scope::Global (id={id}, scope={scope:?})");
        }
        Err(other) => return Err(other),
        Ok(_) => unreachable!("copilot must refuse Global scope"),
    }

    // Local-scope install on copilot still works as expected.
    let local_report = copilot.install(&local, &spec)?;
    println!(
        "  copilot local install touched {} files",
        local_report.created.len() + local_report.patched.len()
    );
    let _ = copilot.uninstall(&local, "myapp")?;

    // Example C: querying scope kinds without a payload.
    println!(
        "\nScope::Local payload root: {}",
        local.local_root().expect("Local has root").display()
    );
    println!(
        "Scope::Global has no payload: {:?}",
        Scope::Global.local_root()
    );
    println!("ScopeKind::Local discriminant: {:?}", local.kind());
    println!("ScopeKind::Global discriminant: {:?}", Scope::Global.kind());

    Ok(())
}

fn print_supported_scopes(id: &str, scopes: &[ScopeKind]) {
    let global = scopes.contains(&ScopeKind::Global);
    let local = scopes.contains(&ScopeKind::Local);
    println!("\n{id}: supported_scopes() = global={global}, local={local}");
}
