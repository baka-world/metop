//! Machine-readable snapshot for `--json`: stable ordering, explicit units,
//! so two runs can be diffed (`metop --json > a; metop --json > b; diff a b`).

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::apps;
use crate::model::{Detail, Snapshot};
use crate::procfs::Rollup;

/// Bumped when a field is renamed or its type changes. `--diff` keeps reading
/// v1 snapshots so old captures remain comparable.
pub const SCHEMA: &str = "metop.snapshot.v2";

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

/// Keep only the requested dotted paths, e.g. `procs.pid, procs.pss.pss_kb`.
/// `schema` and `at_unix_ms` are always kept: a document without them cannot be
/// identified or ordered.
pub fn render(snap: &Snapshot, fields: &[String], include_vmas: bool) -> Result<String, String> {
    let include_apps = fields.is_empty()
        || fields.iter().any(|field| {
            field
                .split('.')
                .map(str::trim)
                .find(|part| !part.is_empty())
                == Some("apps")
        });
    let doc = build_doc(snap, include_apps, include_vmas);
    if fields.is_empty() {
        return Ok(pretty(&doc));
    }
    let mut paths: Vec<Vec<String>> = Vec::with_capacity(fields.len());
    for raw in fields {
        let path: Vec<String> = raw
            .split('.')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if path.is_empty() {
            return Err(format!("empty field path {raw:?}"));
        }
        let empty_apps_field = doc["apps"].as_array().is_some_and(Vec::is_empty)
            && path.len() == 2
            && path[0] == "apps"
            && [
                "name", "source", "pss_kb", "uss_kb", "rss_kb", "procs", "pids",
            ]
            .contains(&path[1].as_str());
        let empty_details_field = doc["details"].as_array().is_some_and(Vec::is_empty)
            && path.len() >= 2
            && path[0] == "details"
            && [
                "pid",
                "comm",
                "cmdline",
                "parse_ms",
                "rollup",
                "vma_count",
                "groups",
            ]
            .contains(&path[1].as_str());
        if !exists(&doc, &path) && !empty_apps_field && !empty_details_field {
            return Err(format!(
                "unknown field path {raw:?}; {}",
                path_hint(&doc, &path)
            ));
        }
        paths.push(path);
    }
    let wants_detail = paths
        .iter()
        .any(|path| path.first().is_some_and(|part| part == "detail"));
    let wants_details = paths
        .iter()
        .any(|path| path.first().is_some_and(|part| part == "details"));
    let mut projected = project(&doc, &paths);
    if let Value::Object(map) = &mut projected {
        // The error fields are the reason a detail section is null. Projecting
        // the section alone must not strip that context.
        for (needed, key) in [
            (wants_detail, "detail_error"),
            (wants_details, "detail_errors"),
        ] {
            if needed
                && !map.contains_key(key)
                && let Some(error) = doc.get(key)
            {
                map.insert(key.into(), error.clone());
            }
        }
    }
    Ok(pretty(&projected))
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".into())
}

fn exists(value: &Value, path: &[String]) -> bool {
    let Some(first) = path.first() else {
        return true;
    };
    match value {
        Value::Object(map) => match map.get(first) {
            // A whole section may be null (`detail` without --pid). Its
            // children are still valid because they exist once there is data,
            // but an arbitrary child of a null non-detail section is a typo.
            Some(Value::Null) => path.len() == 1 || first == "detail",
            Some(v) => exists(v, &path[1..]),
            None => false,
        },
        // Every element is a record; the path applies to each of them.
        Value::Array(items) => items.iter().any(|item| exists(item, path)),
        Value::Null => false,
        _ => false,
    }
}

#[derive(Default)]
struct Field {
    whole: bool,
    children: std::collections::BTreeMap<String, Field>,
}

fn build_trie(paths: &[Vec<String>]) -> Field {
    let mut root = Field::default();
    for path in paths {
        let mut node = &mut root;
        for seg in path {
            node = node.children.entry(seg.clone()).or_default();
        }
        node.whole = true;
    }
    root
}

fn project_with(value: &Value, field: &Field) -> Value {
    if field.whole {
        return value.clone();
    }
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, child) in &field.children {
                if let Some(v) = map.get(key) {
                    out.insert(key.clone(), project_with(v, child));
                }
            }
            Value::Object(out)
        }
        // The path applies to every record in the array.
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| project_with(item, field)).collect())
        }
        other => other.clone(),
    }
}

