//! /proc access is delegated to the `procfs` crate (typed, maintained, MIT).
//! This module only maps its output onto the two-tier cost model:
//!   statm        -> cheap RSS sweep, no page-table walk
//!   smaps_rollup -> exact Rss/Pss/USS, walks the page table
//!   smaps        -> per-VMA breakdown, grouped for display

use std::collections::HashMap;

use procfs::process::{MMapPath, Process, all_processes};
use procfs::{Current, Meminfo};

/// Memory page size, in bytes.
pub fn page_size() -> u64 {
    procfs::page_size()
}

pub fn list_pids() -> Vec<u32> {
    let mut v: Vec<u32> = all_processes()
        .map(|iter| {
            iter.filter_map(|p| p.ok())
                .filter_map(|p| u32::try_from(p.pid()).ok())
                .collect()
        })
        .unwrap_or_default();
    v.sort_unstable();
    v
}

pub fn read_comm(pid: u32) -> Option<String> {
    // Deliberately not Process::stat(): that parses ~50 numeric fields per pid
    // and measured 4x slower for the whole-system sweep. comm is one string.
    let s = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    let s = s.trim_end_matches('\n');
    (!s.is_empty()).then(|| s.replace('\n', " "))
}

/// Resident set in kB from statm. Reads mm counters, no page-table walk.
///
/// Read directly rather than via `Process::statm()`: `Process::new` issues an
/// extra openat(O_PATH) per pid and allocated ~3x the sweep time (43 ms vs
/// 15 ms for 465 pids). This is one integer from one line, not a format.
pub fn read_rss(pid: u32, page_kb: u64) -> Option<u64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let pages: u64 = s.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * page_kb)
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Rollup {
    pub rss_kb: u64,
    pub pss_kb: u64,
    /// Private_Clean + Private_Dirty: memory only this process pays for.
    pub uss_kb: u64,
    pub swap_kb: u64,
    /// Anonymous (heap/stack/malloc) resident.
    pub anon_kb: u64,
    /// PSS split: these three sum to `pss_kb`.
    pub pss_anon_kb: u64,
    pub pss_file_kb: u64,
    pub pss_shmem_kb: u64,
    /// Swap accounted with PSS weighting (more accurate than Swap for shared pages).
    pub swap_pss_kb: u64,
    /// Transparent huge pages holding anonymous memory.
    pub thp_kb: u64,
    /// Pages merged by KSM: memory that exists but is shared away.
    pub ksm_kb: u64,
}

/// smaps field values are in bytes.
fn bytes(map: &HashMap<String, u64>, key: &str) -> u64 {
    map.get(key).copied().unwrap_or(0)
}

const KB: u64 = 1024;

/// Why a process has no PSS. The distinction matters: `Denied` becomes readable
/// under sudo, `NoMemoryMap` (kernel threads) never does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollupError {
    /// EACCES: needs CAP_SYS_PTRACE, i.e. run as root.
    Denied,
    /// The process has no address space (kernel thread) or is already gone.
    NoMemoryMap,
    Other,
}

fn classify(err: &procfs::ProcError) -> RollupError {
    use procfs::ProcError as E;
    match err {
        E::PermissionDenied(_) => RollupError::Denied,
        E::NotFound(_) => RollupError::NoMemoryMap,
        _ => RollupError::Other,
    }
}

/// Exact Rss/Pss/USS. Walks the page table: ~1.5 ms average, up to ~19 ms for
/// a 520 MB browser process.
pub fn read_rollup(pid: u32) -> Result<Rollup, RollupError> {
    let p = Process::new(pid as i32).map_err(|e| classify(&e))?;
    let maps = p.smaps_rollup().map_err(|e| classify(&e))?;
    let map = &maps
        .memory_map_rollup
        .0
        .first()
        .ok_or(RollupError::NoMemoryMap)?
        .extension
        .map;
    Ok(Rollup {
        rss_kb: bytes(map, "Rss") / KB,
        pss_kb: bytes(map, "Pss") / KB,
        uss_kb: (bytes(map, "Private_Clean") + bytes(map, "Private_Dirty")) / KB,
        swap_kb: bytes(map, "Swap") / KB,
        anon_kb: bytes(map, "Anonymous") / KB,
        pss_anon_kb: bytes(map, "Pss_Anon") / KB,
        pss_file_kb: bytes(map, "Pss_File") / KB,
        pss_shmem_kb: bytes(map, "Pss_Shmem") / KB,
        swap_pss_kb: bytes(map, "SwapPss") / KB,
        thp_kb: bytes(map, "AnonHugePages") / KB,
        ksm_kb: bytes(map, "KSM") / KB,
    })
}

/// One virtual memory area, as listed under a group in the detail view.
#[derive(Clone, Debug)]
pub struct Vma {
    pub start: u64,
    pub end: u64,
    /// "rwxp"-style: read/write/execute plus private or shared.
    pub perms: String,
    /// Virtual size (kernel `Size`), not resident memory.
    pub vsize_kb: u64,
    pub rss_kb: u64,
    pub pss_kb: u64,
    pub uss_kb: u64,
    pub anon_kb: u64,
    pub swap_kb: u64,
}

