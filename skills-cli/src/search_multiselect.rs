//! The fuzzy-search multiselect prompt, ported from
//! `src/prompts/search-multiselect.ts` of the upstream CLI (locked universal
//! section, select-all row, groups, detail pane, selection summary).

use std::collections::HashSet;

use crossterm::event::{KeyCode, KeyModifiers};

use crate::color;
use crate::prompt::{is_tty, Key, RawSession};

const S_STEP_ACTIVE: &str = "\u{25C6}";
const S_STEP_CANCEL: &str = "\u{25A0}";
const S_STEP_SUBMIT: &str = "\u{25C7}";
const S_RADIO_ACTIVE: &str = "\u{25CF}";
const S_RADIO_INACTIVE: &str = "\u{25CB}";
const S_BULLET: &str = "\u{2022}";
const S_BAR: &str = "\u{2502}";
const S_BAR_H: &str = "\u{2500}";

/// One selectable row.
#[derive(Clone)]
pub struct SearchItem {
    /// Value string returned on submit.
    pub value: String,
    /// Display label.
    pub label: String,
    /// Optional hint after the label.
    pub hint: Option<String>,
    /// Optional group heading (rendered when `select_groups`).
    pub group: Option<String>,
    /// Detail text for the fixed description pane.
    pub detail: Option<String>,
}

/// The always-selected section (universal agents).
pub struct LockedSection {
    /// Section title.
    pub title: String,
    /// Locked items (always included in the result).
    pub items: Vec<SearchItem>,
    /// Items hidden behind `…and N more`.
    pub hidden_count: usize,
}

/// Options for [`search_multiselect`].
pub struct SearchMultiselectOptions {
    /// Prompt message.
    pub message: String,
    /// Selectable items.
    pub items: Vec<SearchItem>,
    /// Initially selected values.
    pub initial_selected: Vec<String>,
    /// Require at least one selection to submit.
    pub required: bool,
    /// Locked section above the list.
    pub locked_section: Option<LockedSection>,
    /// Render the search input (default true).
    pub searchable: bool,
    /// Show the fixed description pane.
    pub show_detail: bool,
    /// Show the selected-items summary (default true).
    pub show_selected_summary: bool,
    /// Group headings toggle whole groups.
    pub select_groups: bool,
    /// Show the distinct Select All row.
    pub select_all: bool,
    /// Maximum visible rows (default 8).
    pub max_visible: usize,
}

enum Entry<'a> {
    Item(&'a SearchItem),
    Group(&'a str, Vec<&'a SearchItem>, bool),
}

fn build_entries<'a>(
    items: &'a [SearchItem],
    select_groups: bool,
    collapsed: &HashSet<String>,
) -> Vec<Entry<'a>> {
    if !select_groups {
        return items.iter().map(Entry::Item).collect();
    }
    let mut entries = Vec::new();
    let mut index = 0;
    while index < items.len() {
        let item = &items[index];
        match &item.group {
            None => {
                entries.push(Entry::Item(item));
                index += 1;
            }
            Some(group) => {
                let mut group_items = Vec::new();
                while index < items.len() && items[index].group.as_deref() == Some(group) {
                    group_items.push(&items[index]);
                    index += 1;
                }
                let is_collapsed = collapsed.contains(group);
                entries.push(Entry::Group(group, group_items.clone(), is_collapsed));
                if !is_collapsed {
                    for group_item in group_items {
                        entries.push(Entry::Item(group_item));
                    }
                }
            }
        }
    }
    entries
}

fn truncate_to_width(text: &str, width: usize) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if color::visible_len(&out) + color::visible_len(&ch.to_string()) > width {
            break;
        }
        out.push(ch);
    }
    out
}

