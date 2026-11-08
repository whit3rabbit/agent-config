//! Command-line layer: argument types, rendering, and the confirm prompt.
//!
//! Each command module exposes a clap `Args` struct and a `run(&Args) -> i32`
//! entry point; `main.rs` only dispatches and exits.

pub mod add;
pub mod check;
pub mod init;
pub mod list;
pub mod remove;
pub mod update;

use serde::Serialize;

use crate::diag::Diag;
use crate::engine::{OutcomeStatus, ScopeChoice, TargetOutcome};

/// Print a diagnostic to stderr.
pub fn print_diag(diag: &Diag) {
    eprintln!("{diag}");
}

/// Print a batch of advisory diagnostics (warnings, skipped entries).
pub fn print_diags(diags: &[Diag]) {
    for diag in diags {
        eprintln!("warning: {diag}");
    }
}

/// Turn a diagnostic into an exit code, printing it first.
pub fn fail(diag: Diag) -> i32 {
    print_diag(&diag);
    1
}

/// Resolve the scope from the `-g` flag; local scope anchors at the cwd.
pub fn scope_choice(global: bool) -> ScopeChoice {
    if global {
        ScopeChoice::Global
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        ScopeChoice::Local(cwd)
    }
}

/// Ask for confirmation on stdin. Enter defaults to no.
pub fn confirm(prompt: &str) -> bool {
    use std::io::Write;
    eprint!("{prompt} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Render one outcome line.
pub fn outcome_line(outcome: &TargetOutcome) -> String {
    let detail = match &outcome.status {
        OutcomeStatus::WouldChange { changes, refusals } => {
            let mut parts: Vec<String> = Vec::new();
            match (changes.first(), changes.len()) {
                (Some(first), 1) => parts.push(first.clone()),
                (Some(first), n) => parts.push(format!("{first} (+{} more)", n - 1)),
                (None, _) => {}
            }
            for refusal in refusals {
                parts.push(format!("refused: {refusal}"));
            }
            if parts.is_empty() {
                "planned (no file changes)".to_string()
            } else {
                parts.join("; ")
            }
        }
        OutcomeStatus::NoOp => "up to date".to_string(),
        OutcomeStatus::Skipped(why) => format!("skipped: {why}"),
        OutcomeStatus::Installed {
            created,
            patched,
            backed_up,
            already_installed,
        } => {
            if *already_installed {
                "already installed".to_string()
            } else {
                let mut parts = vec![format!("installed ({created} created, {patched} patched)")];
                if *backed_up > 0 {
                    parts.push(format!("{backed_up} backup(s)"));
                }
                parts.join(", ")
            }
        }
        OutcomeStatus::Removed { removed, patched } => {
            format!("removed ({removed} file(s), {patched} patched)")
        }
        OutcomeStatus::Failed(why) => format!("FAILED: {why}"),
    };
    format!(
        "  {} {} -> {}: {}",
        outcome.surface.as_str(),
        outcome.name,
        outcome.agent,
        detail
    )
}

/// Whether any outcome failed.
pub fn any_failed(outcomes: &[TargetOutcome]) -> bool {
    outcomes
        .iter()
        .any(|o| matches!(o.status, OutcomeStatus::Failed(_)))
}

/// Whether every outcome was skipped (nothing actually installed/removed).
pub fn all_skipped(outcomes: &[TargetOutcome]) -> bool {
    !outcomes.is_empty()
        && outcomes
            .iter()
            .all(|o| matches!(o.status, OutcomeStatus::Skipped(_)))
}

/// JSON shape for `--json` output.
#[derive(Serialize)]
pub struct JsonOutcome {
    /// Surface kind.
    pub surface: String,
    /// Item name.
    pub name: String,
    /// Agent id.
    pub agent: String,
    /// Status kind: would_change | noop | skipped | installed | removed | failed.
    pub status: String,
    /// Human detail (skipped reason, failure, change summary, counts).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Convert outcomes to the JSON shape.
pub fn json_outcomes(outcomes: &[TargetOutcome]) -> Vec<JsonOutcome> {
    outcomes
        .iter()
        .map(|o| {
            let detail = match &o.status {
                OutcomeStatus::WouldChange { changes, refusals } => Some(
                    changes
                        .iter()
                        .cloned()
                        .chain(refusals.iter().map(|r| format!("refused: {r}")))
                        .collect::<Vec<_>>()
                        .join("; "),
                ),
                OutcomeStatus::NoOp => None,
                OutcomeStatus::Skipped(why) => Some(why.clone()),
                OutcomeStatus::Installed {
                    created,
                    patched,
                    backed_up,
                    already_installed,
                } => Some(format!(
                    "created={created} patched={patched} backups={backed_up} already_installed={already_installed}"
                )),
                OutcomeStatus::Removed { removed, patched } => {
                    Some(format!("removed={removed} patched={patched}"))
                }
                OutcomeStatus::Failed(why) => Some(why.clone()),
            };
            JsonOutcome {
                surface: o.surface.as_str().to_string(),
                name: o.name.clone(),
                agent: o.agent.clone(),
                status: o.status.kind().to_string(),
                detail,
            }
        })
        .collect()
}
