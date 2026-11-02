//! Render tests: the whole point of TestBackend is that a TUI frame becomes
//! plain text, so layout changes can be asserted instead of eyeballed.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use mtop::app::App;
use mtop::cgroup::CgroupMem;
use mtop::model::{Detail, ProcRow, PssInfo, Snapshot, SortKey};
use mtop::procfs::{MemInfo, Rollup, SmapsReport, Vma, VmaGroup};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn rollup(rss: u64, pss: u64, uss: u64) -> Rollup {
    Rollup {
        rss_kb: rss,
        pss_kb: pss,
        uss_kb: uss,
        anon_kb: pss,
        pss_anon_kb: pss / 2,
        pss_file_kb: pss - pss / 2,
        ..Default::default()
    }
}

fn snapshot() -> Snapshot {
    let mut pss = HashMap::new();
    pss.insert(
        100,
        PssInfo {
            rollup: rollup(500_000, 300_000, 250_000),
            at: Instant::now(),
        },
    );
    pss.insert(
        200,
        PssInfo {
            rollup: rollup(200_000, 180_000, 170_000),
            at: Instant::now(),
        },
    );
    // pid 300 deliberately has no PSS entry: the table must show a dash.
    Snapshot {
        rows: vec![
            ProcRow {
                pid: 100,
                comm: "chrome".into(),
                rss_kb: 500_000,
                delta_kb: 4096,
            },
            ProcRow {
                pid: 200,
                comm: "node".into(),
                rss_kb: 200_000,
                delta_kb: -2048,
            },
            ProcRow {
                pid: 300,
                comm: "sleepy-daemon-that-has-a-very-long-name".into(),
                rss_kb: 100_000,
                delta_kb: 0,
            },
            // pid 400 is unreadable (EACCES), not merely unsampled.
            ProcRow {
                pid: 400,
                comm: "root-owned-daemon".into(),
                rss_kb: 50_000,
                delta_kb: 0,
            },
            // pid 500 is a kernel thread: no address space, no PSS even as root.
            ProcRow {
                pid: 500,
                comm: "kthreadd".into(),
                rss_kb: 0,
                delta_kb: 0,
            },
        ],
        pss,
        mem: MemInfo {
            total_kb: 15_728_640,
            avail_kb: 5_242_880,
            cached_kb: 7_340_032,
            swap_total_kb: 8_388_608,
            swap_free_kb: 8_388_608,
        },
        detail_error: None,
        detail: Some(Detail {
            pid: 100,
            comm: "chrome".into(),
            cmdline: "/usr/bin/chrome --type=renderer".into(),
            report: SmapsReport {
                groups: vec![
                    VmaGroup {
                        label: "[heap]".into(),
                        size_kb: 100_000,
                        pss_kb: 90_000,
                        uss_kb: 88_000,
                        anon_kb: 90_000,
                        swap_kb: 0,
                        count: 2,
                        vmas: vec![
                            Vma {
                                start: 0x7f00_0000_0000,
                                end: 0x7f00_0000_1000,
                                perms: "rw-p".into(),
                                size_kb: 60_000,
                                pss_kb: 60_000,
                                uss_kb: 59_000,
                                anon_kb: 60_000,
                                swap_kb: 0,
                            },
                            Vma {
                                start: 0x7f00_0000_2000,
                                end: 0x7f00_0000_3000,
                                perms: "rw-p".into(),
                                size_kb: 40_000,
                                pss_kb: 30_000,
                                uss_kb: 29_000,
                                anon_kb: 30_000,
                                swap_kb: 0,
                            },
                        ],
                    },
                    VmaGroup {
                        label: "/usr/lib/libc.so.6".into(),
                        size_kb: 2_000,
                        pss_kb: 1_000,
                        uss_kb: 200,
                        anon_kb: 0,
                        swap_kb: 128,
                        count: 1,
                        vmas: vec![Vma {
                            start: 0x7f00_0001_0000,
                            end: 0x7f00_0001_0800,
                            perms: "r-xp".into(),
                            size_kb: 2_000,
                            pss_kb: 1_000,
                            uss_kb: 200,
                            anon_kb: 0,
                            swap_kb: 128,
                        }],
                    },
                ],
                vma_count: 5,
                rss_kb: 500_000,
                pss_kb: 91_000,
                uss_kb: 88_200,
                swap_kb: 0,
                anon_kb: 300_000,
                swap_pss_kb: 128,
                thp_kb: 8_192,
                ksm_kb: 4_096,
            },
            rollup: Rollup {
                rss_kb: 500_000,
                pss_kb: 91_000,
                uss_kb: 88_200,
                anon_kb: 300_000,
                pss_anon_kb: 60_000,
                pss_file_kb: 30_000,
                pss_shmem_kb: 1_000,
                swap_pss_kb: 128,
                thp_kb: 8_192,
                ksm_kb: 4_096,
                ..Default::default()
            },
            parse_ms: 12.0,
            at: Instant::now(),
        }),
        sweep_ms: 18.0,
        pss_ms: 42.0,
        pressure: mtop::procfs::Pressure {
            some10: 1.5,
            full10: 0.25,
        },
        scan_seq: 1,
        pid_exe: Arc::new(HashMap::from([
            (100, "/opt/google/chrome/chrome".to_string()),
            (200, "/usr/bin/node".to_string()),
            (300, "/opt/google/chrome/chrome".to_string()),
            (400, "/usr/sbin/root-daemon".to_string()),
        ])),
        pid_cgroup: Arc::new(HashMap::from([
            (
                100,
                "/user.slice/user-1000.slice/session-2.scope".to_string(),
            ),
            (200, "/system.slice".to_string()),
            (
                300,
                "/user.slice/user-1000.slice/session-2.scope".to_string(),
            ),
            (400, "/system.slice".to_string()),
        ])),
        cgroups: vec![
            CgroupMem {
                path: "/user.slice/user-1000.slice/session-2.scope".into(),
                current_kb: 2_000_000,
                anon_kb: 1_200_000,
                file_kb: 700_000,
                kernel_kb: 90_000,
                slab_kb: 60_000,
                shmem_kb: 4_000,
                limit_kb: Some(4_194_304),
                oom_kills: 0,
                oom_recent: 0,
                psi_some10: 0.0,
                psi_full10: 0.0,
            },
            CgroupMem {
                path: "/system.slice".into(),
                current_kb: 500_000,
                anon_kb: 100_000,
                file_kb: 300_000,
                kernel_kb: 80_000,
                slab_kb: 20_000,
                shmem_kb: 0,
                limit_kb: None,
                oom_kills: 2,
                oom_recent: 1,
                psi_some10: 12.5,
                psi_full10: 3.0,
            },
        ],
        covered_pss_kb: 480_000,
        covered: 2,
        denied: HashSet::from([400]),
        no_mm: HashSet::from([500]),
        at: Instant::now(),
    }
}

