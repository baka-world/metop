use std::process::ExitCode;
use std::sync::mpsc;

use metop::app::App;
use metop::i18n::Lang;
use metop::sampler;
use metop::theme::{ColorSupport, Theme};
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;

const HELP: &str = "\
metop - Linux memory analysis TUI

USAGE:
    metop [--pid PID] [--theme NAME] [--lang auto|en|zh] [--history SECONDS]
    metop --json [--pid PID] [--fields procs.pid,procs.rss_kb]
    metop --diff A.json B.json [--min-delta 50M] [--min-delta-metric max] [--top 20]

OPTIONS:
    -p, --pid PID   start with PID selected (also enables Enter drill-down);
                    repeatable, or use --pids for a comma-separated list
        --pids LIST  comma-separated pids for --json details[] (JSON only)
        --theme NAME  TUI colour theme: default, btop, dracula,
                      gruvbox-dark, kanagawa-wave, light, ansi16
                      (also METOP_THEME; `t` switches at runtime)
        --color MODE  colour depth: auto, truecolor, 256, 16
                      (auto follows COLORTERM/TERM)
        --lang LANG   UI language: auto, en, zh (also METOP_LANG; `l` toggles)
        --history SECONDS  shared trend window, 1-3600 seconds (default 120)
        --json      print one snapshot as JSON and exit (diffable; no TUI)
        --fields P  comma-separated dotted paths to keep, e.g.
                    procs.pid,procs.comm,procs.pss.pss_kb (schema/at always kept)
                    Use `--fields help` to list paths in the current snapshot.
        --vmas      include per-VMA arrays under detail (requires --json)
        --detail / --no-detail  force or suppress the detail section in --json;
                    by default it is included for --pid and whenever --fields
                    names detail
        --diff A B  compare two --json snapshots, print what changed and exit
        --min-delta SIZE  hide diff rows below this delta (50M, 512k, 2G)
        --min-delta-metric METRIC  rss, pss, or max (default: max); also
                    controls row ranking, so a PSS-only change is not hidden
        --top N           how many diff rows to print (default 30)
    -V, --version   print the version
    -h, --help      show this help

KEYS:
    j/k, ↑/↓        move selection          g/G     top / bottom
    Ctrl-d/u        half page               Ctrl-f/b full page
    Ctrl-w/u        delete word / clear filter input
    s               cycle sort key          r       reverse order
    t               cycle colour theme      l       switch language
    ?               page help               Tab     switch view
    f, /            filter process name/pid Enter   per-process VMA breakdown
    f, / (detail)   filter mapping names    Enter   expand a mapping into VMAs
    z / Z (detail)  collapse / expand all   Esc     clear filter / back
    q               quit
";

fn is_detail_path(field: &str) -> bool {
    let field = field.trim();
    field == "detail" || field.starts_with("detail.")
}

fn is_details_path(field: &str) -> bool {
    let field = field.trim();
    field == "details" || field.starts_with("details.")
}

