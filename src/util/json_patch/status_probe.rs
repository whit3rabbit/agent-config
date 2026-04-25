//! Status-only probes for tagged-array hook configs.
//!
//! Read a settings file, find any `_ai_hooker_tag = <tag>` entries under the
//! given parent key, and report a [`ConfigPresence`]. Parse failures are
//! converted to [`ConfigPresence::Invalid`] so the caller can surface them
//! as drift instead of bubbling up an error.

use std::path::Path;

use serde_json::Value;

use crate::error::HookerError;
use crate::status::ConfigPresence;

use super::common::{matches_tag, read_or_empty, traverse_object};

/// Count tagged hook entries (with `_ai_hooker_tag == tag`) across every
/// array directly under `parent_path` in the settings file at `config_path`.
pub(crate) fn tagged_hook_presence(
    config_path: &Path,
    parent_path: &[&str],
    tag: &str,
) -> Result<ConfigPresence, HookerError> {
    if !config_path.exists() {
        return Ok(ConfigPresence::Absent);
    }
    let root = match read_or_empty(config_path) {
        Ok(v) => v,
        Err(HookerError::JsonInvalid { source, .. }) => {
            return Ok(ConfigPresence::Invalid {
                reason: source.to_string(),
            });
        }
        Err(e) => return Err(e),
    };
    let count = count_tagged_under(&root, parent_path, tag);
    Ok(presence_from_count(count))
}

/// Count tagged hook entries inside a *specific* event array at
/// `parent_path` (e.g. `["pre_run_command"]` for Windsurf, where each event
/// is a top-level array rather than a key under a parent object).
pub(crate) fn tagged_hook_presence_for_event(
    config_path: &Path,
    array_path: &[&str],
    tag: &str,
) -> Result<ConfigPresence, HookerError> {
    if !config_path.exists() {
        return Ok(ConfigPresence::Absent);
    }
    let root = match read_or_empty(config_path) {
        Ok(v) => v,
        Err(HookerError::JsonInvalid { source, .. }) => {
            return Ok(ConfigPresence::Invalid {
                reason: source.to_string(),
            });
        }
        Err(e) => return Err(e),
    };
    let count = traverse_array_at(&root, array_path)
        .map(|arr| arr.iter().filter(|v| matches_tag(v, tag)).count())
        .unwrap_or(0);
    Ok(presence_from_count(count))
}

fn count_tagged_under(root: &Value, parent_path: &[&str], tag: &str) -> usize {
    let Some(parent) = traverse_object(root, parent_path) else {
        return 0;
    };
    parent
        .values()
        .map(|v| {
            v.as_array()
                .map(|arr| arr.iter().filter(|entry| matches_tag(entry, tag)).count())
                .unwrap_or(0)
        })
        .sum()
}

fn traverse_array_at<'a>(root: &'a Value, path: &[&str]) -> Option<&'a Vec<Value>> {
    let mut cur = root;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_array()
}

fn presence_from_count(count: usize) -> ConfigPresence {
    match count {
        0 => ConfigPresence::Absent,
        1 => ConfigPresence::Single,
        n => ConfigPresence::Duplicate { count: n },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn write(p: &Path, v: &Value) {
        std::fs::write(p, serde_json::to_string_pretty(v).unwrap()).unwrap();
    }

    #[test]
    fn returns_absent_when_file_missing() {
        let dir = tempdir().unwrap();
        let r =
            tagged_hook_presence(&dir.path().join("missing.json"), &["hooks"], "alpha").unwrap();
        assert!(matches!(r, ConfigPresence::Absent));
    }

    #[test]
    fn returns_single_when_one_match() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("settings.json");
        write(
            &p,
            &json!({
                "hooks": {
                    "PreToolUse": [
                        { "matcher": "Bash", "_ai_hooker_tag": "alpha" }
                    ]
                }
            }),
        );
        let r = tagged_hook_presence(&p, &["hooks"], "alpha").unwrap();
        assert!(matches!(r, ConfigPresence::Single));
    }

    #[test]
    fn returns_duplicate_when_multiple_matches() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("settings.json");
        write(
            &p,
            &json!({
                "hooks": {
                    "PreToolUse": [
                        { "_ai_hooker_tag": "alpha" },
                        { "_ai_hooker_tag": "alpha" }
                    ]
                }
            }),
        );
        let r = tagged_hook_presence(&p, &["hooks"], "alpha").unwrap();
        assert!(matches!(r, ConfigPresence::Duplicate { count: 2 }));
    }

    #[test]
    fn returns_invalid_when_parse_fails() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, b"{not json").unwrap();
        let r = tagged_hook_presence(&p, &["hooks"], "alpha").unwrap();
        assert!(matches!(r, ConfigPresence::Invalid { .. }));
    }

    #[test]
    fn returns_absent_when_tag_not_found() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("settings.json");
        write(
            &p,
            &json!({
                "hooks": { "PreToolUse": [{ "_ai_hooker_tag": "other" }] }
            }),
        );
        let r = tagged_hook_presence(&p, &["hooks"], "alpha").unwrap();
        assert!(matches!(r, ConfigPresence::Absent));
    }

    #[test]
    fn for_event_finds_tag_under_specific_array() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("settings.json");
        write(
            &p,
            &json!({
                "pre_run_command": [{ "_ai_hooker_tag": "alpha" }]
            }),
        );
        let r = tagged_hook_presence_for_event(&p, &["pre_run_command"], "alpha").unwrap();
        assert!(matches!(r, ConfigPresence::Single));
    }
}
