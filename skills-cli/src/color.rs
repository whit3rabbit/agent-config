//! ANSI styling compatible with `picocolors` + node `styleText` as used by the
//! upstream `skills` CLI. Colors disable when stdout is not a TTY or
//! `NO_COLOR`/`TERM=dumb` is set; `FORCE_COLOR` forces them on.

// Full picocolors-equivalent palette: some entries exist for parity with the
// upstream color vocabulary even where the current commands don't use them.
#![allow(dead_code)]

use std::io::IsTerminal;
use std::sync::OnceLock;

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            return false;
        }
        if std::env::var_os("FORCE_COLOR").is_some() {
            return true;
        }
        if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
            return false;
        }
        std::io::stdout().is_terminal()
    })
}

fn wrap(codes: &str, text: &str) -> String {
    if enabled() && !text.is_empty() {
        format!("\x1b[{codes}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

/// Reset.
#[must_use]
pub fn reset(text: &str) -> String {
    wrap("0", text)
}
/// Bold.
#[must_use]
pub fn bold(text: &str) -> String {
    wrap("1", text)
}
/// Dim.
#[must_use]
pub fn dim(text: &str) -> String {
    wrap("2", text)
}
/// Italic.
#[must_use]
pub fn italic(text: &str) -> String {
    wrap("3", text)
}
/// Underline.
#[must_use]
pub fn underline(text: &str) -> String {
    wrap("4", text)
}
/// Inverse.
#[must_use]
pub fn inverse(text: &str) -> String {
    wrap("7", text)
}
/// Hidden.
#[must_use]
pub fn hidden(text: &str) -> String {
    wrap("8", text)
}
/// Strikethrough.
#[must_use]
pub fn strikethrough(text: &str) -> String {
    wrap("9", text)
}
/// Black.
#[must_use]
pub fn black(text: &str) -> String {
    wrap("30", text)
}
/// Red.
#[must_use]
pub fn red(text: &str) -> String {
    wrap("31", text)
}
/// Green.
#[must_use]
pub fn green(text: &str) -> String {
    wrap("32", text)
}
/// Yellow.
#[must_use]
pub fn yellow(text: &str) -> String {
    wrap("33", text)
}
/// Blue.
#[must_use]
pub fn blue(text: &str) -> String {
    wrap("34", text)
}
/// Magenta.
#[must_use]
pub fn magenta(text: &str) -> String {
    wrap("35", text)
}
/// Cyan.
#[must_use]
pub fn cyan(text: &str) -> String {
    wrap("36", text)
}
/// White.
#[must_use]
pub fn white(text: &str) -> String {
    wrap("37", text)
}
/// Gray (bright black).
#[must_use]
pub fn gray(text: &str) -> String {
    wrap("90", text)
}
/// Background red.
#[must_use]
pub fn bg_red(text: &str) -> String {
    wrap("41", text)
}
/// Background cyan.
#[must_use]
pub fn bg_cyan(text: &str) -> String {
    wrap("46", text)
}
/// Background white.
#[must_use]
pub fn bg_white(text: &str) -> String {
    wrap("47", text)
}

/// Combine two styles like `pc.bold(pc.red(x))`: each wraps independently.
#[must_use]
pub fn bold_red(text: &str) -> String {
    bold(&red(text))
}

/// Whether colors are currently enabled.
#[must_use]
pub fn colors_enabled() -> bool {
    enabled()
}

/// Compose a set of style codes in one escape (node `styleText` accepts
/// arrays; the CLI uses it for e.g. `["strikethrough", "dim"]`).
#[must_use]
pub fn styled(codes: &[&str], text: &str) -> String {
    if enabled() && !text.is_empty() {
        format!("\x1b[{}m{text}\x1b[0m", codes.join(";"))
    } else {
        text.to_string()
    }
}

/// Strip ANSI escape sequences from `text` (the `stripVTControlCharacters`
/// equivalent used for width math).
#[must_use]
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    let mut prev = '\0';
                    for c in chars.by_ref() {
                        if c == '\x07' || (prev == '\x1b' && c == '\\') {
                            break;
                        }
                        prev = c;
                    }
                }
                _ => {
                    chars.next();
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

fn char_width(ch: char) -> usize {
    let code = ch as u32;
    if code == 0 {
        return 0;
    }
    let wide = (0x1100..=0x115F).contains(&code)
        || (0x231A..=0x231B).contains(&code)
        || (0x2329..=0x232A).contains(&code)
        || (0x23E9..=0x23EC).contains(&code)
        || code == 0x23F0
        || code == 0x23F3
        || (0x25FD..=0x25FE).contains(&code)
        || (0x2614..=0x2615).contains(&code)
        || (0x2648..=0x2653).contains(&code)
        || (0x2E80..=0xA4CF).contains(&code)
        || (0xAC00..=0xD7A3).contains(&code)
        || (0xF900..=0xFAFF).contains(&code)
        || (0xFE30..=0xFE6F).contains(&code)
        || (0xFF00..=0xFF60).contains(&code)
        || (0xFFE0..=0xFFE6).contains(&code)
        || (0x1F000..=0x1F9FF).contains(&code);
    if wide {
        2
    } else {
        1
    }
}

/// Approximate terminal display width of `text` (ANSI sequences excluded).
#[must_use]
pub fn visible_len(text: &str) -> usize {
    strip_ansi(text).chars().map(char_width).sum()
}

/// The light-gray primary text color used by `find`/`list` (256-color 145).
#[must_use]
pub fn text_color() -> String {
    wrap("38;5;145", "")
}

/// Style text with the light-gray primary text color.
#[must_use]
pub fn text(text: &str) -> String {
    wrap("38;5;145", text)
}

/// Style text with the darker-gray secondary color (256-color 102).
#[must_use]
pub fn dim_gray(text: &str) -> String {
    wrap("38;5;102", text)
}