#[derive(Clone, Debug)]
pub struct VmaGroup {
    pub label: String,
    /// Sum of VMA virtual sizes; can be sparse/uncommitted.
    pub vsize_kb: u64,
    pub rss_kb: u64,
    pub pss_kb: u64,
    pub uss_kb: u64,
    pub anon_kb: u64,
    pub swap_kb: u64,
    pub count: u32,
    /// Individual areas, sorted by PSS descending (filled when expanded).
    pub vmas: Vec<Vma>,
}

fn perms_string(perms: procfs::process::MMPermissions) -> String {
    use procfs::process::MMPermissions as P;
    let mut s = String::with_capacity(4);
    s.push(if perms.contains(P::READ) { 'r' } else { '-' });
    s.push(if perms.contains(P::WRITE) { 'w' } else { '-' });
    s.push(if perms.contains(P::EXECUTE) { 'x' } else { '-' });
    s.push(if perms.contains(P::SHARED) { 's' } else { 'p' });
    s
}

#[derive(Default, Clone, Debug)]
pub struct SmapsReport {
    pub groups: Vec<VmaGroup>,
    pub vma_count: u32,
    pub rss_kb: u64,
    pub pss_kb: u64,
    pub uss_kb: u64,
    pub swap_kb: u64,
    pub anon_kb: u64,
    // NOTE: Pss_Anon / Pss_File / Pss_Shmem are only emitted by smaps_rollup,
    // not by smaps; the per-kind split therefore comes from Rollup, never from
    // summing per-VMA fields (which are simply absent and would read as 0).
    pub swap_pss_kb: u64,
    pub thp_kb: u64,
    pub ksm_kb: u64,
}

fn label(path: &MMapPath) -> String {
    match path {
        MMapPath::Path(path) => path.display().to_string(),
        MMapPath::Heap => "[heap]".into(),
        MMapPath::Stack => "[stack]".into(),
        MMapPath::TStack(tid) => format!("[stack:{tid}]"),
        MMapPath::Vdso => "[vdso]".into(),
        MMapPath::Vvar => "[vvar]".into(),
        MMapPath::Vsyscall => "[vsyscall]".into(),
        MMapPath::Rollup => "[rollup]".into(),
        MMapPath::Anonymous => "[anon]".into(),
        MMapPath::Vsys(key) => format!("[vsys:{key}]"),
        MMapPath::Other(other) => other.clone(),
    }
}

/// Full per-VMA walk grouped by mapping name. ~30-40 ms on a large process, so
/// only ever called on demand for the focused pid.
pub fn read_smaps(pid: u32) -> Result<SmapsReport, procfs::ProcError> {
    let p = Process::new(pid as i32)?;
    let maps = p.smaps()?;
    let mut report = SmapsReport::default();
    let mut groups: Vec<VmaGroup> = Vec::with_capacity(64);
    let mut idx: HashMap<String, usize> = HashMap::with_capacity(64);

    for m in maps.0 {
        let map = &m.extension.map;
        report.vma_count += 1;
        report.rss_kb += bytes(map, "Rss") / KB;
        report.pss_kb += bytes(map, "Pss") / KB;
        report.uss_kb += (bytes(map, "Private_Clean") + bytes(map, "Private_Dirty")) / KB;
        report.swap_kb += bytes(map, "Swap") / KB;
        report.anon_kb += bytes(map, "Anonymous") / KB;
        report.swap_pss_kb += bytes(map, "SwapPss") / KB;
        report.thp_kb += bytes(map, "AnonHugePages") / KB;
        report.ksm_kb += bytes(map, "KSM") / KB;

        let key = label(&m.pathname);
        let pss_kb = bytes(map, "Pss") / KB;
        let uss_kb = (bytes(map, "Private_Clean") + bytes(map, "Private_Dirty")) / KB;
        let vsize_kb = bytes(map, "Size") / KB;
        let rss_kb = bytes(map, "Rss") / KB;
        let anon_kb = bytes(map, "Anonymous") / KB;
        let swap_kb = bytes(map, "Swap") / KB;
        let vma = Vma {
            start: m.address.0,
            end: m.address.1,
            perms: perms_string(m.perms),
            vsize_kb,
            rss_kb,
            pss_kb,
            uss_kb,
            anon_kb,
            swap_kb,
        };
        match idx.get(&key) {
            Some(&i) => {
                let g = &mut groups[i];
                g.vsize_kb += vsize_kb;
                g.rss_kb += rss_kb;
                g.pss_kb += pss_kb;
                g.uss_kb += uss_kb;
                g.anon_kb += anon_kb;
                g.swap_kb += swap_kb;
                g.count += 1;
                g.vmas.push(vma);
            }
            None => {
                idx.insert(key.clone(), groups.len());
                groups.push(VmaGroup {
                    label: key,
                    vsize_kb,
                    rss_kb,
                    pss_kb,
                    uss_kb,
                    anon_kb,
                    swap_kb,
                    count: 1,
                    vmas: vec![vma],
                });
            }
        }
    }

    groups.sort_by_key(|g| std::cmp::Reverse(g.pss_kb));
    for g in &mut groups {
        g.vmas.sort_by_key(|v| std::cmp::Reverse(v.pss_kb));
    }
    report.groups = groups;
    Ok(report)
}

