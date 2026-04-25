//! TOML analogue of [`crate::util::json_patch`] for Codex's `config.toml`.
//!
//! Uses `toml_edit` (not the typed `toml` crate) so user comments and key
//! ordering survive a round trip — Codex users frequently hand-edit
//! `~/.codex/config.toml`, and obliterating their formatting on every
//! `install_mcp` call would be a hostile UX.

use std::fs;
use std::path::Path;

use toml_edit::{DocumentMut, Item, Table};

use crate::error::HookerError;
use crate::status::ConfigPresence;

/// Read a TOML document, returning an empty document when the file is missing.
///
/// Errors propagate verbatim if the file exists but is invalid TOML; callers
/// should surface this rather than overwriting potentially-precious user
/// config.
pub(crate) fn read_or_empty(path: &Path) -> Result<DocumentMut, HookerError> {
    let text = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(DocumentMut::new()),
        Err(e) => return Err(HookerError::io(path, e)),
    };
    if text.trim().is_empty() {
        return Ok(DocumentMut::new());
    }
    text.parse::<DocumentMut>()
        .map_err(|e| HookerError::toml(path, e))
}

/// Serialize a TOML document, ensuring a trailing newline.
pub(crate) fn to_string(doc: &DocumentMut) -> Vec<u8> {
    let mut out = doc.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.into_bytes()
}

/// Insert/replace a named sub-table under `[parent.<name>]`. `parent` is the
/// dotted path to the parent table (e.g. `&["mcp_servers"]` →
/// `[mcp_servers.<name>]`).
///
/// Returns `true` if the on-disk shape would change.
pub(crate) fn upsert_named_table(
    doc: &mut DocumentMut,
    parent: &[&str],
    name: &str,
    table: Table,
) -> Result<bool, HookerError> {
    let parent_table = ensure_table(doc, parent)?;
    let new_item = Item::Table(table);
    let changed = match parent_table.get(name) {
        Some(existing) => existing.to_string() != new_item.to_string(),
        None => true,
    };
    if changed {
        parent_table.insert(name, new_item);
    }
    Ok(changed)
}

/// Remove a named sub-table under `[parent.<name>]`. Returns `true` if a table
/// was removed; prunes the parent table when it becomes empty.
pub(crate) fn remove_named_table(
    doc: &mut DocumentMut,
    parent: &[&str],
    name: &str,
) -> Result<bool, HookerError> {
    let Some(parent_table) = traverse_table_mut(doc, parent) else {
        return Ok(false);
    };
    let removed = parent_table.remove(name).is_some();
    if !removed {
        return Ok(false);
    }
    let parent_now_empty = parent_table.is_empty();
    if parent_now_empty && !parent.is_empty() {
        prune_empty_parent(doc, parent);
    }
    Ok(true)
}

/// True if `[parent.<name>]` exists in the document.
pub(crate) fn contains_named_table(doc: &DocumentMut, parent: &[&str], name: &str) -> bool {
    let mut cur: &Item = doc.as_item();
    for key in parent {
        let Some(next) = cur.as_table().and_then(|t| t.get(key)) else {
            return false;
        };
        cur = next;
    }
    cur.as_table()
        .map(|t| t.contains_key(name))
        .unwrap_or(false)
}

/// Probe whether `[parent.<name>]` exists in the TOML file at `config_path`.
/// Parse failures map to [`ConfigPresence::Invalid`].
pub(crate) fn config_presence(
    config_path: &Path,
    parent: &[&str],
    name: &str,
) -> Result<ConfigPresence, HookerError> {
    if !config_path.exists() {
        return Ok(ConfigPresence::Absent);
    }
    let doc = match read_or_empty(config_path) {
        Ok(d) => d,
        Err(HookerError::TomlInvalid { source, .. }) => {
            return Ok(ConfigPresence::Invalid {
                reason: source.to_string(),
            });
        }
        Err(e) => return Err(e),
    };
    Ok(if contains_named_table(&doc, parent, name) {
        ConfigPresence::Single
    } else {
        ConfigPresence::Absent
    })
}

fn ensure_table<'a>(doc: &'a mut DocumentMut, path: &[&str]) -> Result<&'a mut Table, HookerError> {
    let mut cur: &mut Table = doc.as_table_mut();
    for key in path {
        if !cur.contains_key(key) {
            cur.insert(key, Item::Table(make_implicit_table()));
        }
        let next = cur
            .get_mut(key)
            .and_then(Item::as_table_mut)
            .ok_or_else(|| {
                HookerError::Other(anyhow::anyhow!(
                    "expected TOML table at path segment {:?}, found a non-table item",
                    key
                ))
            })?;
        cur = next;
    }
    Ok(cur)
}

fn traverse_table_mut<'a>(doc: &'a mut DocumentMut, path: &[&str]) -> Option<&'a mut Table> {
    let mut cur: &mut Table = doc.as_table_mut();
    for key in path {
        cur = cur.get_mut(key)?.as_table_mut()?;
    }
    Some(cur)
}

fn prune_empty_parent(doc: &mut DocumentMut, path: &[&str]) {
    // Walk from leaf upward, removing each empty table from its parent.
    for depth in (1..=path.len()).rev() {
        let (parent, leaf) = path[..depth].split_at(depth - 1);
        let key = leaf[0];
        let Some(parent_table) = traverse_table_mut(doc, parent) else {
            return;
        };
        let leaf_empty = parent_table
            .get(key)
            .and_then(Item::as_table)
            .map(Table::is_empty)
            .unwrap_or(false);
        if !leaf_empty {
            return;
        }
        parent_table.remove(key);
    }
}

