use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Sparkline, Table};

use crate::app::{App, DetailRow, View};
use crate::cgroup::CgroupMem;
use crate::model::{PSS_MAX_AGE, SortKey};
use crate::procfs::fmt_kb;
use crate::theme::Palette;

const SHARE_W: usize = 6;
const MIX_W: usize = 16;

/// Default-theme highlight background, kept public for tests and callers that
/// need the built-in value.
pub const HL_BG: Color = Palette::DEFAULT.highlight_bg;

const SPARK_LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Text sparkline: no widget needed, and it can be asserted as a string.
pub fn text_spark(values: &[u64], width: usize) -> String {
    if values.is_empty() || width == 0 {
        return String::new();
    }
    let start = values.len().saturating_sub(width);
    let slice = &values[start..];
    let max = slice.iter().copied().max().unwrap_or(0);
    slice
        .iter()
        .map(|v| {
            let idx = if max == 0 {
                0
            } else {
                ((*v as f64 / max as f64) * 7.0).round() as usize
            };
            SPARK_LEVELS[idx.min(7)]
        })
        .collect()
}

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
        (true, _, View::Apps) => apps_view(f, chunks[1], app),
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
    let p = app.theme.palette;
    let cols = Layout::horizontal([Constraint::Min(48), Constraint::Length(32)]).split(area);
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
        p.crit
    } else if used_frac > 0.75 {
        p.warn
    } else {
        p.ok
    };
    let swap_used = snap.mem.swap_total_kb.saturating_sub(snap.mem.swap_free_kb);
    let line1 = Line::from(vec![
        Span::styled(" mtop ", p.highlight().add_modifier(Modifier::BOLD)),
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
    // Kernel threads have no address space: no privilege will ever give them a
    // PSS, so they must not sit in the denominator forever.
    let no_mm = snap.no_mm.len();
    let eligible = snap.rows.len().saturating_sub(no_mm);
    let mut line2 = vec![
        Span::styled(
            format!(" sweep {:.0}ms ", snap.sweep_ms),
            Style::new().fg(p.dim),
        ),
        Span::styled(format!("pss {:.0}ms ", snap.pss_ms), Style::new().fg(p.dim)),
        Span::styled(
            format!(
                "coverage {}/{} = {} ",
                snap.covered,
                eligible,
                fmt_kb(snap.covered_pss_kb)
            ),
            Style::new().fg(p.dim),
        ),
    ];
    if denied > 0 {
        line2.push(Span::styled(
            format!("{denied} n/a (needs root) "),
            Style::new().fg(p.warn),
        ));
    }
    let psi = snap.pressure;
    let psi_color = if psi.some10 >= 20.0 {
        p.crit
    } else if psi.some10 >= 5.0 {
        p.warn
    } else {
        p.dim
    };
    line2.push(Span::styled(
        format!("psi {:.1}/{:.1}% ", psi.some10, psi.full10),
        Style::new().fg(psi_color),
    ));
    line2.push(Span::styled(
        format!("age {}ms", app.sample_age_ms()),
        Style::new().fg(p.dim),
    ));
    let line2 = Line::from(line2);
    f.render_widget(
        Paragraph::new(vec![line1, line2])
            .block(Block::new().borders(Borders::TOP | Borders::BOTTOM)),
        cols[0],
    );

    // RSS trend of the focused process.
    let (label, data): (String, Vec<u64>) = match app.view {
        View::Apps => match app.selected_app() {
            Some(g) => (
                format!(
                    " {} {}",
                    truncate(&g.name, 24),
                    fmt_kb(app.app_metric.value(g))
                ),
                app.app_series(&g.name, app.app_metric),
            ),
            None => (" apps".into(), Vec::new()),
        },
        View::Cgroups => match app.visible_cgroups().get(app.cgroup_sel) {
            Some(cg) => {
                let hist = app
                    .cgroup_history
                    .get(&cg.path)
                    .map(|h| h.iter().copied().collect::<Vec<u64>>())
                    .unwrap_or_default();
                (
                    format!(" {} psi {:.1}%", truncate_left(&cg.path, 16), cg.psi_some10),
                    hist,
                )
            }
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
    // The cgroup sparkline has a fixed 0-100% scale (values are stored as
    // percent*10), otherwise one 2% sample would fill the whole graph.
    let max = if app.view == View::Cgroups {
        1000
    } else {
        data.iter().copied().max().unwrap_or(1).max(1)
    };
    f.render_widget(
        Sparkline::default()
            .data(data.as_slice())
            .max(max)
            .style(Style::new().fg(p.accent))
            .block(Block::new().borders(Borders::ALL).title(label)),
        cols[1],
    );
}

/// Selected rows and badges are drawn by `p.highlight()` after the cells
/// are rendered, so the explicit background/foreground wins over per-cell
/// colours. That is why no per-cell `plain` mode is needed (and why reverse
/// video must not be used: it swaps the terminal's default colours).
/// Red when a cgroup has ever hit an OOM kill, nothing otherwise.
fn oom_style_color(c: &CgroupMem, crit: Color) -> Option<Color> {
    (c.oom_recent > 0 || c.oom_kills > 0).then_some(crit)
}

fn styled_span(text: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(color))
}

fn styled_cell(text: impl Into<String>, color: Color) -> Cell<'static> {
    Cell::from(Text::from(styled_span(text, color)))
}

fn bar_cell(frac: f64, color: Color, width: usize, empty: Color) -> Cell<'static> {
    let n = ((frac.clamp(0.0, 1.0)) * width as f64).round() as usize;
    let mut filled: String = "█".repeat(n);
    filled.push_str(&" ".repeat(width - n));
    Cell::from(Text::from(Line::from(styled_span(filled, color)))).style(Style::new().fg(empty))
}

