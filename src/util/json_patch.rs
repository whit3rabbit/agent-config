//! Idempotent insert/remove of consumer-tagged objects inside a settings JSON
//! file.
//!
//! Every entry this module writes carries an `_ai_hooker_tag` marker so we can
//! find and remove our own work without disturbing entries the user added by
//! hand.
//!
//! The on-disk JSON's key order is preserved by enabling
//! `serde_json/preserve_order` in `Cargo.toml`.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::error::HookerError;

/// The marker key embedded in every object we insert. Lets us locate and
/// uninstall our own entries without touching user-authored ones.
pub(crate) const TAG_KEY: &str = "_ai_hooker_tag";

/// Read a JSON file, returning `Value::Object(empty)` when the file is missing.
///
/// Errors propagate verbatim if the file exists but is invalid JSON; the
/// caller should surface this rather than overwriting potentially-precious
/// user config.
pub(crate) fn read_or_empty(path: &Path) -> Result<Value, HookerError> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Value::Object(Map::new()));
        }
        Err(e) => return Err(HookerError::io(path, e)),
    };
    if bytes.is_empty() || bytes.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_slice(&bytes).map_err(|e| HookerError::json(path, e))
}

/// Pretty-serialize a JSON value (2-space indent, trailing newline).
pub(crate) fn to_pretty(value: &Value) -> Vec<u8> {
    let mut buf = serde_json::to_vec_pretty(value).expect("valid JSON serializes");
    buf.push(b'\n');
    buf
}

/// Walk `path` (a list of object keys) into `root`, creating empty objects as
/// needed. Returns a mutable reference to the leaf object.
///
/// The leaf is forced to be an object; if anything along the path is a
/// non-object value, returns a [`HookerError::Other`].
pub(crate) fn ensure_object<'a>(
    root: &'a mut Value,
    path: &[&str],
) -> Result<&'a mut Map<String, Value>, HookerError> {
    if !root.is_object() {
        *root = Value::Object(Map::new());
    }
    let mut cur = root.as_object_mut().unwrap();
    for key in path {
        if !cur.contains_key(*key) {
            cur.insert((*key).to_string(), Value::Object(Map::new()));
        } else if !cur[*key].is_object() {
            return Err(HookerError::Other(anyhow::anyhow!(
                "expected object at JSON path segment {:?}, found {:?}",
                key,
                cur[*key]
            )));
        }
        cur = cur.get_mut(*key).unwrap().as_object_mut().unwrap();
    }
    Ok(cur)
}

/// Walk `path` to a JSON array, creating it if missing. Errors if a non-array
/// value blocks the path.
pub(crate) fn ensure_array<'a>(
    root: &'a mut Value,
    path: &[&str],
) -> Result<&'a mut Vec<Value>, HookerError> {
    if path.is_empty() {
        return Err(HookerError::Other(anyhow::anyhow!(
            "ensure_array requires a non-empty path"
        )));
    }
    let (last, parents) = path.split_last().unwrap();
    let parent = ensure_object(root, parents)?;
    let entry = parent
        .entry((*last).to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    match entry {
        Value::Array(a) => Ok(a),
        other => Err(HookerError::Other(anyhow::anyhow!(
            "expected array at JSON path segment {:?}, found {:?}",
            last,
            other
        ))),
    }
}

/// Insert `entry` into the array at `path`, replacing any existing entry that
/// carries the same `_ai_hooker_tag`. Returns `true` if a change was made.
///
/// Tags the entry with `_ai_hooker_tag = tag` before inserting.
pub(crate) fn upsert_tagged_array_entry(
    root: &mut Value,
    path: &[&str],
    tag: &str,
    mut entry: Value,
) -> Result<bool, HookerError> {
    if let Some(obj) = entry.as_object_mut() {
        obj.insert(TAG_KEY.into(), Value::String(tag.into()));
    } else {
        return Err(HookerError::Other(anyhow::anyhow!(
            "tagged array entries must be JSON objects"
        )));
    }

    let arr = ensure_array(root, path)?;
    if let Some(idx) = find_tagged_index(arr, tag) {
        if arr[idx] == entry {
            return Ok(false);
        }
        arr[idx] = entry;
    } else {
        arr.push(entry);
    }
    Ok(true)
}

