//! Idempotent insert/extract/remove of a fenced markdown block in a host file.
//!
//! Each block is delimited by HTML comments keyed on a tag:
//!
//! ```text
//! <!-- BEGIN AGENT-CONFIG:app -->
//! ...content...
//! <!-- END AGENT-CONFIG:app -->
//! ```
//!
//! Multiple consumers can coexist by using distinct tags. The fence is invisible
//! when the markdown is rendered, and stable enough to grep over.

const FENCE_PREFIX: &str = "AGENT-CONFIG";
const INSTRUCTION_FENCE_PREFIX: &str = "AGENT-CONFIG-INSTR";

/// Returns the BEGIN/END marker pair for a hook `tag`.
fn markers(tag: &str) -> (String, String) {
    markers_with_prefix(FENCE_PREFIX, tag)
}

/// Returns the BEGIN/END marker pair for an instruction `name`.
fn instruction_markers(name: &str) -> (String, String) {
    markers_with_prefix(INSTRUCTION_FENCE_PREFIX, name)
}

fn markers_with_prefix(prefix: &str, tag: &str) -> (String, String) {
    (
        format!("<!-- BEGIN {prefix}:{tag} -->"),
        format!("<!-- END {prefix}:{tag} -->"),
    )
}

/// Insert or replace the tagged block in `host`.
///
/// Returns the new file contents. The block is appended on first write and
/// replaced in place on subsequent writes. A trailing newline is enforced so
/// repeated upserts stay idempotent.
pub(crate) fn upsert(host: &str, tag: &str, body: &str) -> String {
    let (begin, end) = markers(tag);
    upsert_with_markers(host, &begin, &end, body)
}

/// Insert or replace the instruction-tagged block in `host`.
///
/// Uses a distinct fence prefix (`AGENT-CONFIG-INSTR`) so instruction names
/// cannot collide with hook tags in the same memory file.
pub(crate) fn upsert_instruction(host: &str, name: &str, body: &str) -> String {
    let (begin, end) = instruction_markers(name);
    upsert_with_markers(host, &begin, &end, body)
}

