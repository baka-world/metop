use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Sparkline, Table};
use ratatui::Frame;

use crate::app::App;
use crate::model::{ProcRow, SortKey, PSS_MAX_AGE};
use crate::procfs::fmt_kb;

const BAR_W: usize = 18;

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
    match (app.snap.is_some(), app.detail) {
        (true, true) if has_detail => detail_view(f, chunks[1], app),
        (true, true) => {
            let cause = detail_error.unwrap_or_else(|| "waiting for sampler…".into());
            f.render_widget(
                Paragraph::new(format!("vm mappings unavailable\n\n{cause}"))
                    .wrap(ratatui::widgets::Wrap { trim: true })
                    .block(Block::new().borders(Borders::ALL).title(" detail ")),
                chunks[1],
            );
        }
        (true, _) => table_view(f, chunks[1], app),
        (false, _) => {
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
    let line2 = Line::from(vec![
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
                snap.rows.len(),
                fmt_kb(snap.covered_pss_kb)
            ),
            Style::new().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("age {}ms", app.sample_age_ms()),
            Style::new().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(
        Paragraph::new(vec![line1, line2]).block(Block::new().borders(Borders::TOP | Borders::BOTTOM)),
        cols[0],
    );

    // RSS trend of the focused process.
    let (label, data): (String, Vec<u64>) = match app.selected_pid() {
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

fn bar_cell(frac: f64, color: Color) -> Cell<'static> {
    let n = ((frac.clamp(0.0, 1.0)) * BAR_W as f64).round() as usize;
    let mut filled: String = "█".repeat(n);
    filled.push_str(&" ".repeat(BAR_W - n));
    Cell::from(Text::from(Line::from(Span::styled(
        filled,
        Style::new().fg(color),
    ))))
    .style(Style::new().fg(Color::DarkGray))
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
                    None => Text::from(Span::styled("        —", Style::new().fg(Color::DarkGray))),
                }),
                Cell::from(match uss_kb {
                    Some(v) if stale => Text::from(Span::styled(
                        format!("{:>8}~", fmt_kb(v)),
                        Style::new().fg(Color::DarkGray),
                    )),
                    Some(v) => Text::from(format!("{:>9}", fmt_kb(v))),
                    None => Text::from(Span::styled("        —", Style::new().fg(Color::DarkGray))),
                }),
                Cell::from(Text::from(Span::styled(ratio_text, Style::new().fg(ratio_color)))),
                Cell::from(Text::from(delta_span(r.delta_kb))),
                bar_cell(frac, bar_color),
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
        Cell::from(" RSS SHARE"),
    ])
    .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    // A PSS/USS ranking is only meaningful once the rollup cache has been
    // filled; say so instead of silently ranking unsampled rows as zero.
    let rank_incomplete = matches!(app.sort, SortKey::Pss | SortKey::Uss | SortKey::Ratio)
        && snap.covered < snap.rows.len();
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
            snap.rows.len()
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
            Constraint::Length(BAR_W as u16),
        ],
    )
    .header(header)
    .block(Block::new().borders(Borders::ALL).title(sort_label))
    .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
    .highlight_symbol("▸");

    f.render_stateful_widget(table, area, &mut state);
    app.table_state = state;
}

fn detail_view(f: &mut Frame, area: Rect, app: &mut App) {
    let mut state = std::mem::take(&mut app.detail_state);
    let Some(d) = app.snap.as_ref().and_then(|s| s.detail.as_ref()) else {
        app.detail_state = state;
        return;
    };
    let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(4)]).split(area);
    let r = &d.report;
    let summary = vec![
        Line::from(vec![
            Span::styled(
                format!(" {} {} ", d.pid, truncate(&d.comm, 24)),
                Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED),
            ),
            Span::raw(format!(
                " RSS {}  PSS {}  USS {}  anon {}  swap {} ",
                fmt_kb(r.rss_kb),
                fmt_kb(r.pss_kb),
                fmt_kb(r.uss_kb),
                fmt_kb(r.anon_kb),
                fmt_kb(r.swap_kb)
            )),
        ]),
        Line::from(Span::styled(
            format!(
                " {} VMAs in {} groups · parsed {:.0}ms · {}",
                r.vma_count,
                r.groups.len(),
                d.parse_ms,
                truncate(&d.cmdline, 140)
            ),
            Style::new().fg(Color::DarkGray),
        )),
    ];
    f.render_widget(
        Paragraph::new(summary).block(Block::new().borders(Borders::BOTTOM)),
        chunks[0],
    );

    let max_pss = r.groups.iter().map(|g| g.pss_kb).max().unwrap_or(1).max(1);
    let rows: Vec<Row> = r
        .groups
        .iter()
        .map(|g| {
            let frac = g.pss_kb as f64 / max_pss as f64;
            let color = if g.pss_kb == 0 {
                Color::DarkGray
            } else if g.uss_kb * 4 > g.pss_kb * 3 {
                Color::Magenta
            } else {
                Color::Cyan
            };
            Row::new(vec![
                Cell::from(truncate(&g.label, 46)),
                Cell::from(format!("{:>9}", fmt_kb(g.pss_kb))),
                Cell::from(format!("{:>9}", fmt_kb(g.uss_kb))),
                Cell::from(format!("{:>9}", fmt_kb(g.size_kb))),
                Cell::from(format!("{:>5}", g.count)),
                bar_cell(frac, color),
            ])
        })
        .collect();

    let header = Row::new(vec![
        Cell::from(format!("{:<46}", "MAPPING")),
        Cell::from(format!("{:>9}", "PSS")),
        Cell::from(format!("{:>9}", "USS")),
        Cell::from(format!("{:>9}", "SIZE")),
        Cell::from(format!("{:>5}", "VMAS")),
        Cell::from(" PSS"),
    ])
    .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let table = Table::new(
        rows,
        [
            Constraint::Length(46),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(5),
            Constraint::Length(BAR_W as u16),
        ],
    )
    .header(header)
    .block(
        Block::new()
            .borders(Borders::ALL)
            .title(" vm mappings by PSS  (Esc back, j/k scroll) "),
    )
    .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(table, chunks[1], &mut state);
    app.detail_state = state;
}

fn footer(f: &mut Frame, area: Rect, app: &App) {
    let line = if app.editing {
        Line::from(vec![
            Span::styled(" filter: ", Style::new().fg(Color::Yellow)),
            Span::raw(app.edit_buf.clone()),
            Span::styled("▏", Style::new().fg(Color::Yellow)),
            Span::styled("   Enter 应用 · Esc 取消", Style::new().fg(Color::DarkGray)),
        ])
    } else if app.detail {
        Line::from(Span::styled(
            " q quit │ j/k scroll │ Esc back │ Enter back │ s sort │ r reverse │ f filter ",
            Style::new().fg(Color::DarkGray),
        ))
    } else {
        Line::from(Span::styled(
            format!(
                " q quit │ j/k move │ g/G ends │ s sort ({}) │ r reverse │ f filter │ Enter vma detail ",
                app.sort.label()
            ),
            Style::new().fg(Color::DarkGray),
        ))
    };
    f.render_widget(Paragraph::new(line), area);
}
