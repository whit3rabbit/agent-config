//! Shared primitives for the per-shape patch helpers: I/O, path traversal,
//! pruning, and the `_agent_config_tag` marker constant.

use std::path::Path;

use serde_json::{Map, Value};

use crate::error::AgentConfigError;
use crate::util::fs_atomic;

/// The marker key embedded in every object we insert. Lets us locate and
/// uninstall our own entries without touching user-authored ones.
pub(crate) const TAG_KEY: &str = "_agent_config_tag";

/// Read a JSON file, returning `Value::Object(empty)` when the file is missing.
///
/// Errors propagate verbatim if the file exists but is invalid JSON; the
/// caller should surface this rather than overwriting potentially-precious
/// user config.
pub(crate) fn read_or_empty(path: &Path) -> Result<Value, AgentConfigError> {
    let bytes = fs_atomic::read_capped_or_empty(path)?;
    if bytes.is_empty() || bytes.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(Value::Object(Map::new()));
    }
    serde_json::from_slice(&bytes).map_err(|e| AgentConfigError::json(path, e))
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
/// non-object value, returns a [`AgentConfigError::Other`].
pub(crate) fn ensure_object<'a>(
    root: &'a mut Value,
    path: &[&str],
) -> Result<&'a mut Map<String, Value>, AgentConfigError> {
    if !root.is_object() {
        *root = Value::Object(Map::new());
    }
    let mut cur = root.as_object_mut().unwrap();
    for key in path {
        if !cur.contains_key(*key) {
            cur.insert((*key).to_string(), Value::Object(Map::new()));
        } else if !cur[*key].is_object() {
            return Err(AgentConfigError::Other(anyhow::anyhow!(
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
) -> Result<&'a mut Vec<Value>, AgentConfigError> {
    if path.is_empty() {
        return Err(AgentConfigError::Other(anyhow::anyhow!(
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
        other => Err(AgentConfigError::Other(anyhow::anyhow!(
            "expected array at JSON path segment {:?}, found {:?}",
            last,
            other
        ))),
    }
}

pub(super) fn find_tagged_index(arr: &[Value], tag: &str) -> Option<usize> {
    arr.iter().position(|v| matches_tag(v, tag))
}

pub(super) fn matches_tag(v: &Value, tag: &str) -> bool {
    v.as_object()
        .and_then(|o| o.get(TAG_KEY))
        .and_then(Value::as_str)
        == Some(tag)
}

pub(super) fn traverse_array<'a>(root: &'a Value, path: &[&str]) -> Option<&'a Vec<Value>> {
    let mut cur = root;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_array()
}

pub(super) fn traverse_object<'a>(
    root: &'a Value,
    path: &[&str],
) -> Option<&'a Map<String, Value>> {
    let mut cur = root;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_object()
}

pub(super) fn traverse_array_mut<'a>(
    root: &'a mut Value,
    path: &[&str],
) -> Option<&'a mut Vec<Value>> {
    let mut cur = root;
    for key in path {
        cur = cur.get_mut(*key)?;
    }
    cur.as_array_mut()
}

/// Walk down `path`; if the leaf is now empty (array or object), remove it
/// from its parent. Continue upward until we hit a non-empty container or the
/// root.
pub(super) fn prune_empty_path(root: &mut Value, path: &[&str]) {
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

pub(super) fn traverse_object_mut<'a>(
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
        assert!(matches!(err, AgentConfigError::JsonInvalid { .. }));
    }

    #[test]
    fn ensure_object_rejects_non_object_collision() {
        let mut root = json!({ "hooks": "oops a string" });
        let err = ensure_object(&mut root, &["hooks"]).unwrap_err();
        assert!(matches!(err, AgentConfigError::Other(_)));
    }

    #[test]
    fn pretty_emits_trailing_newline() {
        let bytes = to_pretty(&json!({ "a": 1 }));
        assert!(bytes.ends_with(b"\n"));
    }
}