fn upsert_with_markers(host: &str, begin: &str, end: &str, body: &str) -> String {
    let block = render_block(begin, end, body);

    if let Some((start, stop)) = find_block(host, begin, end) {
        let mut out = String::with_capacity(host.len() + block.len());
        out.push_str(&host[..start]);
        out.push_str(&block);
        out.push_str(&host[stop..]);
        return out;
    }

    // Not present — append. Ensure exactly one blank line between prior
    // content and our block.
    let mut out = String::with_capacity(host.len() + block.len() + 2);
    if host.is_empty() {
        out.push_str(&block);
    } else {
        out.push_str(host.trim_end_matches('\n'));
        out.push_str("\n\n");
        out.push_str(&block);
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Returns true if a block with `tag` exists in `host`. Currently exercised by
/// unit tests; agents detect installation via [`json_patch::contains_tagged`]
/// or filesystem checks rather than by parsing markdown.
#[allow(dead_code)]
pub(crate) fn contains(host: &str, tag: &str) -> bool {
    let (begin, end) = markers(tag);
    find_block(host, &begin, &end).is_some()
}

/// Returns true if an instruction block with `name` exists in `host`.
pub(crate) fn contains_instruction(host: &str, name: &str) -> bool {
    let (begin, end) = instruction_markers(name);
    find_block(host, &begin, &end).is_some()
}

/// Returns true if an instruction block written with the legacy `AGENT-CONFIG`
/// prefix exists. Used during status detection and uninstall to drain
/// pre-rename installs without leaving orphan content.
pub(crate) fn contains_legacy_instruction(host: &str, name: &str) -> bool {
    let (begin, end) = markers(name);
    find_block(host, &begin, &end).is_some()
}

/// Returns true when `host` contains an incomplete or duplicate fenced block
/// for `tag`.
pub(crate) fn malformed(host: &str, tag: &str) -> bool {
    let (begin, end) = markers(tag);
    is_malformed(host, &begin, &end)
}

/// Returns true when `host` contains an incomplete or duplicate instruction
/// fenced block for `name`.
#[allow(dead_code)]
pub(crate) fn malformed_instruction(host: &str, name: &str) -> bool {
    let (begin, end) = instruction_markers(name);
    is_malformed(host, &begin, &end)
}

fn is_malformed(host: &str, begin: &str, end: &str) -> bool {
    let begin_count = host.matches(begin).count();
    let end_count = host.matches(end).count();
    match (begin_count, end_count) {
        (0, 0) => false,
        (1, 1) => find_block(host, begin, end).is_none(),
        _ => true,
    }
}

/// Remove the tagged block. Returns `(new_contents, removed)` where `removed`
/// is true if a block was actually stripped.
///
/// Collapses any blank-line separator we added during [`upsert`] so repeated
/// upsert/remove cycles don't accumulate whitespace.
pub(crate) fn remove(host: &str, tag: &str) -> (String, bool) {
    let (begin, end) = markers(tag);
    remove_with_markers(host, &begin, &end)
}

/// Remove the instruction-tagged block. Returns `(new_contents, removed)`.
pub(crate) fn remove_instruction(host: &str, name: &str) -> (String, bool) {
    let (begin, end) = instruction_markers(name);
    remove_with_markers(host, &begin, &end)
}

/// Remove a legacy (`AGENT-CONFIG`-prefixed) instruction block. Used during
/// uninstall to clean up pre-rename installs.
pub(crate) fn remove_legacy_instruction(host: &str, name: &str) -> (String, bool) {
    let (begin, end) = markers(name);
    remove_with_markers(host, &begin, &end)
}

fn remove_with_markers(host: &str, begin: &str, end: &str) -> (String, bool) {
    let Some((start, stop)) = find_block(host, begin, end) else {
        return (host.to_string(), false);
    };

    let prefix = host[..start].trim_end_matches('\n');
    let suffix = host[stop..].trim_start_matches('\n');

    let out = match (prefix.is_empty(), suffix.is_empty()) {
        (true, true) => String::new(),
        (true, false) => suffix.to_string(),
        (false, true) => format!("{prefix}\n"),
        (false, false) => format!("{prefix}\n\n{suffix}"),
    };
    (out, true)
}

/// Render the fenced block with surrounding newlines normalized.
fn render_block(begin: &str, end: &str, body: &str) -> String {
    let mut s = String::with_capacity(begin.len() + body.len() + end.len() + 4);
    s.push_str(begin);
    s.push('\n');
    s.push_str(body.trim_end_matches('\n'));
    s.push('\n');
    s.push_str(end);
    s
}

/// Locate the byte range `[start, stop)` covering exactly the markers and
/// their content (no trailing whitespace).
fn find_block(host: &str, begin: &str, end: &str) -> Option<(usize, usize)> {
    let begin_idx = host.find(begin)?;
    let after_begin = begin_idx + begin.len();
    let end_rel = host[after_begin..].find(end)?;
    let end_idx = after_begin + end_rel + end.len();
    Some((begin_idx, end_idx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AgentConfigError;
    use crate::util::{file_lock, fs_atomic};
    use pretty_assertions::assert_eq;
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn run_two<A, B, FA, FB>(a: FA, b: FB) -> (A, B)
    where
        A: Send + 'static,
        B: Send + 'static,
        FA: FnOnce() -> A + Send + 'static,
        FB: FnOnce() -> B + Send + 'static,
    {
        let barrier = Arc::new(Barrier::new(3));
        let a_barrier = Arc::clone(&barrier);
        let b_barrier = Arc::clone(&barrier);
        let a_thread = thread::spawn(move || {
            a_barrier.wait();
            a()
        });
        let b_thread = thread::spawn(move || {
            b_barrier.wait();
            b()
        });
        barrier.wait();
        (
            a_thread.join().expect("first markdown writer panicked"),
            b_thread.join().expect("second markdown writer panicked"),
        )
    }

    #[test]
    fn upsert_appends_on_empty_host() {
        let out = upsert("", "app", "hello");
        assert_eq!(
            out,
            "<!-- BEGIN AGENT-CONFIG:app -->\nhello\n<!-- END AGENT-CONFIG:app -->\n"
        );
    }

    #[test]
    fn upsert_appends_with_separator_on_existing_host() {
        let out = upsert("# Title\n\nIntro.\n", "app", "hello");
        assert!(out.starts_with("# Title\n\nIntro.\n\n<!-- BEGIN AGENT-CONFIG:app -->\n"));
        assert!(out.ends_with("<!-- END AGENT-CONFIG:app -->\n"));
    }

    #[test]
    fn upsert_replaces_in_place() {
        let host =
            "# Top\n\n<!-- BEGIN AGENT-CONFIG:app -->\nold\n<!-- END AGENT-CONFIG:app -->\n\n# Bottom\n";
        let out = upsert(host, "app", "new");
        assert_eq!(
            out,
            "# Top\n\n<!-- BEGIN AGENT-CONFIG:app -->\nnew\n<!-- END AGENT-CONFIG:app -->\n\n# Bottom\n"
        );
    }

    #[test]
    fn upsert_is_idempotent() {
        let once = upsert("# A", "app", "body");
        let twice = upsert(&once, "app", "body");
        assert_eq!(once, twice);
    }

    #[test]
    fn instruction_and_hook_fences_do_not_collide() {
        let mut host = String::new();
        host = upsert(&host, "myapp", "hook body");
        host = upsert_instruction(&host, "myapp", "instruction body");
        assert!(host.contains("<!-- BEGIN AGENT-CONFIG:myapp -->"));
        assert!(host.contains("<!-- BEGIN AGENT-CONFIG-INSTR:myapp -->"));
        assert!(host.contains("hook body"));
        assert!(host.contains("instruction body"));
        assert!(contains(&host, "myapp"));
        assert!(contains_instruction(&host, "myapp"));
    }

    #[test]
    fn instruction_remove_does_not_strip_hook_block() {
        let mut host = String::new();
        host = upsert(&host, "shared", "hook stays");
        host = upsert_instruction(&host, "shared", "instruction goes");
        let (after, removed) = remove_instruction(&host, "shared");
        assert!(removed);
        assert!(after.contains("hook stays"));
        assert!(!after.contains("instruction goes"));
        assert!(after.contains("<!-- BEGIN AGENT-CONFIG:shared -->"));
        assert!(!after.contains("AGENT-CONFIG-INSTR:shared"));
    }

    #[test]
    fn legacy_instruction_helpers_match_hook_prefix() {
        // Pre-rename installs used the AGENT-CONFIG:<name> fence. The legacy
        // helpers must read that exact fence so uninstall drains old content.
        let host = upsert("# Top\n", "old_name", "old body");
        assert!(contains_legacy_instruction(&host, "old_name"));
        let (stripped, removed) = remove_legacy_instruction(&host, "old_name");
        assert!(removed);
        assert!(!stripped.contains("AGENT-CONFIG:old_name"));
        assert!(stripped.contains("# Top"));
    }

    #[test]
    fn distinct_tags_coexist() {
        let mut host = String::new();
        host = upsert(&host, "alpha", "for alpha");
        host = upsert(&host, "beta", "for beta");
        assert!(contains(&host, "alpha"));
        assert!(contains(&host, "beta"));
    }

    #[test]
    fn malformed_detects_incomplete_or_duplicate_fence() {
        assert!(malformed("<!-- BEGIN AGENT-CONFIG:app -->\nbody\n", "app"));
        let duplicated = format!("{}\n{}", upsert("", "app", "one"), upsert("", "app", "two"));
        assert!(malformed(&duplicated, "app"));
        assert!(!malformed(&upsert("", "app", "ok"), "app"));
        assert!(!malformed("# no block\n", "app"));
    }

    #[test]
    fn remove_strips_block_and_collapses_whitespace() {
        let host = upsert("# A\n", "app", "body");
        let (stripped, removed) = remove(&host, "app");
        assert!(removed);
        assert!(!stripped.contains("AGENT-CONFIG:app"));
        // Should not have ended up with double-blank lines between '# A' and EOF.
        assert!(!stripped.contains("\n\n\n"));
    }

    #[test]
    fn remove_missing_is_noop() {
        let (out, removed) = remove("# Hello\n", "app");
        assert!(!removed);
        assert_eq!(out, "# Hello\n");
    }

    #[test]
    fn upsert_then_remove_reverts() {
        let original = "# Top\n\nBody.\n";
        let with_block = upsert(original, "app", "rules go here");
        let (after_remove, _) = remove(&with_block, "app");
        // Don't require exact equality (whitespace may differ by one newline)
        // but require the block is gone and the prose is preserved verbatim.
        assert!(!after_remove.contains("AGENT-CONFIG"));
        assert!(after_remove.contains("# Top"));
        assert!(after_remove.contains("Body."));
    }

    #[test]
    fn body_trailing_newlines_normalized() {
        let a = upsert("", "app", "x");
        let b = upsert("", "app", "x\n");
        let c = upsert("", "app", "x\n\n\n");
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn concurrent_file_upsert_different_tags_keeps_both_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("AGENTS.md");
        let path_a = path.clone();
        let path_b = path.clone();

        let (ra, rb) = run_two(
            move || {
                file_lock::with_lock(&path_a, || {
                    let host = fs_atomic::read_to_string_or_empty(&path_a)?;
                    let updated = upsert(&host, "alpha", "Alpha body.");
                    fs_atomic::write_atomic(&path_a, updated.as_bytes(), true)?;
                    Ok::<(), AgentConfigError>(())
                })
            },
            move || {
                file_lock::with_lock(&path_b, || {
                    let host = fs_atomic::read_to_string_or_empty(&path_b)?;
                    let updated = upsert(&host, "beta", "Beta body.");
                    fs_atomic::write_atomic(&path_b, updated.as_bytes(), true)?;
                    Ok::<(), AgentConfigError>(())
                })
            },
        );

        ra.unwrap();
        rb.unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("BEGIN AGENT-CONFIG:alpha"));
        assert!(text.contains("BEGIN AGENT-CONFIG:beta"));
        assert_eq!(text.matches("BEGIN AGENT-CONFIG:alpha").count(), 1);
        assert_eq!(text.matches("BEGIN AGENT-CONFIG:beta").count(), 1);
    }
}
