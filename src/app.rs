use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::widgets::TableState;

use crate::apps::AppGroup;
use crate::cgroup::CgroupMem;
use crate::i18n::{HelpPage, Lang};
use crate::model::{Focus, ProcRow, Snapshot, SortKey};
use crate::procfs::{Vma, VmaGroup};
use crate::theme::{ColorSupport, Theme};
use crate::ui;

pub const DEFAULT_HISTORY: u64 = 120;
pub const MAX_HISTORY: u64 = 3600;

#[derive(Clone, Copy, Debug)]
pub struct HistorySample<T> {
    pub at: Instant,
    pub value: T,
}

fn expire<T>(history: &mut VecDeque<HistorySample<T>>, now: Instant, window: Duration) {
    while history
        .front()
        .is_some_and(|s| now.saturating_duration_since(s.at) > window)
    {
        history.pop_front();
    }
}

fn record<T>(history: &mut VecDeque<HistorySample<T>>, at: Instant, origin: Instant, value: T) {
    let slot = at.saturating_duration_since(origin).as_secs();
    if history
        .back()
        .is_some_and(|s| s.at.saturating_duration_since(origin).as_secs() == slot)
    {
        history.pop_back();
    }
    history.push_back(HistorySample { at, value });
}

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

/// An open help overlay: which page's documentation is shown and how far it
/// has been scrolled.
pub struct HelpState {
    pub page: HelpPage,
    pub scroll: u16,
}

/// A clickable footer action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FooterAction {
    View,
    Sort,
    Reverse,
    Filter,
    Enter,
    Back,
    Theme,
    Lang,
    Help,
    Quit,
}

/// What a mouse hit region refers to.
#[derive(Clone, Copy, Debug)]
pub enum MouseTarget {
    Row(usize),
    Tile(usize),
    Button(FooterAction),
    HelpArea,
}

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub rect: Rect,
    pub target: MouseTarget,
}

pub struct App {
    pub view: View,
    /// Active built-in theme; `t` cycles it.
    pub theme: Theme,
    /// Terminal colour depth the active palette was rendered for.
    pub support: ColorSupport,
    /// UI language; `l` toggles it.
    pub lang: Lang,
    /// Open help overlay, if any.
    pub help: Option<HelpState>,
    /// Click targets recorded by the last draw.
    pub hits: Vec<Hit>,
    /// Content area from the last frame, used to calculate page movement.
    pub(crate) page_area: Rect,
    pub(crate) help_page_rows: u16,
    pub(crate) help_max_scroll: u16,
    pub sort: SortKey,
    pub cgroup_sort: CgroupSort,
    pub cgroup_sel: usize,
    /// App groups for the treemap, recomputed per snapshot (collapsed to the
    /// top few percent so the map does not dissolve into slivers).
    pub app_groups: Vec<AppGroup>,
    /// Metric history per app name (only the collapsed groups are tracked:
    /// ~20-40 entries, not all 330).
    pub app_history: HashMap<String, VecDeque<HistorySample<AppSample>>>,
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
    pub history: HashMap<u32, VecDeque<HistorySample<u64>>>,
    /// PSI some avg10, percent*10, recorded only when a new scan arrives.
    pub cgroup_history: HashMap<String, VecDeque<HistorySample<u64>>>,
    /// OOM increments, recorded once per new scan.
    pub oom_history: HashMap<String, VecDeque<HistorySample<u64>>>,
    pub history_seconds: u64,
    history_origin: Option<Instant>,
    last_scan_seq: Option<u64>,
    last_sent: Option<(Option<u32>, bool)>,
    started: bool,
    pending_focus: Option<u32>,
    detail_pid: Option<u32>,
}

impl App {
    pub fn new(start_pid: Option<u32>) -> Self {
        Self::with_history(start_pid, DEFAULT_HISTORY)
    }