/// Leaf paths present in a snapshot, for `--fields help`. Arrays contribute
/// one set of paths because the projection applies to every record.
pub fn help_paths(snap: &Snapshot, include_vmas: bool) -> Vec<String> {
    let doc = build_doc(snap, true, include_vmas);
    let mut paths = Vec::new();
    collect_paths(&doc, "", &mut paths);
    paths.sort();
    paths.dedup();
    paths
}

fn collect_paths(value: &Value, prefix: &str, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if map.is_empty() && !prefix.is_empty() {
                out.push(prefix.to_string());
                return;
            }
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                match child {
                    Value::Object(_) | Value::Array(_) => collect_paths(child, &path, out),
                    _ => out.push(path),
                }
            }
        }
        Value::Array(items) => {
            if items.is_empty() {
                if !prefix.is_empty() {
                    out.push(prefix.to_string());
                }
            } else {
                // Consider every record: the first pid may have a null PSS or
                // no cgroup, while later records expose the real children.
                for item in items {
                    collect_paths(item, prefix, out);
                }
            }
        }
        _ => {
            if !prefix.is_empty() {
                out.push(prefix.to_string());
            }
        }
    }
}

/// Point at the longest valid prefix of a typo and list its next segments.
fn path_hint(value: &Value, path: &[String]) -> String {
    let mut leaves = Vec::new();
    collect_paths(value, "", &mut leaves);
    let mut best = 0;
    for leaf in &leaves {
        let parts: Vec<&str> = leaf.split('.').collect();
        let mut n = 0;
        while n < path.len() && n < parts.len() && parts[n] == path[n] {
            n += 1;
        }
        best = best.max(n);
    }
    if best == 0 {
        let keys = value
            .as_object()
            .map(|map| map.keys().cloned().collect::<Vec<_>>().join(", "))
            .unwrap_or_default();
        return format!("top-level keys are: {keys}");
    }
    let prefix = path[..best].join(".");
    let mut children: Vec<String> = leaves
        .iter()
        .filter_map(|leaf| {
            let parts: Vec<&str> = leaf.split('.').collect();
            (parts.len() > best && parts[..best] == path[..best]).then(|| parts[best].to_string())
        })
        .collect();
    children.sort();
    children.dedup();
    format!("available under {prefix:?}: {}", children.join(", "))
}

fn project(doc: &Value, paths: &[Vec<String>]) -> Value {
    let mut out = project_with(doc, &build_trie(paths));
    if let Value::Object(map) = &mut out {
        for key in ["schema", "at_unix_ms"] {
            if !map.contains_key(key)
                && let Some(v) = doc.get(key)
            {
                map.insert(key.to_string(), v.clone());
            }
        }
    }
    out
}

fn detail_json(d: &Detail, include_vmas: bool) -> Value {
    let groups: Vec<Value> = d
        .report
        .groups
        .iter()
        .map(|g| {
            let mut group = json!({
                "label": g.label,
                "vsize_kb": g.vsize_kb,
                "rss_kb": g.rss_kb,
                "pss_kb": g.pss_kb,
                "uss_kb": g.uss_kb,
                "anon_kb": g.anon_kb,
                "swap_kb": g.swap_kb,
                "vma_count": g.count,
            });
            if include_vmas {
                group["vma_list"] = json!(
                    g.vmas
                        .iter()
                        .map(|v| json!({
                            "start": v.start,
                            "end": v.end,
                            "perms": v.perms,
                            "vsize_kb": v.vsize_kb,
                            "rss_kb": v.rss_kb,
                            "pss_kb": v.pss_kb,
                            "uss_kb": v.uss_kb,
                            "anon_kb": v.anon_kb,
                            "swap_kb": v.swap_kb,
                        }))
                        .collect::<Vec<_>>()
                );
            }
            group
        })
        .collect();
    json!({
        "pid": d.pid,
        "comm": d.comm,
        "cmdline": d.cmdline,
        "parse_ms": d.parse_ms,
        "rollup": rollup_json(&d.rollup),
        "vma_count": d.report.vma_count,
        "groups": groups,
    })
}

