//! Terminal-escape sanitization and skill-name sanitization.
//!
//! Ports `src/sanitize.ts` (CWE-150 defense) and `sanitizeName()` from
//! `src/installer.ts` of the upstream `skills` CLI.

/// Strip all terminal escape sequences and dangerous control characters.
///
/// Removes OSC, DCS/PM/APC, CSI, simple `ESC x`, C1 controls, and raw control
/// characters, keeping `\t` and `\n`.
#[must_use]
pub fn strip_terminal_escapes(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out: Vec<char> = chars;

    // OSC: ESC ] ... BEL or ESC \
    out = remove_sequence(&out, &['\x1b', ']'], |s| find_terminator(s, true));
    // DCS / PM / APC: ESC P|^|_ ... ESC \
    for opener in ['P', '^', '_'] {
        out = remove_sequence(&out, &['\x1b', opener], |s| find_terminator(s, false));
    }
    // CSI: ESC [ params* intermediates* final(0x40-0x7E)
    out = remove_sequence(&out, &['\x1b', '['], |s| {
        for (idx, ch) in s.iter().enumerate() {
            let c = *ch as u32;
            if (0x40..=0x7E).contains(&c) {
                return Some(idx);
            }
            // Parameter (0x30-0x3F) or intermediate (0x20-0x2F) bytes continue the sequence.
            if (0x20..=0x3F).contains(&c) {
                continue;
            }
            return None;
        }
        None
    });
    // Simple escapes: ESC followed by one char in 0x20-0x7E.
    out = remove_sequence(&out, &['\x1b'], |s| match s.first() {
        Some(c) if (0x20..=0x7E).contains(&(*c as u32)) => Some(0),
        _ => None,
    });
    // C1 control codes U+0080..=U+009F.
    out.retain(|c| !matches!(c, '\u{0080}'..='\u{009F}'));
    // Raw control characters except \t and \n.
    out.retain(|c| {
        let c = *c as u32;
        !matches!(c, 0x00..=0x06 | 0x07 | 0x08 | 0x0B | 0x0C | 0x0D..=0x1A | 0x1C..=0x1F | 0x7F)
    });

    out.into_iter().collect()
}

/// Find the terminating position of an OSC/DCS-style sequence.
/// For OSC the terminator is BEL or ESC-`\`; for DCS/PM/APC only ESC-`\`.
fn find_terminator(rest: &[char], allow_bel: bool) -> Option<usize> {
    let mut i = 0;
    while i < rest.len() {
        if allow_bel && rest[i] == '\x07' {
            return Some(i);
        }
        if rest[i] == '\x1b' && rest.get(i + 1) == Some(&'\\') {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// Remove every occurrence of `opener`-prefixed sequences; `find_end` returns
/// the inclusive index of the final sequence char relative to the opener end.
fn remove_sequence(
    input: &[char],
    opener: &[char],
    find_end: impl Fn(&[char]) -> Option<usize>,
) -> Vec<char> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i..].starts_with(opener) {
            if let Some(end) = find_end(&input[i + opener.len()..]) {
                i += opener.len() + end + 1;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
    out
}

/// Sanitize a skill metadata string for terminal display: strip escapes, then
/// collapse newline runs into single spaces and trim.
#[must_use]
pub fn sanitize_metadata(input: &str) -> String {
    let stripped = strip_terminal_escapes(input);
    let mut out = String::with_capacity(stripped.len());
    let mut in_newline = false;
    for ch in stripped.chars() {
        if ch == '\r' || ch == '\n' {
            if !in_newline {
                out.push(' ');
                in_newline = true;
            }
        } else {
            out.push(ch);
            in_newline = false;
        }
    }
    out.trim().to_string()
}

/// Sanitize a directory name for the canonical skills store.
///
/// Lowercases, replaces every run of characters outside `a-z0-9._` with a
/// hyphen, trims leading/trailing dots and hyphens, caps at 255 chars, and
/// falls back to `unnamed-skill`.
#[must_use]
pub fn sanitize_name(name: &str) -> String {
    let lowered = name.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut in_run = false;
    for ch in lowered.chars() {
        let keep = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '.' || ch == '_';
        if keep {
            out.push(ch);
            in_run = false;
        } else if !in_run {
            out.push('-');
            in_run = true;
        }
    }
    let trimmed = out.trim_matches(|c| c == '.' || c == '-');
    let truncated: String = trimmed.chars().take(255).collect();
    if truncated.is_empty() {
        "unnamed-skill".to_string()
    } else {
        truncated
    }
}

/// Normalize a skill name for comparison: lowercase, whitespace/underscore
/// runs become hyphens (`normalizeSkillName` in the upstream CLI).
#[must_use]
pub fn normalize_skill_name(name: &str) -> String {
    name.to_lowercase()
        .split(|c: char| c.is_whitespace() || c == '_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Convert a skill name to the URL slug used by the skills.sh download API
/// (`toSkillSlug`).
#[must_use]
pub fn to_skill_slug(name: &str) -> String {
    let lowered = name.to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut in_hyphen = false;
    for ch in lowered.chars() {
        if ch.is_whitespace() || ch == '_' {
            if !in_hyphen {
                out.push('-');
                in_hyphen = true;
            }
        } else if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' {
            if ch == '-' {
                if !in_hyphen {
                    out.push(ch);
                    in_hyphen = true;
                }
            } else {
                out.push(ch);
                in_hyphen = false;
            }
        }
        // Other characters are dropped; in_hyphen stays as-is.
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_escapes_and_controls() {
        assert_eq!(strip_terminal_escapes("\x1b]0;title\x07hi"), "hi");
        assert_eq!(strip_terminal_escapes("\x1b]8;;http://x\x1b\\a"), "a");
        assert_eq!(strip_terminal_escapes("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_terminal_escapes("\x1b7save"), "save");
        assert_eq!(strip_terminal_escapes("a\u{0093}b"), "ab");
        assert_eq!(strip_terminal_escapes("a\x07b\x08c"), "abc");
        assert_eq!(strip_terminal_escapes("keep\ttab\nnewline"), "keep\ttab\nnewline");
    }

    #[test]
    fn sanitizes_metadata() {
        assert_eq!(sanitize_metadata("  hi\nthere  "), "hi there");
        // Note:  is stripped by the escape filter (as upstream), then the
        // remaining newline run collapses to one space.
        assert_eq!(sanitize_metadata("multi\r\nline\rdone"), "multi linedone");
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_name("My Cool Skill!"), "my-cool-skill");
        assert_eq!(sanitize_name("../etc/passwd"), "etc-passwd");
        assert_eq!(sanitize_name("---.--"), "unnamed-skill");
        assert_eq!(sanitize_name("Git Review: Before Commit"), "git-review-before-commit");
        assert_eq!(sanitize_name("ce:review"), "ce-review");
    }

    #[test]
    fn slugs() {
        assert_eq!(to_skill_slug("React Best Practices"), "react-best-practices");
        assert_eq!(to_skill_slug("vercel_optimize!"), "vercel-optimize");
    }
}
