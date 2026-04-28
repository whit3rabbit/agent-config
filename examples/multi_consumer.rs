//! Two consumers (`app-a` and `app-b`) coexist in one project.
//!
//! Demonstrates:
//!
//! - Each consumer installs its own MCP server under its own owner tag
//! - The sidecar ledger (`.agent-config-mcp.json`) records both owners
//! - Trying to uninstall the other consumer's server returns
//!   `NotOwnedByCaller`, never silently removes the entry
//! - Each consumer cleans up only what it installed
//!
//! Run: `cargo run --example multi_consumer`

use agent_config::{mcp_by_id, AgentConfigError, McpSpec, Result, Scope};

fn main() -> Result<()> {
    let project = tempfile::tempdir().expect("create tempdir");
    let scope = Scope::Local(project.path().to_path_buf());
    let claude = mcp_by_id("claude").expect("claude supports MCP");

    let server_a = McpSpec::builder("github")
        .owner("app-a")
        .stdio("npx", ["-y", "@modelcontextprotocol/server-github"])
        .env_from_host("GITHUB_TOKEN")
        .try_build()?;

    let server_b = McpSpec::builder("filesystem")
        .owner("app-b")
        .stdio(
            "npx",
            ["-y", "@modelcontextprotocol/server-filesystem", "."],
        )
        .try_build()?;

    let _ = claude.install_mcp(&scope, &server_a)?;
    let _ = claude.install_mcp(&scope, &server_b)?;
    println!("both servers installed in {:?}", project.path());

    // Inspect the ledger. It is plain JSON keyed by server name.
    let ledger = project.path().join(".agent-config-mcp.json");
    println!(
        "ledger ({}):\n{}",
        ledger.display(),
        std::fs::read_to_string(&ledger).expect("ledger exists")
    );

    // app-a tries to remove app-b's server. Refused with NotOwnedByCaller.
    let err = claude
        .uninstall_mcp(&scope, "filesystem", "app-a")
        .unwrap_err();
    match err {
        AgentConfigError::NotOwnedByCaller {
            kind,
            name,
            expected,
            actual,
        } => {
            println!(
                "\nrefused cross-owner uninstall: kind={kind} name={name} expected={expected} actual={actual:?}"
            );
            assert_eq!(actual.as_deref(), Some("app-b"));
        }
        other => return Err(other),
    }

    // Each consumer tears down its own server.
    let _ = claude.uninstall_mcp(&scope, "github", "app-a")?;
    let _ = claude.uninstall_mcp(&scope, "filesystem", "app-b")?;

    // Hand-installed entries (config entry without ledger entry) are
    // similarly refused. Adoption is opt-in via `adopt_unowned(true)`.
    let recovery_spec = McpSpec::builder("manual")
        .owner("app-a")
        .stdio("manual-cmd", [] as [&str; 0])
        .adopt_unowned(true)
        .try_build()?;
    println!(
        "\nrecovery spec ready (adopt_unowned={})",
        recovery_spec.adopt_unowned
    );
    Ok(())
}
