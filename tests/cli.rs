//! CLI surface: version, help, and rejection of unknown arguments.

use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_metop"))
        .args(args)
        .output()
        .expect("spawn metop")
}

#[test]
fn version_reports_the_package_version() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains(env!("CARGO_PKG_VERSION")),
        "version mismatch: {text}"
    );
    assert!(text.starts_with("metop "), "unexpected format: {text}");

    // -V is the conventional short form.
    let short = run(&["-V"]);
    assert!(short.status.success());
    assert_eq!(short.stdout, out.stdout);
}

#[test]
fn help_lists_the_interfaces_we_actually_have() {
    let out = run(&["--help"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for needle in [
        "--json",
        "--diff",
        "--min-delta",
        "--min-delta-metric",
        "--vmas",
        "--no-detail",
        "--pids",
        "--pid",
        "--theme",
        "gruvbox-dark, kanagawa-wave, light, ansi16",
        "--color",
        "auto, truecolor, 256, 16",
        "--lang",
        "auto, en, zh",
        "USAGE",
    ] {
        assert!(text.contains(needle), "help missing {needle}:\n{text}");
    }
}

#[test]
fn unknown_theme_is_a_usage_error() {
    let out = run(&["--theme", "solarized"]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown theme"), "stderr: {err}");
    assert!(err.contains("ansi16"), "stderr should list themes: {err}");

    let missing = run(&["--theme"]);
    assert_eq!(missing.status.code(), Some(2));
}

#[test]
fn unknown_language_is_a_usage_error() {
    let out = run(&["--lang", "fr"]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("auto, en, zh"), "stderr: {err}");

    let ok = run(&["--lang", "zh", "--version"]);
    assert!(ok.status.success());
}

#[test]
fn unknown_colour_mode_is_a_usage_error() {
    let out = run(&["--color", "512"]);
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("auto, truecolor, 256, 16"), "stderr: {err}");

    // Valid modes parse even when no TUI is started.
    let ok = run(&["--color", "256", "--version"]);
    assert!(ok.status.success());
}

#[test]
fn json_only_flags_are_rejected_outside_json() {
    for args in [
        vec!["--vmas"],
        vec!["--detail"],
        vec!["--no-detail"],
        vec!["--fields", "help"],
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?} accepted");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("requires --json")
                || String::from_utf8_lossy(&out.stderr).contains("--json"),
            "unclear error for {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert_eq!(run(&["--vmas", "--json"]).status.code(), Some(2));
    assert_eq!(run(&["--detail", "--json"]).status.code(), Some(2));
}

#[test]
fn pid_lists_reject_empty_duplicate_and_malformed_entries() {
    for args in [
        vec!["--json", "--pids", ""],
        vec!["--json", "--pids", ","],
        vec!["--json", "--pids", "1,,2"],
        vec!["--json", "--pids", "1,abc"],
        vec!["--json", "--pids", "1,1"],
        vec!["--json", "--pid", "1", "--pid", "1"],
        vec!["--json", "--pid", "1", "--pids", "1,2"],
        vec!["--pids", "1,2"],
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?} accepted");
    }
}

#[test]
fn diff_metric_requires_a_known_value() {
    let out = run(&["--diff", "a", "b", "--min-delta-metric", "both"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("rss, pss, or max"));
}

#[test]
fn unknown_arguments_exit_with_usage_error() {
    let out = run(&["--nope"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown argument"));
}

#[test]
fn history_requires_a_bounded_positive_integer() {
    for value in ["0", "-1", "1.5", "3601", "999999999999999999999999", "nope"] {
        let out = run(&["--history", value]);
        assert_eq!(out.status.code(), Some(2), "{value}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("--history"));
    }
    assert_eq!(run(&["--history"]).status.code(), Some(2));
    for value in ["1", "600", "3600"] {
        assert!(run(&["--history", value, "--version"]).status.success());
    }
}
