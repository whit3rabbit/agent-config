//! ratatui rendering for the TUI dry-run example.

use agent_config::{PlanStatus, ScopeKind};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap};
use ratatui::Frame;

use crate::app::{App, Toast};
use crate::plan_runner::{plan_for, render_change};
use crate::specs::{self, Tab};

// Dark-mode palette. Foreground-only where possible so the example
// composes cleanly over whatever terminal background the user runs in.
const COL_BORDER: Color = Color::DarkGray;
const COL_DIM: Color = Color::DarkGray;
const COL_ACCENT: Color = Color::Cyan;
const COL_ACTIVE: Color = Color::LightCyan;
const COL_LOCAL: Color = Color::Cyan;
const COL_GLOBAL: Color = Color::LightYellow;
const COL_OK: Color = Color::Green;
const COL_ERR: Color = Color::LightRed;
const COL_WARN: Color = Color::Yellow;
const COL_TOAST_FG: Color = Color::White;
const COL_TOAST_BG: Color = Color::Blue;

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    draw_top(frame, app, outer[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(outer[1]);
    draw_agent_list(frame, app, body[0]);
    draw_preview(frame, app, body[1]);

    draw_keybinds(frame, outer[2]);

    if app.help_open {
        draw_help_overlay(frame, area);
    } else if let Some(toast) = &app.toast {
        draw_toast(frame, area, toast);
    }
}

fn draw_top(frame: &mut Frame, app: &App, area: Rect) {
    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(60)])
        .split(area);

    let titles: Vec<Line> = Tab::ALL.iter().map(|t| Line::from(t.label())).collect();
    let tab_idx = Tab::ALL.iter().position(|t| *t == app.tab).unwrap_or(0);
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COL_BORDER))
                .title(Span::styled(
                    " agent-config library example · dry-run preview ",
                    Style::default().fg(COL_ACCENT).add_modifier(Modifier::BOLD),
                )),
        )
        .select(tab_idx)
        .style(Style::default().fg(COL_DIM))
        .highlight_style(Style::default().fg(COL_ACTIVE).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, split[0]);

    let scope_text = match app.scope_kind {
        ScopeKind::Global => " Scope: GLOBAL  (real ~/.* paths) ".to_string(),
        _ => format!(" Scope: LOCAL  {} ", app.local_root_display()),
    };
    let scope_style = match app.scope_kind {
        ScopeKind::Global => Style::default().fg(COL_GLOBAL).add_modifier(Modifier::BOLD),
        _ => Style::default().fg(COL_LOCAL),
    };
    let scope_para = Paragraph::new(scope_text)
        .style(scope_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COL_BORDER)),
        )
        .alignment(Alignment::Right);
    frame.render_widget(scope_para, split[1]);
}

fn draw_agent_list(frame: &mut Frame, app: &App, area: Rect) {
    let agents = app.current_agents();
    let cursor = app.cursor();

    let lines: Vec<Line> = if agents.is_empty() {
        vec![Line::from(" (no agents in this surface) ")]
    } else {
        agents
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let mark = if app.is_selected(row.id) {
                    "[x]"
                } else {
                    "[ ]"
                };
                let style = if i == cursor {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                Line::from(vec![
                    Span::styled(format!(" {mark} "), style),
                    Span::styled(format!("{:<14}", row.id), style),
                    Span::styled(format!(" {}", row.display), style),
                ])
            })
            .collect()
    };

    let title = format!(
        " {} ({})  {} ",
        app.tab.label(),
        agents.len(),
        app.tab.surface_caption()
    );
    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COL_BORDER))
            .title(Span::styled(title, Style::default().fg(COL_ACCENT))),
    );
    frame.render_widget(para, area);
}

