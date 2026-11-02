# mtop

[![CI](https://github.com/baka-world/mtop/actions/workflows/ci.yml/badge.svg)](https://github.com/baka-world/mtop/actions/workflows/ci.yml)
[![Release](https://github.com/baka-world/mtop/actions/workflows/release.yml/badge.svg)](https://github.com/baka-world/mtop/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**English** · [简体中文](README.zh-CN.md)

A Linux memory analysis TUI. `htop` answers "how much RSS does each process have"; mtop answers **"who is actually using the memory, and on what"** — with PSS/USS, per-mapping drill-down, cgroup accounting and an app-level treemap.

![mtop demo](docs/images/demo.gif)

## Why PSS

RSS counts every shared page once per process, so any multi-process app looks inflated. Measured on one desktop at the same instant:

| | Chrome (25 processes) | Whole system (429 processes) |
|---|---|---|
| RSS summed | 4.1 GB | 11.9 GB |
| **PSS** | **1.7 GB** | **5.5 GB** |
| overcount | 2.4× | 2.2× |

RSS sums to roughly **2.2×** the real number system-wide (2.4× for Chrome). mtop uses PSS for ranking and area, USS for "what would I free if I killed it", and shows the composition (anonymous / file / shmem / swap) so you can tell a heap leak from page cache.

## Features

- **Process view** — RSS / PSS / USS / PSS÷RSS / Δ·s⁻¹, share and PSS-composition bars.
- **VMA detail** — aggregate by mapping, then expand one mapping into its individual VMAs (address range, perms, PSS/USS/anon/swap/size).
- **cgroup v2 view** — `memory.current` / `memory.stat` / `memory.max` / `memory.events`, PSI (`some`/`full`), OOM deltas, and drill-down into a subtree's processes.
- **App treemap** — processes grouped into applications (systemd `app-*.scope` → exe → comm), area ∝ PSS, long tail folded into `others`.
- **Scriptable** — `--json` (with `--fields` projection), `--diff` (with `--min-delta` / `--top`), stable pid ordering for diffing.
- **Themes** — seven palettes, including btop-derived `btop`, `dracula`, `gruvbox-dark` and `kanagawa-wave`; pick one with `--theme`, `MTOP_THEME`, or `t` while running (no config file).

## Install

Prebuilt packages are produced by GitHub Actions for `amd64` and `arm64`; every asset ships with a SHA256 checksum.

| Platform | Asset |
|---|---|
| Debian / Ubuntu | `mtop_0.2.0-1_amd64.deb` · `mtop_0.2.0-1_arm64.deb` |
| Fedora / RHEL / openSUSE | `mtop-0.2.0-1.x86_64.rpm` · `mtop-0.2.0-1.aarch64.rpm` |
| Any distro | `mtop-v0.2.0-x86_64-unknown-linux-musl.tar.gz` · `…-aarch64-…` |

```sh
# Debian / Ubuntu
gh release download v0.2.0 --repo baka-world/mtop --pattern '*.deb'
sudo dpkg -i mtop_0.2.0-1_amd64.deb

# Fedora / RHEL
gh release download v0.2.0 --repo baka-world/mtop --pattern '*.rpm'
sudo rpm -i mtop-0.2.0-1.x86_64.rpm

# From source
cargo build --release && sudo install -m755 target/release/mtop /usr/local/bin/
```

Binaries are statically linked (musl), and the `.deb` has an empty `Depends:`.

## Usage

```
mtop                       # whole system
mtop --pid 1234            # start with a process selected
mtop --theme dracula       # default | btop | dracula | gruvbox-dark |
                           # kanagawa-wave | light | ansi16 (or MTOP_THEME)
mtop --json --fields procs.pid,procs.rss_kb
mtop --diff a.json b.json --min-delta 50M
```

| Key | Action |
|---|---|
| `Tab` | cycle views: processes → cgroups → apps |
| `j`/`k`, `↑`/`↓`, `PgUp`/`PgDn` | move |
| `g`/`G` | top / bottom |
| `s` / `r` | cycle sort key (apps: metric) / reverse |
| `t` | cycle colour theme (7 built-in palettes) |
| `f` | filter (process name, pid, cgroup path, mapping name) |
| `Enter` | detail / expand mapping / drill into a cgroup or an app |
| `Esc` | clear filter, leave detail or drill-down |
| `q` | quit |

### Process view

![process view](docs/images/process.png)

The header reports the sampler's own cost (`sweep`/`pss` ms, PSS coverage, sample age) so partial data is never presented as complete. Rows with `n/a` need privileges; a dimmed `~` marks a cached value older than the freshness window.

### VMA detail

![vma detail](docs/images/detail.png)

`Enter` on a process aggregates its mappings by name; `Enter` again expands one group into individual VMAs. `f` filters mapping names, `z`/`Z` collapse or expand everything.

### cgroup v2

![cgroup view](docs/images/cgroups.png)

Values are hierarchical: a parent already includes its children, so the list sorts by `current` instead of summing levels. `Enter` lists the processes in that subtree:

![cgroup drill](docs/images/drill.png)

### Apps treemap

![apps treemap](docs/images/apps.png)

Area is PSS by default; `s` switches to USS (private only) or RSS (which overcounts shared pages, and says so in the title). `Enter` drills into that app's processes.

## Scripting

```sh
mtop --json > a.json                      # one snapshot, stable pid order
mtop --json --fields procs.pid,procs.rss_kb > small.json   # 194 KB -> 23 KB
sleep 30; mtop --json > b.json
mtop --diff a.json b.json --min-delta 50M
```

`--diff` prints ΔRSS / ΔPSS, current values and `new`/`gone` markers; the snapshot writer itself is excluded via `self_pid`.

## Requirements and permissions

- Linux with `/proc`; cgroup v2 optional (the view explains its absence).
- `/proc/PID/smaps_rollup` requires ptrace-read permission: a normal user only sees their own processes (and kernel threads have no address space at all). Run `sudo mtop` for full coverage — verified at `264/264` covered, `denied=0`.
- The usage model is documented in [docs/REQUIREMENTS.md](docs/REQUIREMENTS.md); the root checklist is [docs/VERIFY-ROOT.md](docs/VERIFY-ROOT.md).

## Known issues

- **Counts are partial without privileges.** Non-root runs only cover your own user's processes; kernel threads never have a PSS and are excluded from coverage denominators.
- **Two history windows.** Process/apps keep 120 samples at 1 Hz (2 min); cgroup/PSI keep 120 samples at the 5 s cadence (10 min). Comparing the two trends side by side is misleading.
- **App grouping falls back to the executable.** Without a systemd `app-*.scope`, two Chromium installs merge into one tile; `others (N)` is synthetic and cannot be drilled into.
- **cgroup v1 is unsupported** — the view states why instead of showing zeros.
- **Sampling perturbs the target.** Reading `smaps` walks page tables under the mmap lock, so walks are budgeted (150 ms/tick) and the detail view is on-demand.

## TODO

- Configurable history window (`--history 600`), so all views share one time axis.
- Include app groups in `--json`.
- Optional: KSM / hugepage / pagemap metadata per VMA (via `wora-observability-linux-mm`).
- Optional: per-view metric defaults and a config file.

## Development

```sh
cargo build --release
cargo test --release     # 69 tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## License

MIT — see [LICENSE](LICENSE).