fn render(app: &mut App) -> String {
    let mut term = Terminal::new(TestBackend::new(120, 26)).unwrap();
    term.draw(|f| mtop::ui::draw(f, app)).unwrap();
    let buf = term.backend().buffer();
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

#[test]
fn small_nonzero_delta_renders_in_kb_instead_of_zero_megabytes() {
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.snap.as_mut().unwrap().rows[0].delta_kb = 4;

    let text = render(&mut app);
    assert!(
        text.contains("+    4k"),
        "delta missing from table:\n{text}"
    );
    assert!(
        !text.contains("+  0.0M") && !text.contains("-  0.0M"),
        "nonzero delta rendered as zero megabytes:\n{text}"
    );
}

#[test]
fn process_table_renders_sorted_with_pss_gaps_marked() {
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    let text = render(&mut app);

    assert!(text.contains("PID"), "missing pid header:\n{text}");
    assert!(text.contains("PSS/RSS"), "missing ratio column:\n{text}");
    assert!(text.contains("chrome"), "missing chrome row:\n{text}");
    // RSS sort: biggest first.
    let chrome = text.find("chrome").unwrap();
    let node = text.find("node").unwrap();
    assert!(chrome < node, "chrome should sort above node:\n{text}");
    // Long comm names must be truncated, not overflow the column.
    assert!(text.contains('…'), "long comm not truncated:\n{text}");
    // Missing PSS renders as a dash instead of a fake zero.
    assert!(text.contains('—'), "missing PSS not marked:\n{text}");
    // Header reports coverage over readable processes only, and names the
    // unreadable ones instead of letting them look like zero.
    // Denominator excludes kernel threads (2 of 4 eligible readable), so the
    // number cannot be dragged down by something sudo cannot fix.
    assert!(
        text.contains("coverage 2/4"),
        "coverage line missing:\n{text}"
    );
    assert!(text.contains("1 n/a"), "denied count missing:\n{text}");
    assert!(
        text.contains("needs root"),
        "denied hint must not promise sudo for kernel threads:\n{text}"
    );
    assert!(text.contains("n/a"), "denied rows not marked:\n{text}");
}

#[test]
fn sort_by_pss_reorders_without_pss_entries_at_the_bottom() {
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.sort = SortKey::Pss;
    let order: Vec<u32> = app.visible().iter().map(|r| r.pid).collect();
    assert_eq!(
        order,
        vec![100, 200, 300, 400, 500],
        "pss sort order wrong: {order:?}"
    );
}

#[test]
fn filter_matches_comm_and_pid() {
    let mut app = App::new(None);
    app.on_snapshot(snapshot());

    app.filter = "chr".into();
    assert_eq!(app.visible().len(), 1);

    app.filter = "20".into();
    let pids: Vec<u32> = app.visible().iter().map(|r| r.pid).collect();
    assert!(
        pids.contains(&200),
        "numeric filter should match pid: {pids:?}"
    );

    app.filter = "no-such-process".into();
    assert!(app.visible().is_empty());
    // Selection must stay valid when the list shrinks to empty.
    app.on_snapshot(snapshot());
    assert_eq!(app.sel, 0);
}

#[test]
fn detail_view_shows_grouped_vmas() {
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.detail = true;
    let text = render(&mut app);

    assert!(text.contains("MAPPING"), "missing VMA header:\n{text}");
    assert!(text.contains("[heap]"), "missing heap group:\n{text}");
    assert!(text.contains("libc.so.6"), "missing libc group:\n{text}");
    assert!(
        text.contains("vm mappings by PSS"),
        "missing detail title:\n{text}"
    );
    assert!(
        text.contains("5 VMAs in 2 groups"),
        "missing vma summary:\n{text}"
    );
}

#[test]
fn keyboard_navigation_is_clamped() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(None);
    app.on_snapshot(snapshot());

    for _ in 0..10 {
        app.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    }
    assert_eq!(app.sel, 4, "selection should clamp to the last row");

    app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    assert_eq!(app.sel, 0);

    app.on_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE));
    assert_eq!(app.sel, 4);

    let before = app.sort;
    app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_ne!(before, app.sort);
    assert_eq!(app.sel, 0, "sort change resets selection");

    assert!(
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        "q must quit"
    );
}