fn build_doc(snap: &Snapshot, include_apps: bool, include_vmas: bool) -> Value {
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
                "exe": snap.pid_exe.get(&row.pid),
                "cmdline": snap.pid_cmdline.get(&row.pid),
                "start_unix_ms": snap.pid_start.get(&row.pid),
                "cgroup": snap.pid_cgroup.get(&row.pid),
                "rss_kb": row.rss_kb,
                "delta_kb": if snap.delta_available {
                    Value::from(row.delta_kb)
                } else {
                    Value::Null
                },
                "denied": snap.denied.contains(&row.pid),
                "no_mm": snap.no_mm.contains(&row.pid),
                "pss_age_ms": pss.map(|p| p.at.elapsed().as_millis() as u64),
                "pss": pss.map(|p| rollup_json(&p.rollup)).unwrap_or(Value::Null),
            })
        })
        .collect();

    let apps: Vec<Value> = if include_apps {
        apps::group(&snap.rows, &snap.pss, &snap.pid_cgroup, &snap.pid_exe)
            .into_iter()
            .map(|group| {
                json!({
                    "name": group.name,
                    "source": group
                        .source
                        .unwrap_or(crate::apps::AppSource::Comm)
                        .label(),
                    "pss_kb": group.pss_kb,
                    "uss_kb": group.uss_kb,
                    "rss_kb": group.rss_kb,
                    "procs": group.procs,
                    "pids": group.pids,
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    let detail = snap.detail.as_ref().map(|d| detail_json(d, include_vmas));
    let details: Vec<Value> = snap
        .details
        .iter()
        .map(|d| detail_json(d, include_vmas))
        .collect();
    let detail_errors: Vec<Value> = snap
        .detail_errors
        .iter()
        .map(|e| json!({"pid": e.pid, "error": e.error}))
        .collect();

    let doc = json!({
        "schema": SCHEMA,
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
        "pressure": {
            "some10": snap.pressure.some10,
            "full10": snap.pressure.full10,
        },
        "sampling": {
            "sweep_ms": snap.sweep_ms,
            "pss_ms": snap.pss_ms,
            "procs": snap.rows.len(),
            "covered": snap.covered,
            "denied": snap.denied.len(),
            "no_mm": snap.no_mm.len(),
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
            "psi_some10": c.psi_some10,
            "psi_full10": c.psi_full10,
        })).collect::<Vec<_>>(),
        "detail_error": snap.detail_error,
        "detail_errors": detail_errors,
        "detail": detail,
        "details": details,
        "apps": apps,
        "procs": procs,
    });
    doc
}

pub fn write_str(text: &str) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    out.write_all(text.as_bytes())?;
    if !text.ends_with('\n') {
        out.write_all(b"\n")?;
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProcRow, PssInfo};
    use serde_json::json;
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;
    use std::time::Instant;

    fn snapshot() -> Snapshot {
        let now = Instant::now();
        let row = |pid, comm: &str, rss_kb| ProcRow {
            pid,
            comm: comm.into(),
            rss_kb,
            delta_kb: 0,
        };
        let pss = |pss_kb, uss_kb| PssInfo {
            rollup: Rollup {
                pss_kb,
                uss_kb,
                ..Default::default()
            },
            at: now,
        };
        Snapshot {
            delta_available: false,
            rows: vec![
                row(30, "helper", 30),
                row(20, "beta", 20),
                row(10, "main", 10),
                row(40, "alpha", 40),
                row(50, "unreadable", 50),
            ],
            pss: HashMap::from([(10, pss(15, 8)), (20, pss(5, 3)), (40, pss(5, 2))]),
            mem: Default::default(),
            pressure: Default::default(),
            scan_seq: 0,
            cgroups: vec![],
            pid_cgroup: Arc::new(HashMap::from([
                (10, "/user/app-browser-123.scope".into()),
                (30, "/user/app-browser-456.scope".into()),
            ])),
            pid_exe: Arc::new(HashMap::from([
                (10, "/bin/ignored".into()),
                (20, "/bin/beta".into()),
            ])),
            pid_cmdline: Arc::new(HashMap::new()),
            pid_start: Arc::new(HashMap::new()),
            detail: None,
            details: Vec::new(),
            detail_errors: Vec::new(),
            detail_error: None,
            sweep_ms: 0.0,
            pss_ms: 0.0,
            covered_pss_kb: 25,
            covered: 3,
            denied: HashSet::from([30, 50]),
            no_mm: HashSet::new(),
            at: now,
        }
    }

    #[test]
    fn apps_export_complete_groups_and_project_fields() {
        let snap = snapshot();
        let doc = build_doc(&snap, true, false);
        assert_eq!(
            doc["apps"],
            json!([
                {"name": "browser", "source": "scope", "pss_kb": 15, "uss_kb": 8, "rss_kb": 40, "procs": 2, "pids": [10, 30]},
                {"name": "alpha", "source": "comm", "pss_kb": 5, "uss_kb": 2, "rss_kb": 40, "procs": 1, "pids": [40]},
                {"name": "beta", "source": "exe", "pss_kb": 5, "uss_kb": 3, "rss_kb": 20, "procs": 1, "pids": [20]},
                {"name": "unreadable", "source": "comm", "pss_kb": 0, "uss_kb": 0, "rss_kb": 50, "procs": 1, "pids": [50]},
            ])
        );
        let projected: Value = serde_json::from_str(
            &render(&snap, &["apps.name".into(), "apps.pss_kb".into()], false).unwrap(),
        )
        .unwrap();
        assert_eq!(
            projected["apps"],
            json!([
                {"name": "browser", "pss_kb": 15},
                {"name": "alpha", "pss_kb": 5},
                {"name": "beta", "pss_kb": 5},
                {"name": "unreadable", "pss_kb": 0},
            ])
        );
        assert_eq!(projected.as_object().unwrap().len(), 3);
    }

    #[test]
    fn empty_apps_allow_known_projected_fields_but_reject_typos() {
        let mut snap = snapshot();
        snap.rows.clear();
        let projected: Value = serde_json::from_str(
            &render(&snap, &["apps.name".into(), "apps.pss_kb".into()], false).unwrap(),
        )
        .unwrap();
        assert_eq!(projected["apps"], json!([]));
        assert!(render(&snap, &["apps.nope".into()], false).is_err());
    }

    fn doc() -> Value {
        json!({
            "schema": "metop.snapshot.v1",
            "at_unix_ms": 42,
            "mem": {"total_kb": 1, "avail_kb": 2},
            "procs": [
                {"pid": 7, "comm": "a", "rss_kb": 10, "pss": {"pss_kb": 5, "uss_kb": 4}},
                {"pid": 8, "comm": "b", "rss_kb": 20, "pss": null}
            ]
        })
    }

    fn project_str(fields: &[&str]) -> Value {
        let paths: Vec<Vec<String>> = fields
            .iter()
            .map(|f| f.split('.').map(str::to_string).collect())
            .collect();
        project(&doc(), &paths)
    }

    #[test]
    fn flat_and_nested_paths_are_projected_per_record() {
        let out = project_str(&["procs.pid", "procs.pss.pss_kb"]);
        let procs = out["procs"].as_array().unwrap();
        assert_eq!(procs[0], json!({"pid": 7, "pss": {"pss_kb": 5}}));
        // A null section stays null instead of turning into an empty object.
        assert_eq!(procs[1], json!({"pid": 8, "pss": null}));
        assert!(out.get("mem").is_none(), "unselected section leaked");
    }

    #[test]
    fn meta_keys_are_always_kept() {
        let out = project_str(&["mem.total_kb"]);
        assert_eq!(out["schema"], "metop.snapshot.v1");
        assert_eq!(out["at_unix_ms"], 42);
        assert_eq!(out["mem"], json!({"total_kb": 1}));
    }

    #[test]
    fn a_whole_section_path_keeps_it_intact() {
        let out = project_str(&["mem"]);
        assert_eq!(out["mem"], doc()["mem"]);
    }

    #[test]
    fn validation_rejects_typos_but_allows_null_sections() {
        let d = doc();
        let path = |s: &str| -> Vec<String> { s.split('.').map(str::to_string).collect() };
        assert!(exists(&d, &path("procs.pid")));
        assert!(exists(&d, &path("procs.pss.uss_kb")));
        assert!(!exists(&d, &path("procs.nope")));
        assert!(!exists(&d, &path("nope")));
        // `detail` is null without --pid; asking for its fields is valid.
        let with_null = json!({"schema": "x", "detail": null});
        assert!(exists(&with_null, &path("detail.pid")));
    }
}
