//! JSON5 input support for config files that accept comments/trailing commas.
//!
//! We parse JSON5 into [`serde_json::Value`] so the existing JSON mutation
//! helpers and strict JSON serializer can be reused. Strict JSON output is
//! still valid JSON5, and keeps this helper small.

use std::path::Path;

use serde_json::{Map, Value};

use crate::error::HookerError;
use crate::util::fs_atomic;

/// Read a JSON5 file, returning an empty object when missing or blank.
pub(crate) fn read_or_empty(path: &Path) -> Result<Value, HookerError> {
    let text = fs_atomic::read_to_string_or_empty(path)?;
    if text.trim().is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    json5::from_str::<Value>(&text).map_err(|e| {
        HookerError::Other(anyhow::anyhow!(
            "invalid JSON5 in {}: {}",
            path.display(),
            e
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn reads_json5_with_comments_and_unquoted_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("openclaw.json");
        std::fs::write(
            &path,
            r#"{
  // OpenClaw accepts JSON5
  mcp: { servers: { docs: { url: 'https://example.com' } } },
}
"#,
        )
        .unwrap();

        let root = read_or_empty(&path).unwrap();
        assert_eq!(
            root["mcp"]["servers"]["docs"]["url"],
            json!("https://example.com")
        );
    }

    #[test]
    fn missing_file_is_empty_object() {
        let dir = tempdir().unwrap();
        let root = read_or_empty(&dir.path().join("missing.json")).unwrap();
        assert_eq!(root, Value::Object(Map::new()));
    }
}
