//! Insert/remove/lookup of name-keyed entries inside a JSON array. Used by
//! harness shapes that store MCP servers as an ordered list keyed by a `name`
//! field rather than as a `mcpServers` object.
//!
//! Currently scaffolding for future surfaces; `#[allow(dead_code)]` is
//! deliberate per `CLAUDE.md` and must not be removed.

use serde_json::Value;

use crate::error::HookerError;

use super::common::{ensure_array, prune_empty_path, traverse_array, traverse_array_mut};

/// Insert/replace an entry inside a JSON array, keyed by a string field.
///
/// Unlike [`super::tagged_array::upsert_tagged_array_entry`], no marker is
/// added — the entry's own `name` field is the identity, since the harness
/// loads entries by name.
#[allow(dead_code)]
pub(crate) fn upsert_named_array_entry(
    root: &mut Value,
    path: &[&str],
    name_field: &str,
    name: &str,
    value: Value,
) -> Result<bool, HookerError> {
    let arr = ensure_array(root, path)?;
    let idx = arr.iter().position(|v| {
        v.as_object()
            .and_then(|o| o.get(name_field))
            .and_then(Value::as_str)
            == Some(name)
    });
    match idx {
        Some(i) if arr[i] == value => Ok(false),
        Some(i) => {
            arr[i] = value;
            Ok(true)
        }
        None => {
            arr.push(value);
            Ok(true)
        }
    }
}

/// Remove the array entry whose `name_field` matches `name`. Returns `true`
/// if removed; prunes empty parent containers.
#[allow(dead_code)]
pub(crate) fn remove_named_array_entry(
    root: &mut Value,
    path: &[&str],
    name_field: &str,
    name: &str,
) -> Result<bool, HookerError> {
    let Some(arr) = traverse_array_mut(root, path) else {
        return Ok(false);
    };
    let Some(idx) = arr.iter().position(|v| {
        v.as_object()
            .and_then(|o| o.get(name_field))
            .and_then(Value::as_str)
            == Some(name)
    }) else {
        return Ok(false);
    };
    arr.remove(idx);
    prune_empty_path(root, path);
    Ok(true)
}

/// Returns true if any array entry under `path` matches `name_field == name`.
#[allow(dead_code)]
pub(crate) fn contains_in_named_array(
    root: &Value,
    path: &[&str],
    name_field: &str,
    name: &str,
) -> bool {
    let Some(arr) = traverse_array(root, path) else {
        return false;
    };
    arr.iter().any(|v| {
        v.as_object()
            .and_then(|o| o.get(name_field))
            .and_then(Value::as_str)
            == Some(name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn upsert_named_array_entry_round_trip() {
        let mut root = json!({});
        let changed = upsert_named_array_entry(
            &mut root,
            &["mcp"],
            "name",
            "github",
            json!({ "name": "github", "command": "npx" }),
        )
        .unwrap();
        assert!(changed);
        assert_eq!(
            root,
            json!({ "mcp": [ { "name": "github", "command": "npx" } ] })
        );

        // Same payload -> no change.
        let unchanged = upsert_named_array_entry(
            &mut root,
            &["mcp"],
            "name",
            "github",
            json!({ "name": "github", "command": "npx" }),
        )
        .unwrap();
        assert!(!unchanged);

        // Replace.
        let changed = upsert_named_array_entry(
            &mut root,
            &["mcp"],
            "name",
            "github",
            json!({ "name": "github", "command": "node" }),
        )
        .unwrap();
        assert!(changed);
        let arr = root["mcp"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["command"], json!("node"));
    }

    #[test]
    fn remove_named_array_entry_keeps_other_entries() {
        let mut root = json!({
            "mcp": [
                { "name": "alpha" },
                { "name": "beta" }
            ]
        });
        assert!(remove_named_array_entry(&mut root, &["mcp"], "name", "alpha").unwrap());
        assert_eq!(root, json!({ "mcp": [ { "name": "beta" } ] }));
    }

    #[test]
    fn remove_named_array_entry_prunes_empty_array() {
        let mut root = json!({ "mcp": [ { "name": "alpha" } ] });
        assert!(remove_named_array_entry(&mut root, &["mcp"], "name", "alpha").unwrap());
        assert_eq!(root, json!({}));
    }

    #[test]
    fn contains_in_named_array_finds_entry() {
        let root = json!({ "mcp": [ { "name": "alpha" } ] });
        assert!(contains_in_named_array(&root, &["mcp"], "name", "alpha"));
        assert!(!contains_in_named_array(&root, &["mcp"], "name", "ghost"));
    }
}