/// pid -> /proc/<pid>/exe. ~450 readlinks cost a few ms, so this runs on the
/// slow (5 s) cadence next to the cgroup map, never in the 1 Hz sweep.
pub fn pid_exe() -> HashMap<u32, String> {
    let mut out = HashMap::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if let Ok(path) = std::fs::read_link(format!("/proc/{pid}/exe"))
            && let Some(text) = path.to_str()
        {
            out.insert(pid, text.to_string());
        }
    }
    out
}

pub fn read_cmdline(pid: u32) -> String {
    Process::new(pid as i32)
        .and_then(|p| p.cmdline())
        .map(|args| args.join(" "))
        .unwrap_or_default()
}

/// pid -> argv, for the whole system. Runs on the slow (5 s) cadence next to
/// the executable map; the 1 Hz RSS sweep must stay cheap.
pub fn pid_cmdline() -> HashMap<u32, Vec<String>> {
    let mut out = HashMap::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
            continue;
        };
        let args: Vec<String> = raw
            .split(|b| *b == 0)
            .filter(|arg| !arg.is_empty())
            .map(|arg| String::from_utf8_lossy(arg).into_owned())
            .collect();
        if !args.is_empty() {
            out.insert(pid, args);
        }
    }
    out
}

/// pid -> process start time in Unix milliseconds. The kernel reports start
/// time as clock ticks since boot (`/proc/PID/stat` field 22).
pub fn pid_start() -> HashMap<u32, u64> {
    let mut out = HashMap::new();
    let (Ok(btime), tps) = (procfs::boot_time_secs(), procfs::ticks_per_second()) else {
        return out;
    };
    if tps == 0 {
        return out;
    }
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        let Some(rest) = stat.rfind(')').map(|end| &stat[end + 1..]) else {
            continue;
        };
        let Some(ticks) = rest
            .split_whitespace()
            .nth(19)
            .and_then(|v| v.parse::<u64>().ok())
        else {
            continue;
        };
        out.insert(
            pid,
            btime.saturating_mul(1000) + ticks.saturating_mul(1000) / tps,
        );
    }
    out
}

#[derive(Default, Clone, Debug)]
pub struct MemInfo {
    pub total_kb: u64,
    pub avail_kb: u64,
    pub cached_kb: u64,
    pub swap_total_kb: u64,
    pub swap_free_kb: u64,
}

pub fn read_meminfo() -> MemInfo {
    let Ok(m) = Meminfo::current() else {
        return MemInfo::default();
    };
    // procfs::Meminfo normalizes /proc/meminfo to bytes; the UI works in kB.
    MemInfo {
        total_kb: m.mem_total / KB,
        avail_kb: m.mem_available.unwrap_or(m.mem_free) / KB,
        cached_kb: m.cached / KB,
        swap_total_kb: m.swap_total / KB,
        swap_free_kb: m.swap_free / KB,
    }
}

/// Pressure Stall Information, as percentages of wall-clock time.
#[derive(Default, Clone, Copy, Debug)]
pub struct Pressure {
    /// "some": at least one task stalled on memory.
    pub some10: f32,
    /// "full": every non-idle task stalled.
    pub full10: f32,
}

/// Format: `some avg10=0.00 avg60=0.00 avg300=0.00 total=83528484`.
pub fn parse_pressure(text: &str) -> Pressure {
    let mut p = Pressure::default();
    for line in text.lines() {
        let Some((kind, rest)) = line.split_once(' ') else {
            continue;
        };
        let avg10 = rest
            .split_whitespace()
            .find_map(|f| f.strip_prefix("avg10="))
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0);
        match kind {
            "some" => p.some10 = avg10,
            "full" => p.full10 = avg10,
            _ => {}
        }
    }
    p
}

/// System-wide memory pressure. One small file, so it can be read every tick.
pub fn read_pressure() -> Pressure {
    std::fs::read_to_string("/proc/pressure/memory")
        .map(|t| parse_pressure(&t))
        .unwrap_or_default()
}

pub fn fmt_kb(kb: u64) -> String {
    let mb = kb as f64 / 1024.0;
    if mb >= 1024.0 {
        format!("{:.2}G", mb / 1024.0)
    } else if mb >= 10.0 {
        format!("{mb:.0}M")
    } else {
        format!("{mb:.1}M")
    }
}
