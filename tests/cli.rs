//! CLI surface: version, help, and rejection of unknown arguments.

use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mtop"))
        .args(args)
        .output()
        .expect("spawn mtop")
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
    assert!(text.starts_with("mtop "), "unexpected format: {text}");

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
    for needle in ["--json", "--diff", "--min-delta", "--pid", "USAGE"] {
        assert!(text.contains(needle), "help missing {needle}:\n{text}");
    }
}

#[test]
fn unknown_arguments_exit_with_usage_error() {
    let out = run(&["--nope"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown argument"));
}
