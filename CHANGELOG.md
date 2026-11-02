# Changelog

All notable changes to mtop are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Before 1.0, a
feature release bumps the minor version (`0.MINOR.0`); fixes bump the patch
version (`0.MINOR.PATCH`).

## [Unreleased]

## [0.3.0] - 2026-11-01

### Added

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

[Unreleased]: https://github.com/baka-world/mtop/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/baka-world/mtop/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/baka-world/mtop/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/baka-world/mtop/releases/tag/v0.1.0
