use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::cgroup::CgroupMem;
use crate::procfs::{MemInfo, Pressure, SmapsReport};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Rss,
    Pss,
    Uss,
    Delta,
    Ratio,
    Pid,
    Comm,
}

impl SortKey {
    pub const ALL: [SortKey; 7] = [
        SortKey::Rss,
        SortKey::Pss,
        SortKey::Uss,
        SortKey::Delta,
        SortKey::Ratio,
        SortKey::Pid,
        SortKey::Comm,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Rss => "RSS",
            SortKey::Pss => "PSS",
            SortKey::Uss => "USS",
            SortKey::Delta => "DELTA",
            SortKey::Ratio => "PSS/RSS",
            SortKey::Pid => "PID",
            SortKey::Comm => "COMM",
        }
    }

    pub fn next(self) -> SortKey {
        let i = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Debug)]
pub struct ProcRow {
    pub pid: u32,
    pub comm: String,
    /// kB, from statm (cheap, every tick).
    pub rss_kb: u64,
    /// MB/s-ish: kB change since previous sweep.
    pub delta_kb: i64,
}

#[derive(Clone, Copy, Debug)]
pub struct PssInfo {
    pub rollup: crate::procfs::Rollup,
    pub at: Instant,
}

#[derive(Clone, Debug)]
pub struct Detail {
    pub pid: u32,
    pub comm: String,
    pub cmdline: String,
    pub report: SmapsReport,
    /// Process-level rollup, the only source of the anon/file/shmem PSS split.
    pub rollup: crate::procfs::Rollup,
    pub parse_ms: f32,
    pub at: Instant,
}

#[derive(Clone, Debug)]
pub struct DetailError {
    pub pid: u32,
    pub error: String,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub rows: Vec<ProcRow>,
    /// False on the first sweep of a process: `rows[].delta_kb` is then 0 only
    /// because no previous RSS sample exists, not because nothing changed.
    /// JSON turns those zeros into `null`.
    pub delta_available: bool,
    /// Only pids the sampler has spent page-table-walk budget on.
    pub pss: HashMap<u32, PssInfo>,
    pub mem: MemInfo,
    /// System-wide PSI for memory (/proc/pressure/memory), read every tick.
    pub pressure: Pressure,
    /// Increments on every slow scan (cgroups/pid map). The UI uses it to avoid
    /// pushing the same 5 s sample 5 times at 1 Hz.
    pub scan_seq: u64,
    /// cgroup v2 memory accounting (empty on v1 or when unreadable).
    pub cgroups: Vec<CgroupMem>,
    /// pid -> cgroup path, refreshed on the slow cadence; Arc so the per-tick
    /// snapshot clone stays cheap.
    pub pid_cgroup: Arc<HashMap<u32, String>>,
    /// pid -> executable path, same cadence.
    pub pid_exe: Arc<HashMap<u32, String>>,
    /// pid -> argv, same cadence.
    pub pid_cmdline: Arc<HashMap<u32, Vec<String>>>,
    /// pid -> start time in Unix milliseconds, same cadence.
    pub pid_start: Arc<HashMap<u32, u64>>,
    /// One focused detail for the TUI and single-`--pid` JSON.
    pub detail: Option<Detail>,
    /// More than one requested pid: JSON emits these as `details[]` while the
    /// TUI keeps using the first `detail`.
    pub details: Vec<Detail>,
    pub detail_errors: Vec<DetailError>,
    /// Why the detail view is empty (usually EACCES on a process we don't own).
    pub detail_error: Option<String>,
    pub sweep_ms: f32,
    pub pss_ms: f32,
    /// Sum of PSS for entries younger than PSS_MAX_AGE, and how many that is.
    pub covered_pss_kb: u64,
    pub covered: usize,
    /// Pids whose smaps_rollup needs privileges (EACCES). `sudo` fixes these.
    pub denied: HashSet<u32>,
    /// Pids with no address space (kernel threads) - no privilege will help.
    pub no_mm: HashSet<u32>,
    pub at: Instant,
}

#[derive(Clone, Copy, Debug)]
pub enum Focus {
    Set { pid: Option<u32>, want_detail: bool },
}

/// A cached rollup younger than this is shown as authoritative (dimmed past it).
pub const PSS_MAX_AGE: Duration = Duration::from_millis(6000);
/// /proc/PID/smaps_rollup requires ptrace-read permission: other users'
/// processes (and kernel threads) answer EACCES/ESRCH. Do not retry them every
/// tick — measured, 234 of 450 pids are unreadable as a normal user, and
/// retrying them consumed half the per-tick budget and starved readable pids.
pub const PSS_DENIED_TTL: Duration = Duration::from_secs(30);
/// Never re-walk a process's page table more often than this, unless focused.
/// With ~465 pids a full set costs ~700 ms, so the 150 ms/tick budget refreshes
/// everything about every 5 s — stale numbers, not missing ones.
pub const PSS_REFRESH_AFTER: Duration = Duration::from_millis(2000);