#[test]
fn detail_filter_narrows_mappings() {
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.detail = true;
    app.detail_filter = "libc".into();
    assert_eq!(app.detail_len(), 1, "filter should leave one group");

    let text = render(&mut app);
    assert!(
        text.contains("libc.so.6"),
        "filtered group missing:\n{text}"
    );
    assert!(
        !text.contains("[heap]"),
        "filtered-out group still rendered:\n{text}"
    );
    assert!(
        text.contains("filter libc"),
        "filter not shown in title:\n{text}"
    );
}

#[test]
fn editing_targets_the_detail_filter_not_the_process_filter() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.detail = true;

    app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
    assert!(
        app.editing && app.editing_detail,
        "f must edit the vma filter"
    );
    for c in "libc".chars() {
        app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.detail_filter, "libc");
    assert_eq!(app.filter, "", "process filter must stay untouched");
    assert_eq!(app.detail_len(), 1);

    // Esc clears the detail filter first, and only then leaves the view.
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.detail && app.detail_filter.is_empty());
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.detail, "second Esc must exit the detail view");
}

#[test]
fn pss_mix_bar_has_distinct_segment_colors() {
    // Colors are the whole point of the mix bar, and tmux/NO_COLOR can hide
    // them, so assert on the rendered buffer instead of a terminal capture.
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    let mut term = Terminal::new(TestBackend::new(120, 26)).unwrap();
    term.draw(|f| mtop::ui::draw(f, &mut app)).unwrap();
    let buf = term.backend().buffer();

    // Row 0 (chrome) is selected and therefore intentionally colourless; the
    // mix bar must still be coloured on ordinary rows.
    let mut y_row = None;
    for y in 0..buf.area.height {
        let mut line = String::new();
        for x in 0..buf.area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        if line.contains("node") && line.contains('█') {
            y_row = Some(y);
            break;
        }
    }
    let y = y_row.expect("unselected row with a bar");
    let colors: std::collections::HashSet<String> = (0..buf.area.width)
        .map(|x| &buf[(x, y)])
        .filter(|c| c.symbol() == "█")
        .map(|c| format!("{:?}", c.fg))
        .collect();

    assert!(
        colors.iter().any(|c| c.contains("Rgb(86, 180, 233)")),
        "anon segment missing: {colors:?}"
    );
    assert!(
        colors.iter().any(|c| c.contains("Rgb(0, 158, 115)")),
        "file segment missing: {colors:?}"
    );
}

