# Changelog

All notable changes to metop are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Before 1.0, a
feature release bumps the minor version (`0.MINOR.0`); fixes bump the patch
version (`0.MINOR.PATCH`).

## [Unreleased]

### Added

- A configurable shared trend window (`--history`, 1–3600 seconds; default
  120) for process, app, cgroup PSI and OOM histories.
- Uncollapsed application groups in JSON snapshots, including memory totals,
  process counts and PIDs, with `--fields` projection support.
- JSON process identity and grouping context: `procs[].exe`, `cmdline`,
  `start_unix_ms` and `cgroup`, plus `apps[].source`
  (`scope` / `exe` / `comm` / `mixed`).
- JSON VMA detail: `detail.groups[].rss_kb`, unambiguous `vsize_kb`, and
  opt-in per-VMA records via `--vmas` (`vma_list[]`). The group count is now
  `vma_count`.
- Multiple VMA details in one snapshot: repeatable `--pid` or `--pids A,B`
  emits `details[]` with `detail_errors[]`, sharing one `at_unix_ms`.
- `--fields help` lists the leaf paths available in the current snapshot;
  unknown paths report the children of the failing segment.
- `--detail` / `--no-detail` make the JSON detail section explicit.
- `--min-delta-metric rss|pss|max` for `--diff` (default `max`), so the
  threshold and row ranking include PSS.
- Vim-style `/` shortcut for filtering, alongside the existing `f` key.
- Vim-style half-page and full-page navigation, plus `Ctrl-w` and `Ctrl-u`
  editing in filter inputs.
- `CONTRIBUTING.md`, bug/feature issue forms, a PR template, priority/area
  labels and the `v0.5.0` milestone as the issue-first workflow.

### Changed

- The `--json` schema is now `metop.snapshot.v2`. `detail.groups[].size_kb`
  became `vsize_kb` (virtual address space), `detail.groups[].vmas` became
  `vma_count`, and a one-shot snapshot emits `delta_kb: null` instead of a
  misleading zero. `--diff` still accepts v1 and legacy `mtop.snapshot.v1`.
- `--json --pid` still includes detail, but a projection that does not name
  `detail` (for example `--fields procs.pid`) no longer pays the `smaps` walk.
- `--diff` filters and ranks by `max(|ΔRSS|, |ΔPSS|)` by default; `--min-delta`
  keeps the selected metric.
- `PgUp` and `PgDn` now use the actual viewport height instead of ten rows.

### Fixed

- `--fields detail` retains `detail_error`, so a denied or missing pid is no
  longer an unexplained `detail: null`.
- A PSS-only change is no longer hidden by `--min-delta` or cut by `--top`
  merely because RSS is flat.
- Detail docs and the TUI help call the VMA `VSIZE` value virtual memory, not
  resident memory.

## [0.4.0] - 2026-11-01

### Changed

- Project renamed from `mtop` to `metop`: the binary, library, crate, GitHub
  repository, release assets and deb/rpm package names are now `metop`.
- The `--json` schema is now `metop.snapshot.v1`; `--diff` still accepts old
  `mtop.snapshot.v1` files.
- Old v0.3.0 GitHub assets and the `metop 0.3.0` crate (which installed an
  `mtop` binary) remain available; new installs use `cargo install metop`.

## [0.3.0] - 2026-11-01

### Added

- Published on crates.io as `metop`; the installed binary and Rust library
  remain `mtop`.
- Mouse support: click table rows and treemap tiles (click again to open,
  expand or drill), wheel scrolling, and a clickable footer command bar.
- Colour-depth rendering: themes stay truecolor on capable terminals and are
  quantised to 256 colours (`Color::Indexed`, btop's 6×6×6 cube + grey ramp) or
  named ANSI colours otherwise. `--color auto|truecolor|256|16` overrides
  detection.
- Per-page help overlay (`?`) covering the header, columns, markers and keys
  for processes, cgroups, apps, VMA detail and cgroup drill-down.
- English/Chinese UI with `--lang auto|en|zh`, `MTOP_LANG` and the `l` key.

### Changed

- The footer shortcut hint was replaced by a contextual command bar with
  clickable buttons; global actions stay first so they survive narrow widths.
- PSS MIX follows the active theme again, with a test that its four colours
  stay distinct. The bar is 16 cells wide and keeps at least one cell per
  nonzero category; its footer legend only appears in the process view.
- Dark themes' app treemap fills now use their bright palette colours instead
  of heavily shaded variants, so the apps view matches the rest of the theme.

### Fixed

- Treemap selection no longer reuses the table highlight colour, which could
  match a neighbouring tile fill (most visibly in the `light` theme). The
  selected tile now inverts its own fill and foreground, and the narrow apps
  list draws the selected row's share bar in the tile foreground instead of a
  fill that can match the highlight.
- Nonzero RSS deltas below 1 MB no longer render as `+0.0M`/`-0.0M`; they are
  shown in kB (`+4k`), so a real change can never look like zero.

## [0.2.0] - 2026-11-01

### Added

- Seven built-in colour themes: `default`, `btop`, `dracula`, `gruvbox-dark`,
  `kanagawa-wave`, `light` and `ansi16`. The `btop`, `dracula`, `gruvbox-dark`
  and `kanagawa-wave` palettes are ported from btop's bundled theme files.
- Theme selection with `--theme NAME`, the `MTOP_THEME` environment variable,
  or the `t` key while running. The footer names the active theme.

### Changed

- The UI now draws from a semantic `Palette` instead of hardcoded colours.
  The `default` palette keeps the previous colours exactly, so existing
  screenshots and render tests are unchanged.

### Fixed

- Treemap labels use the active theme's tile foreground, so they remain
  readable on light and ANSI themes.

## [0.1.0] - 2026-10-31

### Added

- Initial release: PSS/USS process table, per-mapping VMA detail, cgroup v2
  accounting with PSI/OOM tracking, app treemap, `--json`/`--diff` scripting,
  and `.deb`/`.rpm`/static-musl packaging.

[Unreleased]: https://github.com/baka-world/metop/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/baka-world/metop/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/baka-world/metop/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/baka-world/metop/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/baka-world/metop/releases/tag/v0.1.0
