use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::model::{
    Detail, Focus, PssInfo, ProcRow, Snapshot, PSS_DENIED_TTL, PSS_MAX_AGE, PSS_REFRESH_AFTER,
};
use crate::procfs;

pub const TICK: Duration = Duration::from_millis(1000);
/// Per-tick ceiling for page-table walks. Measured: one walk is ~1.5 ms average
/// but 16-19 ms for a browser process, so a fixed row count is the wrong knob.
pub const PSS_BUDGET: Duration = Duration::from_millis(150);
/// Detail view re-reads full smaps (~30-40 ms), so it is refreshed lazily.
pub const DETAIL_MAX_AGE: Duration = Duration::from_millis(2000);
const MEMINFO_EVERY: u32 = 5;

struct State {
    focus: Option<u32>,
    want_detail: bool,
    pss: HashMap<u32, PssInfo>,
    prev_rss: HashMap<u32, u64>,
    detail: Option<Detail>,
    detail_error: Option<String>,
    denied: HashMap<u32, Instant>,
    ticks: u32,
    mem: procfs::MemInfo,
    cgroups: Vec<crate::cgroup::CgroupMem>,
    pid_cgroup: std::sync::Arc<HashMap<u32, String>>,
    oom_prev: HashMap<String, u64>,
    scan_seq: u64,
}

impl State {
    fn new() -> Self {
        State {
            focus: None,
            want_detail: false,
            pss: HashMap::new(),
            prev_rss: HashMap::new(),
            detail: None,
            detail_error: None,
            denied: HashMap::new(),
            ticks: 0,
            mem: procfs::read_meminfo(),
            cgroups: crate::cgroup::scan(std::path::Path::new(crate::cgroup::ROOT)),
            pid_cgroup: std::sync::Arc::new(crate::cgroup::pid_map()),
            oom_prev: HashMap::new(),
            scan_seq: 1,
        }
    }
}

/// One synchronous snapshot with a permissive budget, for `--json`.
///
/// The TUI spreads page-table walks over ticks; a one-shot caller cannot wait
/// for that, so the whole readable set is walked in a single pass (~700 ms).
pub fn snapshot_once(focus: Option<u32>, want_detail: bool) -> Snapshot {
    let mut st = State::new();
    st.focus = focus;
    st.want_detail = want_detail;
    tick(&mut st, Duration::from_secs(2))
}

pub fn spawn(rx: Receiver<Focus>, tx: Sender<Snapshot>) {
    thread::Builder::new()
        .name("sampler".into())
        .spawn(move || run(rx, tx))
        .expect("spawn sampler thread");
}