#[test]
fn pss_mix_footer_has_a_legend() {
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    let text = render(&mut app);
    for label in ["mix", "anon", "file", "shmem", "swap"] {
        assert!(text.contains(label), "legend missing {label}:\n{text}");
    }
}

#[test]
fn enter_expands_a_group_into_its_individual_vmas() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.detail = true;
    assert_eq!(app.detail_len(), 2, "two groups, nothing expanded");

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.detail_len(), 4, "[heap] must expand into 2 VMAs");

    let text = render(&mut app);
    assert!(
        text.contains("7f0000000000"),
        "vma address missing:\n{text}"
    );
    assert!(text.contains('▾'), "expanded marker missing:\n{text}");
    assert!(text.contains("rw-p"), "vma perms missing:\n{text}");

    // Collapsing restores the aggregated view.
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.detail_len(), 2);

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.detail, "Esc leaves the detail view");
}

#[test]
fn expand_all_and_collapse_all() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.detail = true;
    app.on_key(KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::NONE));
    assert_eq!(app.detail_len(), 5, "2 groups + 3 VMAs");
    app.on_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
    assert_eq!(app.detail_len(), 2);
}

#[test]
fn tab_switches_to_the_cgroup_view() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(None);
    app.on_snapshot(snapshot());

    let procs = render(&mut app);
    assert!(procs.contains("PSS MIX"), "process view expected first");
    assert!(
        procs.contains("psi 1.5/0.2%"),
        "system psi missing from the header:\n{procs}"
    );

    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    let text = render(&mut app);
    assert!(text.contains("CGROUP"), "cgroup header missing:\n{text}");
    assert!(
        text.contains("session-2.scope"),
        "cgroup path missing:\n{text}"
    );
    assert!(text.contains("CURRENT"), "cgroup columns missing:\n{text}");
    assert!(text.contains("PSI10"), "psi column missing:\n{text}");
    assert!(text.contains("12.5%"), "cgroup psi not rendered:\n{text}");
    assert!(
        !text.contains("PSS MIX"),
        "process columns leaked into cgroup view"
    );
    // Biggest first, and an OOM count is shown for the unlucky slice.
    assert!(
        text.find("session-2.scope").unwrap() < text.find("/system.slice").unwrap(),
        "cgroups not sorted by current:\n{text}"
    );
    assert!(text.contains("2▲"), "recent oom marker missing:\n{text}");
    assert!(
        text.contains("oom +1 since last sample"),
        "oom delta missing from the title:\n{text}"
    );
    assert!(text.contains("max"), "unlimited limit not marked:\n{text}");
}