fn make_implicit_table() -> Table {
    let mut t = Table::new();
    t.set_implicit(true);
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use toml_edit::value;

    #[test]
    fn read_missing_returns_empty_doc() {
        let dir = tempdir().unwrap();
        let doc = read_or_empty(&dir.path().join("absent.toml")).unwrap();
        assert!(doc.as_table().is_empty());
    }

    #[test]
    fn read_invalid_errors() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("bad.toml");
        fs::write(&p, b"=oops\n").unwrap();
        let err = read_or_empty(&p).unwrap_err();
        assert!(matches!(err, HookerError::TomlInvalid { .. }));
    }

    #[test]
    fn upsert_inserts_named_table() {
        let mut doc = DocumentMut::new();
        let mut t = Table::new();
        t["command"] = value("npx");
        t["args"] = value("[\"-y\"]"); // toml_edit::value coerces strings
        let changed = upsert_named_table(&mut doc, &["mcp_servers"], "github", t).unwrap();
        assert!(changed);
        let rendered = doc.to_string();
        assert!(
            rendered.contains("[mcp_servers.github]"),
            "got:\n{rendered}"
        );
        assert!(rendered.contains(r#"command = "npx""#), "got:\n{rendered}");
    }

    #[test]
    fn upsert_idempotent_on_identical_table() {
        let mut doc = DocumentMut::new();
        let mut t = Table::new();
        t["command"] = value("npx");
        upsert_named_table(&mut doc, &["mcp_servers"], "github", t.clone()).unwrap();
        let changed_again = upsert_named_table(&mut doc, &["mcp_servers"], "github", t).unwrap();
        assert!(!changed_again);
    }

    #[test]
    fn upsert_replaces_existing_table() {
        let mut doc = DocumentMut::new();
        let mut t1 = Table::new();
        t1["command"] = value("old");
        upsert_named_table(&mut doc, &["mcp_servers"], "github", t1).unwrap();

        let mut t2 = Table::new();
        t2["command"] = value("new");
        let changed = upsert_named_table(&mut doc, &["mcp_servers"], "github", t2).unwrap();
        assert!(changed);
        assert!(doc.to_string().contains(r#"command = "new""#));
    }

    #[test]
    fn remove_named_table_prunes_empty_parent() {
        let mut doc = DocumentMut::new();
        let mut t = Table::new();
        t["command"] = value("npx");
        upsert_named_table(&mut doc, &["mcp_servers"], "github", t).unwrap();
        let removed = remove_named_table(&mut doc, &["mcp_servers"], "github").unwrap();
        assert!(removed);
        // Parent table should also be gone.
        assert!(!doc.contains_key("mcp_servers"), "got:\n{}", doc);
    }

    #[test]
    fn remove_named_table_keeps_siblings() {
        let mut doc = DocumentMut::new();
        let mut a = Table::new();
        a["command"] = value("a");
        let mut b = Table::new();
        b["command"] = value("b");
        upsert_named_table(&mut doc, &["mcp_servers"], "alpha", a).unwrap();
        upsert_named_table(&mut doc, &["mcp_servers"], "beta", b).unwrap();
        remove_named_table(&mut doc, &["mcp_servers"], "alpha").unwrap();
        assert!(doc.to_string().contains("[mcp_servers.beta]"));
        assert!(!doc.to_string().contains("[mcp_servers.alpha]"));
    }

    #[test]
    fn remove_unknown_is_noop() {
        let mut doc = DocumentMut::new();
        let mut t = Table::new();
        t["command"] = value("a");
        upsert_named_table(&mut doc, &["mcp_servers"], "alpha", t).unwrap();
        let removed = remove_named_table(&mut doc, &["mcp_servers"], "ghost").unwrap();
        assert!(!removed);
    }

    #[test]
    fn contains_named_table_works() {
        let mut doc = DocumentMut::new();
        let mut t = Table::new();
        t["command"] = value("npx");
        upsert_named_table(&mut doc, &["mcp_servers"], "github", t).unwrap();
        assert!(contains_named_table(&doc, &["mcp_servers"], "github"));
        assert!(!contains_named_table(&doc, &["mcp_servers"], "ghost"));
        assert!(!contains_named_table(&doc, &["other"], "github"));
    }

    #[test]
    fn user_comments_preserved_across_round_trip() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("config.toml");
        let original = "\
# Codex configuration
# Hand-edited; do not delete comments.

[some.other.section]
foo = \"bar\"
";
        fs::write(&p, original).unwrap();
        let mut doc = read_or_empty(&p).unwrap();
        let mut t = Table::new();
        t["command"] = value("npx");
        upsert_named_table(&mut doc, &["mcp_servers"], "github", t).unwrap();
        let rendered = String::from_utf8(to_string(&doc)).unwrap();
        assert!(
            rendered.contains("# Codex configuration"),
            "comment lost. got:\n{rendered}"
        );
        assert!(rendered.contains("# Hand-edited"), "comment lost");
        assert!(rendered.contains("[some.other.section]"));
        assert!(rendered.contains("[mcp_servers.github]"));
    }
}
