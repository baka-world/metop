//! Render tests: the whole point of TestBackend is that a TUI frame becomes
//! plain text, so layout changes can be asserted instead of eyeballed.

use std::collections::HashMap;
use std::time::Instant;

use mtop::app::App;
use mtop::model::{Detail, PssInfo, ProcRow, Snapshot, SortKey};
use mtop::procfs::{MemInfo, Rollup, SmapsReport, VmaGroup};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn rollup(rss: u64, pss: u64, uss: u64) -> Rollup {
    Rollup { rss_kb: rss, pss_kb: pss, uss_kb: uss, swap_kb: 0, anon_kb: pss }
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
                    VmaGroup { label: "[heap]".into(), size_kb: 100_000, pss_kb: 90_000, uss_kb: 88_000, count: 1 },
                    VmaGroup { label: "/usr/lib/libc.so.6".into(), size_kb: 2_000, pss_kb: 1_000, uss_kb: 200, count: 4 },
                ],
                vma_count: 5,
                rss_kb: 500_000,
                pss_kb: 91_000,
                uss_kb: 88_200,
                swap_kb: 0,
                anon_kb: 300_000,
            },
            parse_ms: 12.0,
            at: Instant::now(),
        }),
        sweep_ms: 18.0,
        pss_ms: 42.0,
        covered_pss_kb: 480_000,
        covered: 2,
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
    // Header reports coverage rather than pretending PSS is complete.
    assert!(text.contains("coverage 2/3"), "coverage line missing:\n{text}");
}

#[test]
fn sort_by_pss_reorders_without_pss_entries_at_the_bottom() {
    let mut app = App::new(None);
    app.on_snapshot(snapshot());
    app.sort = SortKey::Pss;
    let order: Vec<u32> = app.visible().iter().map(|r| r.pid).collect();
    assert_eq!(order, vec![100, 200, 300], "pss sort order wrong: {order:?}");
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
    assert_eq!(app.sel, 2, "selection should clamp to the last row");

    app.on_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    assert_eq!(app.sel, 0);

    app.on_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE));
    assert_eq!(app.sel, 2);

    let before = app.sort;
    app.on_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_ne!(before, app.sort);
    assert_eq!(app.sel, 0, "sort change resets selection");

    assert!(
        app.on_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        "q must quit"
    );
}