fn draw_preview(frame: &mut Frame, app: &App, area: Rect) {
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(9), Constraint::Min(0)])
        .split(area);

    // Spec snippet: per-agent variant on INSTRUCTIONS (placement
    // differs per harness), static elsewhere.
    let spec_text: String = match (app.tab, app.cursor_row()) {
        (Tab::Instructions, Some(row)) => specs::instruction_spec_snippet_for(row.id),
        _ => app.tab.spec_snippet().to_string(),
    };
    let spec_para = Paragraph::new(spec_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COL_BORDER))
                .title(Span::styled(" Spec ", Style::default().fg(COL_ACCENT))),
        )
        .style(Style::default().fg(COL_ACCENT))
        .wrap(Wrap { trim: false });
    frame.render_widget(spec_para, split[0]);

    let (plan_title, lines): (String, Vec<Line>) = match app.cursor_row() {
        None => (
            " Plan ".to_string(),
            vec![Line::from(" (no agent highlighted) ")],
        ),
        Some(row) => {
            let scope = app.scope();
            let scope_label = match app.scope_kind {
                ScopeKind::Global => "Global",
                _ => "Local",
            };
            let header = format!(" Plan · {} · scope={} ", row.id, scope_label);
            match plan_for(app.tab, row.id, &scope) {
                Ok(plan) => {
                    let (status_text, status_color) = match plan.status {
                        PlanStatus::WillChange => ("status  : WillChange", COL_OK),
                        PlanStatus::NoOp => ("status  : NoOp", COL_DIM),
                        PlanStatus::Refused => ("status  : Refused", COL_ERR),
                        _ => ("status  : Unknown", COL_DIM),
                    };
                    let mut out: Vec<Line> = vec![Line::from(Span::styled(
                        status_text.to_string(),
                        Style::default()
                            .fg(status_color)
                            .add_modifier(Modifier::BOLD),
                    ))];
                    if plan.changes.is_empty() {
                        out.push(Line::from("(no changes)"));
                    } else {
                        for change in &plan.changes {
                            out.push(Line::from(render_change(change)));
                        }
                    }
                    for w in &plan.warnings {
                        let path = w
                            .path
                            .as_ref()
                            .map(|p| format!(" ({})", p.display()))
                            .unwrap_or_default();
                        out.push(Line::from(Span::styled(
                            format!("warning : {}{path}", w.message),
                            Style::default().fg(COL_WARN),
                        )));
                    }
                    (header, out)
                }
                Err(e) => (
                    header,
                    vec![Line::from(Span::styled(
                        format!("error   : {e}"),
                        Style::default().fg(COL_ERR),
                    ))],
                ),
            }
        }
    };

    let plan_para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COL_BORDER))
                .title(Span::styled(plan_title, Style::default().fg(COL_ACCENT))),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(plan_para, split[1]);
}

fn draw_keybinds(frame: &mut Frame, area: Rect) {
    let para = Paragraph::new(
        " Tab/Shift-Tab tabs  |  Up/Down nav  |  Space check  |  a all  |  g scope  |  Enter run  |  ? help  |  q quit ",
    )
    .style(Style::default().fg(COL_DIM));
    frame.render_widget(para, area);
}

fn draw_toast(frame: &mut Frame, area: Rect, toast: &Toast) {
    let lines = match &toast.line2 {
        Some(l2) => vec![Line::from(toast.line1.clone()), Line::from(l2.clone())],
        None => vec![Line::from(toast.line1.clone())],
    };
    let height = (lines.len() as u16) + 2;
    let width = area.width.clamp(40, 80);
    let x = area.right().saturating_sub(width + 2);
    let y = area.bottom().saturating_sub(height + 2);
    let toast_area = Rect {
        x,
        y,
        width,
        height,
    };
    frame.render_widget(Clear, toast_area);
    let toast_style = Style::default().fg(COL_TOAST_FG).bg(COL_TOAST_BG);
    let para = Paragraph::new(lines)
        .style(toast_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    " toast · auto-dismiss 3s · any key clears ",
                    toast_style.add_modifier(Modifier::BOLD),
                ))
                .style(toast_style),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(para, toast_area);
}

fn draw_help_overlay(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(" Tab / Shift-Tab    switch surface tabs"),
        Line::from(" Up Down / j k       move cursor in agent list"),
        Line::from(" Space               toggle the highlighted row"),
        Line::from(" a                   toggle all rows in current tab"),
        Line::from(" g                   flip scope: Local <-> Global"),
        Line::from(" Enter               run dry-run for all checked rows"),
        Line::from(" ?                   toggle this help"),
        Line::from(" q / Esc             quit help / quit app"),
        Line::from(""),
        Line::from(" Dry-run only: only plan_install_* is called, no writes. "),
    ];
    let height = (lines.len() as u16) + 2;
    let width = 64u16.min(area.width);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let help_area = Rect {
        x,
        y,
        width,
        height,
    };
    frame.render_widget(Clear, help_area);
    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(COL_ACCENT))
                .title(Span::styled(
                    " Help ",
                    Style::default().fg(COL_ACTIVE).add_modifier(Modifier::BOLD),
                )),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(para, help_area);
}
