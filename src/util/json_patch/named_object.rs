//! Insert/remove/lookup of named entries inside a JSON object (the
//! `mcpServers` shape).

use serde_json::Value;

use crate::error::HookerError;

use super::common::{ensure_object, prune_empty_path, traverse_object_mut};

/// Insert or replace a value under a named key inside an object at `path`.
/// Used by the MCP installers where each server is a top-level entry of the
/// `mcpServers` object keyed by server name (Claude/Cursor/Gemini/Windsurf).
///
/// Returns `true` if the on-disk shape changed.
pub(crate) fn upsert_named_object_entry(
    root: &mut Value,
    path: &[&str],
    name: &str,
    value: Value,
) -> Result<bool, HookerError> {
    let parent = ensure_object(root, path)?;
    match parent.get(name) {
        Some(existing) if existing == &value => Ok(false),
        _ => {
            parent.insert(name.to_string(), value);
            Ok(true)
        }
    }
}

/// Remove a named entry from the object at `path`. Returns `true` if a value
/// was removed. Prunes empty parents the same way as the tagged-array helper.
pub(crate) fn remove_named_object_entry(
    root: &mut Value,
    path: &[&str],
    name: &str,
) -> Result<bool, HookerError> {
    let Some(parent) = traverse_object_mut(root, path) else {
        return Ok(false);
    };
    if parent.remove(name).is_none() {
        return Ok(false);
    }
    prune_empty_path(root, path);
    Ok(true)
}

/// Returns true if a value is recorded at `<path>.<name>`.
pub(crate) fn contains_named(root: &Value, path: &[&str], name: &str) -> bool {
    let mut cur = root;
    for key in path {
        let Some(next) = cur.get(*key) else {
            return false;
        };
        cur = next;
    }
    cur.get(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn upsert_named_object_entry_inserts_and_replaces() {
        let mut root = json!({});
        let changed = upsert_named_object_entry(
            &mut root,
            &["mcpServers"],
            "github",
            json!({ "command": "npx" }),
        )
        .unwrap();
        assert!(changed);
        assert_eq!(
            root,
            json!({ "mcpServers": { "github": { "command": "npx" } } })
        );

        // Replacing with different value -> changed.
        let changed = upsert_named_object_entry(
            &mut root,
            &["mcpServers"],
            "github",
            json!({ "command": "node" }),
        )
        .unwrap();
        assert!(changed);

        // Same value -> not changed.
        let unchanged = upsert_named_object_entry(
            &mut root,
            &["mcpServers"],
            "github",
            json!({ "command": "node" }),
        )
        .unwrap();
        assert!(!unchanged);
    }

    #[test]
    fn remove_named_object_entry_prunes_empty_parents() {
        let mut root = json!({
            "mcpServers": { "github": { "command": "npx" } }
        });
        let removed = remove_named_object_entry(&mut root, &["mcpServers"], "github").unwrap();
        assert!(removed);
        assert_eq!(root, json!({}));
    }

    #[test]
    fn remove_named_object_entry_unknown_is_noop() {
        let mut root = json!({ "mcpServers": { "alpha": {} } });
        let removed = remove_named_object_entry(&mut root, &["mcpServers"], "ghost").unwrap();
        assert!(!removed);
    }

    #[test]
    fn contains_named_finds_entry() {
        let root = json!({ "mcpServers": { "github": {} } });
        assert!(contains_named(&root, &["mcpServers"], "github"));
        assert!(!contains_named(&root, &["mcpServers"], "missing"));
        assert!(!contains_named(&root, &["other"], "github"));
    }
}
