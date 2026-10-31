//! Invariants against the real /proc of the test process.

use mtop::procfs;

#[test]
fn list_pids_contains_self() {
    let pid = std::process::id();
    assert!(procfs::list_pids().contains(&pid), "own pid {pid} missing");
}

#[test]
fn self_rollup_is_internally_consistent() {
    let r = procfs::read_rollup(std::process::id()).expect("read own smaps_rollup");
    assert!(r.rss_kb > 0, "rss must be positive: {r:?}");
    assert!(r.pss_kb > 0, "pss must be positive: {r:?}");
    // PSS is RSS with shared pages weighted down, so it can never exceed RSS.
    assert!(
        r.pss_kb <= r.rss_kb,
        "pss {} > rss {}: {}",
        r.pss_kb,
        r.rss_kb,
        r.rss_kb
    );
    assert!(r.uss_kb <= r.rss_kb, "uss {} > rss {}", r.uss_kb, r.rss_kb);
    assert!(r.anon_kb <= r.rss_kb, "anon {} > rss {}", r.anon_kb, r.rss_kb);
}

#[test]
fn statm_rss_agrees_with_rollup_within_noise() {
    let pid = std::process::id();
    let page_kb = procfs::page_size() / 1024;
    let statm = procfs::read_rss(pid, page_kb).expect("statm");
    let rollup = procfs::read_rollup(pid).expect("rollup");
    let diff = statm.abs_diff(rollup.rss_kb);
    assert!(
        diff * 100 <= rollup.rss_kb.max(1) * 10,
        "statm rss {statm} kB vs rollup {} kB (diff {diff})",
        rollup.rss_kb
    );
}

#[test]
fn smaps_groups_reconstruct_the_rollup() {
    let pid = std::process::id();
    let rollup = procfs::read_rollup(pid).expect("rollup");
    let report = procfs::read_smaps(pid).expect("smaps");

    assert!(report.vma_count > 0, "no VMAs parsed");
    assert!(!report.groups.is_empty(), "no groups produced");

    let sum_pss: u64 = report.groups.iter().map(|g| g.pss_kb).sum();
    let sum_uss: u64 = report.groups.iter().map(|g| g.uss_kb).sum();
    assert_eq!(sum_pss, report.pss_kb, "group PSS must sum to the total");
    assert_eq!(sum_uss, report.uss_kb, "group USS must sum to the total");

    // smaps and smaps_rollup are two separate kernel walks, so allow drift.
    let allow = (rollup.pss_kb / 5).max(4096);
    assert!(
        report.pss_kb.abs_diff(rollup.pss_kb) <= allow,
        "smaps pss {} vs rollup {} (allow {allow})",
        report.pss_kb,
        rollup.pss_kb
    );
}

#[test]
fn meminfo_is_plausible() {
    let m = procfs::read_meminfo();
    assert!(m.total_kb > 0);
    assert!(m.avail_kb <= m.total_kb);
    assert!(m.cached_kb <= m.total_kb);
}

#[test]
fn kernel_threads_report_zero_rss_without_panicking() {
    // pid 2 (kthreadd) is a kernel thread; statm exists, memory is ~0.
    if let Some(rss) = procfs::read_rss(2, procfs::page_size() / 1024) {
        assert!(rss < 1024 * 1024, "kernel thread rss unexpectedly large: {rss}");
    }
}

#[test]
fn meminfo_is_normalized_to_kb() {
    // procfs::Meminfo normalizes to bytes while the UI works in kB; a missing
    // /1024 shows up as 15.7 TB instead of 15 GB. Pin it against the kernel.
    let raw = std::fs::read_to_string("/proc/meminfo").expect("/proc/meminfo");
    let kernel_total: u64 = raw
        .lines()
        .find(|l| l.starts_with("MemTotal:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .expect("MemTotal");
    let ours = procfs::read_meminfo();
    assert_eq!(ours.total_kb, kernel_total, "mem_total not converted to kB");
    assert!(
        (1_000_000..10_000_000_000).contains(&ours.total_kb),
        "implausible total: {} kB",
        ours.total_kb
    );
}
