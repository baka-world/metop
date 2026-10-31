use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Sparkline, Table};
use ratatui::Frame;

use crate::app::{App, DetailRow, View};
use crate::model::{ProcRow, SortKey, PSS_MAX_AGE};
use crate::procfs::fmt_kb;

const SHARE_W: usize = 6;
const MIX_W: usize = 12;

/// Keep the tail of a path: "/user.slice/…/app.slice/terminal.scope" is far
/// more useful than "/user.slice/user-1000.slice/user@1000.serv…".
fn truncate_left(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n {
        s.to_string()
    } else {
        format!("…{}", s.chars().skip(count - (n - 1)).collect::<String>())
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(6),
        Constraint::Length(2),
    ])
    .split(area);

    header(f, chunks[0], app);
    let has_detail = app.snap.as_ref().is_some_and(|s| s.detail.is_some());
    let detail_error = app.snap.as_ref().and_then(|s| s.detail_error.clone());
    match (app.snap.is_some(), app.detail, app.view) {
        (true, true, View::Procs) if has_detail => detail_view(f, chunks[1], app),
        (true, true, View::Procs) => {
            let cause = detail_error.unwrap_or_else(|| "waiting for sampler…".into());
            f.render_widget(
                Paragraph::new(format!("vm mappings unavailable\n\n{cause}"))
                    .wrap(ratatui::widgets::Wrap { trim: true })
                    .block(Block::new().borders(Borders::ALL).title(" detail ")),
                chunks[1],
            );
        }
        (true, _, View::Cgroups) => {
            if app.drill.is_some() {
                cgroup_drill(f, chunks[1], app);
            } else {
                cgroup_view(f, chunks[1], app);
            }
        }
        (true, _, _) => table_view(f, chunks[1], app),
        (false, ..) => {
            f.render_widget(
                Paragraph::new("sampling /proc…")
                    .alignment(Alignment::Center)
                    .block(Block::new().borders(Borders::ALL).title(" mtop ")),
                chunks[1],
            );
        }
    }
    footer(f, chunks[2], app);
}

fn header(f: &mut Frame, area: Rect, app: &App) {
    let cols = Layout::horizontal([Constraint::Min(48), Constraint::Length(36)]).split(area);
    let Some(snap) = &app.snap else {
        f.render_widget(Paragraph::new("mtop"), cols[0]);
        return;
    };
    let used = snap.mem.total_kb.saturating_sub(snap.mem.avail_kb);
    let used_frac = if snap.mem.total_kb == 0 {
        0.0
    } else {
        used as f64 / snap.mem.total_kb as f64
    };
    let mem_color = if used_frac > 0.9 {
        Color::Red
    } else if used_frac > 0.75 {
        Color::Yellow
    } else {
        Color::Green
    };
    let swap_used = snap.mem.swap_total_kb.saturating_sub(snap.mem.swap_free_kb);
    let line1 = Line::from(vec![
        Span::styled(" mtop ", Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED)),
        Span::raw(format!(" {} procs  mem ", snap.rows.len())),
        Span::styled(
            format!("{}/{}", fmt_kb(used), fmt_kb(snap.mem.total_kb)),
            Style::new().fg(mem_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  avail {}  cached {}  swap {}/{}",
            fmt_kb(snap.mem.avail_kb),
            fmt_kb(snap.mem.cached_kb),
            fmt_kb(swap_used),
            fmt_kb(snap.mem.swap_total_kb)
        )),
    ]);
    let denied = snap.denied.len();
    let eligible = snap.rows.len().saturating_sub(denied);
    let mut line2 = vec![
        Span::styled(
            format!(" sweep {:.0}ms ", snap.sweep_ms),
            Style::new().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("pss {:.0}ms ", snap.pss_ms),
            Style::new().fg(Color::DarkGray),
        ),
        Span::styled(
            format!(
                "coverage {}/{} = {} ",
                snap.covered,
                eligible,
                fmt_kb(snap.covered_pss_kb)
            ),
            Style::new().fg(Color::DarkGray),
        ),
    ];
    if denied > 0 {
        line2.push(Span::styled(
            format!("{denied} n/a (sudo for full) "),
            Style::new().fg(Color::Yellow),
        ));
    }
    line2.push(Span::styled(
        format!("age {}ms", app.sample_age_ms()),
        Style::new().fg(Color::DarkGray),
    ));
    let line2 = Line::from(line2);
    f.render_widget(
        Paragraph::new(vec![line1, line2]).block(Block::new().borders(Borders::TOP | Borders::BOTTOM)),
        cols[0],
    );

    // RSS trend of the focused process.
    let (label, data): (String, Vec<u64>) = match app.view {
        View::Cgroups => match app.visible_cgroups().get(app.cgroup_sel) {
            Some(cg) => (
                format!(" {} {}", truncate_left(&cg.path, 26), fmt_kb(cg.current_kb)),
                Vec::new(),
            ),
            None => (" cgroups".into(), Vec::new()),
        },
        View::Procs => match app.selected_pid() {
        Some(pid) => {
            let hist = app.history.get(&pid).cloned().unwrap_or_default();
            let rss = hist.back().copied().unwrap_or(0);
            let comm = app
                .snap
                .as_ref()
                .and_then(|s| s.rows.iter().find(|r| r.pid == pid))
                .map(|r| r.comm.clone())
                .unwrap_or_default();
            (
                format!(" {} {} {}", pid, truncate(&comm, 12), fmt_kb(rss)),
                hist.into_iter().collect(),
            )
        }
        None => (" rss trend".into(), Vec::new()),
        },
    };
    let max = data.iter().copied().max().unwrap_or(1).max(1);
    f.render_widget(
        Sparkline::default()
            .data(data.as_slice())
            .max(max)
            .style(Style::new().fg(Color::Cyan))
            .block(Block::new().borders(Borders::ALL).title(label)),
        cols[1],
    );
}