/// Split like [`segments`], but give every nonzero category at least one cell
/// so a small shmem/swap segment cannot disappear through rounding.
fn segments_min1(values: &[u64], width: usize) -> Vec<usize> {
    let mut out = vec![0usize; values.len()];
    let nonzero: Vec<usize> = values
        .iter()
        .enumerate()
        .filter(|(_, value)| **value > 0)
        .map(|(index, _)| index)
        .collect();
    if nonzero.is_empty() {
        return out;
    }
    // More categories than cells: fall back to plain proportional rounding.
    if nonzero.len() > width {
        return segments(values, width);
    }
    for &index in &nonzero {
        out[index] = 1;
    }
    let total: u128 = values.iter().map(|value| *value as u128).sum();
    let remaining = width - nonzero.len();
    let mut used = 0usize;
    for &index in &nonzero {
        let extra = (values[index] as u128 * remaining as u128 / total) as usize;
        out[index] += extra;
        used += extra;
    }
    let largest = nonzero
        .iter()
        .copied()
        .max_by_key(|&index| values[index])
        .expect("at least one nonzero value");
    out[largest] += remaining - used;
    out
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
    let widths = segments_min1(values, MIX_W);
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(values.len() + 1);
    let mut used = 0;
    for (w, color) in widths.iter().zip(colors) {
        if *w > 0 {
            spans.push(styled_span("█".repeat(*w), *color));
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
fn mix_cell(anon_kb: u64, file_kb: u64, shmem_kb: u64, swap_kb: u64, p: &Palette) -> Cell<'static> {
    segmented_cell(
        &[anon_kb, file_kb, shmem_kb, swap_kb],
        &[p.anon, p.file, p.shmem, p.swap],
    )
}

/// Private (only this process pays) vs shared (PSS-weighted) pages, with the
/// total length scaled by `frac` so magnitude and split are both visible.
fn split_cell_scaled(private_kb: u64, shared_kb: u64, frac: f64, p: &Palette) -> Cell<'static> {
    let filled = ((frac.clamp(0.0, 1.0)) * MIX_W as f64).round() as usize;
    let widths = segments(&[private_kb, shared_kb], filled);
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(3);
    for (w, color) in widths.iter().zip([p.anon, p.file]) {
        if *w > 0 {
            spans.push(styled_span("█".repeat(*w), color));
        }
    }
    if filled < MIX_W {
        spans.push(Span::raw(" ".repeat(MIX_W - filled)));
    }
    Cell::from(Text::from(Line::from(spans)))
}

/// Format the per-tick RSS delta.
///
/// `fmt_kb` renders everything below 10 MB with one decimal of MB, so a
/// one-page change (4 kB) used to print as `+0.0M`. Keep sub-megabyte deltas
/// in kB: a nonzero change must never look like zero.
fn delta_text(d: i64) -> String {
    if d == 0 {
        return "     0".to_string();
    }
    let kb = d.unsigned_abs();
    let sign = if d > 0 { "+" } else { "-" };
    let value = if kb < 1024 {
        format!("{kb}k")
    } else {
        fmt_kb(kb)
    };
    format!("{sign}{value:>6}")
}

fn delta_span(d: i64, dim_color: Color, p: &Palette) -> Span<'static> {
    let color = if d > 0 {
        p.crit
    } else if d < 0 {
        p.ok
    } else {
        dim_color
    };
    styled_span(delta_text(d), color)
}