fn main() -> ExitCode {
    let mut start_pids: Vec<u32> = Vec::new();
    let mut theme: Option<Theme> = None;
    let mut support: Option<ColorSupport> = None;
    let mut lang: Option<Lang> = None;
    let mut json_out = false;
    let mut fields: Vec<String> = Vec::new();
    let mut include_vmas = false;
    let mut force_detail: Option<bool> = None;
    let mut diff: Option<(String, String)> = None;
    let mut diff_opts = metop::diff::Opts::default();
    let mut history_seconds = metop::app::DEFAULT_HISTORY;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-p" | "--pid" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse::<u32>().ok()) {
                    Some(pid) if start_pids.contains(&pid) => {
                        eprintln!("metop: duplicate pid {pid}");
                        return ExitCode::from(2);
                    }
                    Some(pid) => start_pids.push(pid),
                    None => {
                        eprintln!("metop: --pid needs a numeric pid");
                        return ExitCode::from(2);
                    }
                }
            }
            "--pids" => {
                i += 1;
                let Some(list) = args.get(i) else {
                    eprintln!("metop: --pids needs a comma-separated pid list");
                    return ExitCode::from(2);
                };
                let mut parsed = Vec::new();
                for part in list.split(',') {
                    let part = part.trim();
                    if part.is_empty() {
                        eprintln!("metop: --pids has an empty entry");
                        return ExitCode::from(2);
                    }
                    let Some(pid) = part.parse::<u32>().ok() else {
                        eprintln!("metop: --pids needs numeric pids");
                        return ExitCode::from(2);
                    };
                    if parsed.contains(&pid) || start_pids.contains(&pid) {
                        eprintln!("metop: duplicate pid {pid}");
                        return ExitCode::from(2);
                    }
                    parsed.push(pid);
                }
                if parsed.is_empty() {
                    eprintln!("metop: --pids needs at least one pid");
                    return ExitCode::from(2);
                }
                start_pids.extend(parsed);
            }
            "--history" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse::<u64>().ok()) {
                    Some(n) if (1..=metop::app::MAX_HISTORY).contains(&n) => history_seconds = n,
                    _ => {
                        eprintln!(
                            "metop: --history needs an integer from 1 to {}",
                            metop::app::MAX_HISTORY
                        );
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
                                "metop: unknown theme {name:?}; choose one of {}",
                                Theme::NAMES
                            );
                            return ExitCode::from(2);
                        }
                    },
                    None => {
                        eprintln!("metop: --theme needs a name ({})", Theme::NAMES);
                        return ExitCode::from(2);
                    }
                }
            }
            "--lang" => {
                i += 1;
                match args.get(i).and_then(|name| Lang::parse(name)) {
                    Some(mode) => lang = Some(mode),
                    None => {
                        eprintln!("metop: --lang needs one of {}", Lang::NAMES);
                        return ExitCode::from(2);
                    }
                }
            }
            "--color" => {
                i += 1;
                match args.get(i).and_then(|name| ColorSupport::parse(name)) {
                    Some(mode) => support = Some(mode),
                    None => {
                        eprintln!("metop: --color needs one of {}", ColorSupport::NAMES);
                        return ExitCode::from(2);
                    }
                }
            }
            "-V" | "--version" => {
                println!("metop {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--json" => json_out = true,
            "--vmas" => include_vmas = true,
            "--detail" => {
                if force_detail == Some(false) {
                    eprintln!("metop: --detail conflicts with --no-detail");
                    return ExitCode::from(2);
                }
                force_detail = Some(true);
            }
            "--no-detail" => {
                if force_detail == Some(true) {
                    eprintln!("metop: --no-detail conflicts with --detail");
                    return ExitCode::from(2);
                }
                force_detail = Some(false);
            }
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
                            eprintln!("metop: --fields needs at least one path");
                            return ExitCode::from(2);
                        }
                    }
                    None => {
                        eprintln!("metop: --fields needs a comma-separated list");
                        return ExitCode::from(2);
                    }
                }
            }
            "--min-delta" => {
                i += 1;
                match args.get(i).and_then(|s| metop::diff::parse_size(s)) {
                    Some(kb) => diff_opts.min_delta_kb = kb,
                    None => {
                        eprintln!("metop: --min-delta needs a size like 50M or 512k");
                        return ExitCode::from(2);
                    }
                }
            }
            "--min-delta-metric" => {
                i += 1;
                match args.get(i).and_then(|s| metop::diff::Metric::parse(s)) {
                    Some(metric) => diff_opts.min_delta_metric = metric,
                    None => {
                        eprintln!("metop: --min-delta-metric needs rss, pss, or max");
                        return ExitCode::from(2);
                    }
                }
            }
            "--top" => {
                i += 1;
                match args.get(i).and_then(|s| s.parse::<usize>().ok()) {
                    Some(n) => diff_opts.top = n,
                    None => {
                        eprintln!("metop: --top needs a number");
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
                        eprintln!("metop: --diff needs two JSON snapshot paths");
                        return ExitCode::from(2);
                    }
                }
            }
            "-h" | "--help" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("metop: unknown argument {other:?}\n\n{HELP}");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let start_pid = start_pids.first().copied();

    if start_pids.len() > 1 && !json_out {
        eprintln!("metop: multiple pids require --json");
        return ExitCode::from(2);
    }
    if include_vmas && !json_out {
        eprintln!("metop: --vmas requires --json");
        return ExitCode::from(2);
    }
    if force_detail.is_some() && !json_out {
        eprintln!("metop: --detail/--no-detail require --json");
        return ExitCode::from(2);
    }
    if include_vmas && start_pids.is_empty() {
        eprintln!("metop: --vmas needs --pid or --pids");
        return ExitCode::from(2);
    }
    if force_detail == Some(true) && start_pids.is_empty() {
        eprintln!("metop: --detail needs --pid or --pids");
        return ExitCode::from(2);
    }

    if start_pids.len() > 1
        && fields.iter().any(|field| is_detail_path(field))
        && !fields.iter().any(|field| is_details_path(field))
    {
        eprintln!("metop: multiple pids use --fields details, not detail");
        return ExitCode::from(2);
    }

    if fields.len() == 1 && matches!(fields[0].as_str(), "help" | "?") {
        if !json_out {
            eprintln!("metop: --fields help requires --json");
            return ExitCode::from(2);
        }
        let help_detail = !start_pids.is_empty() && force_detail != Some(false);
        let snap = if start_pids.len() > 1 && help_detail {
            sampler::snapshot_with_details(&start_pids)
        } else {
            sampler::snapshot_once(start_pid, help_detail)
        };
        for path in metop::json::help_paths(&snap, include_vmas) {
            println!("{path}");
        }
        return ExitCode::SUCCESS;
    }

    if let Some((a_path, b_path)) = diff {
        let read = |p: &str| std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"));
        let text = read(&a_path)
            .and_then(|a| Ok((a, read(&b_path)?)))
            .and_then(|(a, b)| metop::diff::render(&a, &b, &diff_opts));
        return match text {
            Ok(text) => match metop::json::write_str(&text) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("metop: {e}");
                    ExitCode::FAILURE
                }
            },
            Err(e) => {
                eprintln!("metop: {e}");
                ExitCode::from(2)
            }
        };
    }

    if json_out {
        // Plain `--json --pid` keeps detail for compatibility, but a projection
        // that cannot contain detail must not pay the smaps walk.
        let field_wants_detail = fields.is_empty()
            || fields.iter().any(|field| is_detail_path(field))
            || fields.iter().any(|field| is_details_path(field));
        let want_detail = force_detail.unwrap_or(!start_pids.is_empty() && field_wants_detail);
        let wants_details = fields.iter().any(|field| is_details_path(field));
        let snap = if want_detail && (start_pids.len() > 1 || wants_details) {
            sampler::snapshot_with_details(&start_pids)
        } else if start_pids.len() > 1 {
            sampler::snapshot_once(None, false)
        } else {
            sampler::snapshot_once(start_pid, want_detail)
        };
        let text = match metop::json::render(&snap, &fields, include_vmas) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("metop: {e}");
                return ExitCode::from(2);
            }
        };
        return match metop::json::write_str(&text) {
            Ok(()) => ExitCode::SUCCESS,
            // `metop --json | head` is normal usage, not an error.
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("metop: {e}");
                ExitCode::FAILURE
            }
        };
    }

    // --theme wins; METOP_THEME is the shell/alias convenience. A bad env value
    // warns instead of making every metop invocation fail.
    let theme = theme.unwrap_or_else(|| match std::env::var("METOP_THEME") {
        Ok(name) => Theme::by_name(&name).unwrap_or_else(|| {
            eprintln!(
                "metop: ignoring METOP_THEME={name:?}; choose one of {}",
                Theme::NAMES
            );
            Theme::DEFAULT
        }),
        Err(_) => Theme::DEFAULT,
    });

    let support = support.unwrap_or_else(ColorSupport::detect);
    let lang = lang.unwrap_or_else(|| match std::env::var("METOP_LANG") {
        Ok(value) => Lang::parse(&value).unwrap_or_else(|| {
            eprintln!(
                "metop: ignoring METOP_LANG={value:?}; choose one of {}",
                Lang::NAMES
            );
            Lang::detect()
        }),
        Err(_) => Lang::detect(),
    });

    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (snap_tx, snap_rx) = mpsc::channel();
    sampler::spawn(cmd_rx, snap_tx);

    let mut app = App::with_history(start_pid, history_seconds);
    app.support = support;
    app.theme = theme.rendered(support);
    app.lang = lang;
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    let result = app.run(&mut terminal, cmd_tx, snap_rx);
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("metop: {e}");
            ExitCode::FAILURE
        }
    }
}
