//! Terminal prompts replicating `@clack/prompts` 1.7.0 rendering: intro,
//! outro, log lines, notes, spinner, select, confirm, and multiselect.
//!
//! Symbols and layout mirror the upstream constants:
//! `◆ ● ○ ■ ▲ │ └ ┌ ─ ╮ ╰ ◼ ◻` with the same color assignments.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crossterm::event::{read, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal;

use crate::color;

// ─── Symbols (clack `unicodeOr` unicode set) ────────────────────────────────

const S_STEP_ACTIVE: &str = "\u{25C6}"; // ◆
const S_STEP_CANCEL: &str = "\u{25A0}"; // ■
#[allow(dead_code)]
const S_STEP_ERROR: &str = "\u{25B2}"; // ▲
const S_STEP_SUBMIT: &str = "\u{25C7}"; // ◇
const S_BAR_START: &str = "\u{250C}"; // ┌
const S_BAR: &str = "\u{2502}"; // │
const S_BAR_END: &str = "\u{2514}"; // └
const S_RADIO_ACTIVE: &str = "\u{25CF}"; // ●
const S_RADIO_INACTIVE: &str = "\u{25CB}"; // ○
const S_CHECKBOX_SELECTED: &str = "\u{25FC}"; // ◼
const S_CHECKBOX_INACTIVE: &str = "\u{25FB}"; // ◻
const S_BAR_H: &str = "\u{2500}"; // ─
#[allow(dead_code)]
const S_CONNECT_LEFT: &str = "\u{251C}"; // ├
const S_CORNER_TOP_RIGHT: &str = "\u{256E}"; // ╮
const S_CORNER_BOTTOM_RIGHT: &str = "\u{256F}"; // ╯
const S_CORNER_BOTTOM_LEFT: &str = "\u{2570}"; // ╰
const S_INFO: &str = "\u{25CF}"; // ●
const S_SUCCESS: &str = "\u{25C6}"; // ◆
const S_WARN: &str = "\u{25B2}"; // ▲
const S_ERROR: &str = "\u{25A0}"; // ■

fn stdout() -> std::io::Stdout {
    std::io::stdout()
}

fn write_str(s: &str) {
    let mut out = stdout();
    let _ = out.write_all(s.as_bytes());
    let _ = out.flush();
}

/// Visible width of a rendered line (ANSI-free content length).
fn visible_len(line: &str) -> usize {
    color::visible_len(line)
}

fn term_columns() -> usize {
    crossterm::terminal::size().map(|(c, _)| c as usize).unwrap_or(80)
}

// ─── Frame tracking ─────────────────────────────────────────────────────────

/// Rows written since the last clear, so prompts can redraw in place.
static FRAME_ROWS: AtomicUsize = AtomicUsize::new(0);

fn clear_frame() {
    let rows = FRAME_ROWS.swap(0, Ordering::SeqCst);
    if rows > 0 {
        write_str(&format!("\x1b[{rows}A\x1b[J"));
    }
}

fn emit_lines(lines: &[String]) {
    // Count soft-wrapped rows so the next clear removes everything.
    let columns = term_columns();
    let rows: usize = lines
        .iter()
        .map(|l| (visible_len(l) / columns).max(1))
        .sum();
    FRAME_ROWS.store(rows, Ordering::SeqCst);
    write_str(&format!("{}\n", lines.join("\n")));
}

// ─── State machine pieces ───────────────────────────────────────────────────

/// Whether stdin is interactive.
#[must_use]
pub fn is_tty() -> bool {
    std::io::stdin().is_terminal()
}

enum PromptState {
    Active,
    Submit,
    Cancel,
}

fn symbol(state: &PromptState) -> String {
    match state {
        PromptState::Active => color::cyan(S_STEP_ACTIVE),
        PromptState::Submit => color::green(S_STEP_SUBMIT),
        PromptState::Cancel => color::red(S_STEP_CANCEL),
    }
}