fn bar_cell(frac: f64, color: Color, width: usize) -> Cell<'static> {
    let n = ((frac.clamp(0.0, 1.0)) * width as f64).round() as usize;
    let mut filled: String = "█".repeat(n);
    filled.push_str(&" ".repeat(width - n));
    Cell::from(Text::from(Line::from(Span::styled(
        filled,
        Style::new().fg(color),
    ))))
    .style(Style::new().fg(Color::DarkGray))
}

/// Split `width` cells across `values` proportionally; the last segment takes
/// the rounding remainder so the widths always sum to exactly `width`.
pub fn segments(values: &[u64], width: usize) -> Vec<usize> {
    let mut out = vec![0usize; values.len()];
    // u128: summing kB fields can overflow u64 (the test suite pins this).
    let total: u128 = values.iter().map(|v| *v as u128).sum();
    if total == 0 || width == 0 || values.is_empty() {
        return out;
    }
    let mut used = 0usize;
    for (i, v) in values.iter().enumerate() {
        let w = if i + 1 == values.len() {
            width - used
        } else {
            ((*v as u128 as f64 / total as f64) * width as f64).round() as usize
        };
        let w = w.min(width - used);
        out[i] = w;
        used += w;
    }
    out
}

fn segmented_cell(values: &[u64], colors: &[Color]) -> Cell<'static> {
    let widths = segments(values, MIX_W);
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(values.len() + 1);
    let mut used = 0;
    for (w, color) in widths.iter().zip(colors) {
        if *w > 0 {
            spans.push(Span::styled("█".repeat(*w), Style::new().fg(*color)));
            used += w;
        }
    }
    if used < MIX_W {
        spans.push(Span::raw(" ".repeat(MIX_W - used)));
    }
    Cell::from(Text::from(Line::from(spans)))
}

/// Stacked PSS composition: anonymous / file-backed / shmem / swapped-out.
/// This is the part htop cannot show — it answers "is this heap or page cache".
fn mix_cell(anon_kb: u64, file_kb: u64, shmem_kb: u64, swap_kb: u64) -> Cell<'static> {
    segmented_cell(
        &[anon_kb, file_kb, shmem_kb, swap_kb],
        &[Color::Magenta, Color::Cyan, Color::Yellow, Color::Blue],
    )
}

/// Private (only this process pays) vs shared (PSS-weighted) pages, with the
/// total length scaled by `frac` so magnitude and split are both visible.
fn split_cell_scaled(private_kb: u64, shared_kb: u64, frac: f64) -> Cell<'static> {
    let filled = ((frac.clamp(0.0, 1.0)) * MIX_W as f64).round() as usize;
    let widths = segments(&[private_kb, shared_kb], filled);
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(3);
    for (w, color) in widths.iter().zip([Color::Magenta, Color::Cyan]) {
        if *w > 0 {
            spans.push(Span::styled("█".repeat(*w), Style::new().fg(color)));
        }
    }
    if filled < MIX_W {
        spans.push(Span::raw(" ".repeat(MIX_W - filled)));
    }
    Cell::from(Text::from(Line::from(spans)))
}