#[test]
fn cgroup_view_sorts_and_filters() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    // /system.slice has less anon but more file than the user slice? both less.
    app.cgroup_sort = mtop::app::CgroupSort::File;
    let order: Vec<&str> = app
        .visible_cgroups()
        .iter()
        .map(|c| c.path.as_str())
        .collect();
    assert_eq!(
        order,
        vec![
            "/user.slice/user-1000.slice/session-2.scope",
            "/system.slice"
        ]
    );

    app.filter = "system".into();
    assert_eq!(app.visible_cgroups().len(), 1);
    assert!(render(&mut app).contains("/system.slice"));
}

#[test]
fn cgroup_drill_lists_members_of_the_subtree() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        app.drill.as_deref(),
        Some("/user.slice/user-1000.slice/session-2.scope")
    );
    assert_eq!(app.cgroup_members().len(), 2, "only this cgroup's procs");

    let text = render(&mut app);
    assert!(
        text.contains("procs in cgroup subtree"),
        "drill view missing:\n{text}"
    );
    assert!(
        text.contains("oom "),
        "oom series missing from the drill summary:\n{text}"
    );
    assert!(text.contains("total"), "oom totals missing:\n{text}");
    assert!(text.contains("chrome"), "member missing:\n{text}");
    // The ratio must survive its column width and the share bar must not be
    // scaled against total RAM (which rounds a 500 MB row to zero cells).
    assert!(
        text.contains("60%"),
        "PSS/RSS clipped by the column:\n{text}"
    );
    assert!(text.contains('█'), "share bar rendered empty:\n{text}");
    assert!(
        !text.contains("node"),
        "member of another cgroup leaked:\n{text}"
    );

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.drill.is_none(), "Esc must return to the cgroup list");
    assert!(render(&mut app).contains("CGROUP"));
}

#[test]
fn drill_down_uses_subtree_membership() {
    let mut app = App::new(None);
    app.on_snapshot(snapshot());

    app.drill = Some("/user.slice".into());
    assert_eq!(
        app.cgroup_members().len(),
        2,
        "parent must include descendants"
    );

    app.drill = Some("/".into());
    assert_eq!(app.cgroup_members().len(), 4, "root subtree is everything");

    app.drill = Some("/nope".into());
    assert!(app.cgroup_members().is_empty());
}

#[test]
fn cgroup_psi_history_is_recorded_and_plotted() {
    let mut first = snapshot();
    first.cgroups[0].psi_some10 = 3.0;
    let mut second = snapshot();
    second.scan_seq = 2;
    second.cgroups[0].psi_some10 = 7.5;

    let mut app = App::new(None);
    app.on_snapshot(first);
    app.on_snapshot(second);

    let history = app
        .cgroup_history
        .get("/user.slice/user-1000.slice/session-2.scope")
        .expect("history for the cgroup");
    assert_eq!(history.len(), 2, "one sample per snapshot");
    assert_eq!(history[0], 30, "percent*10 for a 0-1000 scale");
    assert_eq!(history[1], 75);

    app.view = mtop::app::View::Cgroups;
    let text = render(&mut app);
    assert!(
        text.contains("psi 7.5%"),
        "psi trend label missing:\n{text}"
    );
}

/// Selected rows are tinted (background only) so semantic colours survive.
fn selected_row_styles(app: &mut App) -> Vec<(ratatui::style::Color, ratatui::style::Color)> {
    let mut term = Terminal::new(TestBackend::new(120, 26)).unwrap();
    term.draw(|f| mtop::ui::draw(f, app)).unwrap();
    let buf = term.backend().buffer();
    let y = (0..buf.area.height)
        .find(|y| (0..buf.area.width).any(|x| buf[(x, *y)].symbol() == "▸"))
        .expect("a selected row");
    (0..buf.area.width)
        .map(|x| (buf[(x, y)].fg, buf[(x, y)].bg))
        .collect()
}

