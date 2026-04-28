//! Ratatui-based example: side-by-side dry-run preview of the four
//! installable surfaces (skills, MCP, hooks, instructions) across every
//! harness that supports them.
//!
//! Read `examples/tui_dry_run/specs.rs` first: each tab demonstrates one
//! library surface using one canonical spec. Pick agents with Space, flip
//! scope with `g`, press Enter to run the dry-run plans for the selected
//! agents. No file is ever written.
//!
//! Run: `cargo run --example tui_dry_run`

mod app;
mod plan_runner;
mod specs;
mod ui;

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use app::App;

type TerminalBackend = CrosstermBackend<Stdout>;

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    // Restore terminal state on panic so the user is not left with a
    // broken prompt.
    let prior_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        prior_hook(info);
    }));

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let result = run(&mut terminal);

    // Best-effort restore on the normal exit path.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
    result
}

fn run(terminal: &mut Terminal<TerminalBackend>) -> io::Result<()> {
    let mut app = App::new()?;
    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                // crossterm emits Press / Release / Repeat; we only care
                // about presses (the default mode emits press-only, but
                // be explicit).
                if key.kind == KeyEventKind::Press {
                    handle_key(&mut app, key);
                }
            }
        }
        app.tick();
        if app.should_quit {
            break;
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) {
    // Any key dismisses an open toast. The key itself still processes
    // afterwards so navigation isn't blocked.
    if app.toast.is_some() {
        app.toast = None;
    }

    if app.help_open {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') => app.toggle_help(),
            _ => {}
        }
        return;
    }

    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('?') => app.toggle_help(),
        KeyCode::Tab => app.cycle_tab(!shift),
        KeyCode::BackTab => app.cycle_tab(false),
        KeyCode::Up | KeyCode::Char('k') => app.move_cursor(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_cursor(1),
        KeyCode::Char(' ') => app.toggle_current(),
        KeyCode::Char('a') => app.toggle_all(),
        KeyCode::Char('g') => app.flip_scope(),
        KeyCode::Enter => app.run_bulk(),
        _ => {}
    }
}