fn delta_span(d: i64) -> Span<'static> {
    if d == 0 {
        return Span::styled("     0", Style::new().fg(Color::DarkGray));
    }
    let kb = d.unsigned_abs();
    let sign = if d > 0 { "+" } else { "-" };
    let color = if d > 0 { Color::Red } else { Color::Green };
    Span::styled(
        format!("{sign}{:>6}", fmt_kb(kb)),
        Style::new().fg(color),
    )
}

fn table_view(f: &mut Frame, area: Rect, app: &mut App) {
    let mut state = std::mem::take(&mut app.table_state);
    let Some(snap) = &app.snap else {
        app.table_state = state;
        return;
    };
    let visible = app.visible();
    let max_rss = visible.iter().map(|r| r.rss_kb).max().unwrap_or(1).max(1);
    let total = snap.mem.total_kb.max(1);

    let rows: Vec<Row> = visible
        .iter()
        .map(|r: &&ProcRow| {
            let pss = snap.pss.get(&r.pid);
            let pss_kb = pss.map(|p| p.rollup.pss_kb);
            let uss_kb = pss.map(|p| p.rollup.uss_kb);
            // Older than the freshness window: last known value, suffixed with
            // '~' so a stale number can never be mistaken for a live one.
            let stale = pss.is_some_and(|p| p.at.elapsed() > PSS_MAX_AGE);
            let denied = snap.denied.contains(&r.pid);
            let ratio = pss.filter(|_| r.rss_kb > 0).map(|p| p.rollup.pss_kb as f64 / r.rss_kb as f64);
            let ratio_text = match ratio {
                Some(v) => format!("{:>6.0}%", v * 100.0),
                None => "     —".to_string(),
            };
            let ratio_color = match ratio {
                Some(_) if stale => Color::DarkGray,
                Some(v) if v > 0.8 => Color::Yellow,
                Some(_) => Color::DarkGray,
                None => Color::DarkGray,
            };
            let frac = r.rss_kb as f64 / max_rss as f64;
            let mem_frac = r.rss_kb as f64 / total as f64;
            let mix = pss.map(|p| {
                (
                    p.rollup.pss_anon_kb,
                    p.rollup.pss_file_kb,
                    p.rollup.pss_shmem_kb,
                    p.rollup.swap_pss_kb,
                )
            });
            let bar_color = if mem_frac > 0.15 {
                Color::Red
            } else if mem_frac > 0.05 {
                Color::Yellow
            } else {
                Color::Cyan
            };
            Row::new(vec![
                Cell::from(format!("{:>7}", r.pid)),
                Cell::from(truncate(&r.comm, 20)),
                Cell::from(format!("{:>9}", fmt_kb(r.rss_kb))),
                Cell::from(match pss_kb {
                    Some(v) if stale => Text::from(Span::styled(
                        format!("{:>8}~", fmt_kb(v)),
                        Style::new().fg(Color::DarkGray),
                    )),
                    Some(v) => Text::from(format!("{:>9}", fmt_kb(v))),
                    None if denied => {
                        Text::from(Span::styled("      n/a", Style::new().fg(Color::DarkGray)))
                    }
                    None => Text::from(Span::styled("        —", Style::new().fg(Color::DarkGray))),
                }),
                Cell::from(match uss_kb {
                    Some(v) if stale => Text::from(Span::styled(
                        format!("{:>8}~", fmt_kb(v)),
                        Style::new().fg(Color::DarkGray),
                    )),
                    Some(v) => Text::from(format!("{:>9}", fmt_kb(v))),
                    None if denied => {
                        Text::from(Span::styled("      n/a", Style::new().fg(Color::DarkGray)))
                    }
                    None => Text::from(Span::styled("        —", Style::new().fg(Color::DarkGray))),
                }),
                Cell::from(Text::from(Span::styled(ratio_text, Style::new().fg(ratio_color)))),
                Cell::from(Text::from(delta_span(r.delta_kb))),
                bar_cell(frac, bar_color, SHARE_W),
                match mix {
                    Some((a, f, sh, sw)) => mix_cell(a, f, sh, sw),
                    None => Cell::from(Span::raw(" ".repeat(MIX_W))),
                },
            ])
        })
        .collect();

    let header = Row::new(vec![
        Cell::from(format!("{:>7}", "PID")),
        Cell::from(format!("{:<20}", "COMM")),
        Cell::from(format!("{:>9}", "RSS")),
        Cell::from(format!("{:>9}", "PSS")),
        Cell::from(format!("{:>9}", "USS")),
        Cell::from(format!("{:>7}", "PSS/RSS")),
        Cell::from(format!("{:>7}", "Δ/s")),
        Cell::from(" SHARE"),
        Cell::from(" PSS MIX"),
    ])
    .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    // A PSS/USS ranking is only meaningful once the rollup cache has been
    // filled; say so instead of silently ranking unsampled rows as zero.
    let rank_incomplete = matches!(app.sort, SortKey::Pss | SortKey::Uss | SortKey::Ratio)
        && snap.covered + snap.denied.len() < snap.rows.len();
    let mut sort_label = format!(
        " procs  sort {} {}  filter {} ",
        app.sort.label(),
        if app.desc { "↓" } else { "↑" },
        if app.filter.is_empty() { "-" } else { app.filter.as_str() }
    );
    if rank_incomplete {
        sort_label.push_str(&format!(
            " pss {}/{} · ranking incomplete ",
            snap.covered,
            snap.rows.len().saturating_sub(snap.denied.len())
        ));
    }
    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Length(20),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(SHARE_W as u16),
            Constraint::Length(MIX_W as u16),
        ],
    )
    .header(header)
    .block(Block::new().borders(Borders::ALL).title(sort_label))
    .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
    .highlight_symbol("▸");

    f.render_stateful_widget(table, area, &mut state);
    app.table_state = state;
}