fn format_detail_lines(detail: Option<&str>, width: usize, max_lines: usize) -> Vec<String> {
    let normalized = detail
        .map(|d| d.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    let mut remaining = normalized;
    while !remaining.is_empty() && lines.len() < max_lines {
        if color::visible_len(&remaining) <= width {
            lines.push(remaining.clone());
            remaining.clear();
            break;
        }
        let candidate = truncate_to_width(&remaining, width);
        match candidate.rfind(' ') {
            Some(break_at) if break_at > 0 => {
                lines.push(candidate[..break_at].trim_end().to_string());
                let taken = break_at.min(remaining.len());
                remaining = remaining[taken..].trim_start().to_string();
            }
            _ => {
                let taken = candidate.len().min(remaining.len());
                lines.push(candidate.clone());
                remaining = remaining[taken..].trim_start().to_string();
            }
        }
    }
    if !remaining.is_empty() && !lines.is_empty() {
        let last = lines.len() - 1;
        let shortened = truncate_to_width(&lines[last], width.saturating_sub(1));
        lines[last] = format!("{}\u{2026}", shortened.trim_end());
    }
    while lines.len() < max_lines {
        lines.push(String::new());
    }
    lines
}

/// Run the interactive search multiselect. Returns `None` on cancel.
#[allow(clippy::too_many_lines)]
pub fn search_multiselect(options: &SearchMultiselectOptions) -> Option<Vec<String>> {
    let searchable = options.searchable;
    let max_visible = if options.max_visible == 0 {
        8
    } else {
        options.max_visible
    };
    let locked_values: Vec<String> = options
        .locked_section
        .as_ref()
        .map(|section| {
            section
                .items
                .iter()
                .map(|item| item.value.clone())
                .collect()
        })
        .unwrap_or_default();

    let mut query = String::new();
    let mut cursor: usize = 0;
    let mut selected: HashSet<String> = options.initial_selected.iter().cloned().collect();
    let mut collapsed_groups: HashSet<String> = HashSet::new();
    let mut last_render_height: usize = 0;

    let filter = |item: &SearchItem, q: &str| -> bool {
        if q.is_empty() {
            return true;
        }
        let lower_q = q.to_lowercase();
        item.label.to_lowercase().contains(&lower_q) || item.value.to_lowercase().contains(&lower_q)
    };

    let columns = || {
        crossterm::terminal::size()
            .map(|(c, _)| c as usize)
            .unwrap_or(80)
    };

    let draw = |lines: &[String], last_height: &mut usize| {
        if *last_height > 0 {
            print!("\x1b[{last_height}A\x1b[J");
        }
        // Count wrapped rows.
        let cols = columns();
        let rows: usize = lines
            .iter()
            .map(|l| (color::visible_len(l) / cols).max(1))
            .sum();
        *last_height = rows;
        println!("{}", lines.join("\n"));
        let _ = std::io::Write::flush(&mut std::io::stdout());
    };

    let _session = if is_tty() {
        Some(RawSession::new())
    } else {
        None
    };
    _session.as_ref()?;

    let result: Option<Vec<String>> = loop {
        let filtered: Vec<SearchItem> = options
            .items
            .iter()
            .filter(|item| filter(item, &query))
            .cloned()
            .collect();
        let entries = build_entries(&filtered, options.select_groups, &collapsed_groups);
        let has_select_all = options.select_all && !options.items.is_empty();
        let entry_cursor = cursor.saturating_sub(if has_select_all { 1 } else { 0 });

        let mut lines: Vec<String> = Vec::new();
        lines.push(format!(
            "{}  {}",
            color::cyan(S_STEP_ACTIVE),
            color::bold(&options.message)
        ));

        // Locked section.
        if let Some(section) = &options.locked_section {
            if !section.items.is_empty() {
                lines.push(color::gray(S_BAR));
                let title = format!(
                    "{} {}",
                    color::bold(&section.title),
                    color::dim("\u{2500}\u{2500} always included")
                );
                lines.push(format!(
                    "{}  {} {} {}",
                    color::gray(S_BAR),
                    color::gray(S_BAR_H.repeat(2).as_str()),
                    title,
                    color::gray(S_BAR_H.repeat(12).as_str())
                ));
                for item in &section.items {
                    lines.push(format!(
                        "{}    {} {}",
                        color::gray(S_BAR),
                        color::green(S_BULLET),
                        color::bold(&item.label)
                    ));
                }
                if section.hidden_count > 0 {
                    lines.push(format!(
                        "{}    {}",
                        color::gray(S_BAR),
                        color::dim(&format!("\u{2026}and {} more", section.hidden_count))
                    ));
                }
                lines.push(color::gray(S_BAR));
                lines.push(format!(
                    "{}  {} {} {}",
                    color::gray(S_BAR),
                    color::gray(S_BAR_H.repeat(2).as_str()),
                    color::bold("Additional agents"),
                    color::gray(S_BAR_H.repeat(29).as_str())
                ));
            }
        }

        if searchable {
            lines.push(format!(
                "{}  {} {}{}",
                color::gray(S_BAR),
                color::dim("Search:"),
                query,
                color::inverse(" ")
            ));
            lines.push(format!(
                "{}  {}",
                color::gray(S_BAR),
                color::dim("\u{2191}\u{2193} move, space select, ctrl+a select all, enter confirm")
            ));
            lines.push(color::gray(S_BAR));
        }

        if has_select_all {
            let selected_count = options
                .items
                .iter()
                .filter(|item| selected.contains(&item.value))
                .count();
            let all = selected_count == options.items.len();
            let radio = if all {
                color::green(S_RADIO_ACTIVE)
            } else if selected_count > 0 {
                color::yellow("\u{25D0}")
            } else {
                color::dim(S_RADIO_INACTIVE)
            };
            let is_cursor = cursor == 0;
            let prefix = if is_cursor {
                color::cyan("\u{276F}")
            } else {
                " ".to_string()
            };
            let label = if is_cursor {
                color::underline(&color::bold("Select All"))
            } else {
                color::bold("Select All")
            };
            lines.push(format!(
                "{} {} {} {} {}",
                color::gray(S_BAR),
                prefix,
                radio,
                label,
                color::dim(&format!("({selected_count}/{})", options.items.len()))
            ));
            lines.push(format!(
                "{}   {}",
                color::gray(S_BAR),
                color::gray(S_BAR_H.repeat(36).as_str())
            ));
        }

        let render_footer = |lines: &mut Vec<String>,
                             entry_cursor: usize,
                             include_detail: bool,
                             include_summary: bool,
                             entries: &[Entry]| {
            if include_detail {
                let detail = if has_select_all && cursor == 0 {
                    Some(format!(
                        "Select or clear all {} skills.",
                        options.items.len()
                    ))
                } else {
                    match entries.get(entry_cursor) {
                        Some(Entry::Group(group, items, _)) => {
                            Some(format!("Select all {} skills in {group}.", items.len()))
                        }
                        Some(Entry::Item(item)) => item.detail.clone(),
                        None => None,
                    }
                };
                let detail_width = columns().saturating_sub(5).max(1);
                lines.push(color::gray(S_BAR));
                lines.push(format!(
                    "{}  {}",
                    color::gray(S_BAR),
                    color::dim("Description")
                ));
                for line in format_detail_lines(detail.as_deref(), detail_width, 2) {
                    lines.push(format!("{}  {}", color::gray(S_BAR), color::dim(&line)));
                }
            }
            if include_summary {
                lines.push(color::gray(S_BAR));
                let mut all_labels: Vec<String> = Vec::new();
                if let Some(section) = &options.locked_section {
                    all_labels.extend(section.items.iter().map(|i| i.label.clone()));
                }
                all_labels.extend(
                    options
                        .items
                        .iter()
                        .filter(|item| selected.contains(&item.value))
                        .map(|item| item.label.clone()),
                );
                if all_labels.is_empty() {
                    lines.push(format!(
                        "{}  {}",
                        color::gray(S_BAR),
                        color::dim("Selected: (none)")
                    ));
                } else {
                    let summary = if all_labels.len() <= 3 {
                        all_labels.join(", ")
                    } else {
                        format!(
                            "{} +{} more",
                            all_labels[..3].join(", "),
                            all_labels.len() - 3
                        )
                    };
                    lines.push(format!(
                        "{}  {} {}",
                        color::gray(S_BAR),
                        color::green("Selected:"),
                        summary
                    ));
                }
            }
            if !searchable {
                lines.push(color::gray(S_BAR));
                lines.push(format!(
                    "{}  {}",
                    color::gray(S_BAR),
                    color::dim("\u{2191}\u{2193} move, \u{2190}\u{2192} collapse/expand, space select, enter confirm")
                ));
            }
            lines.push(color::dim("\u{2514}"));
        };

        // Item rows with a sliding window.
        let build_item_lines = |lines: &mut Vec<String>,
                                entry_cursor: usize,
                                visible_limit: usize,
                                entries: &[Entry]| {
            if filtered.is_empty() {
                lines.push(format!(
                    "{}  {}",
                    color::gray(S_BAR),
                    color::dim("No matches found")
                ));
                return;
            }
            let visible_start = (entry_cursor.saturating_sub(visible_limit / 2))
                .min(entries.len().saturating_sub(visible_limit));
            let visible_end = (visible_start + visible_limit).min(entries.len());
            for (index, entry) in entries
                .iter()
                .enumerate()
                .skip(visible_start)
                .take(visible_end - visible_start)
            {
                let is_cursor = index == entry_cursor;
                match entry {
                    Entry::Group(group, items, collapsed) => {
                        let selected_count = items
                            .iter()
                            .filter(|item| selected.contains(&item.value))
                            .count();
                        let radio = if selected_count == items.len() {
                            color::green(S_RADIO_ACTIVE)
                        } else if selected_count > 0 {
                            color::yellow("\u{25D0}")
                        } else {
                            color::dim(S_RADIO_INACTIVE)
                        };
                        let label = if is_cursor {
                            color::underline(&color::bold(group))
                        } else {
                            color::bold(group)
                        };
                        let prefix = if is_cursor {
                            color::cyan("\u{276F}")
                        } else {
                            " ".to_string()
                        };
                        let disclosure = if *collapsed {
                            color::dim("\u{25B8}")
                        } else {
                            color::dim("\u{25BE}")
                        };
                        lines.push(format!(
                            "{} {} {} {} {}",
                            color::gray(S_BAR),
                            prefix,
                            disclosure,
                            radio,
                            label
                        ));
                    }
                    Entry::Item(item) => {
                        let is_selected = selected.contains(&item.value);
                        let radio = if is_selected {
                            color::green(S_RADIO_ACTIVE)
                        } else {
                            color::dim(S_RADIO_INACTIVE)
                        };
                        let label = if is_cursor {
                            color::underline(&item.label)
                        } else {
                            item.label.clone()
                        };
                        let hint = item
                            .hint
                            .as_ref()
                            .map(|h| format!(" {}", color::dim(&format!("({h})"))))
                            .unwrap_or_default();
                        let prefix = if is_cursor {
                            color::cyan("\u{276F}")
                        } else {
                            " ".to_string()
                        };
                        let mut tree = String::new();
                        if options.select_groups {
                            if let Some(group) = &item.group {
                                let group_items: Vec<&SearchItem> = filtered
                                    .iter()
                                    .filter(|i| i.group.as_deref() == Some(group.as_str()))
                                    .collect();
                                if let Some(last) = group_items.last() {
                                    let is_last = last.value == item.value;
                                    tree = format!(
                                        "{} ",
                                        color::dim(if is_last {
                                            "\u{2514}\u{2500}"
                                        } else {
                                            "\u{251C}\u{2500}"
                                        })
                                    );
                                }
                            }
                        }
                        lines.push(format!(
                            "{} {} {}{} {}{}",
                            color::gray(S_BAR),
                            prefix,
                            tree,
                            radio,
                            label,
                            hint
                        ));
                    }
                }
            }
            let hidden_before = visible_start;
            let hidden_after = entries.len().saturating_sub(visible_end);
            if hidden_before > 0 || hidden_after > 0 {
                let mut parts: Vec<String> = Vec::new();
                if hidden_before > 0 {
                    parts.push(format!("\u{2191} {hidden_before} more"));
                }
                if hidden_after > 0 {
                    parts.push(format!("\u{2193} {hidden_after} more"));
                }
                lines.push(format!(
                    "{}  {}",
                    color::gray(S_BAR),
                    color::dim(&parts.join("  "))
                ));
            }
        };

        let mut frame: Vec<String> = Vec::new();
        frame.extend(lines);
        build_item_lines(&mut frame, entry_cursor, max_visible, &entries);
        let mut footer: Vec<String> = Vec::new();
        render_footer(
            &mut footer,
            entry_cursor,
            options.show_detail,
            options.show_selected_summary,
            &entries,
        );
        frame.extend(footer);
        draw(&frame, &mut last_render_height);

        // Handle input.
        let Some(session) = &_session else { break None };
        let Some(key) = session.next_key() else {
            break None;
        };
        let entries_now = build_entries(&filtered, options.select_groups, &collapsed_groups);
        let cursor_offset = if has_select_all { 1 } else { 0 };
        let entry = if entry_cursor < entries_now.len() {
            Some(&entries_now[entry_cursor])
        } else {
            None
        };
        match key {
            Key::Enter => {
                if options.required && selected.is_empty() && locked_values.is_empty() {
                    continue;
                }
                break Some(locked_values.clone());
            }
            Key::Escape => break None,
            Key::CtrlC if false => break None,
            Key::Up => {
                cursor = cursor.saturating_sub(1);
            }
            Key::Down => {
                cursor = (entries_now.len() + cursor_offset)
                    .saturating_sub(1)
                    .min(cursor + 1);
            }
            Key::Right => {
                if options.select_groups {
                    if let Some(Entry::Group(group, _, collapsed)) = entry {
                        if *collapsed {
                            collapsed_groups.remove(*group);
                        }
                    }
                }
            }
            Key::Left => {
                if options.select_groups {
                    let group = match entry {
                        Some(Entry::Group(group, _, _)) => Some((*group).to_string()),
                        Some(Entry::Item(item)) => item.group.clone(),
                        None => None,
                    };
                    if let Some(group) = group {
                        collapsed_groups.insert(group.clone());
                        let collapsed_entries =
                            build_entries(&filtered, options.select_groups, &collapsed_groups);
                        if let Some(position) = collapsed_entries
                            .iter()
                            .position(|e| matches!(e, Entry::Group(name, _, _) if *name == group))
                        {
                            cursor = position + cursor_offset;
                        }
                    }
                }
            }
            Key::Space => {
                if has_select_all && cursor == 0 {
                    let all_selected = options
                        .items
                        .iter()
                        .all(|item| selected.contains(&item.value));
                    for item in &options.items {
                        if all_selected {
                            selected.remove(&item.value);
                        } else {
                            selected.insert(item.value.clone());
                        }
                    }
                } else {
                    match entry {
                        Some(Entry::Group(_, items, _)) => {
                            let all_selected =
                                items.iter().all(|item| selected.contains(&item.value));
                            for item in items {
                                if all_selected {
                                    selected.remove(&item.value);
                                } else {
                                    selected.insert(item.value.clone());
                                }
                            }
                        }
                        Some(Entry::Item(item)) => {
                            if selected.contains(&item.value) {
                                selected.remove(&item.value);
                            } else {
                                selected.insert(item.value.clone());
                            }
                        }
                        None => {}
                    }
                }
            }
            Key::CtrlA => {
                let all_selected = filtered.iter().all(|item| selected.contains(&item.value));
                for item in &filtered {
                    if all_selected {
                        selected.remove(&item.value);
                    } else {
                        selected.insert(item.value.clone());
                    }
                }
            }
            Key::Backspace => {
                query.pop();
                cursor = 0;
            }
            Key::Char(c) => {
                if searchable && c != '\0' {
                    query.push(c);
                    cursor = 0;
                }
            }
            Key::CtrlC => break None,
        }
        // Ctrl+A handled above; Ctrl+C via Escape only.
        let _ = KeyModifiers::NONE;
        let _ = KeyCode::Null;
    };

    // Final render.
    let final_lines = match &result {
        Some(_) => {
            let mut all_labels: Vec<String> = Vec::new();
            if let Some(section) = &options.locked_section {
                all_labels.extend(section.items.iter().map(|i| i.label.clone()));
            }
            all_labels.extend(
                options
                    .items
                    .iter()
                    .filter(|item| selected.contains(&item.value))
                    .map(|item| item.label.clone()),
            );
            vec![
                format!(
                    "{}  {}",
                    color::green(S_STEP_SUBMIT),
                    color::bold(&options.message)
                ),
                format!(
                    "{}  {}",
                    color::gray(S_BAR),
                    color::dim(&all_labels.join(", "))
                ),
            ]
        }
        None => {
            vec![
                format!(
                    "{}  {}",
                    color::red(S_STEP_CANCEL),
                    color::bold(&options.message)
                ),
                format!(
                    "{}  {}",
                    color::gray(S_BAR),
                    color::styled(&["strikethrough", "dim"], "Cancelled")
                ),
            ]
        }
    };
    if last_render_height > 0 {
        print!("\x1b[{last_render_height}A\x1b[J");
    }
    println!("{}", final_lines.join("\n"));
    let _ = std::io::Write::flush(&mut std::io::stdout());

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_wraps_and_pads() {
        let lines = format_detail_lines(Some("a b c d e f g h i j"), 10, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| color::visible_len(l) <= 10));
    }

    #[test]
    fn entries_respect_collapse() {
        let items = vec![
            SearchItem {
                value: "a".into(),
                label: "a".into(),
                hint: None,
                group: Some("G".into()),
                detail: None,
            },
            SearchItem {
                value: "b".into(),
                label: "b".into(),
                hint: None,
                group: Some("G".into()),
                detail: None,
            },
        ];
        let mut collapsed = HashSet::new();
        collapsed.insert("G".to_string());
        let entries = build_entries(&items, true, &collapsed);
        assert_eq!(entries.len(), 1);
        let open = build_entries(&items, true, &HashSet::new());
        assert_eq!(open.len(), 3);
    }
}
