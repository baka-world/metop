//! `--diff` is a public text interface; test it through the binary with
//! hand-written fixtures so the numbers are exact.

use std::io::Write;
use std::process::{Command, Output};

fn fixture(name: &str, body: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("mtop-diff-{}-{name}", std::process::id()));
    let mut f = std::fs::File::create(&path).expect("create fixture");
    f.write_all(body.as_bytes()).expect("write fixture");
    path
}

fn run(a: &std::path::Path, b: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mtop"))
        .arg("--diff")
        .arg(a)
        .arg(b)
        .output()
        .expect("spawn mtop --diff")
}

const A: &str = r#"{
  "schema": "mtop.snapshot.v1",
  "at_unix_ms": 1000,
  "procs": [
    {"pid": 10, "comm": "alpha", "rss_kb": 100000, "pss": {"pss_kb": 50000}},
    {"pid": 20, "comm": "beta",  "rss_kb": 200000, "pss": null}
  ]
}"#;

const B: &str = r#"{
  "schema": "mtop.snapshot.v1",
  "at_unix_ms": 31000,
  "procs": [
    {"pid": 10, "comm": "alpha", "rss_kb": 300000, "pss": {"pss_kb": 70000}},
    {"pid": 30, "comm": "gamma", "rss_kb": 50000,  "pss": {"pss_kb": 10000}}
  ]
}"#;

#[test]
fn diff_reports_growth_new_and_gone() {
    let (a, b) = (fixture("a.json", A), fixture("b.json", B));
    let out = run(&a, &b);
    let text = String::from_utf8_lossy(&out.stdout);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("over 30s"), "missing timespan:\n{text}");
    assert!(text.contains("(2 -> 2 procs)"), "missing counts:\n{text}");
    assert!(
        text.contains("1 grew, 0 shrank, 1 new, 1 gone"),
        "missing summary:\n{text}"
    );

    let alpha = text
        .lines()
        .find(|l| l.contains("alpha"))
        .expect("alpha row");
    assert!(alpha.contains("+195M"), "delta rss wrong: {alpha}");
    assert!(alpha.contains("+20M"), "delta pss wrong: {alpha}");

    assert!(
        text.lines()
            .any(|l| l.contains("gamma") && l.contains("new")),
        "new process not marked:\n{text}"
    );
    assert!(
        text.lines()
            .any(|l| l.contains("beta") && l.contains("gone")),
        "vanished process not marked:\n{text}"
    );
    // Sorted by absolute delta: alpha (+195M) before gamma (+48M).
    let pos = |needle: &str| text.find(needle).unwrap();
    assert!(
        pos("alpha") < pos("gamma"),
        "rows not sorted by |ΔRSS|:\n{text}"
    );
}

#[test]
fn diff_rejects_bad_input_with_a_clear_error() {
    let (a, b) = (
        fixture("bad.json", "not json at all"),
        fixture("b2.json", B),
    );
    let out = run(&a, &b);
    assert!(!out.status.success(), "bad input must fail");
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not JSON"), "unhelpful error: {err}");

    let (c, d) = (
        fixture("schema.json", r#"{"schema":"other.v9","procs":[]}"#),
        fixture("b3.json", B),
    );
    let out = run(&c, &d);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("unsupported snapshot schema"),
        "schema mismatch not reported"
    );
}

#[test]
fn diff_reports_a_missing_file() {
    let a = fixture("a2.json", A);
    let missing = std::env::temp_dir().join("mtop-diff-does-not-exist.json");
    let _ = std::fs::remove_file(&missing);
    let out = run(&a, &missing);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does-not-exist"),
        "missing path not named: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn snapshot_writers_are_not_reported() {
    let a = fixture(
        "self_a.json",
        r#"{"schema":"mtop.snapshot.v1","at_unix_ms":1,"self_pid":999,
            "procs":[{"pid":999,"comm":"mtop","rss_kb":3000,"pss":null},
                     {"pid":7,"comm":"keep","rss_kb":1000,"pss":null}]}"#,
    );
    let b = fixture(
        "self_b.json",
        r#"{"schema":"mtop.snapshot.v1","at_unix_ms":2001,"self_pid":1001,
            "procs":[{"pid":1001,"comm":"mtop","rss_kb":3000,"pss":null},
                     {"pid":7,"comm":"keep","rss_kb":1000,"pss":null}]}"#,
    );
    let out = run(&a, &b);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    // Skip the summary line ("mtop diff ...") and the column header.
    assert!(
        !text.lines().skip(2).any(|l| l.contains("mtop")),
        "snapshot writer leaked into diff:\n{text}"
    );
    assert!(
        text.contains("0 grew, 0 shrank, 0 new, 0 gone"),
        "writer counted as a change:\n{text}"
    );
}

#[test]
fn min_delta_filters_small_changes() {
    let (a, b) = (fixture("md_a.json", A), fixture("md_b.json", B));
    let out = Command::new(env!("CARGO_BIN_EXE_mtop"))
        .args([
            "--diff",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--min-delta",
            "100M",
        ])
        .output()
        .expect("spawn mtop --diff");
    let text = String::from_utf8_lossy(&out.stdout);

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("min |ΔRSS| 100M"),
        "threshold not in header:\n{text}"
    );
    assert!(text.contains("alpha"), "big change filtered out:\n{text}");
    assert!(
        !text.contains("gamma"),
        "small change passed the filter:\n{text}"
    );
}

#[test]
fn top_limits_the_row_count() {
    let (a, b) = (fixture("top_a.json", A), fixture("top_b.json", B));
    let out = Command::new(env!("CARGO_BIN_EXE_mtop"))
        .args([
            "--diff",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--top",
            "1",
        ])
        .output()
        .expect("spawn mtop --diff");
    let text = String::from_utf8_lossy(&out.stdout);
    let rows = text
        .lines()
        .skip(2)
        .filter(|l| !l.trim().is_empty())
        .count();
    assert_eq!(rows, 1, "expected one data row:\n{text}");
    assert!(text.contains("alpha"), "wrong row kept:\n{text}");
}

#[test]
fn bad_flags_are_rejected() {
    let (a, b) = (fixture("badf_a.json", A), fixture("badf_b.json", B));
    for bad in [["--min-delta", "lots"], ["--top", "many"]] {
        let out = Command::new(env!("CARGO_BIN_EXE_mtop"))
            .args(["--diff", a.to_str().unwrap(), b.to_str().unwrap()])
            .args(bad)
            .output()
            .expect("spawn mtop");
        assert_eq!(out.status.code(), Some(2), "bad flag {bad:?} accepted");
    }
}
