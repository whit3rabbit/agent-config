//! Application state and event-handling logic for the TUI example.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use agent_config::{Scope, ScopeKind};
use tempfile::TempDir;

use crate::plan_runner::{agents_for, plan_for, AgentRow, Aggregate};
use crate::specs::Tab;

pub struct Toast {
    pub line1: String,
    pub line2: Option<String>,
    pub deadline: Instant,
}

pub struct App {
    pub tab: Tab,
    pub scope_kind: ScopeKind,
    /// Held for the lifetime of the app. Dropped at exit, which removes
    /// the directory. The example never writes into it (planning only).
    local_root: TempDir,
    selected: HashMap<Tab, BTreeSet<&'static str>>,
    cursor: HashMap<Tab, usize>,
    agents_cache: HashMap<Tab, Vec<AgentRow>>,
    pub toast: Option<Toast>,
    pub help_open: bool,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> std::io::Result<Self> {
        let local_root = TempDir::new()?;
        let mut agents_cache = HashMap::new();
        for tab in Tab::ALL {
            agents_cache.insert(tab, agents_for(tab));
        }
        Ok(Self {
            tab: Tab::Skills,
            // Default to Global so the live preview shows real
            // `~/.claude/...`, `~/.gemini/...`, etc. paths up front.
            // Local stays one keypress away (`g`) and routes writes to
            // the held tempdir.
            scope_kind: ScopeKind::Global,
            local_root,
            selected: HashMap::new(),
            cursor: HashMap::new(),
            agents_cache,
            toast: None,
            help_open: false,
            should_quit: false,
        })
    }

    pub fn local_root_display(&self) -> String {
        self.local_root.path().display().to_string()
    }

    pub fn scope(&self) -> Scope {
        match self.scope_kind {
            ScopeKind::Global => Scope::Global,
            // Local + any future variant defaults to the tempdir; user-
            // visible toggle still says "LOCAL".
            _ => Scope::Local(self.local_root.path().to_path_buf()),
        }
    }

    pub fn current_agents(&self) -> &[AgentRow] {
        self.agents_cache
            .get(&self.tab)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn cursor(&self) -> usize {
        *self.cursor.get(&self.tab).unwrap_or(&0)
    }

    fn set_cursor(&mut self, idx: usize) {
        self.cursor.insert(self.tab, idx);
    }

    pub fn cursor_row(&self) -> Option<&AgentRow> {
        self.current_agents().get(self.cursor())
    }

    pub fn move_cursor(&mut self, delta: i32) {
        let len = self.current_agents().len();
        if len == 0 {
            return;
        }
        let cur = self.cursor() as i32;
        let len = len as i32;
        let next = ((cur + delta).rem_euclid(len)) as usize;
        self.set_cursor(next);
    }

    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.get(&self.tab).is_some_and(|s| s.contains(id))
    }

    pub fn toggle_current(&mut self) {
        let Some(row) = self.cursor_row() else {
            return;
        };
        let id = row.id;
        let set = self.selected.entry(self.tab).or_default();
        if !set.insert(id) {
            set.remove(id);
        }
    }

    pub fn toggle_all(&mut self) {
        let ids: Vec<&'static str> = self.current_agents().iter().map(|a| a.id).collect();
        let set = self.selected.entry(self.tab).or_default();
        if !ids.is_empty() && set.len() == ids.len() {
            set.clear();
        } else {
            for id in ids {
                set.insert(id);
            }
        }
    }

    pub fn flip_scope(&mut self) {
        // ScopeKind is #[non_exhaustive]; treat anything new as "behave
        // like Local" for the toggle so future variants don't break.
        self.scope_kind = match self.scope_kind {
            ScopeKind::Local => ScopeKind::Global,
            _ => ScopeKind::Local,
        };
    }

    pub fn cycle_tab(&mut self, forward: bool) {
        let idx = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
        let n = Tab::ALL.len();
        let next = if forward {
            (idx + 1) % n
        } else {
            (idx + n - 1) % n
        };
        self.tab = Tab::ALL[next];
    }

    pub fn toggle_help(&mut self) {
        self.help_open = !self.help_open;
    }

    /// Run `plan_install_*` for every checked agent on the active tab and
    /// drop the aggregate into a toast.
    pub fn run_bulk(&mut self) {
        let scope = self.scope();
        let tab = self.tab;
        let ids: Vec<&'static str> = self
            .selected
            .get(&tab)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        if ids.is_empty() {
            self.toast = Some(Toast {
                line1: "No agents selected. Space toggles a row; 'a' toggles all.".to_string(),
                line2: None,
                deadline: Instant::now() + Duration::from_secs(3),
            });
            return;
        }
        let mut agg = Aggregate::default();
        for id in &ids {
            let plan = plan_for(tab, id, &scope);
            agg.record(&plan);
        }
        self.toast = Some(Toast {
            line1: agg.summary_line(),
            line2: agg.first_error.clone(),
            deadline: Instant::now() + Duration::from_secs(3),
        });
    }

    /// Drop the toast if its deadline has elapsed. Called from the event
    /// loop tick.
    pub fn tick(&mut self) {
        if let Some(t) = &self.toast {
            if Instant::now() >= t.deadline {
                self.toast = None;
            }
        }
    }
}