/// Remove the entry tagged with `tag` from the array at `path`. Returns `true`
/// if an entry was removed.
pub(crate) fn remove_tagged_array_entry(
    root: &mut Value,
    path: &[&str],
    tag: &str,
) -> Result<bool, HookerError> {
    let Some(arr) = traverse_array_mut(root, path) else {
        return Ok(false);
    };
    let Some(idx) = find_tagged_index(arr, tag) else {
        return Ok(false);
    };
    arr.remove(idx);

    // If the array is now empty, prune it (and its parent objects, if they
    // would also be left empty by our removal). Avoids leaving dangling empty
    // `"hooks": { "PreToolUse": [] }` clutter.
    prune_empty_path(root, path);
    Ok(true)
}

/// True if an entry with `_ai_hooker_tag == tag` exists at `path`.
pub(crate) fn contains_tagged(root: &Value, path: &[&str], tag: &str) -> bool {
    let Some(arr) = traverse_array(root, path) else {
        return false;
    };
    arr.iter().any(|v| matches_tag(v, tag))
}

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

/// Insert/replace an entry inside a JSON array, keyed by a string field
/// (`name` for OpenCode's `mcp` array). Returns `true` if the array changed.
///
/// Unlike [`upsert_tagged_array_entry`], no marker is added — the entry's own
/// `name` field is the identity, since the harness loads MCPs by name.
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

fn find_tagged_index(arr: &[Value], tag: &str) -> Option<usize> {
    arr.iter().position(|v| matches_tag(v, tag))
}

fn matches_tag(v: &Value, tag: &str) -> bool {
    v.as_object()
        .and_then(|o| o.get(TAG_KEY))
        .and_then(Value::as_str)
        == Some(tag)
}

fn traverse_array<'a>(root: &'a Value, path: &[&str]) -> Option<&'a Vec<Value>> {
    let mut cur = root;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_array()
}

fn traverse_array_mut<'a>(root: &'a mut Value, path: &[&str]) -> Option<&'a mut Vec<Value>> {
    let mut cur = root;
    for key in path {
        cur = cur.get_mut(*key)?;
    }
    cur.as_array_mut()
}

/// Walk down `path`; if the leaf is now empty (array or object), remove it
/// from its parent. Continue upward until we hit a non-empty container or the
/// root.
fn prune_empty_path(root: &mut Value, path: &[&str]) {
    for depth in (1..=path.len()).rev() {
        let (parent_path, key) = path[..depth].split_at(depth - 1);
        let key = key[0];
        let Some(parent) = traverse_object_mut(root, parent_path) else {
            return;
        };
        let should_remove = match parent.get(key) {
            Some(Value::Array(a)) => a.is_empty(),
            Some(Value::Object(o)) => o.is_empty(),
            _ => return,
        };
        if !should_remove {
            return;
        }
        parent.remove(key);
    }
}