#[test]
fn selected_row_is_tinted_without_losing_semantic_colours() {
    use mtop::ui::HL_BG;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Color;

    let mut app = App::new(None);
    app.on_snapshot(snapshot());

    let procs = selected_row_styles(&mut app);
    let tinted = procs.iter().filter(|(_, bg)| *bg == HL_BG).count();
    assert!(
        tinted > 60,
        "selected row background missing ({tinted} cells)"
    );

    // The SHARE bar keeps its colour instead of being flattened to one tone.
    let bar_cells = procs
        .iter()
        .filter(|(fg, bg)| *bg == HL_BG && matches!(fg, Color::Cyan | Color::Red | Color::Yellow))
        .count();
    assert!(
        bar_cells > 0,
        "selected row lost its bar colours: {procs:?}"
    );

    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    let cgroups = selected_row_styles(&mut app);
    let tinted = cgroups.iter().filter(|(_, bg)| *bg == HL_BG).count();
    assert!(tinted > 60, "cgroup selected row background missing");
}

#[test]
fn help_overlay_is_per_page_and_bilingual() {
    use mtop::app::View;
    use mtop::i18n::Lang;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let key = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
    let mut app = App::new(None);
    app.on_snapshot(snapshot());

    assert!(!app.on_key(key('?')), "opening help must not quit");
    let text = render(&mut app);
    assert!(
        text.contains("Help · Processes"),
        "missing help title:\n{text}"
    );
    assert!(
        text.contains("Header / status"),
        "missing help body:\n{text}"
    );

    // l switches the whole UI language, including the open overlay.
    app.on_key(key('l'));
    assert_eq!(app.lang, Lang::Zh);
    let text = render(&mut app);
    // Wide CJK cells put a blank cell after each glyph; compare compacted text.
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        compact.contains("帮助·进程"),
        "missing Chinese help:\n{text}"
    );
    assert!(
        compact.contains("顶部状态"),
        "missing Chinese body:\n{text}"
    );

    // Esc closes help; q keeps its global meaning and quits.
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.help.is_none());
    app.on_key(key('?'));
    assert!(app.on_key(key('q')));
    app.help = None;

    // The overlay follows the current page.
    app.lang = Lang::En;
    for (view, detail, drill, title) in [
        (View::Apps, false, None, "Help · Applications"),
        (View::Cgroups, false, None, "Help · cgroups v2"),
        (View::Cgroups, false, Some("/x"), "Help · cgroup drill-down"),
        (View::Procs, true, None, "Help · VMA detail"),
    ] {
        app.view = view;
        app.detail = detail;
        app.drill = drill.map(str::to_string);
        app.help = None;
        app.on_key(key('?'));
        assert!(
            render(&mut app).contains(title),
            "wrong help page for {view:?} ({title})"
        );
    }

    // l also works outside the overlay and moves the footer/titles.
    app.help = None;
    app.view = View::Procs;
    app.detail = false;
    app.drill = None;
    app.lang = Lang::En;
    app.on_key(key('l'));
    assert_eq!(app.lang, Lang::Zh);
    let compact: String = render(&mut app)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(compact.contains("进程"), "footer/title did not switch");
}

#[test]
fn help_scroll_is_clamped_by_the_renderer() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.on_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    app.on_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE));
    assert_eq!(app.help.as_ref().unwrap().scroll, u16::MAX);

    // A small terminal makes the help text wrap, so the scroll limit must be
    // computed from the wrapped line count.
    let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
    term.draw(|f| mtop::ui::draw(f, &mut app)).unwrap();
    let buf = term.backend().buffer();
    let mut text = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            text.push_str(buf[(x, y)].symbol());
        }
        text.push('\n');
    }
    assert!(
        text.contains("MemTotal"),
        "scrolling lost the help body:\n{text}"
    );
}