fn cgroup_drill(f: &mut Frame, area: Rect, app: &mut App) {
    let mut state = std::mem::take(&mut app.drill_state);
    let (Some(snap), Some(path)) = (app.snap.as_ref(), app.drill.clone()) else {
        app.drill_state = state;
        return;
    };
    let members = app.cgroup_members();
    let cg = snap.cgroups.iter().find(|c| c.path == path);
    let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(4)]).split(area);

    let summary = Line::from(vec![
        Span::styled(
            format!(" {} ", truncate_left(&path, 60)),
            Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED),
        ),
        Span::raw(format!(
            " current {} · {} procs in subtree · limit {} ",
            cg.map(|c| fmt_kb(c.current_kb)).unwrap_or_else(|| "—".into()),
            members.len(),
            cg.and_then(|c| c.limit_kb)
                .map(fmt_kb)
                .unwrap_or_else(|| "max".into())
        )),
    ]);
    f.render_widget(
        Paragraph::new(summary).block(Block::new().borders(Borders::BOTTOM)),
        chunks[0],
    );

    // Scale the share bar against the largest member, not total RAM: a 500 MB
    // process in a 15 GB machine would otherwise round to zero cells.
    let max_rss = members.iter().map(|r| r.rss_kb).max().unwrap_or(1).max(1);
    let rows: Vec<Row> = members
        .iter()
        .map(|r| {
            let pss = snap.pss.get(&r.pid);
            let ratio = pss
                .filter(|_| r.rss_kb > 0)
                .map(|p| p.rollup.pss_kb as f64 / r.rss_kb as f64);
            let frac = r.rss_kb as f64 / max_rss as f64;
            let color = if frac > 0.15 {
                Color::Red
            } else if frac > 0.05 {
                Color::Yellow
            } else {
                Color::Cyan
            };
            Row::new(vec![
                Cell::from(format!("{:>7}", r.pid)),
                Cell::from(truncate(&r.comm, 26)),
                Cell::from(format!("{:>9}", fmt_kb(r.rss_kb))),
                Cell::from(match pss {
                    Some(p) => Text::from(format!("{:>9}", fmt_kb(p.rollup.pss_kb))),
                    None => Text::from(Span::styled("        —", Style::new().fg(Color::DarkGray))),
                }),
                Cell::from(match ratio {
                    Some(v) => format!("{:>7.0}%", v * 100.0),
                    None => "       —".to_string(),
                }),
                bar_cell(frac, color, SHARE_W),
            ])
        })
        .collect();

    let header = Row::new(vec![
        Cell::from(format!("{:>7}", "PID")),
        Cell::from(format!("{:<26}", "COMM")),
        Cell::from(format!("{:>9}", "RSS")),
        Cell::from(format!("{:>9}", "PSS")),
        Cell::from(format!("{:>8}", "PSS/RSS")),
        Cell::from(" SHARE"),
    ])
    .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Length(26),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(8),
            Constraint::Length(SHARE_W as u16),
        ],
    )
    .header(header)
    .block(
        Block::new()
            .borders(Borders::ALL)
            .title(" procs in cgroup subtree  (Esc back) "),
    )
    .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
    .highlight_symbol("▸");

    f.render_stateful_widget(table, chunks[1], &mut state);
    app.drill_state = state;
}

