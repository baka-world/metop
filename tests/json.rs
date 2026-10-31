//! The JSON mode is a public interface (diffing, scripts), so it is tested
//! through the real binary rather than by calling the serializer.

use std::process::Command;

fn run(args: &[&str]) -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_mtop"))
        .args(args)
        .output()
        .expect("spawn mtop");
    assert!(
        out.status.success(),
        "mtop {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "invalid JSON: {e}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

#[test]
fn snapshot_is_sorted_and_self_consistent() {
    let v = run(&["--json"]);
    assert_eq!(v["schema"], "mtop.snapshot.v1");

    let procs = v["procs"].as_array().expect("procs array");
    assert!(
        procs.len() > 5,
        "suspiciously few processes: {}",
        procs.len()
    );

    // Stable pid ordering is what makes `diff a.json b.json` usable.
    let pids: Vec<u64> = procs.iter().map(|p| p["pid"].as_u64().unwrap()).collect();
    assert!(
        pids.windows(2).all(|w| w[0] < w[1]),
        "procs are not sorted by pid"
    );

    let psi = &v["pressure"];
    assert!(psi["some10"].is_number(), "system pressure missing");
    let some = psi["some10"].as_f64().unwrap();
    assert!((0.0..=100.0).contains(&some), "implausible psi: {some}");

    let total = v["mem"]["total_kb"].as_u64().unwrap();
    assert!(
        (1_000_000..10_000_000_000).contains(&total),
        "mem total looks like the wrong unit: {total} kB"
    );

    let readable: Vec<&serde_json::Value> = procs.iter().filter(|p| !p["pss"].is_null()).collect();
    assert!(!readable.is_empty(), "not one process had a readable PSS");
    for p in readable {
        let pss = p["pss"]["pss_kb"].as_u64().unwrap();
        let rss = p["pss"]["rss_kb"].as_u64().unwrap();
        assert!(
            pss <= rss,
            "{} reports pss {pss} kB > rss {rss} kB",
            p["comm"]
        );
        // The per-kind split must add up to the total (both come from the same rollup).
        let parts = p["pss"]["pss_anon_kb"].as_u64().unwrap()
            + p["pss"]["pss_file_kb"].as_u64().unwrap()
            + p["pss"]["pss_shmem_kb"].as_u64().unwrap();
        assert!(
            parts.abs_diff(pss) <= 1024,
            "{} kind split {parts} kB vs pss {pss} kB",
            p["comm"]
        );
    }
}

#[test]
fn detail_is_serialized_for_the_requested_pid() {
    let me = std::process::id();
    let v = run(&["--json", "--pid", &me.to_string()]);

    let d = &v["detail"];
    assert_eq!(d["pid"].as_u64(), Some(me as u64));
    assert!(
        v["detail_error"].is_null(),
        "detail failed: {}",
        v["detail_error"]
    );
    assert!(d["vma_count"].as_u64().unwrap() > 0, "no VMAs parsed");

    let groups = d["groups"].as_array().expect("groups");
    assert!(!groups.is_empty(), "no groups");
    // Group PSS must not exceed the process-level rollup by more than drift.
    let sum: u64 = groups.iter().map(|g| g["pss_kb"].as_u64().unwrap()).sum();
    let total = d["rollup"]["pss_kb"].as_u64().unwrap();
    assert!(
        sum.abs_diff(total) <= total / 5 + 4096,
        "group pss {sum} kB vs rollup {total} kB"
    );
}

#[test]
fn cgroups_are_exported_when_the_system_has_them() {
    let v = run(&["--json"]);
    let cgs = v["cgroups"].as_array().expect("cgroups array");
    let v2 = std::path::Path::new("/sys/fs/cgroup/cgroup.controllers").exists();
    // v2 alone is not enough: the memory controller may not be delegated to
    // this container. Check for an actual memory.current before asserting.
    let memory_controller = std::fs::read_dir("/sys/fs/cgroup")
        .map(|dir| {
            dir.flatten()
                .any(|e| e.path().join("memory.current").exists())
        })
        .unwrap_or(false);
    if !v2 || !memory_controller {
        return;
    }
    assert!(!cgs.is_empty(), "cgroup v2 present but nothing exported");
    let first = &cgs[0];
    for key in [
        "path",
        "current_kb",
        "anon_kb",
        "file_kb",
        "kernel_kb",
        "slab_kb",
        "shmem_kb",
        "oom_kills",
    ] {
        assert!(!first[key].is_null(), "missing {key} in cgroup export");
    }
    // current ≈ anon + file + kernel; slab ⊂ kernel, shmem ⊂ file.
    let sum = first["anon_kb"].as_u64().unwrap()
        + first["file_kb"].as_u64().unwrap()
        + first["kernel_kb"].as_u64().unwrap();
    let cur = first["current_kb"].as_u64().unwrap();
    assert!(
        sum.abs_diff(cur) <= cur / 5 + 4096,
        "{}: current {cur} kB vs anon+file+kernel {sum} kB",
        first["path"]
    );
}

#[test]
fn fields_projection_keeps_only_requested_paths() {
    let full = run(&["--json"]);
    let filtered = run(&["--json", "--fields", "procs.pid,procs.rss_kb"]);

    assert!(filtered.get("mem").is_none(), "unselected section leaked");
    assert_eq!(filtered["schema"], "mtop.snapshot.v1");
    assert!(filtered["at_unix_ms"].is_u64());
    let proc = &filtered["procs"][0];
    assert!(proc["pid"].is_u64());
    assert!(proc["rss_kb"].is_u64());
    assert!(proc.get("comm").is_none(), "unselected proc field leaked");

    // The unfiltered document still has the sections, so the filter is what
    // shrank it (measured: 194 KB -> 23 KB for two fields on this machine).
    assert!(full["mem"]["total_kb"].is_u64());
    assert!(full["procs"][0].get("comm").is_some());
}

#[test]
fn unknown_field_paths_are_rejected() {
    let out = Command::new(env!("CARGO_BIN_EXE_mtop"))
        .args(["--json", "--fields", "procs.nope"])
        .output()
        .expect("spawn mtop");
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown field path"), "unhelpful error: {err}");
    assert!(
        err.contains("top-level keys"),
        "no hint about valid keys: {err}"
    );
}
