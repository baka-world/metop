//! Render tests: the whole point of TestBackend is that a TUI frame becomes
//! plain text, so layout changes can be asserted instead of eyeballed.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use mtop::app::App;
use mtop::cgroup::CgroupMem;
use mtop::model::{Detail, PssInfo, ProcRow, Snapshot, SortKey};
use mtop::procfs::{MemInfo, Rollup, SmapsReport, Vma, VmaGroup};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

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
    pss.insert(100, PssInfo { rollup: rollup(500_000, 300_000, 250_000), at: Instant::now() });
    pss.insert(200, PssInfo { rollup: rollup(200_000, 180_000, 170_000), at: Instant::now() });
    // pid 300 deliberately has no PSS entry: the table must show a dash.
    Snapshot {
        rows: vec![
            ProcRow { pid: 100, comm: "chrome".into(), rss_kb: 500_000, delta_kb: 4096 },
            ProcRow { pid: 200, comm: "node".into(), rss_kb: 200_000, delta_kb: -2048 },
            ProcRow { pid: 300, comm: "sleepy-daemon-that-has-a-very-long-name".into(), rss_kb: 100_000, delta_kb: 0 },
            // pid 400 is unreadable (EACCES), not merely unsampled.
            ProcRow { pid: 400, comm: "root-owned-daemon".into(), rss_kb: 50_000, delta_kb: 0 },
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
                            Vma { start: 0x7f00_0000_0000, end: 0x7f00_0000_1000, perms: "rw-p".into(), size_kb: 60_000, pss_kb: 60_000, uss_kb: 59_000, anon_kb: 60_000, swap_kb: 0 },
                            Vma { start: 0x7f00_0000_2000, end: 0x7f00_0000_3000, perms: "rw-p".into(), size_kb: 40_000, pss_kb: 30_000, uss_kb: 29_000, anon_kb: 30_000, swap_kb: 0 },
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
                        vmas: vec![Vma { start: 0x7f00_0001_0000, end: 0x7f00_0001_0800, perms: "r-xp".into(), size_kb: 2_000, pss_kb: 1_000, uss_kb: 200, anon_kb: 0, swap_kb: 128 }],
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
        pid_cgroup: Arc::new(HashMap::from([
            (100, "/user.slice/user-1000.slice/session-2.scope".to_string()),
            (200, "/system.slice".to_string()),
            (300, "/user.slice/user-1000.slice/session-2.scope".to_string()),
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
            },
        ],
        covered_pss_kb: 480_000,
        covered: 2,
        denied: HashSet::from([400]),
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
    assert!(text.contains("coverage 2/3"), "coverage line missing:\n{text}");
    assert!(text.contains("1 n/a"), "denied count missing:\n{text}");
    assert!(text.contains("n/a"), "denied rows not marked:\n{text}");
}

#[test]
fn sort_by_pss_reorders_without_pss_entries_at_the_bottom() {
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.sort = SortKey::Pss;
    let order: Vec<u32> = app.visible().iter().map(|r| r.pid).collect();
    assert_eq!(order, vec![100, 200, 300, 400], "pss sort order wrong: {order:?}");
}

#[test]
fn filter_matches_comm_and_pid() {
    let mut app = App::new(None);
    app.on_snapshot(snapshot());

    app.filter = "chr".into();
    assert_eq!(app.visible().len(), 1);

    app.filter = "20".into();
    let pids: Vec<u32> = app.visible().iter().map(|r| r.pid).collect();
    assert!(pids.contains(&200), "numeric filter should match pid: {pids:?}");

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
    assert!(text.contains("vm mappings by PSS"), "missing detail title:\n{text}");
    assert!(text.contains("5 VMAs in 2 groups"), "missing vma summary:\n{text}");
}

#[test]
fn keyboard_navigation_is_clamped() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(None);
    app.on_snapshot(snapshot());

    for _ in 0..10 {
        app.on_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    }
    assert_eq!(app.sel, 3, "selection should clamp to the last row");

    app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    assert_eq!(app.sel, 0);

    app.on_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE));
    assert_eq!(app.sel, 3);

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
    assert!(text.contains("libc.so.6"), "filtered group missing:\n{text}");
    assert!(!text.contains("[heap]"), "filtered-out group still rendered:\n{text}");
    assert!(text.contains("filter libc"), "filter not shown in title:\n{text}");
}

#[test]
fn editing_targets_the_detail_filter_not_the_process_filter() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.detail = true;

    app.on_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
    assert!(app.editing && app.editing_detail, "f must edit the vma filter");
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

    let mut y_row = None;
    for y in 0..buf.area.height {
        let mut line = String::new();
        for x in 0..buf.area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        if line.contains("chrome") && line.contains('█') {
            y_row = Some(y);
            break;
        }
    }
    let y = y_row.expect("chrome row with a bar");
    let colors: std::collections::HashSet<String> = (0..buf.area.width)
        .map(|x| &buf[(x, y)])
        .filter(|c| c.symbol() == "█")
        .map(|c| format!("{:?}", c.fg))
        .collect();

    assert!(
        colors.iter().any(|c| c.contains("Magenta")),
        "anon segment missing: {colors:?}"
    );
    assert!(
        colors.iter().any(|c| c.contains("Cyan")),
        "file segment missing: {colors:?}"
    );
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
    assert!(text.contains("7f0000000000"), "vma address missing:\n{text}");
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

    app.on_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    let text = render(&mut app);
    assert!(text.contains("CGROUP"), "cgroup header missing:\n{text}");
    assert!(text.contains("session-2.scope"), "cgroup path missing:\n{text}");
    assert!(text.contains("CURRENT"), "cgroup columns missing:\n{text}");
    assert!(!text.contains("PSS MIX"), "process columns leaked into cgroup view");
    // Biggest first, and an OOM count is shown for the unlucky slice.
    assert!(
        text.find("session-2.scope").unwrap() < text.find("/system.slice").unwrap(),
        "cgroups not sorted by current:\n{text}"
    );
    assert!(text.contains("   2"), "oom_kills not rendered:\n{text}");
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
    let order: Vec<&str> = app.visible_cgroups().iter().map(|c| c.path.as_str()).collect();
    assert_eq!(order, vec!["/user.slice/user-1000.slice/session-2.scope", "/system.slice"]);

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
    assert!(text.contains("procs in cgroup subtree"), "drill view missing:\n{text}");
    assert!(text.contains("chrome"), "member missing:\n{text}");
    // The ratio must survive its column width and the share bar must not be
    // scaled against total RAM (which rounds a 500 MB row to zero cells).
    assert!(text.contains("60%"), "PSS/RSS clipped by the column:\n{text}");
    assert!(text.contains('█'), "share bar rendered empty:\n{text}");
    assert!(!text.contains("node"), "member of another cgroup leaked:\n{text}");

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.drill.is_none(), "Esc must return to the cgroup list");
    assert!(render(&mut app).contains("CGROUP"));
}

#[test]
fn drill_down_uses_subtree_membership() {
    let mut app = App::new(None);
    app.on_snapshot(snapshot());

    app.drill = Some("/user.slice".into());
    assert_eq!(app.cgroup_members().len(), 2, "parent must include descendants");

    app.drill = Some("/".into());
    assert_eq!(app.cgroup_members().len(), 4, "root subtree is everything");

    app.drill = Some("/nope".into());
    assert!(app.cgroup_members().is_empty());
}
