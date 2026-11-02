use std::process::ExitCode;
use std::sync::mpsc;

use mtop::app::App;
use mtop::sampler;
use mtop::theme::{ColorSupport, Theme};

const HELP: &str = "\
mtop - Linux memory analysis TUI

USAGE:
    mtop [--pid PID] [--theme NAME]
    mtop --json [--pid PID] [--fields procs.pid,procs.rss_kb]
    mtop --diff A.json B.json [--min-delta 50M] [--top 20]

OPTIONS:
    -p, --pid PID   start with PID selected (also enables Enter drill-down)
        --theme NAME  TUI colour theme: default, btop, dracula,
                      gruvbox-dark, kanagawa-wave, light, ansi16
                      (also MTOP_THEME; `t` switches at runtime)
        --color MODE  colour depth: auto, truecolor, 256, 16
                      (auto follows COLORTERM/TERM)
        --json      print one snapshot as JSON and exit (diffable; no TUI)
        --fields P  comma-separated dotted paths to keep, e.g.
                    procs.pid,procs.comm,procs.pss.pss_kb (schema/at always kept)
        --diff A B  compare two --json snapshots, print what changed and exit
        --min-delta SIZE  hide diff rows below this |ΔRSS| (50M, 512k, 2G)
        --top N           how many diff rows to print (default 30)
    -V, --version   print the version
    -h, --help      show this help

KEYS:
    j/k, ↑/↓        move selection          g/G     top / bottom
    s               cycle sort key          r       reverse order
    t               cycle colour theme      Tab     switch view
    f               filter process name/pid Enter   per-process VMA breakdown
    f (detail)      filter mapping names    Enter   expand a mapping into VMAs
    z / Z (detail)  collapse / expand all   Esc     clear filter / back
    q               quit
";

fn main() -> ExitCode {
    let mut start_pid = None;
    let mut theme: Option<Theme> = None;
    let mut support: Option<ColorSupport> = None;
    let mut json_out = false;
    let mut fields: Vec<String> = Vec::new();
    let mut diff: Option<(String, String)> = None;
    let mut diff_opts = mtop::diff::Opts::default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-p" | "--pid" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse::<u32>().ok()) {
                    Some(pid) => start_pid = Some(pid),
                    None => {
                        eprintln!("mtop: --pid needs a numeric pid");
                        return ExitCode::from(2);
                    }
                }
            }
            "--theme" => {
                i += 1;
                match args.get(i) {
                    Some(name) => match Theme::by_name(name) {
                        Some(t) => theme = Some(t),
                        None => {
                            eprintln!(
                                "mtop: unknown theme {name:?}; choose one of {}",
                                Theme::NAMES
                            );
                            return ExitCode::from(2);
                        }
                    },
                    None => {
                        eprintln!("mtop: --theme needs a name ({})", Theme::NAMES);
                        return ExitCode::from(2);
                    }
                }
            }
            "--color" => {
                i += 1;
                match args.get(i).and_then(|name| ColorSupport::parse(name)) {
                    Some(mode) => support = Some(mode),
                    None => {
                        eprintln!("mtop: --color needs one of {}", ColorSupport::NAMES);
                        return ExitCode::from(2);
                    }
                }
            }
            "-V" | "--version" => {
                println!("mtop {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--json" => json_out = true,
            "--fields" => {
                i += 1;
                match args.get(i) {
                    Some(list) => {
                        fields = list
                            .split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect();
                        if fields.is_empty() {
                            eprintln!("mtop: --fields needs at least one path");
                            return ExitCode::from(2);
                        }
                    }
                    None => {
                        eprintln!("mtop: --fields needs a comma-separated list");
                        return ExitCode::from(2);
                    }
                }
            }
            "--min-delta" => {
                i += 1;
                match args.get(i).and_then(|s| mtop::diff::parse_size(s)) {
                    Some(kb) => diff_opts.min_delta_kb = kb,
                    None => {
                        eprintln!("mtop: --min-delta needs a size like 50M or 512k");
                        return ExitCode::from(2);
                    }
                }
            }
            "--top" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse::<usize>().ok()) {
                    Some(n) => diff_opts.top = n,
                    None => {
                        eprintln!("mtop: --top needs a number");
                        return ExitCode::from(2);
                    }
                }
            }
            "--diff" => {
                let a = args.get(i + 1).cloned();
                let b = args.get(i + 2).cloned();
                match (a, b) {
                    (Some(a), Some(b)) => {
                        diff = Some((a, b));
                        i += 2;
                    }
                    _ => {
                        eprintln!("mtop: --diff needs two JSON snapshot paths");
                        return ExitCode::from(2);
                    }
                }
            }
            "-h" | "--help" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("mtop: unknown argument {other:?}\n\n{HELP}");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    if let Some((a_path, b_path)) = diff {
        let read = |p: &str| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
        let text = read(&a_path)
            .and_then(|a| Ok((a, read(&b_path)?)))
            .and_then(|(a, b)| mtop::diff::render(&a, &b, &diff_opts));
        return match text {
            Ok(text) => match mtop::json::write_str(&text) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("mtop: {e}");
                    ExitCode::FAILURE
                }
            },
            Err(e) => {
                eprintln!("mtop: {e}");
                ExitCode::from(2)
            }
        };
    }

    if json_out {
        // Detail is only worth serializing when a pid was explicitly asked for.
        let snap = sampler::snapshot_once(start_pid, start_pid.is_some());
        let text = match mtop::json::render(&snap, &fields) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("mtop: {e}");
                return ExitCode::from(2);
            }
        };
        return match mtop::json::write_str(&text) {
            Ok(()) => ExitCode::SUCCESS,
            // `mtop --json | head` is normal usage, not an error.
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("mtop: {e}");
                ExitCode::FAILURE
            }
        };
    }

    // --theme wins; MTOP_THEME is the shell/alias convenience. A bad env value
    // warns instead of making every mtop invocation fail.
    let theme = theme.unwrap_or_else(|| match std::env::var("MTOP_THEME") {
        Ok(name) => Theme::by_name(&name).unwrap_or_else(|| {
            eprintln!(
                "mtop: ignoring MTOP_THEME={name:?}; choose one of {}",
                Theme::NAMES
            );
            Theme::DEFAULT
        }),
        Err(_) => Theme::DEFAULT,
    });

    let support = support.unwrap_or_else(ColorSupport::detect);

    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (snap_tx, snap_rx) = mpsc::channel();
    sampler::spawn(cmd_rx, snap_tx);

    let mut terminal = ratatui::init();
    let result = App::run(&mut terminal, start_pid, theme, support, cmd_tx, snap_rx);
    ratatui::restore();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mtop: {e}");
            ExitCode::FAILURE
        }
    }
}