fn table_view(f: &mut Frame, area: Rect, app: &mut App) {
    let p = app.theme.palette;
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
        .enumerate()
        .map(|(i, r)| {
            let selected = i == app.sel;
            let pss = snap.pss.get(&r.pid);
            let pss_kb = pss.map(|p| p.rollup.pss_kb);
            let uss_kb = pss.map(|p| p.rollup.uss_kb);
            // Older than the freshness window: last known value, suffixed with
            // '~' so a stale number can never be mistaken for a live one.
            let stale = pss.is_some_and(|p| p.at.elapsed() > PSS_MAX_AGE);
            let denied = snap.denied.contains(&r.pid) || snap.no_mm.contains(&r.pid);
            let ratio = pss
                .filter(|_| r.rss_kb > 0)
                .map(|p| p.rollup.pss_kb as f64 / r.rss_kb as f64);
            let ratio_text = match ratio {
                Some(v) => format!("{:>6.0}%", v * 100.0),
                None => "     —".to_string(),
            };
            let ratio_color = match ratio {
                Some(_) if stale => p.dim(selected),
                Some(v) if v > 0.8 => p.warn,
                Some(_) => p.dim(selected),
                None => p.dim(selected),
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
                p.crit
            } else if mem_frac > 0.05 {
                p.warn
            } else {
                p.bar
            };
            Row::new(vec![
                Cell::from(format!("{:>7}", r.pid)),
                Cell::from(truncate(&r.comm, 20)),
                Cell::from(format!("{:>9}", fmt_kb(r.rss_kb))),
                Cell::from(match pss_kb {
                    Some(v) if stale => styled_span(format!("{:>8}~", fmt_kb(v)), p.dim(selected)),
                    Some(v) => Span::raw(format!("{:>9}", fmt_kb(v))),
                    None if denied => styled_span("      n/a", p.dim(selected)),
                    None => styled_span("        —", p.dim(selected)),
                }),
                Cell::from(match uss_kb {
                    Some(v) if stale => styled_span(format!("{:>8}~", fmt_kb(v)), p.dim(selected)),
                    Some(v) => Span::raw(format!("{:>9}", fmt_kb(v))),
                    None if denied => styled_span("      n/a", p.dim(selected)),
                    None => styled_span("        —", p.dim(selected)),
                }),
                styled_cell(ratio_text, ratio_color),
                Cell::from(Text::from(delta_span(r.delta_kb, p.dim(selected), &p))),
                bar_cell(frac, bar_color, SHARE_W, p.dim),
                match mix {
                    Some((a, f, sh, sw)) => mix_cell(a, f, sh, sw, &p),
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
    .style(Style::new().fg(p.accent).add_modifier(Modifier::BOLD));

    // A PSS/USS ranking is only meaningful once the rollup cache has been
    // filled; say so instead of silently ranking unsampled rows as zero.
    let rank_incomplete = matches!(app.sort, SortKey::Pss | SortKey::Uss | SortKey::Ratio)
        && snap.covered + snap.denied.len() < snap.rows.len().saturating_sub(snap.no_mm.len());
    let mut sort_label = format!(
        " procs  sort {} {}  filter {} ",
        app.sort.label(),
        if app.desc { "↓" } else { "↑" },
        if app.filter.is_empty() {
            "-"
        } else {
            app.filter.as_str()
        }
    );
    if let Some(name) = &app.pid_filter_name {
        sort_label.push_str(&format!("  app: {name} (Esc clears) "));
    }
    if rank_incomplete {
        sort_label.push_str(&format!(
            " pss {}/{} · ranking incomplete ",
            snap.covered,
            snap.rows.len().saturating_sub(snap.no_mm.len())
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
    .row_highlight_style(p.highlight())
    .highlight_symbol("▸");

    f.render_stateful_widget(table, area, &mut state);
    app.table_state = state;
}

fn apps_view(f: &mut Frame, area: Rect, app: &mut App) {
    let p = app.theme.palette;
    let apps: Vec<crate::apps::AppGroup> = app.visible_apps().into_iter().cloned().collect();
    if apps.is_empty() {
        f.render_widget(
            Paragraph::new("no app groups (no PSS data yet)")
                .block(Block::new().borders(Borders::ALL).title(" apps ")),
            area,
        );
        return;
    }
    let total: u64 = apps.iter().map(|g| app.app_metric.value(g)).sum();
    // RSS adds shared pages once per process, so the map would inflate every
    // multi-process app (2.7x on this machine). Say so instead of pretending.
    let metric_hint = match app.app_metric {
        crate::app::AppMetric::Pss => "",
        crate::app::AppMetric::Uss => " (private only)",
        crate::app::AppMetric::Rss => " (overcounts shared)",
    };
    let title = format!(
        " apps  metric {}{}  filter {}  ({} groups · {}) ",
        app.app_metric.label(),
        metric_hint,
        if app.filter.is_empty() {
            "-"
        } else {
            app.filter.as_str()
        },
        apps.len(),
        fmt_kb(total)
    );
    let block = Block::new().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Narrow or short terminals get a list: a treemap under ~100x14 has more
    // borders than data.
    if inner.width < 100 || inner.height < 14 {
        apps_list(f, inner, app, &apps);
        return;
    }

    let values: Vec<u64> = apps.iter().map(|g| app.app_metric.value(g)).collect();
    let rects = crate::apps::squarify(&values, inner);
    for (i, (g, r)) in apps.iter().zip(rects.iter()).enumerate() {
        if r.width == 0 || r.height == 0 {
            continue;
        }
        let selected = i == app.app_sel;
        let fill = p.treemap[i % p.treemap.len()];
        // Invert the selected tile instead of reusing the table highlight
        // colour: `highlight_bg` can match a neighbouring tile's fill (the
        // light theme's blue was almost identical to its first tile), which
        // made the selection indistinguishable. Swapping the tile's own
        // foreground and fill is always distinct and keeps the same contrast.
        let (bg, fg) = if selected {
            (p.tile_fg, fill)
        } else {
            (fill, p.tile_fg)
        };
        let buf = f.buffer_mut();
        buf.set_style(*r, Style::new().bg(bg));
        if r.width >= 8 && r.height >= 2 {
            let name = truncate(&g.name, (r.width as usize).saturating_sub(2));
            let style = if selected {
                Style::new().bg(bg).fg(fg).add_modifier(Modifier::BOLD)
            } else {
                Style::new().bg(bg).fg(fg)
            };
            buf.set_stringn(
                r.x + 1,
                r.y,
                name,
                (r.width as usize).saturating_sub(2),
                style,
            );
        }
        if r.width >= 12 && r.height >= 3 {
            let sub = format!("{} · {}p", fmt_kb(app.app_metric.value(g)), g.procs);
            buf.set_stringn(
                r.x + 1,
                r.y + 1,
                sub,
                (r.width as usize).saturating_sub(2),
                Style::new().bg(bg).fg(fg),
            );
        }
    }
}

fn apps_list(f: &mut Frame, area: Rect, app: &mut App, apps: &[crate::apps::AppGroup]) {
    let p = app.theme.palette;
    let mut state = std::mem::take(&mut app.app_state);
    let max = apps
        .iter()
        .map(|g| app.app_metric.value(g))
        .max()
        .unwrap_or(1)
        .max(1);
    let rows: Vec<Row> = apps
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let frac = app.app_metric.value(g) as f64 / max as f64;
            Row::new(vec![
                Cell::from(truncate(&g.name, 26)),
                Cell::from(format!("{:>9}", fmt_kb(app.app_metric.value(g)))),
                Cell::from(format!("{:>5}", g.procs)),
                Cell::from(format!("{:>9}", fmt_kb(g.pss_kb))),
                bar_cell(
                    frac,
                    if i == app.app_sel {
                        // A treemap fill can match the row highlight (notably
                        // in the light theme); the tile foreground cannot.
                        p.tile_fg
                    } else {
                        p.treemap[i % p.treemap.len()]
                    },
                    10,
                    p.dim,
                ),
            ])
        })
        .collect();
    let header = Row::new(vec![
        Cell::from(format!("{:<26}", "APP")),
        Cell::from(format!("{:>9}", app.app_metric.label())),
        Cell::from(format!("{:>5}", "PROCS")),
        Cell::from(format!("{:>9}", "PSS")),
        Cell::from(" SHARE"),
    ])
    .style(Style::new().fg(p.accent).add_modifier(Modifier::BOLD));
    let table = Table::new(
        rows,
        [
            Constraint::Length(26),
            Constraint::Length(9),
            Constraint::Length(5),
            Constraint::Length(9),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .row_highlight_style(p.highlight())
    .highlight_symbol("▸");
    f.render_stateful_widget(table, area, &mut state);
    app.app_state = state;
}

fn cgroup_drill(f: &mut Frame, area: Rect, app: &mut App) {
    let p = app.theme.palette;
    let mut state = std::mem::take(&mut app.drill_state);
    let (Some(snap), Some(path)) = (app.snap.as_ref(), app.drill.clone()) else {
        app.drill_state = state;
        return;
    };
    let members = app.cgroup_members();
    let cg = snap.cgroups.iter().find(|c| c.path == path);
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(4)]).split(area);

    let summary = Line::from(vec![
        Span::styled(
            format!(" {} ", truncate_left(&path, 60)),
            p.highlight().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            " current {} · {} procs in subtree · limit {} · psi {:.1}/{:.1}% ",
            cg.map(|c| fmt_kb(c.current_kb))
                .unwrap_or_else(|| "—".into()),
            members.len(),
            cg.and_then(|c| c.limit_kb)
                .map(fmt_kb)
                .unwrap_or_else(|| "max".into()),
            cg.map(|c| c.psi_some10).unwrap_or(0.0),
            cg.map(|c| c.psi_full10).unwrap_or(0.0)
        )),
    ]);
    let oom_hist: Vec<u64> = app
        .oom_history
        .get(&path)
        .map(|h| h.iter().copied().collect())
        .unwrap_or_default();
    let oom_window: u64 = oom_hist.iter().sum();
    let total_kills = cg.map(|c| c.oom_kills).unwrap_or(0);
    let oom_line = Line::from(vec![
        Span::styled("oom ", Style::new().fg(p.dim)),
        Span::styled(text_spark(&oom_hist, 60), Style::new().fg(p.crit)),
        Span::raw(format!(
            "  {total_kills} total · +{oom_window} in last {} scans",
            oom_hist.len()
        )),
    ]);
    f.render_widget(
        Paragraph::new(vec![summary, oom_line]).block(Block::new().borders(Borders::BOTTOM)),
        chunks[0],
    );

    // Scale the share bar against the largest member, not total RAM: a 500 MB
    // process in a 15 GB machine would otherwise round to zero cells.
    let max_rss = members.iter().map(|r| r.rss_kb).max().unwrap_or(1).max(1);
    let rows: Vec<Row> = members
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let selected = i == app.drill_sel;
            let pss = snap.pss.get(&r.pid);
            let ratio = pss
                .filter(|_| r.rss_kb > 0)
                .map(|p| p.rollup.pss_kb as f64 / r.rss_kb as f64);
            let frac = r.rss_kb as f64 / max_rss as f64;
            let color = if frac > 0.15 {
                p.crit
            } else if frac > 0.05 {
                p.warn
            } else {
                p.bar
            };
            Row::new(vec![
                Cell::from(format!("{:>7}", r.pid)),
                Cell::from(truncate(&r.comm, 26)),
                Cell::from(format!("{:>9}", fmt_kb(r.rss_kb))),
                Cell::from(match pss {
                    Some(p) => Text::from(format!("{:>9}", fmt_kb(p.rollup.pss_kb))),
                    None => Text::from(styled_span("        —", p.dim(selected))),
                }),
                Cell::from(match ratio {
                    Some(v) => format!("{:>7.0}%", v * 100.0),
                    None => "       —".to_string(),
                }),
                bar_cell(frac, color, SHARE_W, p.dim),
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
    .style(Style::new().fg(p.accent).add_modifier(Modifier::BOLD));

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
    .row_highlight_style(p.highlight())
    .highlight_symbol("▸");

    f.render_stateful_widget(table, chunks[1], &mut state);
    app.drill_state = state;
}

fn cgroup_view(f: &mut Frame, area: Rect, app: &mut App) {
    let p = app.theme.palette;
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
    let max_cur = snap
        .cgroups
        .iter()
        .map(|c| c.current_kb)
        .max()
        .unwrap_or(1)
        .max(1);
    let rows: Vec<Row> = visible
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let selected = i == app.cgroup_sel;
            let (frac, over) = match c.limit_kb {
                Some(limit) if limit > 0 => (c.current_kb as f64 / limit as f64, limit),
                _ => (c.current_kb as f64 / max_cur as f64, 0),
            };
            let color = if c.oom_kills > 0 {
                p.crit
            } else if over > 0 && frac > 0.9 {
                p.warn
            } else {
                p.bar
            };
            // "7▲" means: 7 kills total, at least one since the last scan.
            let oom_text = if c.oom_recent > 0 {
                format!("{:>3}▲", c.oom_kills)
            } else {
                format!("{:>4}", c.oom_kills)
            };
            Row::new(vec![
                Cell::from(truncate_left(&c.path, 42)),
                Cell::from(format!("{:>9}", fmt_kb(c.current_kb))),
                Cell::from(format!("{:>9}", fmt_kb(c.anon_kb))),
                Cell::from(format!("{:>9}", fmt_kb(c.file_kb))),
                Cell::from(format!("{:>9}", fmt_kb(c.kernel_kb))),
                Cell::from(format!("{:>9}", fmt_kb(c.slab_kb))),
                Cell::from(match c.limit_kb {
                    Some(l) => Text::from(format!("{:>9}", fmt_kb(l))),
                    None => Text::from(Span::styled("      max", Style::new().fg(p.dim(selected)))),
                }),
                styled_cell(
                    format!("{:>6.1}%", c.psi_some10),
                    if c.psi_some10 >= 20.0 {
                        p.crit
                    } else if c.psi_some10 >= 5.0 {
                        p.warn
                    } else {
                        p.dim(selected)
                    },
                ),
                Cell::from(Text::from(match oom_style_color(c, p.crit) {
                    Some(color) => styled_span(oom_text, color),
                    None => Span::raw(oom_text),
                })),
                bar_cell(frac, color, 10, p.dim),
            ])
        })
        .collect();

    let header = Row::new(vec![
        Cell::from(format!("{:<42}", "CGROUP")),
        Cell::from(format!("{:>9}", "CURRENT")),
        Cell::from(format!("{:>9}", "ANON")),
        Cell::from(format!("{:>9}", "FILE")),
        Cell::from(format!("{:>9}", "KERNEL")),
        Cell::from(format!("{:>9}", "SLAB")),
        Cell::from(format!("{:>9}", "MAX")),
        Cell::from(format!("{:>7}", "PSI10")),
        Cell::from(format!("{:>4}", "OOM")),
        Cell::from(" CUR/MAX"),
    ])
    .style(Style::new().fg(p.accent).add_modifier(Modifier::BOLD));

    let recent: u64 = snap.cgroups.iter().map(|c| c.oom_recent).sum();
    let mut title = format!(
        " cgroups  sort {} {}  filter {}  ({} of {}) ",
        app.cgroup_sort.label(),
        if app.desc { "↓" } else { "↑" },
        if app.filter.is_empty() {
            "-"
        } else {
            app.filter.as_str()
        },
        visible.len(),
        snap.cgroups.len()
    );
    if recent > 0 {
        title.push_str(&format!("  oom +{recent} since last sample "));
    }
    let table = Table::new(
        rows,
        [
            Constraint::Length(42),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(7),
            Constraint::Length(4),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(Block::new().borders(Borders::ALL).title(title))
    .row_highlight_style(p.highlight())
    .highlight_symbol("▸");

    f.render_stateful_widget(table, area, &mut state);
    app.cgroup_state = state;
}

fn detail_view(f: &mut Frame, area: Rect, app: &mut App) {
    let p = app.theme.palette;
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
                p.highlight().add_modifier(Modifier::BOLD),
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
            Span::styled("pss by kind  ", Style::new().fg(p.dim)),
            Span::styled(
                format!("anon {}  ", fmt_kb(d.rollup.pss_anon_kb)),
                Style::new().fg(p.anon),
            ),
            Span::styled(
                format!("file {}  ", fmt_kb(d.rollup.pss_file_kb)),
                Style::new().fg(p.file),
            ),
            Span::styled(
                format!("shmem {}  ", fmt_kb(d.rollup.pss_shmem_kb)),
                Style::new().fg(p.warn),
            ),
            Span::styled(
                format!("swap-pss {}  ", fmt_kb(d.rollup.swap_pss_kb)),
                Style::new().fg(p.swap_pss),
            ),
            Span::styled(
                format!(
                    "THP {}  KSM {}  ",
                    fmt_kb(d.rollup.thp_kb),
                    fmt_kb(d.rollup.ksm_kb)
                ),
                Style::new().fg(p.dim),
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
            Style::new().fg(p.dim),
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
        .enumerate()
        .map(|(i, row)| {
            let selected = i == app.detail_sel;
            match row {
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
                        split_cell_scaled(g.uss_kb, g.pss_kb.saturating_sub(g.uss_kb), frac, &p),
                    ])
                }
                DetailRow::Vma(g, v) => {
                    // Scale inside the group: a 2 MB VMA next to a 234 MB group
                    // would otherwise round to zero cells.
                    let denom = g.vmas.first().map(|v| v.pss_kb).unwrap_or(1).max(1);
                    let frac = v.pss_kb as f64 / denom as f64;
                    let vma_dim = Style::new().fg(p.dim(selected));
                    Row::new(vec![
                        Cell::from(Text::from(Span::styled(
                            truncate(&format!("    {:x}-{:x} {}", v.start, v.end, v.perms), 44),
                            vma_dim,
                        ))),
                        Cell::from(format!("{:>9}", fmt_kb(v.pss_kb))),
                        Cell::from(format!("{:>9}", fmt_kb(v.uss_kb))),
                        Cell::from(format!("{:>9}", fmt_kb(v.anon_kb))),
                        Cell::from(format!("{:>9}", fmt_kb(v.swap_kb))),
                        Cell::from(format!("{:>9}", fmt_kb(v.size_kb))),
                        Cell::from(""),
                        split_cell_scaled(v.uss_kb, v.pss_kb.saturating_sub(v.uss_kb), frac, &p),
                    ])
                }
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
    .style(Style::new().fg(p.accent).add_modifier(Modifier::BOLD));

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
    .row_highlight_style(p.highlight());

    f.render_stateful_widget(table, chunks[1], &mut state);
    app.detail_state = state;
}

fn footer(f: &mut Frame, area: Rect, app: &App) {
    let p = app.theme.palette;
    let line = if app.editing {
        Line::from(vec![
            Span::styled(
                if app.editing_detail {
                    " vma filter: "
                } else {
                    " filter: "
                },
                Style::new().fg(p.focus),
            ),
            Span::raw(app.edit_buf.clone()),
            Span::styled("▏", Style::new().fg(p.focus)),
            Span::styled("   Enter 应用 · Esc 取消", Style::new().fg(p.dim)),
        ])
    } else if app.view == View::Apps {
        Line::from(Span::styled(
            format!(
                " q quit │ Tab views │ j/k select │ Enter drill into procs │ s metric ({}) │ f filter │ Esc clear ",
                app.app_metric.label()
            ),
            Style::new().fg(p.dim),
        ))
    } else if app.drill.is_some() {
        Line::from(Span::styled(
            " q quit │ j/k scroll │ Esc back to cgroups │ Tab procs ",
            Style::new().fg(p.dim),
        ))
    } else if app.view == View::Cgroups {
        Line::from(Span::styled(
            format!(
                " q quit │ Tab procs │ j/k │ Enter list procs │ s sort ({}) │ r reverse │ f filter path ",
                app.cgroup_sort.label()
            ),
            Style::new().fg(p.dim),
        ))
    } else if app.detail {
        Line::from(Span::styled(
            " q quit │ j/k │ Enter expand │ z/Z collapse/expand all │ f filter │ Esc back ",
            Style::new().fg(p.dim),
        ))
    } else {
        Line::from(Span::styled(
            format!(
                " q quit │ Tab cgroups │ j/k move │ g/G ends │ s sort ({}) │ r reverse │ f filter │ Enter vma detail ",
                app.sort.label()
            ),
            Style::new().fg(p.dim),
        ))
    };
    let mut legend = vec![Span::styled(
        format!(" theme: {} · t switch · mix ", app.theme.name),
        Style::new().fg(p.dim),
    )];
    for (color, label) in [
        (p.anon, "anon"),
        (p.file, "file"),
        (p.shmem, "shmem"),
        (p.swap, "swap"),
    ] {
        legend.push(Span::styled("█", Style::new().fg(color)));
        legend.push(Span::styled(format!("{label} "), Style::new().fg(p.dim)));
    }
    f.render_widget(Paragraph::new(vec![line, Line::from(legend)]), area);
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
    fn text_spark_renders_a_normalised_series() {
        assert_eq!(super::text_spark(&[0, 0, 4, 0], 4), "▁▁█▁");
        assert_eq!(super::text_spark(&[2, 3], 2), "▆█");
        assert_eq!(super::text_spark(&[1, 1, 1], 3), "███");
        assert!(super::text_spark(&[], 5).is_empty());
        // Only the last `width` samples are drawn.
        assert_eq!(super::text_spark(&[9, 0, 0, 1], 3), "▁▁█");
    }

    #[test]
    fn small_nonzero_deltas_use_kb() {
        assert_eq!(super::delta_text(0), "     0");
        assert_eq!(super::delta_text(4), "+    4k");
        assert_eq!(super::delta_text(-4), "-    4k");
        assert_eq!(super::delta_text(512), "+  512k");
        assert_eq!(super::delta_text(1024), "+  1.0M");
        assert_eq!(super::delta_text(4096), "+  4.0M");
    }

    #[test]
    fn nonzero_mix_segments_never_round_to_zero() {
        let got = super::segments_min1(&[1000, 1, 0, 1], 8);
        assert_eq!(got.iter().sum::<usize>(), 8);
        assert!(got[1] > 0 && got[3] > 0, "small segments vanished: {got:?}");
        assert_eq!(got[2], 0, "empty segment grew: {got:?}");
        assert_eq!(super::segments_min1(&[0, 0, 0, 0], 8), vec![0; 4]);
    }

    #[test]
    fn segments_are_proportional_and_exact() {
        assert_eq!(segments(&[60, 30, 10, 0], 12), vec![7, 4, 1, 0]);
        assert_eq!(segments(&[0, 0, 0, 0], 12), vec![0, 0, 0, 0]);
        assert_eq!(segments(&[5], 12), vec![12]);
    }
}
