//! Machine-readable snapshot for `--json`: stable ordering, explicit units,
//! so two runs can be diffed (`mtop --json > a; mtop --json > b; diff a b`).

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::model::Snapshot;
use crate::procfs::Rollup;

fn rollup_json(r: &Rollup) -> Value {
    json!({
        "rss_kb": r.rss_kb,
        "pss_kb": r.pss_kb,
        "uss_kb": r.uss_kb,
        "swap_kb": r.swap_kb,
        "anon_kb": r.anon_kb,
        "pss_anon_kb": r.pss_anon_kb,
        "pss_file_kb": r.pss_file_kb,
        "pss_shmem_kb": r.pss_shmem_kb,
        "swap_pss_kb": r.swap_pss_kb,
        "thp_kb": r.thp_kb,
        "ksm_kb": r.ksm_kb,
    })
}

pub fn snapshot_json(snap: &Snapshot) -> String {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    // Sorted by pid, not by the UI sort: a diff should only change when memory
    // changes, not when the view changes.
    let mut rows: Vec<&crate::model::ProcRow> = snap.rows.iter().collect();
    rows.sort_by_key(|r| r.pid);

    let procs: Vec<Value> = rows
        .iter()
        .map(|row| {
            let pss = snap.pss.get(&row.pid);
            json!({
                "pid": row.pid,
                "comm": row.comm,
                "rss_kb": row.rss_kb,
                "delta_kb": row.delta_kb,
                "denied": snap.denied.contains(&row.pid),
                "pss_age_ms": pss.map(|p| p.at.elapsed().as_millis() as u64),
                "pss": pss.map(|p| rollup_json(&p.rollup)).unwrap_or(Value::Null),
            })
        })
        .collect();

    let detail = snap.detail.as_ref().map(|d| {
        json!({
            "pid": d.pid,
            "comm": d.comm,
            "cmdline": d.cmdline,
            "parse_ms": d.parse_ms,
            "rollup": rollup_json(&d.rollup),
            "vma_count": d.report.vma_count,
            "groups": d.report.groups.iter().map(|g| json!({
                "label": g.label,
                "size_kb": g.size_kb,
                "pss_kb": g.pss_kb,
                "uss_kb": g.uss_kb,
                "anon_kb": g.anon_kb,
                "swap_kb": g.swap_kb,
                "vmas": g.count,
            })).collect::<Vec<_>>(),
        })
    });

    let doc = json!({
        "schema": "mtop.snapshot.v1",
        "at_unix_ms": now_ms,
        // The process that wrote this snapshot. A later --diff must ignore it:
        // its pid is new every run, so it would show up as a phantom +3 MB.
        "self_pid": std::process::id(),
        "mem": {
            "total_kb": snap.mem.total_kb,
            "avail_kb": snap.mem.avail_kb,
            "cached_kb": snap.mem.cached_kb,
            "swap_total_kb": snap.mem.swap_total_kb,
            "swap_free_kb": snap.mem.swap_free_kb,
        },
        "sampling": {
            "sweep_ms": snap.sweep_ms,
            "pss_ms": snap.pss_ms,
            "procs": snap.rows.len(),
            "covered": snap.covered,
            "denied": snap.denied.len(),
            "covered_pss_kb": snap.covered_pss_kb,
        },
        "cgroups": snap.cgroups.iter().map(|c| json!({
            "path": c.path,
            "current_kb": c.current_kb,
            "anon_kb": c.anon_kb,
            "file_kb": c.file_kb,
            "kernel_kb": c.kernel_kb,
            "slab_kb": c.slab_kb,
            "shmem_kb": c.shmem_kb,
            "limit_kb": c.limit_kb,
            "oom_kills": c.oom_kills,
            "oom_recent": c.oom_recent,
        })).collect::<Vec<_>>(),
        "detail_error": snap.detail_error,
        "detail": detail,
        "procs": procs,
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}

pub fn write_json(snap: &Snapshot) -> std::io::Result<()> {
    write_str(&snapshot_json(snap))
}

pub fn write_str(text: &str) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes())?;
    if !text.ends_with('\n') {
        out.write_all(b"\n")?;
    }
    out.flush()
}
