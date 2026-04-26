//! Insert/remove/lookup of tagged objects inside a JSON array. Each entry
//! carries an `_agent_config_tag` marker so multiple consumers coexist without
//! stepping on one another.

use serde_json::Value;

use crate::error::AgentConfigError;

use super::common::{
    ensure_array, find_tagged_index, matches_tag, prune_empty_path, traverse_array,
    traverse_array_mut, traverse_object, TAG_KEY,
};

/// Insert `entry` into the array at `path`, replacing any existing entry that
/// carries the same `_agent_config_tag`. Returns `true` if a change was made.
///
/// Tags the entry with `_agent_config_tag = tag` before inserting.
pub(crate) fn upsert_tagged_array_entry(
    root: &mut Value,
    path: &[&str],
    tag: &str,
    mut entry: Value,
) -> Result<bool, AgentConfigError> {
    if let Some(obj) = entry.as_object_mut() {
        obj.insert(TAG_KEY.into(), Value::String(tag.into()));
    } else {
        return Err(AgentConfigError::Other(anyhow::anyhow!(
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
) -> Result<bool, AgentConfigError> {
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

/// True if an entry with `_agent_config_tag == tag` exists at `path`.
#[allow(dead_code)]
pub(crate) fn contains_tagged(root: &Value, path: &[&str], tag: &str) -> bool {
    let Some(arr) = traverse_array(root, path) else {
        return false;
    };
    arr.iter().any(|v| matches_tag(v, tag))
}

/// True if any array directly under `parent_path` contains a tagged entry.
///
/// This is used for hook configs where callers may install custom event names,
/// so uninstall/detection cannot be limited to the built-in event keys.
#[allow(dead_code)]
pub(crate) fn contains_tagged_array_entry_under(
    root: &Value,
    parent_path: &[&str],
    tag: &str,
) -> bool {
    let Some(parent) = traverse_object(root, parent_path) else {
        return false;
    };
    parent.values().any(|v| {
        v.as_array()
            .is_some_and(|arr| arr.iter().any(|entry| matches_tag(entry, tag)))
    })
}

/// Remove tagged entries from every array directly under `parent_path`.
/// Returns true if any entry was removed.
pub(crate) fn remove_tagged_array_entries_under(
    root: &mut Value,
    parent_path: &[&str],
    tag: &str,
) -> Result<bool, AgentConfigError> {
    let Some(parent) = traverse_object(root, parent_path) else {
        return Ok(false);
    };
    let keys: Vec<String> = parent
        .iter()
        .filter_map(|(key, value)| {
            value.as_array().and_then(|arr| {
                arr.iter()
                    .any(|entry| matches_tag(entry, tag))
                    .then(|| key.clone())
            })
        })
        .collect();

    let mut changed = false;
    for key in keys {
        let mut path = parent_path.to_vec();
        path.push(key.as_str());
        if remove_tagged_array_entry(root, &path, tag)? {
            changed = true;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

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
                        { "matcher": "Bash", "command": "do", "_agent_config_tag": "alpha" }
                    ]
                }
            })
        );
    }

    #[test]
    fn upsert_replaces_same_tag_in_place() {
        let mut root = json!({
            "hooks": { "PreToolUse": [
                { "matcher": "Bash", "command": "old", "_agent_config_tag": "alpha" }
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
        assert_eq!(root["hooks"]["PreToolUse"][0]["command"], json!("new"));
    }

    #[test]
    fn upsert_is_idempotent_on_identical_input() {
        let mut root = json!({});
        let entry = json!({ "matcher": "Bash", "command": "do" });
        upsert_tagged_array_entry(&mut root, &["h"], "alpha", entry.clone()).unwrap();
        let changed_again = upsert_tagged_array_entry(&mut root, &["h"], "alpha", entry).unwrap();
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
                { "matcher": "Bash", "command": "user", "_agent_config_tag": "user" },
                { "matcher": "Bash", "command": "ours", "_agent_config_tag": "alpha" }
            ]}
        });
        let removed =
            remove_tagged_array_entry(&mut root, &["hooks", "PreToolUse"], "alpha").unwrap();
        assert!(removed);
        let arr = root["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["_agent_config_tag"], json!("user"));
    }

    #[test]
    fn remove_prunes_empty_arrays_and_parents() {
        let mut root = json!({
            "hooks": { "PreToolUse": [
                { "matcher": "Bash", "_agent_config_tag": "alpha" }
            ]}
        });
        remove_tagged_array_entry(&mut root, &["hooks", "PreToolUse"], "alpha").unwrap();
        assert_eq!(root, json!({}));
    }

    #[test]
    fn remove_unknown_tag_is_noop() {
        let mut root = json!({ "hooks": { "PreToolUse": [
            { "_agent_config_tag": "alpha" }
        ]}});
        let removed =
            remove_tagged_array_entry(&mut root, &["hooks", "PreToolUse"], "ghost").unwrap();
        assert!(!removed);
    }

    #[test]
    fn contains_tagged_finds_entry() {
        let root = json!({
            "hooks": { "PreToolUse": [{ "_agent_config_tag": "alpha" }]}
        });
        assert!(contains_tagged(&root, &["hooks", "PreToolUse"], "alpha"));
        assert!(!contains_tagged(&root, &["hooks", "PreToolUse"], "beta"));
        assert!(!contains_tagged(&root, &["hooks", "missing"], "alpha"));
    }

    #[test]
    fn contains_tagged_under_finds_custom_event() {
        let root = json!({
            "version": 1,
            "hooks": {
                "customEvent": [{ "_agent_config_tag": "alpha" }],
                "preToolUse": []
            }
        });
        assert!(contains_tagged_array_entry_under(
            &root,
            &["hooks"],
            "alpha"
        ));
        assert!(!contains_tagged_array_entry_under(
            &root,
            &["hooks"],
            "beta"
        ));
    }

    #[test]
    fn remove_tagged_under_prunes_custom_event_but_keeps_siblings() {
        let mut root = json!({
            "version": 1,
            "hooks": {
                "customEvent": [{ "_agent_config_tag": "alpha" }],
                "preToolUse": [{ "_agent_config_tag": "beta" }]
            }
        });
        let changed = remove_tagged_array_entries_under(&mut root, &["hooks"], "alpha").unwrap();
        assert!(changed);
        assert_eq!(root["version"], json!(1));
        assert!(root["hooks"]["customEvent"].is_null());
        assert_eq!(
            root["hooks"]["preToolUse"][0]["_agent_config_tag"],
            json!("beta")
        );
    }
}
