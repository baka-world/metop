use std::process::ExitCode;
use std::sync::mpsc;

use mtop::app::App;
use mtop::sampler;

const HELP: &str = "\
mtop - Linux memory analysis TUI

USAGE:
    mtop [--pid PID]

OPTIONS:
    -p, --pid PID   start with PID selected (also enables Enter drill-down)
    -h, --help      show this help

KEYS:
    j/k, ↑/↓        move selection          g/G     top / bottom
    s               cycle sort key          r       reverse order
    f               filter process name/pid Enter   per-process VMA breakdown
    Esc             back / clear filter     q       quit
";

fn main() -> ExitCode {
    let mut start_pid = None;
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

    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (snap_tx, snap_rx) = mpsc::channel();
    sampler::spawn(cmd_rx, snap_tx);

    let mut terminal = ratatui::init();
    let result = App::run(&mut terminal, start_pid, cmd_tx, snap_rx);
    ratatui::restore();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mtop: {e}");
            ExitCode::FAILURE
        }
    }
}