#[test]
fn theme_key_cycles_the_palette_and_names_it_in_the_footer() {
    use mtop::theme::Theme;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Color;

    let mut app = App::new(None);
    app.on_snapshot(snapshot());

    let mut term = Terminal::new(TestBackend::new(120, 26)).unwrap();
    term.draw(|f| mtop::ui::draw(f, &mut app)).unwrap();
    let bgs = |term: &Terminal<TestBackend>| -> HashSet<Color> {
        term.backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.bg)
            .collect()
    };
    assert!(render(&mut app).contains("theme: default"));
    assert!(!bgs(&term).contains(&Color::Rgb(0x6a, 0x2f, 0x2f)));

    app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    term.draw(|f| mtop::ui::draw(f, &mut app)).unwrap();
    assert!(render(&mut app).contains("theme: btop"));
    assert!(
        bgs(&term).contains(&Color::Rgb(0x6a, 0x2f, 0x2f)),
        "btop highlight background missing"
    );

    // Every press advances one theme; the last wraps to default.
    for expected in &Theme::ALL[1..] {
        assert!(
            render(&mut app).contains(&format!("theme: {}", expected.name)),
            "footer missing {}",
            expected.name
        );
        app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    }
    assert!(render(&mut app).contains("theme: default"));
}

#[test]
fn ansi256_rendering_uses_indexed_colours_only() {
    use mtop::theme::{ColorSupport, Theme};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Color;

    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.support = ColorSupport::Ansi256;
    app.theme = Theme::BTOP.rendered(app.support);

    let mut term = Terminal::new(TestBackend::new(120, 26)).unwrap();
    term.draw(|f| mtop::ui::draw(f, &mut app)).unwrap();
    let fgs: HashSet<Color> = term
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.fg)
        .collect();
    assert!(
        fgs.iter().any(|color| matches!(color, Color::Indexed(_))),
        "256-colour theme did not use indexed colours: {fgs:?}"
    );
    assert!(
        !fgs.iter().any(|color| matches!(color, Color::Rgb(..))),
        "256-colour output leaked RGB: {fgs:?}"
    );

    // Cycling must re-render the next theme at the same depth.
    app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    let next = Theme::BTOP.next();
    assert!(render(&mut app).contains(&format!("theme: {}", next.name)));
    term.draw(|f| mtop::ui::draw(f, &mut app)).unwrap();
    assert!(
        term.backend().buffer().content().iter().all(|cell| {
            !matches!(cell.fg, Color::Rgb(..)) && !matches!(cell.bg, Color::Rgb(..))
        })
    );
}

#[test]
fn highlight_never_falls_back_to_the_terminal_background() {
    use mtop::ui::HL_BG;
    use ratatui::style::{Color, Modifier};

    assert_ne!(
        HL_BG,
        Color::Reset,
        "highlight must not inherit the background"
    );
    assert_ne!(HL_BG, Color::Black, "black on a dark terminal is invisible");

    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    let mut term = Terminal::new(TestBackend::new(120, 26)).unwrap();
    term.draw(|f| mtop::ui::draw(f, &mut app)).unwrap();
    let buf = term.backend().buffer();
    let y = (0..buf.area.height)
        .find(|y| (0..buf.area.width).any(|x| buf[(x, *y)].symbol() == "▸"))
        .expect("a selected row");
    // Reverse video must not be used at all on the row.
    assert!(
        (0..buf.area.width).all(|x| !buf[(x, y)].modifier.contains(Modifier::REVERSED)),
        "selected row still relies on reverse video"
    );
}

#[test]
fn treemap_selection_inverts_the_tile_instead_of_reusing_highlight_bg() {
    use mtop::app::View;
    use mtop::theme::Theme;
    use ratatui::style::Color;

    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.view = View::Apps;
    app.theme = Theme::LIGHT;

    let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
    term.draw(|f| mtop::ui::draw(f, &mut app)).unwrap();
    let buf = term.backend().buffer();
    // Skip the header/summary rows and the footer: only the treemap remains.
    let tile_cells = || {
        buf.content()
            .iter()
            .skip(4 * buf.area.width as usize)
            .take((buf.area.height as usize - 6) * buf.area.width as usize)
    };
    let bgs: HashSet<Color> = tile_cells().map(|cell| cell.bg).collect();
    let p = Theme::LIGHT.palette;
    assert!(
        bgs.contains(&p.tile_fg),
        "selected tile should invert to tile_fg: {bgs:?}"
    );
    assert!(
        !bgs.contains(&p.highlight_bg),
        "treemap still uses the table highlight colour: {bgs:?}"
    );

    let fgs: HashSet<Color> = tile_cells().map(|cell| cell.fg).collect();
    assert!(
        p.treemap.iter().any(|fill| fgs.contains(fill)),
        "selected tile text should use its fill colour: {fgs:?}"
    );
}

