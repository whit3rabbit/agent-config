//! A config file larger than the 8 MiB cap surfaces ConfigTooLarge,
//! not a parse failure or OOM.

#![cfg(unix)]

use std::fs::File;

use agent_config::{mcp_by_id, AgentConfigError, McpSpec, Scope};
use tempfile::tempdir;

#[test]
fn install_mcp_rejects_oversized_existing_config() {
    let claude = mcp_by_id("claude").expect("claude MCP support registered");
    let project = tempdir().unwrap();
    let cfg = project.path().join(".mcp.json");
    // Sparse file: logical length above the cap, no actual disk pressure.
    let f = File::create(&cfg).unwrap();
    f.set_len(16 * 1024 * 1024).unwrap();

    let spec = McpSpec::builder("alpha")
        .owner("appA")
        .stdio("/bin/true", [] as [&str; 0])
        .build();

    let err = claude
        .install_mcp(&Scope::Local(project.path().to_path_buf()), &spec)
        .expect_err("install_mcp must refuse an oversized config");
    assert!(
        matches!(err, AgentConfigError::ConfigTooLarge { .. }),
        "expected ConfigTooLarge, got {err:?}",
    );
}
