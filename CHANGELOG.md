# Changelog

All notable changes to mtop are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Before 1.0, a
feature release bumps the minor version (`0.MINOR.0`); fixes bump the patch
version (`0.MINOR.PATCH`).

## [Unreleased]

### Added

- Colour-depth rendering: themes stay truecolor on capable terminals and are
  quantised to 256 colours (`Color::Indexed`, btop's 6×6×6 cube + grey ramp) or
  named ANSI colours otherwise. `--color auto|truecolor|256|16` overrides
  detection.

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

- PSS MIX now uses a fixed, colour-blind-safe palette (Okabe–Ito: sky blue
  anon, green file, orange shmem, purple swap) instead of theme colours, so the
  four categories mean the same thing in every theme. The bar is 16 cells wide,
  keeps at least one cell per nonzero category, and the footer shows a legend.


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

[Unreleased]: https://github.com/baka-world/mtop/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/baka-world/mtop/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/baka-world/mtop/releases/tag/v0.1.0
