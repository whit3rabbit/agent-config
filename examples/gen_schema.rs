//! Render the live agent schema to `schema/agents.json`.
//!
//! Calls [`agent_config::schema::build`] and writes the result with a
//! consistent pretty-printed shape. The first key in the document is
//! `"_warning"`, which makes the auto-generated origin obvious to anyone
//! grepping the file.
//!
//! Run: `cargo run --example gen_schema`
//!
//! The companion test at `tests/schema_golden.rs` verifies that the
//! checked-in file matches the live build. Set `AGENT_SCHEMA_UPDATE=1`
//! when running that test to regenerate.

use std::path::PathBuf;

use agent_config::schema;

fn main() -> std::io::Result<()> {
    let value = schema::build();
    let mut text = serde_json::to_string_pretty(&value).expect("schema serialises");
    text.push('\n');

    let out = output_path();
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out, &text)?;

    eprintln!("wrote {} ({} bytes)", out.display(), text.len());
    Ok(())
}

fn output_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join("agents.json")
}