fn run(rx: Receiver<Focus>, tx: Sender<Snapshot>) {
    let mut st = State::new();
    let mut next = Instant::now();
    let mut urgent = false;

    loop {
        let wait = if urgent {
            Duration::ZERO
        } else {
            next.saturating_duration_since(Instant::now())
        };
        match rx.recv_timeout(wait) {
            Ok(Focus::Set { pid, want_detail }) => {
                st.focus = pid;
                st.want_detail = want_detail;
                while let Ok(Focus::Set { pid, want_detail }) = rx.try_recv() {
                    st.focus = pid;
                    st.want_detail = want_detail;
                }
                urgent = true;
            }
            Err(RecvTimeoutError::Timeout) => {
                let snap = tick(&mut st, PSS_BUDGET);
                if tx.send(snap).is_err() {
                    return;
                }
                next = Instant::now() + TICK;
                urgent = false;
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Order in which to spend the page-table budget this tick.
///
/// Focused pid first (always), then pids with no cached rollup in RSS order —
/// the biggest unknowns are exactly the rows whose rank a PSS sort would get
/// wrong — then cached pids oldest-first. Fresh entries are skipped so the
/// budget is never burned re-reading the same process.
pub fn pss_targets(
    focus: Option<u32>,
    rows: &[ProcRow],
    pss: &HashMap<u32, PssInfo>,
    denied: &HashMap<u32, Instant>,
    refresh_after: Duration,
    denied_ttl: Duration,
) -> Vec<u32> {
    // Denied pids keep their place in the row list but must not consume the
    // budget again until the denial expires.
    let allowed = |pid: u32| !denied.get(&pid).is_some_and(|at| at.elapsed() < denied_ttl);
    let mut order: Vec<u32> = Vec::with_capacity(rows.len() + 1);
    if let Some(pid) = focus
        && allowed(pid)
    {
        order.push(pid);
    }
    order.extend(
        rows.iter()
            .filter(|r| !pss.contains_key(&r.pid) && allowed(r.pid))
            .map(|r| r.pid),
    );
    let mut stale: Vec<(Instant, u32)> = rows
        .iter()
        .filter_map(|r| {
            let info = pss.get(&r.pid)?;
            (info.at.elapsed() > refresh_after && allowed(r.pid)).then_some((info.at, r.pid))
        })
        .collect();
    stale.sort_by_key(|(at, _)| *at);
    order.extend(stale.into_iter().map(|(_, pid)| pid));
    order
}

fn tick(st: &mut State, pss_budget: Duration) -> Snapshot {
    let page_kb = procfs::page_size() / 1024;
    let t_sweep = Instant::now();

    let pids = procfs::list_pids();
    let mut rows: Vec<ProcRow> = Vec::with_capacity(pids.len());
    for pid in pids {
        let (Some(rss_kb), Some(comm)) = (procfs::read_rss(pid, page_kb), procfs::read_comm(pid)) else {
            continue;
        };
        let delta_kb = match st.prev_rss.get(&pid) {
            Some(prev) => rss_kb as i64 - *prev as i64,
            None => 0,
        };
        st.prev_rss.insert(pid, rss_kb);
        rows.push(ProcRow { pid, comm, rss_kb, delta_kb });
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.rss_kb));
    let sweep_ms = t_sweep.elapsed().as_secs_f32() * 1000.0;

    // Keep the previous map from growing forever.
    st.prev_rss.retain(|pid, _| rows.iter().any(|r| r.pid == *pid));

    // --- Tier 2: exact PSS/USS, budgeted -------------------------------------
    let t_pss = Instant::now();
    for pid in pss_targets(
        st.focus,
        &rows,
        &st.pss,
        &st.denied,
        PSS_REFRESH_AFTER,
        PSS_DENIED_TTL,
    ) {
        if t_pss.elapsed() > pss_budget && Some(pid) != st.focus {
            break;
        }
        match procfs::read_rollup(pid) {
            Some(rollup) => {
                st.pss.insert(pid, PssInfo { rollup, at: Instant::now() });
                st.denied.remove(&pid);
            }
            None => {
                st.pss.remove(&pid);
                st.denied.insert(pid, Instant::now());
            }
        }
    }
    // Forget dead pids so coverage numbers stay honest.
    let alive: std::collections::HashSet<u32> = rows.iter().map(|r| r.pid).collect();
    st.pss.retain(|pid, _| alive.contains(pid));
    st.denied
        .retain(|pid, at| alive.contains(pid) && at.elapsed() < PSS_DENIED_TTL);
    let pss_ms = t_pss.elapsed().as_secs_f32() * 1000.0;

    // --- Detail: full smaps for the focused pid, only when asked -------------
    let mut detail = None;
    if st.want_detail {
        if let Some(pid) = st.focus {
            let stale = st.detail.as_ref().is_none_or(|d| {
                d.pid != pid || d.at.elapsed() > DETAIL_MAX_AGE
            });
            if stale {
                let t = Instant::now();
                match procfs::read_smaps(pid) {
                    Ok(report) => {
                        let rollup = st
                            .pss
                            .get(&pid)
                            .map(|p| p.rollup)
                            .or_else(|| procfs::read_rollup(pid))
                            .unwrap_or_default();
                        st.detail = Some(Detail {
                            pid,
                            comm: rows.iter().find(|r| r.pid == pid).map(|r| r.comm.clone()).unwrap_or_default(),
                            cmdline: procfs::read_cmdline(pid),
                            report,
                            rollup,
                            parse_ms: t.elapsed().as_secs_f32() * 1000.0,
                            at: Instant::now(),
                        });
                        st.detail_error = None;
                    }
                    Err(e) => {
                        st.detail = None;
                        st.detail_error = Some(format!("/proc/{pid}/smaps: {e}"));
                    }
                }
            }
        }
    } else {
        st.detail = None;
        st.detail_error = None;
    }
    if let Some(d) = &st.detail
        && st.focus == Some(d.pid)
    {
        detail = Some(d.clone());
    }

    st.ticks = st.ticks.wrapping_add(1);
    if st.ticks.is_multiple_of(MEMINFO_EVERY) {
        st.mem = procfs::read_meminfo();
        st.cgroups = crate::cgroup::scan(std::path::Path::new(crate::cgroup::ROOT));
        // OOM events are cumulative; report the delta since the last scan and
        // treat the first sighting as "no events" rather than a spike.
        for cg in &mut st.cgroups {
            let prev = st.oom_prev.get(&cg.path).copied().unwrap_or(cg.oom_kills);
            cg.oom_recent = cg.oom_kills.saturating_sub(prev);
        }
        st.oom_prev = st
            .cgroups
            .iter()
            .map(|c| (c.path.clone(), c.oom_kills))
            .collect();
        st.scan_seq = st.scan_seq.wrapping_add(1);
        st.pid_cgroup = std::sync::Arc::new(crate::cgroup::pid_map());
    }

    let fresh: Vec<&PssInfo> = st.pss.values().filter(|p| p.at.elapsed() < PSS_MAX_AGE).collect();
    let covered_pss_kb = fresh.iter().map(|p| p.rollup.pss_kb).sum();
    let covered = fresh.len();

    Snapshot {
        rows,
        pss: st.pss.clone(),
        mem: st.mem.clone(),
        pressure: procfs::read_pressure(),
        scan_seq: st.scan_seq,
        cgroups: st.cgroups.clone(),
        pid_cgroup: st.pid_cgroup.clone(),
        detail,
        detail_error: if st.want_detail { st.detail_error.clone() } else { None },
        sweep_ms,
        pss_ms,
        covered_pss_kb,
        covered,
        denied: st.denied.keys().copied().collect(),
        at: Instant::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procfs::Rollup;
    use std::time::Duration;

    fn row(pid: u32, rss_kb: u64) -> ProcRow {
        ProcRow { pid, comm: format!("p{pid}"), rss_kb, delta_kb: 0 }
    }

    fn cached(age: Duration) -> PssInfo {
        PssInfo { rollup: Rollup::default(), at: Instant::now() - age }
    }

    #[test]
    fn focus_is_always_first_even_when_fresh() {
        let rows = [row(2, 100), row(1, 300)];
        let pss = HashMap::from([(2, cached(Duration::ZERO))]);
        let got = pss_targets(
            Some(2),
            &rows,
            &pss,
            &HashMap::new(),
            Duration::from_secs(2),
            Duration::from_secs(30),
        );
        assert_eq!(got[0], 2, "focus must be sampled first: {got:?}");
        assert!(got.contains(&1), "unsampled pid must be queued: {got:?}");
    }

    #[test]
    fn unsampled_rows_are_queued_in_rss_order() {
        // rows arrive sorted by rss desc; the biggest unknown is the row a PSS
        // sort would misrank, so it must be resolved first.
        let rows = [row(10, 500), row(11, 400), row(12, 300)];
        let got = pss_targets(
            None,
            &rows,
            &HashMap::new(),
            &HashMap::new(),
            Duration::from_secs(2),
            Duration::from_secs(30),
        );
        assert_eq!(got, vec![10, 11, 12]);
    }

    #[test]
    fn stale_entries_go_oldest_first_and_fresh_ones_are_skipped() {
        let rows = [row(1, 500), row(2, 400), row(3, 300)];
        let pss = HashMap::from([
            (1, cached(Duration::from_secs(10))),
            (2, cached(Duration::from_secs(3))),
            (3, cached(Duration::from_secs(1))),
        ]);
        let got = pss_targets(
            None,
            &rows,
            &pss,
            &HashMap::new(),
            Duration::from_secs(2),
            Duration::from_secs(30),
        );
        assert_eq!(got, vec![1, 2], "oldest first, fresh pid 3 untouched: {got:?}");
    }

    #[test]
    fn empty_system_yields_no_targets() {
        assert!(
            pss_targets(
                None,
                &[],
                &HashMap::new(),
                &HashMap::new(),
                Duration::from_secs(2),
                Duration::from_secs(30)
            )
            .is_empty()
        );
    }

    #[test]
    fn denied_pids_do_not_consume_the_budget_until_the_denial_expires() {
        // 234 of ~450 pids are EACCES as a normal user; retrying them every
        // tick is what starved the readable ones.
        let rows = [row(1, 500), row(2, 400), row(3, 300)];
        let denied = HashMap::from([
            (1, Instant::now()),
            (3, Instant::now() - Duration::from_secs(31)),
        ]);
        let got = pss_targets(
            Some(1),
            &rows,
            &HashMap::new(),
            &denied,
            Duration::from_secs(2),
            Duration::from_secs(30),
        );
        assert_eq!(got, vec![2, 3], "fresh denials skipped, expired retried: {got:?}");
    }
}
