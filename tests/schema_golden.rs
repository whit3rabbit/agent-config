//! Golden test for the auto-generated agent schema.
//!
//! - Default (Linux): builds the schema live and asserts byte-equality
//!   against the checked-in `schema/agents.json`.
//! - macOS / Windows: still builds the schema (catches panics, validates
//!   that every registered agent serialises) but skips the byte compare,
//!   because a few harness paths flow through `paths::config_dir()` and
//!   render differently per OS.
//! - `AGENT_SCHEMA_UPDATE=1`: regenerates `schema/agents.json` instead of
//!   asserting. Mirrors the `AGENT_CONFIG_UPDATE_GOLDENS=1` pattern used by
//!   the per-agent fixtures in `tests/golden.rs`. Run on Linux for the
//!   canonical output.

use std::path::PathBuf;

use agent_config::schema;

fn schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join("agents.json")
}

fn render_live() -> String {
    let value = schema::build();
    let mut text = serde_json::to_string_pretty(&value).expect("schema serialises");
    text.push('\n');
    text
}

#[test]
fn schema_matches_checked_in_fixture() {
    let live = render_live();
    let path = schema_path();

    if std::env::var_os("AGENT_SCHEMA_UPDATE").is_some() {
        if !cfg!(target_os = "linux") {
            eprintln!(
                "AGENT_SCHEMA_UPDATE on non-Linux host: schema will reflect this OS's paths and \
                 will not match Linux CI. Regenerate on Linux for the canonical view."
            );
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create schema dir");
        }
        std::fs::write(&path, &live).expect("write schema/agents.json");
        eprintln!("regenerated {}", path.display());
        return;
    }

    if !cfg!(target_os = "linux") {
        // Still exercises the generator (panics propagate, every agent
        // serialises). Just skip the byte-equality assertion.
        eprintln!(
            "schema_golden: skipping byte compare on non-Linux ({} bytes built)",
            live.len()
        );
        return;
    }

    let on_disk = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "could not read {}: {e}. Run `AGENT_SCHEMA_UPDATE=1 cargo test --test schema_golden` \
             or `cargo run --example gen_schema` to generate it.",
            path.display()
        )
    });

    if on_disk != live {
        panic!(
            "{} is out of date. Run `AGENT_SCHEMA_UPDATE=1 cargo test --test schema_golden` \
             or `cargo run --example gen_schema` to regenerate.",
            path.display()
        );
    }
}
