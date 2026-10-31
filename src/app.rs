use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::TableState;

use crate::apps::AppGroup;
use crate::cgroup::CgroupMem;
use crate::model::{Focus, ProcRow, Snapshot, SortKey};
use crate::procfs::{Vma, VmaGroup};
use crate::ui;

pub const HISTORY: usize = 120;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Procs,
    Cgroups,
    Apps,
}

/// One sample of an app's three metrics, so switching the map metric does not
/// mix series (a PSS history is useless when the view shows RSS).
#[derive(Clone, Copy, Default)]
pub struct AppSample {
    pub pss_kb: u64,
    pub uss_kb: u64,
    pub rss_kb: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppMetric {
    Pss,
    Uss,
    Rss,
}

impl AppMetric {
    pub fn label(self) -> &'static str {
        match self {
            AppMetric::Pss => "PSS",
            AppMetric::Uss => "USS",
            AppMetric::Rss => "RSS",
        }
    }

    pub fn value(self, g: &AppGroup) -> u64 {
        match self {
            AppMetric::Pss => g.pss_kb,
            AppMetric::Uss => g.uss_kb,
            AppMetric::Rss => g.rss_kb,
        }
    }

    fn next(self) -> Self {
        match self {
            AppMetric::Pss => AppMetric::Uss,
            AppMetric::Uss => AppMetric::Rss,
            AppMetric::Rss => AppMetric::Pss,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CgroupSort {
    Current,
    Anon,
    File,
    Slab,
}

impl CgroupSort {
    const ALL: [CgroupSort; 4] = [
        CgroupSort::Current,
        CgroupSort::Anon,
        CgroupSort::File,
        CgroupSort::Slab,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CgroupSort::Current => "CUR",
            CgroupSort::Anon => "ANON",
            CgroupSort::File => "FILE",
            CgroupSort::Slab => "SLAB",
        }
    }

    fn next(self) -> Self {
        let i = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    fn key(self, c: &CgroupMem) -> u64 {
        match self {
            CgroupSort::Current => c.current_kb,
            CgroupSort::Anon => c.anon_kb,
            CgroupSort::File => c.file_kb,
            CgroupSort::Slab => c.slab_kb,
        }
    }
}

/// A row in the detail view: either an aggregated mapping or one VMA inside it.
pub enum DetailRow<'a> {
    Group(&'a VmaGroup),
    Vma(&'a VmaGroup, &'a Vma),
}

pub struct App {
    pub view: View,
    pub sort: SortKey,
    pub cgroup_sort: CgroupSort,
    pub cgroup_sel: usize,
    /// App groups for the treemap, recomputed per snapshot (collapsed to the
    /// top few percent so the map does not dissolve into slivers).
    pub app_groups: Vec<AppGroup>,
    /// Metric history per app name (only the collapsed groups are tracked:
    /// ~20-40 entries, not all 330).
    pub app_history: HashMap<String, VecDeque<AppSample>>,
    pub app_sel: usize,
    pub app_metric: AppMetric,
    pub app_state: TableState,
    /// Drill target coming from the apps view: only these pids are shown in
    /// the process table.
    pub pid_filter: Option<std::collections::HashSet<u32>>,
    pub pid_filter_name: Option<String>,
    /// cgroup path whose processes are being listed (drill-down).
    pub drill: Option<String>,
    pub drill_sel: usize,
    pub desc: bool,
    pub filter: String,
    /// Mapping-name filter for the VMA detail view (independent of `filter`).
    pub detail_filter: String,
    /// Group labels whose VMAs are expanded.
    pub expanded: std::collections::HashSet<String>,
    pub editing: bool,
    /// Whether the in-progress edit targets the detail filter.
    pub editing_detail: bool,
    pub edit_buf: String,
    pub sel: usize,
    pub detail: bool,
    pub detail_sel: usize,
    pub snap: Option<Snapshot>,
    pub table_state: TableState,
    pub cgroup_state: TableState,
    pub drill_state: TableState,
    pub detail_state: TableState,
    pub history: HashMap<u32, VecDeque<u64>>,
    /// PSI `some avg10` over time per cgroup, stored as percent*10 for the
    /// sparkline (u64) with a fixed 0-1000 scale.
    pub cgroup_history: HashMap<String, VecDeque<u64>>,
    /// OOM kills per scan per cgroup (5 s cadence, 120 samples = 10 min).
    pub oom_history: HashMap<String, VecDeque<u64>>,
    last_scan_seq: u64,
    last_sent: Option<(Option<u32>, bool)>,
    started: bool,
    pending_focus: Option<u32>,
    detail_pid: Option<u32>,
}

impl App {
    pub fn new(start_pid: Option<u32>) -> Self {
        let mut app = App {
            view: View::Procs,
            sort: SortKey::Rss,
            cgroup_sort: CgroupSort::Current,
            cgroup_sel: 0,
            app_groups: Vec::new(),
            app_history: HashMap::new(),
            app_sel: 0,
            app_metric: AppMetric::Pss,
            app_state: TableState::default(),
            pid_filter: None,
            pid_filter_name: None,
            drill: None,
            drill_sel: 0,
            desc: true,
            filter: String::new(),
            detail_filter: String::new(),
            expanded: std::collections::HashSet::new(),
            editing: false,
            editing_detail: false,
            edit_buf: String::new(),
            sel: 0,
            detail: false,
            detail_sel: 0,
            snap: None,
            table_state: TableState::default(),
            cgroup_state: TableState::default(),
            drill_state: TableState::default(),
            detail_state: TableState::default(),
            history: HashMap::new(),
            cgroup_history: HashMap::new(),
            oom_history: HashMap::new(),
            last_scan_seq: 0,
            last_sent: None,
            started: false,
            pending_focus: None,
            detail_pid: None,
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
                self.pid_filter
                    .as_ref()
                    .is_none_or(|set| set.contains(&r.pid))
            })
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
                SortKey::Comm => a.comm.to_lowercase().cmp(&b.comm.to_lowercase()),
            };
            let ord = if desc { ord.reverse() } else { ord };
            ord.then_with(|| a.pid.cmp(&b.pid))
        });
        rows
    }

    pub fn visible_cgroups(&self) -> Vec<&CgroupMem> {
        let Some(snap) = &self.snap else {
            return Vec::new();
        };
        let needle = self.filter.to_lowercase();
        let mut rows: Vec<&CgroupMem> = snap
            .cgroups
            .iter()
            .filter(|c| needle.is_empty() || c.path.to_lowercase().contains(&needle))
            .collect();
        rows.sort_by_key(|c| self.cgroup_sort.key(c));
        if self.desc {
            rows.reverse();
        }
        rows
    }

    /// Processes whose cgroup is `drill` or a descendant of it. The cgroup
    /// numbers are hierarchical, so the subtree is what you want to see.
    pub fn cgroup_members(&self) -> Vec<&ProcRow> {
        let (Some(path), Some(snap)) = (&self.drill, &self.snap) else {
            return Vec::new();
        };
        let prefix = if path == "/" {
            "/".to_string()
        } else {
            format!("{path}/")
        };
        let mut rows: Vec<&ProcRow> = snap
            .rows
            .iter()
            .filter(|r| {
                snap.pid_cgroup
                    .get(&r.pid)
                    .is_some_and(|c| c == path || c.starts_with(&prefix))
            })
            .collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.rss_kb));
        rows
    }

    /// Series for the selected metric of `name`, for the header sparkline.
    pub fn app_series(&self, name: &str, metric: AppMetric) -> Vec<u64> {
        self.app_history
            .get(name)
            .map(|h| {
                h.iter()
                    .map(|s| match metric {
                        AppMetric::Pss => s.pss_kb,
                        AppMetric::Uss => s.uss_kb,
                        AppMetric::Rss => s.rss_kb,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn visible_apps(&self) -> Vec<&AppGroup> {
        let needle = self.filter.to_lowercase();
        self.app_groups
            .iter()
            .filter(|g| needle.is_empty() || g.name.to_lowercase().contains(&needle))
            .collect()
    }

    pub fn selected_app(&self) -> Option<&AppGroup> {
        self.visible_apps().get(self.app_sel).copied()
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
        Some(Focus::Set {
            pid: target.0,
            want_detail: target.1,
        })
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
        self.app_groups = crate::apps::collapse(
            crate::apps::group(&snap.rows, &snap.pss, &snap.pid_cgroup, &snap.pid_exe),
            0.005,
        );
        let mut app_seen: std::collections::HashSet<&str> =
            std::collections::HashSet::with_capacity(self.app_groups.len());
        for g in &self.app_groups {
            app_seen.insert(g.name.as_str());
            let h = self.app_history.entry(g.name.clone()).or_default();
            h.push_back(AppSample {
                pss_kb: g.pss_kb,
                uss_kb: g.uss_kb,
                rss_kb: g.rss_kb,
            });
            while h.len() > HISTORY {
                h.pop_front();
            }
        }
        self.app_history
            .retain(|name, _| app_seen.contains(name.as_str()));

        // The cgroup list only changes on the 5 s scan; sampling it every tick
        // would record the same value five times and shrink the window.
        if snap.scan_seq != self.last_scan_seq && !snap.cgroups.is_empty() {
            self.last_scan_seq = snap.scan_seq;
            let mut cg_seen: std::collections::HashSet<&str> =
                std::collections::HashSet::with_capacity(snap.cgroups.len());
            for cg in &snap.cgroups {
                cg_seen.insert(cg.path.as_str());
                let h = self.cgroup_history.entry(cg.path.clone()).or_default();
                h.push_back((cg.psi_some10.max(0.0) * 10.0).round() as u64);
                while h.len() > HISTORY {
                    h.pop_front();
                }
                let o = self.oom_history.entry(cg.path.clone()).or_default();
                o.push_back(cg.oom_recent);
                while o.len() > HISTORY {
                    o.pop_front();
                }
            }
            self.cgroup_history
                .retain(|path, _| cg_seen.contains(path.as_str()));
            self.oom_history
                .retain(|path, _| cg_seen.contains(path.as_str()));
        }
        self.snap = Some(snap);
        // A different process means a different mapping tree: reset expansion.
        let pid = self
            .snap
            .as_ref()
            .and_then(|s| s.detail.as_ref())
            .map(|d| d.pid);
        if pid != self.detail_pid {
            self.detail_pid = pid;
            self.expanded.clear();
            self.detail_sel = 0;
        }
        if let Some(pid) = self.pending_focus
            && let Some(i) = self.visible().iter().position(|r| r.pid == pid)
        {
            self.sel = i;
            self.pending_focus = None;
        }
        self.clamp();
    }

    /// Flattened detail rows: every matching group, followed by its VMAs when
    /// expanded. Selection indexes into this vector.
    pub fn detail_rows(&self) -> Vec<DetailRow<'_>> {
        let needle = self.detail_filter.to_lowercase();
        let Some(d) = self.snap.as_ref().and_then(|s| s.detail.as_ref()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(d.report.groups.len());
        for g in &d.report.groups {
            if !needle.is_empty() && !g.label.to_lowercase().contains(&needle) {
                continue;
            }
            out.push(DetailRow::Group(g));
            if self.expanded.contains(&g.label) {
                out.extend(g.vmas.iter().map(|v| DetailRow::Vma(g, v)));
            }
        }
        out
    }

    pub fn detail_len(&self) -> usize {
        self.detail_rows().len()
    }

    fn toggle_expand(&mut self) {
        let label = match self.detail_rows().get(self.detail_sel) {
            Some(DetailRow::Group(g)) => g.label.clone(),
            _ => return,
        };
        if !self.expanded.remove(&label) {
            self.expanded.insert(label);
        }
    }

    fn clamp(&mut self) {
        let n_proc = self.visible().len();
        self.sel = self.sel.min(n_proc.saturating_sub(1));
        let n_cg = self.visible_cgroups().len();
        self.cgroup_sel = self.cgroup_sel.min(n_cg.saturating_sub(1));
        let n_members = self.cgroup_members().len();
        self.drill_sel = self.drill_sel.min(n_members.saturating_sub(1));
        let n_apps = self.visible_apps().len();
        self.app_sel = self.app_sel.min(n_apps.saturating_sub(1));
        self.detail_sel = self.detail_sel.min(self.detail_len().saturating_sub(1));
        self.table_state.select(Some(self.sel));
        self.cgroup_state.select(Some(self.cgroup_sel));
        self.app_state.select(Some(self.app_sel));
        self.drill_state.select(Some(self.drill_sel));
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
            KeyCode::Tab => {
                self.view = match self.view {
                    View::Procs => View::Cgroups,
                    View::Cgroups => View::Apps,
                    View::Apps => View::Procs,
                };
                self.detail = false;
                self.drill = None;
                self.editing = false;
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_sel(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_sel(-1),
            KeyCode::PageDown => self.move_sel(10),
            KeyCode::PageUp => self.move_sel(-10),
            KeyCode::Char('g') | KeyCode::Home => {
                self.sel = 0;
                self.detail_sel = 0;
                self.cgroup_sel = 0;
                self.drill_sel = 0;
                self.app_sel = 0;
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.sel = usize::MAX / 2;
                self.cgroup_sel = usize::MAX / 2;
                self.drill_sel = usize::MAX / 2;
                self.app_sel = usize::MAX / 2;
            }
            KeyCode::Char('s') => {
                if self.view == View::Apps {
                    self.app_metric = self.app_metric.next();
                    self.app_sel = 0;
                } else if self.view == View::Cgroups {
                    self.cgroup_sort = self.cgroup_sort.next();
                    self.cgroup_sel = 0;
                } else {
                    self.sort = self.sort.next();
                    self.sel = 0;
                }
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
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.view == View::Apps {
                    let target = self
                        .selected_app()
                        .filter(|g| !g.synthetic)
                        .map(|g| (g.name.clone(), g.pids.clone()));
                    if let Some((name, pids)) = target {
                        self.pid_filter = Some(pids.into_iter().collect());
                        self.pid_filter_name = Some(name);
                        self.view = View::Procs;
                        self.sel = 0;
                        self.detail = false;
                    }
                } else if self.view == View::Cgroups {
                    // Enter opens the process list of the selected subtree.
                    if self.drill.is_none()
                        && let Some(path) = self
                            .visible_cgroups()
                            .get(self.cgroup_sel)
                            .map(|c| c.path.clone())
                    {
                        self.drill = Some(path);
                        self.drill_sel = 0;
                    }
                } else if self.detail {
                    // Enter expands a mapping into its individual VMAs; Esc is
                    // the way back (standard tree navigation).
                    self.toggle_expand();
                } else if self.selected_pid().is_some() {
                    self.detail = true;
                    self.detail_sel = 0;
                }
            }
            KeyCode::Char('z') if self.detail => self.expanded.clear(),
            KeyCode::Char('Z') if self.detail => {
                if let Some(d) = self.snap.as_ref().and_then(|s| s.detail.as_ref()) {
                    self.expanded
                        .extend(d.report.groups.iter().map(|g| g.label.clone()));
                }
            }
            KeyCode::Esc => {
                if self.pid_filter.is_some() {
                    self.pid_filter = None;
                    self.pid_filter_name = None;
                    self.sel = 0;
                } else if self.view == View::Cgroups && self.drill.is_some() {
                    self.drill = None;
                    self.drill_sel = 0;
                } else if self.detail && !self.detail_filter.is_empty() {
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
        if self.view == View::Apps {
            let n = self.visible_apps().len();
            self.app_sel = step(self.app_sel, delta, n);
            return;
        }
        if self.view == View::Cgroups {
            if self.drill.is_some() {
                let n = self.cgroup_members().len();
                self.drill_sel = step(self.drill_sel, delta, n);
            } else {
                let n = self.visible_cgroups().len();
                self.cgroup_sel = step(self.cgroup_sel, delta, n);
            }
            return;
        }
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