    pub fn with_history(start_pid: Option<u32>, history_seconds: u64) -> Self {
        assert!((1..=MAX_HISTORY).contains(&history_seconds));
        let mut app = App {
            view: View::Procs,
            theme: Theme::DEFAULT,
            support: ColorSupport::TrueColor,
            lang: Lang::En,
            help: None,
            hits: Vec::new(),
            page_area: Rect::default(),
            help_page_rows: 10,
            help_max_scroll: u16::MAX,
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
            history_seconds,
            history_origin: None,
            last_scan_seq: None,
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
        let now = snap.at;
        let origin = *self.history_origin.get_or_insert(now);
        let window = Duration::from_secs(self.history_seconds);
        for h in self.cgroup_history.values_mut() {
            expire(h, now, window);
        }
        for h in self.oom_history.values_mut() {
            expire(h, now, window);
        }
        let mut seen: Vec<u32> = Vec::with_capacity(snap.rows.len());
        for row in &snap.rows {
            seen.push(row.pid);
            let h = self.history.entry(row.pid).or_default();
            expire(h, now, window);
            record(h, now, origin, row.rss_kb);
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
            expire(h, now, window);
            record(
                h,
                now,
                origin,
                AppSample {
                    pss_kb: g.pss_kb,
                    uss_kb: g.uss_kb,
                    rss_kb: g.rss_kb,
                },
            );
        }
        self.app_history
            .retain(|name, _| app_seen.contains(name.as_str()));

        // A repeated snapshot carries the previous scan, not a new measurement.
        if Some(snap.scan_seq) != self.last_scan_seq {
            self.last_scan_seq = Some(snap.scan_seq);
            let mut cg_seen: std::collections::HashSet<&str> =
                std::collections::HashSet::with_capacity(snap.cgroups.len());
            for cg in &snap.cgroups {
                cg_seen.insert(cg.path.as_str());
                let h = self.cgroup_history.entry(cg.path.clone()).or_default();
                h.push_back(HistorySample {
                    at: now,
                    value: (cg.psi_some10.max(0.0) * 10.0).round() as u64,
                });
                let o = self.oom_history.entry(cg.path.clone()).or_default();
                o.push_back(HistorySample {
                    at: now,
                    value: cg.oom_recent,
                });
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

    fn help_page(&self) -> HelpPage {
        match self.view {
            View::Procs if self.detail => HelpPage::Detail,
            View::Procs => HelpPage::Procs,
            View::Cgroups if self.drill.is_some() => HelpPage::CgroupDrill,
            View::Cgroups => HelpPage::Cgroups,
            View::Apps => HelpPage::Apps,
        }
    }

    fn help_scroll(&mut self, delta: i32) {
        if let Some(help) = &mut self.help {
            let next = if delta >= 0 {
                help.scroll.saturating_add(delta as u16)
            } else {
                help.scroll.saturating_sub((-delta) as u16)
            };
            help.scroll = next.min(self.help_max_scroll);
        }
    }

    fn cycle_view(&mut self) {
        self.view = match self.view {
            View::Procs => View::Cgroups,
            View::Cgroups => View::Apps,
            View::Apps => View::Procs,
        };
        self.detail = false;
        self.drill = None;
        self.editing = false;
    }

    fn cycle_sort(&mut self) {
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

    fn start_filter(&mut self) {
        self.editing = true;
        self.editing_detail = self.detail;
        self.edit_buf = if self.detail {
            self.detail_filter.clone()
        } else {
            self.filter.clone()
        };
    }

    fn toggle_reverse(&mut self) {
        self.desc = !self.desc;
    }

    fn toggle_theme(&mut self) {
        self.theme = self.theme.next().rendered(self.support);
    }

    fn toggle_lang(&mut self) {
        self.lang = self.lang.toggle();
    }

    fn open_help(&mut self) {
        self.help_max_scroll = u16::MAX;
        self.help = Some(HelpState {
            page: self.help_page(),
            scroll: 0,
        });
    }

    fn enter_action(&mut self) {
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
            self.toggle_expand();
        } else if self.selected_pid().is_some() {
            self.detail = true;
            self.detail_sel = 0;
        }
    }

    fn escape_action(&mut self) {
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

    fn click_row(&mut self, index: usize) {
        match self.view {
            View::Procs if self.detail => {
                if self.detail_sel == index {
                    self.enter_action();
                } else {
                    self.detail_sel = index;
                }
            }
            View::Procs => {
                if self.sel == index {
                    self.enter_action();
                } else {
                    self.sel = index;
                }
            }
            View::Cgroups if self.drill.is_some() => {
                self.drill_sel = index;
            }
            View::Cgroups => {
                if self.cgroup_sel == index {
                    self.enter_action();
                } else {
                    self.cgroup_sel = index;
                }
            }
            View::Apps => {
                if self.app_sel == index {
                    self.enter_action();
                } else {
                    self.app_sel = index;
                }
            }
        }
    }

    fn click_tile(&mut self, index: usize) {
        if self.app_sel == index {
            self.enter_action();
        } else {
            self.app_sel = index;
        }
    }

    fn footer_action(&mut self, action: FooterAction) -> bool {
        match action {
            FooterAction::Quit => return true,
            FooterAction::Help => self.open_help(),
            FooterAction::Lang => self.toggle_lang(),
            FooterAction::Theme => self.toggle_theme(),
            FooterAction::View => self.cycle_view(),
            FooterAction::Sort => self.cycle_sort(),
            FooterAction::Reverse => self.toggle_reverse(),
            FooterAction::Filter => self.start_filter(),
            FooterAction::Enter => self.enter_action(),
            FooterAction::Back => self.escape_action(),
        }
        self.clamp();
        false
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) -> bool {
        if self.editing {
            return false;
        }
        if self.help.is_some() {
            match mouse.kind {
                MouseEventKind::ScrollDown => self.help_scroll(3),
                MouseEventKind::ScrollUp => self.help_scroll(-3),
                MouseEventKind::Down(MouseButton::Left) => {
                    let inside = self.hits.iter().any(|hit| {
                        matches!(hit.target, MouseTarget::HelpArea)
                            && mouse.column >= hit.rect.x
                            && mouse.column < hit.rect.x.saturating_add(hit.rect.width)
                            && mouse.row >= hit.rect.y
                            && mouse.row < hit.rect.y.saturating_add(hit.rect.height)
                    });
                    if !inside {
                        self.help = None;
                    }
                }
                _ => {}
            }
            return false;
        }
        match mouse.kind {
            MouseEventKind::ScrollDown => self.move_sel(3),
            MouseEventKind::ScrollUp => self.move_sel(-3),
            MouseEventKind::Down(MouseButton::Left) => {
                let target = self
                    .hits
                    .iter()
                    .rev()
                    .find(|hit| {
                        let r = hit.rect;
                        mouse.column >= r.x
                            && mouse.column < r.x.saturating_add(r.width)
                            && mouse.row >= r.y
                            && mouse.row < r.y.saturating_add(r.height)
                    })
                    .map(|hit| hit.target);
                match target {
                    Some(MouseTarget::Row(index)) => self.click_row(index),
                    Some(MouseTarget::Tile(index)) => self.click_tile(index),
                    Some(MouseTarget::Button(action)) => return self.footer_action(action),
                    Some(MouseTarget::HelpArea) | None => return false,
                }
            }
            _ => return false,
        }
        self.clamp();
        false
    }

    pub fn handle_event(&mut self, event: Event) -> bool {
        match event {
            Event::Key(key) => self.on_key(key),
            Event::Mouse(mouse) => self.on_mouse(mouse),
            _ => false,
        }
    }

    /// Returns true when the app should exit.
    pub fn on_key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }
        if self.help.is_some() {
            if let Some((direction, half)) = page_key(&key) {
                let distance = page_distance(self.help_page_rows as usize, half);
                self.help_scroll((direction * distance) as i32);
                return false;
            }
            match key.code {
                KeyCode::Char('q') => return true,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
                KeyCode::Esc | KeyCode::Char('?') => self.help = None,
                KeyCode::Char('j') | KeyCode::Down => self.help_scroll(1),
                KeyCode::Char('k') | KeyCode::Up => self.help_scroll(-1),
                KeyCode::Char('g') | KeyCode::Home => {
                    if let Some(help) = &mut self.help {
                        help.scroll = 0;
                    }
                }
                KeyCode::Char('G') | KeyCode::End => {
                    if let Some(help) = &mut self.help {
                        help.scroll = self.help_max_scroll;
                    }
                }
                KeyCode::Char('l') => self.lang = self.lang.toggle(),
                _ => {}
            }
            return false;
        }
        if self.editing {
            return self.on_edit_key(key);
        }
        if let Some((direction, half)) = page_key(&key) {
            let rows = ui::page_rows(self);
            let distance = page_distance(rows, half);
            self.move_page(direction * distance, rows);
            self.clamp();
            return false;
        }
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('?') => self.open_help(),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,
            KeyCode::Tab => self.cycle_view(),
            KeyCode::Char('j') | KeyCode::Down => self.move_sel(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_sel(-1),
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
            KeyCode::Char('s') => self.cycle_sort(),
            KeyCode::Char('r') => self.toggle_reverse(),
            KeyCode::Char('t') => self.toggle_theme(),
            KeyCode::Char('l') => self.toggle_lang(),
            KeyCode::Char('f' | '/') => self.start_filter(),
            KeyCode::Enter | KeyCode::Char(' ') => self.enter_action(),
            KeyCode::Char('z') if self.detail => self.expanded.clear(),
            KeyCode::Char('Z') if self.detail => {
                if let Some(d) = self.snap.as_ref().and_then(|s| s.detail.as_ref()) {
                    self.expanded
                        .extend(d.report.groups.iter().map(|g| g.label.clone()));
                }
            }
            KeyCode::Esc => self.escape_action(),
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

    fn move_page(&mut self, delta: isize, rows: usize) {
        let count = match (self.view, self.detail, self.drill.is_some()) {
            (View::Apps, _, _) => self.visible_apps().len(),
            (View::Cgroups, _, true) => self.cgroup_members().len(),
            (View::Cgroups, _, false) => self.visible_cgroups().len(),
            (View::Procs, true, _) => self.detail_len(),
            (View::Procs, false, _) => self.visible().len(),
        };
        let (selected, state) = match (self.view, self.detail, self.drill.is_some()) {
            (View::Apps, _, _) => (&mut self.app_sel, &mut self.app_state),
            (View::Cgroups, _, true) => (&mut self.drill_sel, &mut self.drill_state),
            (View::Cgroups, _, false) => (&mut self.cgroup_sel, &mut self.cgroup_state),
            (View::Procs, true, _) => (&mut self.detail_sel, &mut self.detail_state),
            (View::Procs, false, _) => (&mut self.sel, &mut self.table_state),
        };
        let current = (*selected).min(count.saturating_sub(1));
        let offset = state.offset().min(current).min(count.saturating_sub(rows));
        let next = step(current, delta, count);
        *selected = next;
        *state.offset_mut() = offset
            .saturating_add_signed(delta)
            .min(next)
            .min(count.saturating_sub(rows));
    }

    pub fn run(
        mut self,
        terminal: &mut DefaultTerminal,
        tx: std::sync::mpsc::Sender<Focus>,
        rx: std::sync::mpsc::Receiver<Snapshot>,
    ) -> std::io::Result<()> {
        loop {
            let mut got = false;
            while let Ok(snap) = rx.try_recv() {
                self.on_snapshot(snap);
                got = true;
            }
            if got || self.started {
                terminal.draw(|f| ui::draw(f, &mut self))?;
                self.started = true;
            }
            if let Some(focus) = self.take_focus_update() {
                let _ = tx.send(focus);
            }
            if event::poll(Duration::from_millis(250))? {
                if self.handle_event(event::read()?) {
                    return Ok(());
                }
                while event::poll(Duration::ZERO)? {
                    if self.handle_event(event::read()?) {
                        return Ok(());
                    }
                }
                if let Some(focus) = self.take_focus_update() {
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

fn page_key(key: &KeyEvent) -> Option<(isize, bool)> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::PageDown => Some((1, false)),
        KeyCode::PageUp => Some((-1, false)),
        KeyCode::Char('d') if ctrl => Some((1, true)),
        KeyCode::Char('u') if ctrl => Some((-1, true)),
        KeyCode::Char('f') if ctrl => Some((1, false)),
        KeyCode::Char('b') if ctrl => Some((-1, false)),
        _ => None,
    }
}

fn page_distance(rows: usize, half: bool) -> isize {
    (if half { rows / 2 } else { rows }).max(1) as isize
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