fn symbol_bar(state: &PromptState) -> String {
    match state {
        PromptState::Active => color::cyan(S_BAR),
        PromptState::Submit => color::green(S_BAR),
        PromptState::Cancel => color::red(S_BAR),
    }
}

#[allow(dead_code)] // kept for parity with clack's wrapTextWithPrefix
fn wrap_with_prefix(message: &str, prefix: &str, first_prefix: &str) -> String {
    let columns = term_columns().saturating_sub(prefix.len() + 1);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in message.split(' ') {
        let candidate_len = current.len() + if current.is_empty() { 0 } else { 1 } + word.len();
        if !current.is_empty() && candidate_len > columns {
            lines.push(current.clone());
            current.clear();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    lines.push(current);
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate() {
        let prefix = if index == 0 { first_prefix } else { prefix };
        out.push_str(prefix);
        out.push_str(line);
        if index + 1 < lines.len() {
            out.push('\n');
        }
    }
    out
}

/// A raw-mode prompt session: enables raw mode and yields key events.
pub(crate) struct RawSession {
    raw_enabled: bool,
}

impl RawSession {
    /// Enter raw mode.
    pub(crate) fn new() -> Self {
        let raw_enabled = is_tty() && terminal::enable_raw_mode().is_ok();
        Self { raw_enabled }
    }

    /// Read one key event; `None` on EOF / read failure.
    /// Read the next key event.
    pub(crate) fn next_key(&self) -> Option<Key> {
        loop {
            match read() {
                Ok(Event::Key(key)) => {
                    // Filter key-release events on Windows-style repeat flags.
                    if key.kind == crossterm::event::KeyEventKind::Press {
                        return Some(Key::from_event(key));
                    }
                }
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
    }
}

impl Drop for RawSession {
    fn drop(&mut self) {
        if self.raw_enabled {
            let _ = terminal::disable_raw_mode();
        }
    }
}

pub(crate) enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Backspace,
    CtrlC,
    CtrlA,
    Space,
    Char(char),
}

impl Key {
    fn from_event(event: KeyEvent) -> Self {
        match event.code {
            KeyCode::Up => Key::Up,
            KeyCode::Down => Key::Down,
            KeyCode::Left => Key::Left,
            KeyCode::Right => Key::Right,
            KeyCode::Enter => Key::Enter,
            KeyCode::Esc => Key::Escape,
            KeyCode::Backspace => Key::Backspace,
            KeyCode::Char('c') if event.modifiers.contains(KeyModifiers::CONTROL) => Key::CtrlC,
            KeyCode::Char('a') if event.modifiers.contains(KeyModifiers::CONTROL) => Key::CtrlA,
            KeyCode::Char(' ') => Key::Space,
            KeyCode::Char(c) => Key::Char(c),
            other => Key::Char(primary_char(other)),
        }
    }
}

fn primary_char(code: KeyCode) -> char {
    match code {
        KeyCode::Char(c) => c,
        _ => '\0',
    }
}

// ─── intro / outro / cancel ─────────────────────────────────────────────────

/// `p.intro(message)`.
pub fn intro(message: &str) {
    FRAME_ROWS.store(0, Ordering::SeqCst);
    write_str(&format!("{}  {message}\n", color::gray(S_BAR_START)));
}

/// `p.outro(message)`.
pub fn outro(message: &str) {
    FRAME_ROWS.store(0, Ordering::SeqCst);
    write_str(&format!(
        "{}\n{}  {message}\n\n",
        color::gray(S_BAR),
        color::gray(S_BAR_END)
    ));
}

/// `p.cancel(message)`.
pub fn cancel(message: &str) {
    FRAME_ROWS.store(0, Ordering::SeqCst);
    write_str(&format!("{}  {}\n\n", color::gray(S_BAR_END), color::red(message)));
}

// ─── log ────────────────────────────────────────────────────────────────────

fn log_message(message: &str, symbol_text: &str) {
    let mut lines = vec![color::gray(S_BAR)];
    for (index, part) in message.split('\n').enumerate() {
        if index == 0 {
            lines.push(format!("{}  {part}", symbol_text));
        } else {
            lines.push(format!("{}  {part}", color::gray(S_BAR)));
        }
    }
    write_str(&format!("{}\n", lines.join("\n")));
}

/// `p.log.message`.
pub fn log_plain(message: &str) {
    log_message(message, &color::gray(S_BAR));
}

/// `p.log.info`.
pub fn log_info(message: &str) {
    log_message(message, &color::blue(S_INFO));
}

/// `p.log.success`.
pub fn log_success(message: &str) {
    log_message(message, &color::green(S_SUCCESS));
}

/// `p.log.step`.
pub fn log_step(message: &str) {
    log_message(message, &color::green(S_STEP_SUBMIT));
}

/// `p.log.warn`.
pub fn log_warn(message: &str) {
    log_message(message, &color::yellow(S_WARN));
}

/// `p.log.error`.
#[allow(dead_code)]
pub fn log_error(message: &str) {
    log_message(message, &color::red(S_ERROR));
}

// ─── note ───────────────────────────────────────────────────────────────────

/// `p.note(body, title)` — a bordered box with a `◇  title ──╮` header.
pub fn note(body: &str, title: &str) {
    let columns = term_columns();
    let wrap_width = columns.saturating_sub(6).max(1);
    let mut content: Vec<String> = vec![String::new()];
    for paragraph in body.split('\n') {
        content.push(wrap_hard(paragraph, wrap_width));
    }
    content.push(String::new());

    let title_width = visible_len(title);
    let box_width = content
        .iter()
        .map(|l| visible_len(l))
        .max()
        .unwrap_or(0)
        .max(title_width)
        + 2;

    let mut out = String::new();
    out.push_str(&format!("{}\n", color::gray(S_BAR)));
    let dashes = box_width.saturating_sub(title_width).saturating_sub(1).max(1);
    out.push_str(&format!(
        "{}  {} {}\n",
        color::green(S_STEP_SUBMIT),
        color::reset(title),
        color::gray(format!("{}{}", S_BAR_H.repeat(dashes), S_CORNER_TOP_RIGHT).as_str())
    ));
    for line in &content {
        let pad = box_width.saturating_sub(visible_len(line));
        out.push_str(&format!(
            "{}  {}{}{}\n",
            color::gray(S_BAR),
            line,
            " ".repeat(pad),
            color::gray(S_BAR)
        ));
    }
    out.push_str(&format!(
        "{}\n",
        color::gray(
            format!(
                "{}{}{}",
                S_CORNER_BOTTOM_LEFT,
                S_BAR_H.repeat(box_width + 2),
                S_CORNER_BOTTOM_RIGHT
            )
            .as_str()
        )
    ));
    FRAME_ROWS.store(0, Ordering::SeqCst);
    write_str(&out);
}

fn wrap_hard(text: &str, width: usize) -> String {
    if width == 0 {
        return text.to_string();
    }
    let mut out = String::new();
    let mut current = 0;
    for ch in text.chars() {
        if current >= width {
            out.push('\n');
            current = 0;
        }
        out.push(ch);
        current += 1;
    }
    out
}

// ─── spinner ────────────────────────────────────────────────────────────────

/// A clack-compatible spinner (`p.spinner()`).
pub struct Spinner {
    inner: std::sync::Arc<SpinnerInner>,
}

struct SpinnerInner {
    handle: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Shared message the animation thread reads.
    msg: std::sync::Arc<std::sync::Mutex<String>>,
    /// Shared stop flag the animation thread watches.
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    finished: std::sync::atomic::AtomicBool,
    is_ci: bool,
}

const SPINNER_FRAMES: [&str; 4] = ["\u{25D2}", "\u{25D0}", "\u{25D3}", "\u{25D1}"]; // ◐◓◑◒

impl Spinner {
    /// Create and `start()` a spinner with the given message.
    pub fn start(message: &str) -> Self {
        let msg = std::sync::Arc::new(std::sync::Mutex::new(message.to_string()));
        let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_msg = msg.clone();
        let thread_stop = stop_flag.clone();
        let is_ci = std::env::var("CI").as_deref() == Ok("true");
        let handle = std::thread::spawn(move || {
            if is_ci {
                return;
            }
            let mut frame = 0usize;
            let mut dot_t = 0.0f32;
            let start = Instant::now();
            let mut last_len = 0usize;
            while !thread_stop.load(Ordering::SeqCst) {
                {
                    let current = thread_msg.lock().unwrap().clone();
                    let stripped = strip_dots(&current);
                    let mut line = format!("{}  {}", color::cyan(SPINNER_FRAMES[frame]), stripped);
                    let elapsed = start.elapsed().as_secs_f32();
                    let minutes = (elapsed / 60.0) as u64;
                    let seconds = (elapsed % 60.0) as u64;
                    if minutes > 0 {
                        line = format!("{line} [{minutes}m {seconds}s]");
                    }
                    // Erase previous frame and write the new one.
                    if last_len > 0 {
                        print!("\x1b[{last_len}D\x1b[K");
                    }
                    last_len = visible_len(&line);
                    print!("{line}");
                    let _ = std::io::stdout().flush();
                }
                frame = (frame + 1) % SPINNER_FRAMES.len();
                dot_t = if dot_t < 4.0 { dot_t + 0.125 } else { 0.0 };
                std::thread::sleep(Duration::from_millis(80));
            }
        });
        write_str(&format!("{}\n", color::gray(S_BAR)));
        Self {
            inner: std::sync::Arc::new(SpinnerInner {
                handle: std::sync::Mutex::new(Some(handle)),
                msg,
                stop_flag,
                finished: std::sync::atomic::AtomicBool::new(false),
                is_ci,
            }),
        }
    }

    /// `spinner.message(...)`.
    pub fn message(&self, message: &str) {
        if let Ok(mut msg) = self.inner.msg.lock() {
            *msg = strip_dots(message).to_string();
        }
    }

    /// `spinner.stop(message)` — green submit marker.
    pub fn stop(&self, message: &str) {
        self.finish(message, color::green(S_STEP_SUBMIT));
    }

    /// `spinner.error(message)` — red error marker.
    #[allow(dead_code)]
    pub fn error(&self, message: &str) {
        self.finish(message, color::red(S_STEP_ERROR));
    }

    fn finish(&self, message: &str, marker: String) {
        if self.inner.finished.swap(true, Ordering::SeqCst) {
            return;
        }
        self.inner.stop_flag.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.inner.handle.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
        if !self.inner.is_ci {
            print!("\r\x1b[K");
            let _ = std::io::stdout().flush();
        }
        let lines = vec![format!("{}  {message}", marker)];
        FRAME_ROWS.store(0, Ordering::SeqCst);
        emit_lines(&lines);
    }
}

fn strip_dots(message: &str) -> &str {
    message.trim_end_matches('.')
}



// ─── select ─────────────────────────────────────────────────────────────────

/// One selectable option.
pub struct SelectOption {
    /// Value returned on selection.
    pub value: String,
    /// Display label.
    pub label: String,
    /// Optional hint shown after the label.
    pub hint: Option<String>,
}

/// Result of a select prompt: `Some(value)` or `None` on cancel.
pub type SelectResult = Option<String>;

/// Interactive single-select prompt (clack `p.select`).
pub fn select(message: &str, options: &[SelectOption], initial: Option<usize>) -> SelectResult {
    if options.is_empty() {
        return None;
    }
    let _session = RawSession::new();
    let mut cursor = initial.unwrap_or(0).min(options.len() - 1);
    let mut state = PromptState::Active;
    let instructions = format!(
        "{} to navigate • {}: confirm",
        color::dim("\u{2191}/\u{2193}"),
        color::dim("Enter:")
    );

    loop {
        let mut lines: Vec<String> = Vec::new();
        lines.push(color::gray(S_BAR));
        lines.push(format!("{}  {}", symbol(&state), message));
        let prefix = format!("{}  ", symbol_bar(&state));
        match state {
            PromptState::Active => {
                for (index, option) in options.iter().enumerate() {
                    let radio = if index == cursor {
                        color::green(S_RADIO_ACTIVE)
                    } else {
                        color::dim(S_RADIO_INACTIVE)
                    };
                    let label = if index == cursor {
                        option.label.clone()
                    } else {
                        color::dim(&option.label)
                    };
                    let hint = option
                        .hint
                        .as_ref()
                        .map(|h| format!(" {}", color::dim(&format!("({h})"))))
                        .unwrap_or_default();
                    lines.push(format!("{prefix}{radio} {label}{hint}"));
                }
                lines.push(format!("{prefix}{instructions}"));
                lines.push(color::cyan(S_BAR_END));
                render_frame(&lines);
                match _session.next_key() {
                    Some(Key::Up) => cursor = cursor.saturating_sub(1),
                    Some(Key::Down) => cursor = (cursor + 1).min(options.len() - 1),
                    Some(Key::Enter) => state = PromptState::Submit,
                    Some(Key::Escape) | Some(Key::CtrlC) => state = PromptState::Cancel,
                    Some(_) => {}
                    None => {
                        state = PromptState::Cancel;
                    }
                }
            }
            PromptState::Submit => {
                lines.push(format!(
                    "{prefix}{}",
                    color::dim(&options[cursor].label)
                ));
                clear_frame();
                write_str(&format!("{}\n", lines.join("\n")));
                return Some(options[cursor].value.clone());
            }
            PromptState::Cancel => {
                lines.push(format!(
                    "{prefix}{}",
                    color::styled(&["strikethrough", "dim"], &options[cursor].label)
                ));
                lines.push(color::gray(S_BAR));
                clear_frame();
                write_str(&format!("{}\n", lines.join("\n")));
                return None;
            }
        }
    }
}

// ─── confirm ────────────────────────────────────────────────────────────────

/// Interactive yes/no prompt (clack `p.confirm`). `None` on cancel.
pub fn confirm(message: &str, initial: bool) -> Option<bool> {
    let _session = RawSession::new();
    let mut value = initial;
    let mut state = PromptState::Active;

    loop {
        let mut lines: Vec<String> = Vec::new();
        lines.push(color::gray(S_BAR));
        lines.push(format!("{}  {}", symbol(&state), message));
        let prefix = format!("{}  ", symbol_bar(&state));
        match state {
            PromptState::Active => {
                let yes = if value {
                    format!("{} {}", color::green(S_RADIO_ACTIVE), "Yes")
                } else {
                    format!("{} {}", color::dim(S_RADIO_INACTIVE), color::dim("Yes"))
                };
                let no = if value {
                    format!("{} {}", color::dim(S_RADIO_INACTIVE), color::dim("No"))
                } else {
                    format!("{} {}", color::green(S_RADIO_ACTIVE), "No")
                };
                lines.push(format!("{prefix}{yes} {} {no}", color::dim("/")));
                lines.push(color::cyan(S_BAR_END));
                render_frame(&lines);
                match _session.next_key() {
                    Some(Key::Left) | Some(Key::Right) => value = !value,
                    Some(Key::Up) | Some(Key::Down) => value = !value,
                    Some(Key::Enter) => state = PromptState::Submit,
                    Some(Key::Escape) | Some(Key::CtrlC) => state = PromptState::Cancel,
                    Some(_) => {}
                    None => state = PromptState::Cancel,
                }
            }
            PromptState::Submit => {
                let label = if value { "Yes" } else { "No" };
                lines.push(format!("{prefix}{}", color::dim(label)));
                clear_frame();
                write_str(&format!("{}\n", lines.join("\n")));
                return Some(value);
            }
            PromptState::Cancel => {
                let label = if value { "Yes" } else { "No" };
                lines.push(format!(
                    "{prefix}{}",
                    color::styled(&["strikethrough", "dim"], label)
                ));
                lines.push(color::gray(S_BAR));
                clear_frame();
                write_str(&format!("{}\n", lines.join("\n")));
                return None;
            }
        }
    }
}

// ─── multiselect ────────────────────────────────────────────────────────────

/// One checkbox option.
pub struct MultiOption {
    /// Value string.
    pub value: String,
    /// Display label.
    pub label: String,
    /// Optional hint.
    pub hint: Option<String>,
}

/// Interactive multi-select (clack `p.multiselect`). `None` on cancel.
pub fn multiselect(
    message: &str,
    options: &[MultiOption],
    initial_values: &[String],
    required: bool,
) -> Option<Vec<String>> {
    if options.is_empty() {
        return Some(Vec::new());
    }
    let _session = RawSession::new();
    let mut selected: Vec<bool> = options
        .iter()
        .map(|o| initial_values.contains(&o.value))
        .collect();
    let mut cursor = 0usize;
    let mut state = PromptState::Active;
    let instructions = format!(
        "{} to navigate • {}: select • {}: confirm",
        color::dim("\u{2191}/\u{2193}"),
        color::dim("Space:"),
        color::dim("Enter:")
    );

    loop {
        let mut lines: Vec<String> = Vec::new();
        lines.push(color::gray(S_BAR));
        lines.push(format!("{}  {}", symbol(&state), message));
        let prefix = format!("{}  ", symbol_bar(&state));
        match state {
            PromptState::Active => {
                for (index, option) in options.iter().enumerate() {
                    let checkbox = if selected[index] {
                        color::green(S_CHECKBOX_SELECTED)
                    } else if index == cursor {
                        color::cyan(S_CHECKBOX_INACTIVE)
                    } else {
                        color::dim(S_CHECKBOX_INACTIVE)
                    };
                    let label = if selected[index] {
                        color::dim(&option.label)
                    } else if index == cursor {
                        option.label.clone()
                    } else {
                        color::dim(&option.label)
                    };
                    let hint = option
                        .hint
                        .as_ref()
                        .map(|h| format!(" {}", color::dim(&format!("({h})"))))
                        .unwrap_or_default();
                    lines.push(format!("{prefix}{checkbox} {label}{hint}"));
                }
                lines.push(format!("{prefix}{instructions}"));
                lines.push(color::cyan(S_BAR_END));
                render_frame(&lines);
                match _session.next_key() {
                    Some(Key::Up) => cursor = cursor.saturating_sub(1),
                    Some(Key::Down) => cursor = (cursor + 1).min(options.len() - 1),
                    Some(Key::Space) => selected[cursor] = !selected[cursor],
                    Some(Key::Enter) => {
                        if required && !selected.iter().any(|s| *s) {
                            continue;
                        }
                        state = PromptState::Submit;
                    }
                    Some(Key::Escape) | Some(Key::CtrlC) => state = PromptState::Cancel,
                    Some(_) => {}
                    None => state = PromptState::Cancel,
                }
            }
            PromptState::Submit => {
                let labels: Vec<String> = options
                    .iter()
                    .zip(&selected)
                    .filter(|(_, s)| **s)
                    .map(|(o, _)| color::dim(&o.label))
                    .collect();
                let joined = if labels.is_empty() {
                    color::dim("none")
                } else {
                    labels.join(&color::dim(", "))
                };
                lines.push(format!("{prefix}{joined}"));
                clear_frame();
                write_str(&format!("{}\n", lines.join("\n")));
                return Some(
                    options
                        .iter()
                        .zip(&selected)
                        .filter(|(_, s)| **s)
                        .map(|(o, _)| o.value.clone())
                        .collect(),
                );
            }
            PromptState::Cancel => {
                lines.push(format!("{prefix}{}", color::styled(&["strikethrough", "dim"], "")));
                lines.push(color::gray(S_BAR));
                clear_frame();
                write_str(&format!("{}\n", lines.join("\n")));
                return None;
            }
        }
    }
}

fn render_frame(lines: &[String]) {
    clear_frame();
    emit_lines(lines);
}

/// Wrap a message for header lines (used by command modules).
#[allow(dead_code)]
pub(crate) fn wrap_header(message: &str, prefix: &str, first_prefix: &str) -> String {
    wrap_with_prefix(message, prefix, first_prefix)
}
