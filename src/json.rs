//! Machine-readable snapshot for `--json`: stable ordering, explicit units,
//! so two runs can be diffed (`metop --json > a; metop --json > b; diff a b`).

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

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

/// Keep only the requested dotted paths, e.g. `procs.pid, procs.pss.pss_kb`.
/// `schema` and `at_unix_ms` are always kept: a document without them cannot be
/// identified or ordered.
pub fn render(snap: &Snapshot, fields: &[String]) -> Result<String, String> {
    let doc = build_doc(snap);
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
        if !exists(&doc, &path) {
            return Err(format!(
                "unknown field path {raw:?}; top-level keys are: {}",
                doc.as_object()
                    .map(|m| m.keys().cloned().collect::<Vec<_>>().join(", "))
                    .unwrap_or_default()
            ));
        }
        paths.push(path);
    }
    let projected = project(&doc, &paths);
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
        Value::Object(map) => map.get(first).is_some_and(|v| exists(v, &path[1..])),
        // Every element is a record; the path applies to each of them.
        Value::Array(items) => items.iter().any(|item| exists(item, path)),
        // A null section (e.g. `detail` without --pid) is allowed: the caller
        // asked for a path that would exist once there is data.
        Value::Null => true,
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

fn build_doc(snap: &Snapshot) -> Value {
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
                "no_mm": snap.no_mm.contains(&row.pid),
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
        "schema": "metop.snapshot.v1",
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
        "detail": detail,
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
    use serde_json::json;

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