fn cgroup_view(f: &mut Frame, area: Rect, app: &mut App) {
    let mut state = std::mem::take(&mut app.cgroup_state);
    let Some(snap) = &app.snap else {
        app.cgroup_state = state;
        return;
    };
    if snap.cgroups.is_empty() {
        f.render_widget(
            Paragraph::new(
                "cgroup v2 memory controller not available\n\n                 (needs /sys/fs/cgroup/memory.current; v1 or a restricted hierarchy shows nothing)",
            )
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(Block::new().borders(Borders::ALL).title(" cgroups ")),
            area,
        );
        app.cgroup_state = state;
        return;
    }

    let visible = app.visible_cgroups();
    let max_cur = snap.cgroups.iter().map(|c| c.current_kb).max().unwrap_or(1).max(1);
    let rows: Vec<Row> = visible
        .iter()
        .map(|c| {
            let (frac, over) = match c.limit_kb {
                Some(limit) if limit > 0 => (c.current_kb as f64 / limit as f64, limit),
                _ => (c.current_kb as f64 / max_cur as f64, 0),
            };
            let color = if c.oom_kills > 0 {
                Color::Red
            } else if over > 0 && frac > 0.9 {
                Color::Yellow
            } else {
                Color::Cyan
            };
            // "7▲" means: 7 kills total, at least one since the last scan.
            let (oom_text, oom_style) = if c.oom_recent > 0 {
                (
                    format!("{:>3}▲", c.oom_kills),
                    Style::new()
                        .fg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                )
            } else if c.oom_kills > 0 {
                (format!("{:>4}", c.oom_kills), Style::new().fg(Color::Red))
            } else {
                (format!("{:>4}", 0), Style::new().fg(Color::DarkGray))
            };
            Row::new(vec![
                Cell::from(truncate_left(&c.path, 46)),
                Cell::from(format!("{:>9}", fmt_kb(c.current_kb))),
                Cell::from(format!("{:>9}", fmt_kb(c.anon_kb))),
                Cell::from(format!("{:>9}", fmt_kb(c.file_kb))),
                Cell::from(format!("{:>9}", fmt_kb(c.kernel_kb))),
                Cell::from(format!("{:>9}", fmt_kb(c.slab_kb))),
                Cell::from(match c.limit_kb {
                    Some(l) => Text::from(format!("{:>9}", fmt_kb(l))),
                    None => Text::from(Span::styled("      max", Style::new().fg(Color::DarkGray))),
                }),
                Cell::from(Text::from(Span::styled(oom_text, oom_style))),
                bar_cell(frac, color, 10),
            ])
        })
        .collect();

    let header = Row::new(vec![
        Cell::from(format!("{:<46}", "CGROUP")),
        Cell::from(format!("{:>9}", "CURRENT")),
        Cell::from(format!("{:>9}", "ANON")),
        Cell::from(format!("{:>9}", "FILE")),
        Cell::from(format!("{:>9}", "KERNEL")),
        Cell::from(format!("{:>9}", "SLAB")),
        Cell::from(format!("{:>9}", "MAX")),
        Cell::from(format!("{:>4}", "OOM")),
        Cell::from(" CUR/MAX"),
    ])
    .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let recent: u64 = snap.cgroups.iter().map(|c| c.oom_recent).sum();
    let mut title = format!(
        " cgroups  sort {} {}  filter {}  ({} of {}) ",
        app.cgroup_sort.label(),
        if app.desc { "↓" } else { "↑" },
        if app.filter.is_empty() { "-" } else { app.filter.as_str() },
        visible.len(),
        snap.cgroups.len()
    );
    if recent > 0 {
        title.push_str(&format!("  oom +{recent} since last sample "));
    }
    let table = Table::new(
        rows,
        [
            Constraint::Length(46),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(4),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(Block::new().borders(Borders::ALL).title(title))
    .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
    .highlight_symbol("▸");

    f.render_stateful_widget(table, area, &mut state);
    app.cgroup_state = state;
}

fn detail_view(f: &mut Frame, area: Rect, app: &mut App) {
    let mut state = std::mem::take(&mut app.detail_state);
    let Some(d) = app.snap.as_ref().and_then(|s| s.detail.as_ref()) else {
        app.detail_state = state;
        return;
    };
    let chunks = Layout::vertical([Constraint::Length(4), Constraint::Min(4)]).split(area);
    let r = &d.report;
    let summary = vec![
        Line::from(vec![
            Span::styled(
                format!(" {} {} ", d.pid, truncate(&d.comm, 24)),
                Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED),
            ),
            Span::raw(format!(
                " RSS {}  PSS {}  USS {}  swap {} ",
                fmt_kb(r.rss_kb),
                fmt_kb(r.pss_kb),
                fmt_kb(r.uss_kb),
                fmt_kb(r.swap_kb)
            )),
        ]),
        Line::from(vec![
            Span::styled("pss by kind  ", Style::new().fg(Color::DarkGray)),
            Span::styled(
                format!("anon {}  ", fmt_kb(d.rollup.pss_anon_kb)),
                Style::new().fg(Color::Magenta),
            ),
            Span::styled(
                format!("file {}  ", fmt_kb(d.rollup.pss_file_kb)),
                Style::new().fg(Color::Cyan),
            ),
            Span::styled(
                format!("shmem {}  ", fmt_kb(d.rollup.pss_shmem_kb)),
                Style::new().fg(Color::Yellow),
            ),
            Span::styled(
                format!("swap-pss {}  ", fmt_kb(d.rollup.swap_pss_kb)),
                Style::new().fg(Color::Blue),
            ),
            Span::styled(
                format!(
                    "THP {}  KSM {}  ",
                    fmt_kb(d.rollup.thp_kb),
                    fmt_kb(d.rollup.ksm_kb)
                ),
                Style::new().fg(Color::DarkGray),
            ),
        ]),
        Line::from(Span::styled(
            format!(
                " {} VMAs in {} groups · parsed {:.0}ms · {}",
                r.vma_count,
                r.groups.len(),
                d.parse_ms,
                truncate(&d.cmdline, 120)
            ),
            Style::new().fg(Color::DarkGray),
        )),
    ];
    f.render_widget(
        Paragraph::new(summary).block(Block::new().borders(Borders::BOTTOM)),
        chunks[0],
    );

    let detail_rows = app.detail_rows();
    let max_pss = detail_rows
        .iter()
        .filter_map(|row| match row {
            DetailRow::Group(g) => Some(g.pss_kb),
            DetailRow::Vma(..) => None,
        })
        .max()
        .unwrap_or(1)
        .max(1);
    let rows: Vec<Row> = detail_rows
        .iter()
        .map(|row| match row {
            DetailRow::Group(g) => {
                let frac = g.pss_kb as f64 / max_pss as f64;
                let marker = if g.vmas.is_empty() {
                    "  "
                } else if app.expanded.contains(&g.label) {
                    "▾ "
                } else {
                    "▸ "
                };
                Row::new(vec![
                    Cell::from(truncate(&format!("{marker}{}", g.label), 44)),
                    Cell::from(format!("{:>9}", fmt_kb(g.pss_kb))),
                    Cell::from(format!("{:>9}", fmt_kb(g.uss_kb))),
                    Cell::from(format!("{:>9}", fmt_kb(g.anon_kb))),
                    Cell::from(format!("{:>9}", fmt_kb(g.swap_kb))),
                    Cell::from(format!("{:>9}", fmt_kb(g.size_kb))),
                    Cell::from(format!("{:>5}", g.count)),
                    split_cell_scaled(g.uss_kb, g.pss_kb.saturating_sub(g.uss_kb), frac),
                ])
            }
            DetailRow::Vma(g, v) => {
                // Scale inside the group: a 2 MB VMA next to a 234 MB group
                // would otherwise round to zero cells.
                let denom = g.vmas.first().map(|v| v.pss_kb).unwrap_or(1).max(1);
                let frac = v.pss_kb as f64 / denom as f64;
                let dim = Style::new().fg(Color::DarkGray);
                Row::new(vec![
                    Cell::from(Text::from(Span::styled(
                        truncate(&format!("    {:x}-{:x} {}", v.start, v.end, v.perms), 44),
                        dim,
                    ))),
                    Cell::from(format!("{:>9}", fmt_kb(v.pss_kb))),
                    Cell::from(format!("{:>9}", fmt_kb(v.uss_kb))),
                    Cell::from(format!("{:>9}", fmt_kb(v.anon_kb))),
                    Cell::from(format!("{:>9}", fmt_kb(v.swap_kb))),
                    Cell::from(format!("{:>9}", fmt_kb(v.size_kb))),
                    Cell::from(""),
                    split_cell_scaled(v.uss_kb, v.pss_kb.saturating_sub(v.uss_kb), frac),
                ])
            }
        })
        .collect();

    let header = Row::new(vec![
        Cell::from(format!("{:<44}", "MAPPING")),
        Cell::from(format!("{:>9}", "PSS")),
        Cell::from(format!("{:>9}", "USS")),
        Cell::from(format!("{:>9}", "ANON")),
        Cell::from(format!("{:>9}", "SWAP")),
        Cell::from(format!("{:>9}", "SIZE")),
        Cell::from(format!("{:>5}", "VMAS")),
        Cell::from("  PRIV|SHARED"),
    ])
    .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let title = if app.detail_filter.is_empty() {
        " vm mappings by PSS  (Enter expand, Esc back, f filter) ".to_string()
    } else {
        format!(" vm mappings by PSS  filter {} ", app.detail_filter)
    };
    let table = Table::new(
        rows,
        [
            Constraint::Length(44),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(5),
            Constraint::Length(MIX_W as u16),
        ],
    )
    .header(header)
    .block(Block::new().borders(Borders::ALL).title(title))
    .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(table, chunks[1], &mut state);
    app.detail_state = state;
}

fn footer(f: &mut Frame, area: Rect, app: &App) {
    let line = if app.editing {
        Line::from(vec![
            Span::styled(
                if app.editing_detail { " vma filter: " } else { " filter: " },
                Style::new().fg(Color::Yellow),
            ),
            Span::raw(app.edit_buf.clone()),
            Span::styled("▏", Style::new().fg(Color::Yellow)),
            Span::styled("   Enter 应用 · Esc 取消", Style::new().fg(Color::DarkGray)),
        ])
    } else if app.drill.is_some() {
        Line::from(Span::styled(
            " q quit │ j/k scroll │ Esc back to cgroups │ Tab procs ",
            Style::new().fg(Color::DarkGray),
        ))
    } else if app.view == View::Cgroups {
        Line::from(Span::styled(
            format!(
                " q quit │ Tab procs │ j/k │ Enter list procs │ s sort ({}) │ r reverse │ f filter path ",
                app.cgroup_sort.label()
            ),
            Style::new().fg(Color::DarkGray),
        ))
    } else if app.detail {
        Line::from(Span::styled(
            " q quit │ j/k │ Enter expand │ z/Z collapse/expand all │ f filter │ Esc back ",
            Style::new().fg(Color::DarkGray),
        ))
    } else {
        Line::from(Span::styled(
            format!(
                " q quit │ Tab cgroups │ j/k move │ g/G ends │ s sort ({}) │ r reverse │ f filter │ Enter vma detail ",
                app.sort.label()
            ),
            Style::new().fg(Color::DarkGray),
        ))
    };
    f.render_widget(Paragraph::new(line), area);
}

#[cfg(test)]
mod tests {
    use super::segments;

    #[test]
    fn segments_always_sum_to_width() {
        for values in [
            vec![60, 30, 10, 0],
            vec![1, 1, 1],
            vec![0, 0, 5],
            vec![u64::MAX, 1],
            vec![7],
        ] {
            let w = segments(&values, 12);
            assert_eq!(w.iter().sum::<usize>(), 12, "{values:?} -> {w:?}");
        }
    }

    #[test]
    fn segments_are_proportional_and_exact() {
        assert_eq!(segments(&[60, 30, 10, 0], 12), vec![7, 4, 1, 0]);
        assert_eq!(segments(&[0, 0, 0, 0], 12), vec![0, 0, 0, 0]);
        assert_eq!(segments(&[5], 12), vec![12]);
    }
}
