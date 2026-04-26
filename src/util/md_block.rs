//! Idempotent insert/extract/remove of a fenced markdown block in a host file.
//!
//! Each block is delimited by HTML comments keyed on a tag:
//!
//! ```text
//! <!-- BEGIN AI-HOOKER:app -->
//! ...content...
//! <!-- END AI-HOOKER:app -->
//! ```
//!
//! Multiple consumers can coexist by using distinct tags. The fence is invisible
//! when the markdown is rendered, and stable enough to grep over.

const FENCE_PREFIX: &str = "AI-HOOKER";

/// Returns the BEGIN/END marker pair for `tag`.
fn markers(tag: &str) -> (String, String) {
    (
        format!("<!-- BEGIN {FENCE_PREFIX}:{tag} -->"),
        format!("<!-- END {FENCE_PREFIX}:{tag} -->"),
    )
}

/// Insert or replace the tagged block in `host`.
///
/// Returns the new file contents. The block is appended on first write and
/// replaced in place on subsequent writes. A trailing newline is enforced so
/// repeated upserts stay idempotent.
pub(crate) fn upsert(host: &str, tag: &str, body: &str) -> String {
    let (begin, end) = markers(tag);
    let block = render_block(&begin, &end, body);

    if let Some((start, stop)) = find_block(host, &begin, &end) {
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

/// Returns true when `host` contains an incomplete or duplicate fenced block
/// for `tag`.
pub(crate) fn malformed(host: &str, tag: &str) -> bool {
    let (begin, end) = markers(tag);
    let begin_count = host.matches(&begin).count();
    let end_count = host.matches(&end).count();
    match (begin_count, end_count) {
        (0, 0) => false,
        (1, 1) => find_block(host, &begin, &end).is_none(),
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
    let Some((start, stop)) = find_block(host, &begin, &end) else {
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
    use crate::error::HookerError;
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
            "<!-- BEGIN AI-HOOKER:app -->\nhello\n<!-- END AI-HOOKER:app -->\n"
        );
    }

    #[test]
    fn upsert_appends_with_separator_on_existing_host() {
        let out = upsert("# Title\n\nIntro.\n", "app", "hello");
        assert!(out.starts_with("# Title\n\nIntro.\n\n<!-- BEGIN AI-HOOKER:app -->\n"));
        assert!(out.ends_with("<!-- END AI-HOOKER:app -->\n"));
    }

    #[test]
    fn upsert_replaces_in_place() {
        let host =
            "# Top\n\n<!-- BEGIN AI-HOOKER:app -->\nold\n<!-- END AI-HOOKER:app -->\n\n# Bottom\n";
        let out = upsert(host, "app", "new");
        assert_eq!(
            out,
            "# Top\n\n<!-- BEGIN AI-HOOKER:app -->\nnew\n<!-- END AI-HOOKER:app -->\n\n# Bottom\n"
        );
    }

    #[test]
    fn upsert_is_idempotent() {
        let once = upsert("# A", "app", "body");
        let twice = upsert(&once, "app", "body");
        assert_eq!(once, twice);
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
        assert!(malformed("<!-- BEGIN AI-HOOKER:app -->\nbody\n", "app"));
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
        assert!(!stripped.contains("AI-HOOKER:app"));
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
        assert!(!after_remove.contains("AI-HOOKER"));
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
                    Ok::<(), HookerError>(())
                })
            },
            move || {
                file_lock::with_lock(&path_b, || {
                    let host = fs_atomic::read_to_string_or_empty(&path_b)?;
                    let updated = upsert(&host, "beta", "Beta body.");
                    fs_atomic::write_atomic(&path_b, updated.as_bytes(), true)?;
                    Ok::<(), HookerError>(())
                })
            },
        );

        ra.unwrap();
        rb.unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("BEGIN AI-HOOKER:alpha"));
        assert!(text.contains("BEGIN AI-HOOKER:beta"));
        assert_eq!(text.matches("BEGIN AI-HOOKER:alpha").count(), 1);
        assert_eq!(text.matches("BEGIN AI-HOOKER:beta").count(), 1);
    }
}
