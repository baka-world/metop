use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::TableState;
use ratatui::DefaultTerminal;

use crate::model::{Focus, ProcRow, Snapshot, SortKey};
use crate::ui;

pub const HISTORY: usize = 120;

pub struct App {
    pub sort: SortKey,
    pub desc: bool,
    pub filter: String,
    /// Mapping-name filter for the VMA detail view (independent of `filter`).
    pub detail_filter: String,
    pub editing: bool,
    /// Whether the in-progress edit targets the detail filter.
    pub editing_detail: bool,
    pub edit_buf: String,
    pub sel: usize,
    pub detail: bool,
    pub detail_sel: usize,
    pub snap: Option<Snapshot>,
    pub table_state: TableState,
    pub detail_state: TableState,
    pub history: HashMap<u32, VecDeque<u64>>,
    last_sent: Option<(Option<u32>, bool)>,
    started: bool,
    pending_focus: Option<u32>,
}

impl App {
    pub fn new(start_pid: Option<u32>) -> Self {
        let mut app = App {
            sort: SortKey::Rss,
            desc: true,
            filter: String::new(),
            detail_filter: String::new(),
            editing: false,
            editing_detail: false,
            edit_buf: String::new(),
            sel: 0,
            detail: false,
            detail_sel: 0,
            snap: None,
            table_state: TableState::default(),
            detail_state: TableState::default(),
            history: HashMap::new(),
            last_sent: None,
            started: false,
            pending_focus: None,
        };
        if let Some(pid) = start_pid {
            app.pending_focus = Some(pid);
        }
        app
    }

    fn ratio(&self, r: &ProcRow) -> f64 {
        let pss = self
            .snap
            .as_ref()
            .and_then(|s| s.pss.get(&r.pid))
            .map(|p| p.rollup.pss_kb)
            .unwrap_or(0);
        if r.rss_kb == 0 {
            0.0
        } else {
            pss as f64 / r.rss_kb as f64
        }
    }

    pub fn visible(&self) -> Vec<&ProcRow> {
        let Some(snap) = &self.snap else {
            return Vec::new();
        };
        let needle = self.filter.to_lowercase();
        let mut rows: Vec<&ProcRow> = snap
            .rows
            .iter()
            .filter(|r| {
                needle.is_empty()
                    || r.comm.to_lowercase().contains(&needle)
                    || r.pid.to_string().starts_with(&needle)
            })
            .collect();
        let pss = |pid: u32| snap.pss.get(&pid).map(|p| p.rollup.pss_kb).unwrap_or(0);
        let uss = |pid: u32| snap.pss.get(&pid).map(|p| p.rollup.uss_kb).unwrap_or(0);
        let sort = self.sort;
        let desc = self.desc;
        rows.sort_by(|a, b| {
            let ord = match sort {
                SortKey::Rss => a.rss_kb.cmp(&b.rss_kb),
                SortKey::Pss => pss(a.pid).cmp(&pss(b.pid)),
                SortKey::Uss => uss(a.pid).cmp(&uss(b.pid)),
                SortKey::Delta => a.delta_kb.cmp(&b.delta_kb),
                SortKey::Ratio => self
                    .ratio(a)
                    .partial_cmp(&self.ratio(b))
                    .unwrap_or(Ordering::Equal),
                SortKey::Pid => a.pid.cmp(&b.pid),
                SortKey::Comm => a
                    .comm
                    .to_lowercase()
                    .cmp(&b.comm.to_lowercase()),
            };
            let ord = if desc { ord.reverse() } else { ord };
            ord.then_with(|| a.pid.cmp(&b.pid))
        });
        rows
    }

    pub fn selected_pid(&self) -> Option<u32> {
        // While the detail view is open the focused pid comes from the loaded
        // detail, but it must fall back to the table selection: the Enter
        // keypress happens one snapshot before detail exists, and returning
        // None there drops the focus request entirely.
        if self.detail
            && let Some(pid) = self
                .snap
                .as_ref()
                .and_then(|s| s.detail.as_ref())
                .map(|d| d.pid)
        {
            return Some(pid);
        }
        self.visible().get(self.sel).map(|r| r.pid)
    }

    /// The (pid, want_detail) pair the sampler should be working on.
    pub fn focus_target(&self) -> (Option<u32>, bool) {
        if let Some(pid) = self.pending_focus {
            return (Some(pid), self.detail);
        }
        (self.selected_pid(), self.detail)
    }

    pub fn take_focus_update(&mut self) -> Option<Focus> {
        let target = self.focus_target();
        if self.last_sent == Some(target) {
            return None;
        }
        self.last_sent = Some(target);
        Some(Focus::Set { pid: target.0, want_detail: target.1 })
    }

    pub fn on_snapshot(&mut self, snap: Snapshot) {
        let mut seen: Vec<u32> = Vec::with_capacity(snap.rows.len());
        for row in &snap.rows {
            seen.push(row.pid);
            let h = self.history.entry(row.pid).or_default();
            h.push_back(row.rss_kb);
            while h.len() > HISTORY {
                h.pop_front();
            }
        }
        self.history.retain(|pid, _| seen.contains(pid));
        self.snap = Some(snap);
        if let Some(pid) = self.pending_focus
            && let Some(i) = self.visible().iter().position(|r| r.pid == pid)
        {
            self.sel = i;
            self.pending_focus = None;
        }
        self.clamp();
    }