fn traverse_object_mut<'a>(
    root: &'a mut Value,
    path: &[&str],
) -> Option<&'a mut Map<String, Value>> {
    let mut cur = root;
    for key in path {
        cur = cur.get_mut(*key)?;
    }
    cur.as_object_mut()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn read_missing_file_returns_empty_object() {
        let dir = tempfile::tempdir().unwrap();
        let v = read_or_empty(&dir.path().join("absent.json")).unwrap();
        assert_eq!(v, json!({}));
    }

    #[test]
    fn read_invalid_json_errors() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.json");
        std::fs::write(&p, b"{not valid").unwrap();
        let err = read_or_empty(&p).unwrap_err();
        assert!(matches!(err, HookerError::JsonInvalid { .. }));
    }

    #[test]
    fn upsert_inserts_tagged_entry() {
        let mut root = json!({});
        let changed = upsert_tagged_array_entry(
            &mut root,
            &["hooks", "PreToolUse"],
            "alpha",
            json!({ "matcher": "Bash", "command": "do" }),
        )
        .unwrap();
        assert!(changed);
        assert_eq!(
            root,
            json!({
                "hooks": {
                    "PreToolUse": [
                        { "matcher": "Bash", "command": "do", "_ai_hooker_tag": "alpha" }
                    ]
                }
            })
        );
    }

    #[test]
    fn upsert_replaces_same_tag_in_place() {
        let mut root = json!({
            "hooks": { "PreToolUse": [
                { "matcher": "Bash", "command": "old", "_ai_hooker_tag": "alpha" }
            ]}
        });
        let changed = upsert_tagged_array_entry(
            &mut root,
            &["hooks", "PreToolUse"],
            "alpha",
            json!({ "matcher": "Bash", "command": "new" }),
        )
        .unwrap();
        assert!(changed);
        assert_eq!(
            root["hooks"]["PreToolUse"][0]["command"],
            json!("new")
        );
    }

    #[test]
    fn upsert_is_idempotent_on_identical_input() {
        let mut root = json!({});
        let entry = json!({ "matcher": "Bash", "command": "do" });
        upsert_tagged_array_entry(&mut root, &["h"], "alpha", entry.clone()).unwrap();
        let changed_again =
            upsert_tagged_array_entry(&mut root, &["h"], "alpha", entry).unwrap();
        assert!(!changed_again);
    }

    #[test]
    fn distinct_tags_coexist_in_same_array() {
        let mut root = json!({});
        upsert_tagged_array_entry(&mut root, &["h"], "alpha", json!({ "x": 1 })).unwrap();
        upsert_tagged_array_entry(&mut root, &["h"], "beta", json!({ "x": 2 })).unwrap();
        let arr = root["h"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn remove_strips_only_tagged_entry() {
        let mut root = json!({
            "hooks": { "PreToolUse": [
                { "matcher": "Bash", "command": "user", "_ai_hooker_tag": "user" },
                { "matcher": "Bash", "command": "ours", "_ai_hooker_tag": "alpha" }
            ]}
        });
        let removed = remove_tagged_array_entry(
            &mut root,
            &["hooks", "PreToolUse"],
            "alpha",
        )
        .unwrap();
        assert!(removed);
        let arr = root["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["_ai_hooker_tag"], json!("user"));
    }

    #[test]
    fn remove_prunes_empty_arrays_and_parents() {
        let mut root = json!({
            "hooks": { "PreToolUse": [
                { "matcher": "Bash", "_ai_hooker_tag": "alpha" }
            ]}
        });
        remove_tagged_array_entry(&mut root, &["hooks", "PreToolUse"], "alpha").unwrap();
        assert_eq!(root, json!({}));
    }

    #[test]
    fn remove_unknown_tag_is_noop() {
        let mut root = json!({ "hooks": { "PreToolUse": [
            { "_ai_hooker_tag": "alpha" }
        ]}});
        let removed =
            remove_tagged_array_entry(&mut root, &["hooks", "PreToolUse"], "ghost").unwrap();
        assert!(!removed);
    }

    #[test]
    fn ensure_object_rejects_non_object_collision() {
        let mut root = json!({ "hooks": "oops a string" });
        let err = ensure_object(&mut root, &["hooks"]).unwrap_err();
        assert!(matches!(err, HookerError::Other(_)));
    }

    #[test]
    fn pretty_emits_trailing_newline() {
        let bytes = to_pretty(&json!({ "a": 1 }));
        assert!(bytes.ends_with(b"\n"));
    }

    #[test]
    fn contains_tagged_finds_entry() {
        let root = json!({
            "hooks": { "PreToolUse": [{ "_ai_hooker_tag": "alpha" }]}
        });
        assert!(contains_tagged(&root, &["hooks", "PreToolUse"], "alpha"));
        assert!(!contains_tagged(&root, &["hooks", "PreToolUse"], "beta"));
        assert!(!contains_tagged(&root, &["hooks", "missing"], "alpha"));
    }

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