#[test]
fn apps_view_groups_processes_and_drills_into_the_process_table() {
    use mtop::app::View;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = App::new(None);
    app.on_snapshot(snapshot());

    // procs -> cgroups -> apps
    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(matches!(app.view, View::Apps));

    let groups: Vec<String> = app.visible_apps().iter().map(|g| g.name.clone()).collect();
    assert_eq!(groups[0], "chrome", "biggest app first: {groups:?}");
    assert!(
        groups.contains(&"others (2)".to_string()),
        "tail not collapsed: {groups:?}"
    );

    let text = render(&mut app);
    assert!(text.contains("apps"), "apps view title missing:\n{text}");
    assert!(text.contains("chrome"), "tile label missing:\n{text}");

    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(app.view, View::Procs),
        "Enter must drill into procs"
    );
    assert!(app.pid_filter.is_some());
    let pids: Vec<u32> = app.visible().iter().map(|r| r.pid).collect();
    assert_eq!(pids, vec![100, 300], "only chrome processes: {pids:?}");
    assert!(render(&mut app).contains("app: chrome"));

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.pid_filter.is_none(), "Esc clears the app drill filter");
    assert_eq!(app.visible().len(), 5);
}

#[test]
fn apps_view_falls_back_to_a_list_when_narrow() {
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.view = mtop::app::View::Apps;

    let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
    term.draw(|f| mtop::ui::draw(f, &mut app)).unwrap();
    let buf = term.backend().buffer();
    let mut text = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            text.push_str(buf[(x, y)].symbol());
        }
        text.push('\n');
    }
    assert!(
        text.contains("APP"),
        "list fallback header missing:\n{text}"
    );
    assert!(
        text.contains("PROCS"),
        "list fallback columns missing:\n{text}"
    );

    // The selected row's share bar must not reuse the highlight colour.
    app.theme = mtop::theme::Theme::LIGHT;
    term.draw(|f| mtop::ui::draw(f, &mut app)).unwrap();
    let buf = term.backend().buffer();
    let p = app.theme.palette;
    let y = (0..buf.area.height)
        .find(|y| (0..buf.area.width).any(|x| buf[(x, *y)].symbol() == "▸"))
        .expect("selected app row");
    let bars: Vec<ratatui::style::Color> = (0..buf.area.width)
        .filter(|x| buf[(*x, y)].symbol() == "█")
        .map(|x| buf[(x, y)].fg)
        .collect();
    assert!(!bars.is_empty(), "selected row has no share bar");
    assert!(
        bars.iter().all(|color| *color == p.tile_fg),
        "selected share bar should contrast with the highlight: {bars:?}"
    );
    assert!(!bars.contains(&p.highlight_bg));
}

#[test]
fn apps_history_tracks_each_metric_separately() {
    use mtop::app::AppMetric;

    let mut app = App::new(None);
    let first = snapshot();
    app.on_snapshot(first.clone());

    let mut second = snapshot();
    second.rows[0].rss_kb = 900_000; // chrome's main process grew
    app.on_snapshot(second);

    // RSS moved; PSS did not (the fixture's PssInfo is unchanged).
    assert_eq!(
        app.app_series("chrome", AppMetric::Rss),
        vec![600_000, 1_000_000]
    );
    assert_eq!(
        app.app_series("chrome", AppMetric::Pss),
        vec![300_000, 300_000]
    );
    assert_eq!(
        app.app_series("chrome", AppMetric::Uss),
        vec![250_000, 250_000]
    );
    assert!(app.app_series("nope", AppMetric::Pss).is_empty());
}