    /// Number of VMA groups currently visible under `detail_filter`.
    pub fn detail_len(&self) -> usize {
        let needle = self.detail_filter.to_lowercase();
        self.snap
            .as_ref()
            .and_then(|s| s.detail.as_ref())
            .map(|d| {
                d.report
                    .groups
                    .iter()
                    .filter(|g| needle.is_empty() || g.label.to_lowercase().contains(&needle))
                    .count()
            })
            .unwrap_or(0)
    }

    fn clamp(&mut self) {
        let n_proc = self.visible().len();
        self.sel = self.sel.min(n_proc.saturating_sub(1));
        self.detail_sel = self.detail_sel.min(self.detail_len().saturating_sub(1));
        self.table_state.select(Some(self.sel));
        self.detail_state.select(Some(self.detail_sel));
    }

    /// Returns true when the app should exit.
    pub fn on_key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }
        if self.editing {
            return self.on_edit_key(key);
        }
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Char('j') | KeyCode::Down => self.move_sel(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_sel(-1),
            KeyCode::PageDown => self.move_sel(10),
            KeyCode::PageUp => self.move_sel(-10),
            KeyCode::Char('g') | KeyCode::Home => {
                self.sel = 0;
                self.detail_sel = 0;
            }
            KeyCode::Char('G') | KeyCode::End => self.sel = usize::MAX / 2,
            KeyCode::Char('s') => {
                self.sort = self.sort.next();
                self.sel = 0;
            }
            KeyCode::Char('r') => self.desc = !self.desc,
            KeyCode::Char('f') => {
                self.editing = true;
                self.editing_detail = self.detail;
                self.edit_buf = if self.detail {
                    self.detail_filter.clone()
                } else {
                    self.filter.clone()
                };
            }
            KeyCode::Enter => {
                if self.detail {
                    self.detail = false;
                    self.detail_sel = 0;
                } else if self.selected_pid().is_some() {
                    self.detail = true;
                    self.detail_sel = 0;
                }
            }
            KeyCode::Esc => {
                if self.detail && !self.detail_filter.is_empty() {
                    self.detail_filter.clear();
                    self.detail_sel = 0;
                } else if self.detail {
                    self.detail = false;
                    self.detail_sel = 0;
                } else if !self.filter.is_empty() {
                    self.filter.clear();
                    self.sel = 0;
                }
            }
            _ => {}
        }
        self.clamp();
        false
    }

    fn on_edit_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => {
                if self.editing_detail {
                    self.detail_filter = self.edit_buf.clone();
                    self.detail_sel = 0;
                } else {
                    self.filter = self.edit_buf.clone();
                    self.sel = 0;
                }
                self.editing = false;
                self.editing_detail = false;
            }
            KeyCode::Esc => {
                self.editing = false;
                self.editing_detail = false;
                self.edit_buf.clear();
            }
            KeyCode::Backspace => {
                self.edit_buf.pop();
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Char(c) => self.edit_buf.push(c),
            _ => {}
        }
        self.clamp();
        false
    }

    fn move_sel(&mut self, delta: isize) {
        if self.detail {
            let n = self.detail_len();
            self.detail_sel = step(self.detail_sel, delta, n);
            return;
        }
        let n = self.visible().len();
        self.sel = step(self.sel, delta, n);
    }

    pub fn run(
        terminal: &mut DefaultTerminal,
        start_pid: Option<u32>,
        tx: std::sync::mpsc::Sender<Focus>,
        rx: std::sync::mpsc::Receiver<Snapshot>,
    ) -> std::io::Result<()> {
        let mut app = App::new(start_pid);
        loop {
            let mut got = false;
            while let Ok(snap) = rx.try_recv() {
                app.on_snapshot(snap);
                got = true;
            }
            if got || app.started {
                terminal.draw(|f| ui::draw(f, &mut app))?;
                app.started = true;
            }
            if let Some(focus) = app.take_focus_update() {
                let _ = tx.send(focus);
            }
            if event::poll(Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()?
                    && app.on_key(key)
                {
                    return Ok(());
                }
                while event::poll(Duration::ZERO)? {
                    if let Event::Key(key) = event::read()?
                        && app.on_key(key)
                    {
                        return Ok(());
                    }
                }
                if let Some(focus) = app.take_focus_update() {
                    let _ = tx.send(focus);
                }
            }
        }
    }

    pub fn sample_age_ms(&self) -> u128 {
        self.snap
            .as_ref()
            .map(|s| s.at.elapsed().as_millis())
            .unwrap_or(0)
    }
}

fn step(cur: usize, delta: isize, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let max = n - 1;
    if delta >= 0 {
        (cur + delta as usize).min(max)
    } else {
        cur.saturating_sub((-delta) as usize)
    }
}
