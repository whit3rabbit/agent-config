//! Install an MCP server safely, including the secret policy.
//!
//! Demonstrates:
//!
//! - `McpSpec::builder` with `stdio(...)` and `env_from_host(...)` to keep the
//!   actual secret out of the project file
//! - That a default-policy install with an inline secret is refused with
//!   `InlineSecretInLocalScope`
//! - That `allow_local_inline_secrets()` opts into writing the value
//!   verbatim, when the caller has explicitly decided that is acceptable
//! - Idempotent reinstall + uninstall keyed on `(name, owner_tag)`
//!
//! Run: `cargo run --example mcp_install`

use agent_config::{mcp_by_id, AgentConfigError, McpSpec, Result, Scope};

fn main() -> Result<()> {
    let project = tempfile::tempdir().expect("create tempdir");
    let scope = Scope::Local(project.path().to_path_buf());

    let claude = mcp_by_id("claude").expect("claude supports MCP");

    // 1. Inline secret with default policy. Refused before any write.
    let inline_spec = McpSpec::builder("github")
        .owner("myapp")
        .stdio("npx", ["-y", "@modelcontextprotocol/server-github"])
        .env("GITHUB_TOKEN", "ghp_xxxxxxxxxxxxxxxxxxxx")
        .try_build()?;

    match claude.install_mcp(&scope, &inline_spec) {
        Err(AgentConfigError::InlineSecretInLocalScope { name, key }) => {
            println!("refused: server={name:?} env_key={key:?}");
        }
        Err(other) => return Err(other),
        Ok(_) => unreachable!("default policy must refuse inline secrets locally"),
    }

    // 2. Same server, but reading the secret from the host environment at
    //    harness launch time. The project file only contains "${GITHUB_TOKEN}".
    let safe_spec = McpSpec::builder("github")
        .owner("myapp")
        .stdio("npx", ["-y", "@modelcontextprotocol/server-github"])
        .env_from_host("GITHUB_TOKEN")
        .try_build()?;

    let install = claude.install_mcp(&scope, &safe_spec)?;
    println!(
        "\ninstalled github MCP (created={:?}, patched={:?})",
        install.created, install.patched
    );

    // 3. Reinstall with byte-identical content is a no-op.
    let again = claude.install_mcp(&scope, &safe_spec)?;
    assert!(again.already_installed, "reinstall must be a no-op");
    println!(
        "reinstall was idempotent: already_installed = {}",
        again.already_installed
    );

    // 4. Optional escape hatch: when the caller knows the inline value is
    //    safe to commit (e.g. a non-secret API key for a public service),
    //    `allow_local_inline_secrets()` opts out of the policy.
    let public_spec = McpSpec::builder("public-key")
        .owner("myapp")
        .stdio("npx", ["-y", "@example/server"])
        .env("API_KEY", "intentionally-public-value")
        .allow_local_inline_secrets()
        .try_build()?;
    let _ = claude.install_mcp(&scope, &public_spec)?;

    // 5. Uninstall is keyed on (server name, owner tag). Trying to uninstall
    //    with the wrong owner returns `NotOwnedByCaller`, never silently
    //    removes another consumer's server.
    let removed = claude.uninstall_mcp(&scope, "github", "myapp")?;
    println!(
        "\nuninstalled github (removed={:?}, patched={:?})",
        removed.removed, removed.patched
    );

    let wrong_owner_err = claude
        .uninstall_mcp(&scope, "public-key", "not-myapp")
        .unwrap_err();
    assert!(matches!(
        wrong_owner_err,
        AgentConfigError::NotOwnedByCaller { .. }
    ));
    println!("wrong-owner uninstall refused as expected: {wrong_owner_err}");

    let _ = claude.uninstall_mcp(&scope, "public-key", "myapp")?;
    Ok(())
}
