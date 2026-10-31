//! `mtop --diff A.json B.json`: compare two snapshots produced by `--json`.
//!
//! Deliberately text, not a TUI mode: the interesting question ("what grew?")
//! belongs in a pipeline, and the JSON contract is already tested.

use std::collections::{BTreeSet, HashMap};

use serde_json::Value;

use crate::procfs::fmt_kb;

struct Entry {
    comm: String,
    rss_kb: u64,
    pss_kb: Option<u64>,
}

/// `--diff` knobs.
pub struct Opts {
    /// Hide rows whose |ΔRSS| is smaller than this (kB).
    pub min_delta_kb: u64,
    pub top: usize,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            min_delta_kb: 0,
            top: 30,
        }
    }
}

/// Parse "50M" / "512k" / "2G" / "4096" (bare numbers are kB).
pub fn parse_size(text: &str) -> Option<u64> {
    let t = text.trim();
    let (num, mult) = match t.chars().last()? {
        'k' | 'K' => (&t[..t.len() - 1], 1u64),
        'm' | 'M' => (&t[..t.len() - 1], 1024),
        'g' | 'G' => (&t[..t.len() - 1], 1024 * 1024),
        _ => (t, 1),
    };
    num.parse::<u64>().ok().map(|n| n.saturating_mul(mult))
}

/// One parsed `--json` snapshot.
struct Doc {
    at_ms: u64,
    self_pid: Option<u32>,
    procs: HashMap<u32, Entry>,
}

fn parse(text: &str, name: &str) -> Result<Doc, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("{name}: not JSON ({e})"))?;
    match v.get("schema").and_then(Value::as_str) {
        Some("mtop.snapshot.v1") => {}
        Some(other) => return Err(format!("{name}: unsupported snapshot schema {other:?}")),
        None => return Err(format!("{name}: missing schema")),
    }
    let at = v.get("at_unix_ms").and_then(Value::as_u64).unwrap_or(0);
    let self_pid = v.get("self_pid").and_then(Value::as_u64).map(|p| p as u32);
    let procs = v
        .get("procs")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{name}: missing procs"))?;
    let mut map = HashMap::with_capacity(procs.len());
    for (i, p) in procs.iter().enumerate() {
        let pid = p
            .get("pid")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("{name}: procs[{i}] has no pid"))?;
        map.insert(
            pid as u32,
            Entry {
                comm: p
                    .get("comm")
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_string(),
                rss_kb: p.get("rss_kb").and_then(Value::as_u64).unwrap_or(0),
                pss_kb: p
                    .get("pss")
                    .and_then(|v| v.get("pss_kb"))
                    .and_then(Value::as_u64),
            },
        );
    }
    Ok(Doc {
        at_ms: at,
        self_pid,
        procs: map,
    })
}

fn signed_kb(delta: i64) -> String {
    let sign = if delta > 0 {
        "+"
    } else if delta < 0 {
        "-"
    } else {
        " "
    };
    format!("{sign}{}", fmt_kb(delta.unsigned_abs()))
}

struct Row {
    pid: u32,
    comm: String,
    drss: i64,
    rss_now: u64,
    pss_now: Option<u64>,
    dpss: Option<i64>,
    note: &'static str,
}

pub fn render(a_text: &str, b_text: &str, opts: &Opts) -> Result<String, String> {
    let a = parse(a_text, "A")?;
    let b = parse(b_text, "B")?;
    let (a_at, a_self) = (a.at_ms, a.self_pid);
    let (b_at, b_self) = (b.at_ms, b.self_pid);

    let mut pids: BTreeSet<u32> = BTreeSet::new();
    pids.extend(a.procs.keys().copied());
    pids.extend(b.procs.keys().copied());

    let mut rows: Vec<Row> = Vec::new();
    let (mut grew, mut shrank, mut new, mut gone) = (0, 0, 0, 0);
    for pid in pids {
        // Never report the snapshot writers themselves: their pids differ every
        // run, so they would look like a new 3 MB process on every diff.
        if Some(pid) == a_self || Some(pid) == b_self {
            continue;
        }
        match (a.procs.get(&pid), b.procs.get(&pid)) {
            (Some(x), Some(y)) => {
                let drss = y.rss_kb as i64 - x.rss_kb as i64;
                if drss > 0 {
                    grew += 1;
                } else if drss < 0 {
                    shrank += 1;
                }
                rows.push(Row {
                    pid,
                    comm: y.comm.clone(),
                    drss,
                    rss_now: y.rss_kb,
                    pss_now: y.pss_kb,
                    dpss: match (x.pss_kb, y.pss_kb) {
                        (Some(p), Some(q)) => Some(q as i64 - p as i64),
                        _ => None,
                    },
                    note: "",
                });
            }
            (None, Some(y)) => {
                new += 1;
                rows.push(Row {
                    pid,
                    comm: y.comm.clone(),
                    drss: y.rss_kb as i64,
                    rss_now: y.rss_kb,
                    pss_now: y.pss_kb,
                    dpss: None,
                    note: "new",
                });
            }
            (Some(x), None) => {
                gone += 1;
                rows.push(Row {
                    pid,
                    comm: x.comm.clone(),
                    drss: -(x.rss_kb as i64),
                    rss_now: 0,
                    pss_now: None,
                    dpss: None,
                    note: "gone",
                });
            }
            (None, None) => {}
        }
    }

    rows.retain(|r| r.drss.unsigned_abs() >= opts.min_delta_kb);
    rows.sort_by_key(|r| (std::cmp::Reverse(r.drss.unsigned_abs()), r.pid));
    rows.truncate(opts.top);

    let span = b_at.saturating_sub(a_at) / 1000;
    let mut out = String::new();
    out.push_str(&format!(
        "mtop diff  over {span}s  ({} -> {} procs)   {grew} grew, {shrank} shrank, {new} new, {gone} gone",
        a.procs.len(),
        b.procs.len()
    ));
    if opts.min_delta_kb > 0 {
        out.push_str(&format!("   min |ΔRSS| {}", fmt_kb(opts.min_delta_kb)));
    }
    out.push('\n');
    out.push_str(&format!(
        "{:>8} {:<24} {:>9} {:>9} {:>9} {:>9}  {}\n",
        "PID", "COMM", "ΔRSS", "ΔPSS", "RSS now", "PSS now", "note"
    ));
    for r in &rows {
        out.push_str(&format!(
            "{:>8} {:<24} {:>9} {:>9} {:>9} {:>9}  {}\n",
            r.pid,
            truncate(&r.comm, 24),
            signed_kb(r.drss),
            r.dpss.map(signed_kb).unwrap_or_else(|| "—".into()),
            fmt_kb(r.rss_now),
            r.pss_now.map(fmt_kb).unwrap_or_else(|| "—".into()),
            r.note
        ));
    }
    if rows.is_empty() {
        if opts.min_delta_kb > 0 {
            out.push_str(&format!(
                "(no rows above min |ΔRSS| {})\n",
                fmt_kb(opts.min_delta_kb)
            ));
        } else {
            out.push_str("(no process changes)\n");
        }
    }
    Ok(out)
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::parse_size;

    #[test]
    fn sizes_accept_suffixes_and_bare_kb() {
        assert_eq!(parse_size("50M"), Some(51_200));
        assert_eq!(parse_size("512k"), Some(512));
        assert_eq!(parse_size("2G"), Some(2_097_152));
        assert_eq!(parse_size("4096"), Some(4_096));
        assert_eq!(parse_size(" 8m "), Some(8_192));
        assert_eq!(parse_size("garbage"), None);
        assert_eq!(parse_size("M"), None);
    }
}
