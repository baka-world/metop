//! Built-in colour themes.
//!
//! A theme is a compile-time [`Palette`]: mtop intentionally has no config
//! file (see `docs/REQUIREMENTS.md`), so selecting one is a CLI flag
//! (`--theme`), the `MTOP_THEME` environment variable, or the `t` key while
//! the TUI is running.
//!
//! Every palette slot is semantic rather than per-widget: a light terminal can
//! turn "too much memory" into a dark amber without any draw-code change, and
//! the default palette is exactly the colours mtop shipped with, so the default
//! look and the existing render tests are unchanged.

use ratatui::style::{Color, Style};

/// Semantic colours used by the whole UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// Secondary text, graph chrome, unfilled bar cells.
    pub dim: Color,
    /// `dim`, lifted so it stays readable on the tinted selected row.
    pub dim_selected: Color,
    /// Selected row / badge background.
    ///
    /// Only the background is set: per-cell foregrounds (bars, deltas) must
    /// survive selection instead of being flattened to one colour.
    pub highlight_bg: Color,
    /// Titles, table headers, sparklines.
    pub accent: Color,
    /// Share bar in its normal (not warning/critical) range.
    pub bar: Color,
    /// Healthy state: low memory pressure, negative deltas.
    pub ok: Color,
    /// Warning state: 5–20% PSI, 75–90% memory use.
    pub warn: Color,
    /// Critical state: OOM kills, >20% PSI, >90% memory use.
    pub crit: Color,
    /// Input prompt and cursor.
    pub focus: Color,
    /// PSS mix / composition segments: anonymous, file-backed, shmem, swap.
    pub anon: Color,
    pub file: Color,
    pub shmem: Color,
    pub swap: Color,
    /// Swap-PSS shown numerically in the detail summary.
    pub swap_pss: Color,
    /// Secondary text drawn on treemap tiles.
    pub tile_fg: Color,
    /// Treemap tile fills, cycled by group index.
    pub treemap: [Color; 6],
}

impl Palette {
    /// The colours mtop had before themes existed.
    pub const DEFAULT: Self = Self {
        dim: Color::DarkGray,
        dim_selected: Color::Gray,
        highlight_bg: Color::Rgb(38, 79, 120),
        accent: Color::Cyan,
        bar: Color::Cyan,
        ok: Color::Green,
        warn: Color::Yellow,
        crit: Color::Red,
        focus: Color::Yellow,
        anon: Color::Magenta,
        file: Color::Cyan,
        shmem: Color::Yellow,
        swap: Color::LightBlue,
        swap_pss: Color::Blue,
        tile_fg: Color::Gray,
        treemap: [
            Color::Rgb(46, 64, 94),
            Color::Rgb(60, 47, 82),
            Color::Rgb(36, 72, 72),
            Color::Rgb(82, 62, 42),
            Color::Rgb(52, 72, 46),
            Color::Rgb(78, 47, 58),
        ],
    };

    pub fn highlight(&self) -> Style {
        Style::new().bg(self.highlight_bg)
    }

    /// Secondary text; on the tinted selected row `dim` would vanish.
    pub fn dim(&self, selected: bool) -> Color {
        if selected {
            self.dim_selected
        } else {
            self.dim
        }
    }
}

/// `Color::Rgb` with byte components, so theme tables read like the hex they
/// came from.
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

/// Darken a theme hue for treemap fills: tile labels are light, so the fills
/// must stay dark enough for the foreground to read.
const fn shade((r, g, b): (u8, u8, u8), pct: u16) -> Color {
    Color::Rgb(
        ((r as u16 * pct) / 100) as u8,
        ((g as u16 * pct) / 100) as u8,
        ((b as u16 * pct) / 100) as u8,
    )
}

/// A named built-in palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
    pub palette: Palette,
}

impl Theme {
    pub const DEFAULT: Self = Self {
        name: "default",
        palette: Palette::DEFAULT,
    };

    /// btop's built-in Default theme (`src/btop_theme.cpp`, `Default_theme`).
    /// btop's two-digit hex values are greyscale (`#ee` -> 238,238,238).
    pub const BTOP: Self = Self {
        name: "btop",
        palette: Palette {
            dim: rgb(0x40, 0x40, 0x40),
            dim_selected: rgb(0xee, 0xee, 0xee),
            highlight_bg: rgb(0x6a, 0x2f, 0x2f),
            accent: rgb(0xee, 0xee, 0xee),
            bar: rgb(0x80, 0xd0, 0xa3),
            ok: rgb(0x77, 0xca, 0x9b),
            warn: rgb(0xcb, 0xc0, 0x6c),
            crit: rgb(0xdc, 0x4c, 0x4c),
            focus: rgb(0xb5, 0x40, 0x40),
            anon: rgb(0xff, 0x47, 0x69),
            file: rgb(0x26, 0xc5, 0xff),
            shmem: rgb(0xff, 0xb8, 0x14),
            swap: rgb(0xb0, 0xa9, 0xde),
            swap_pss: rgb(0xb0, 0xa9, 0xde),
            tile_fg: rgb(0xcc, 0xcc, 0xcc),
            treemap: [
                rgb(0x59, 0x2b, 0x26),
                rgb(0x16, 0x33, 0x50),
                rgb(0x4e, 0x3f, 0x0e),
                rgb(0x38, 0x4f, 0x21),
                rgb(0x29, 0x1f, 0x75),
                rgb(0x62, 0x06, 0x65),
            ],
        },
    };

    /// btop's `dracula.theme`: selected_bg/title/hi_fg/gradients are taken from
    /// the file; its CPU gradient is purple->cyan->green, so severity uses
    /// Dracula's canonical red/yellow/green instead.
    pub const DRACULA: Self = Self {
        name: "dracula",
        palette: Palette {
            dim: rgb(0x44, 0x47, 0x5a),
            dim_selected: rgb(0xf8, 0xf8, 0xf2),
            highlight_bg: rgb(0xff, 0x79, 0xc6),
            accent: rgb(0xf8, 0xf8, 0xf2),
            bar: rgb(0x50, 0xfa, 0x7b),
            ok: rgb(0x50, 0xfa, 0x7b),
            warn: rgb(0xf1, 0xfa, 0x8c),
            crit: rgb(0xff, 0x55, 0x55),
            focus: rgb(0x62, 0x72, 0xa4),
            anon: rgb(0x0d, 0xfa, 0x49),
            file: rgb(0x26, 0xd7, 0xfd),
            shmem: rgb(0xff, 0x9c, 0x33),
            swap: rgb(0x8b, 0xe9, 0xfd),
            swap_pss: rgb(0x8b, 0xe9, 0xfd),
            tile_fg: rgb(0xf8, 0xf8, 0xf2),
            treemap: [
                shade((0x50, 0xfa, 0x7b), 40),
                shade((0xbd, 0x93, 0xf9), 40),
                shade((0xff, 0x79, 0xc6), 40),
                shade((0x8b, 0xe9, 0xfd), 40),
                shade((0xff, 0x9c, 0x33), 40),
                shade((0xff, 0x33, 0xa8), 40),
            ],
        },
    };

    /// btop's `gruvbox_dark.theme`. btop's inactive_fg/selected_bg are nearly
    /// the same as main_bg, so the slots mtop uses for readable secondary text
    /// and selection are lifted to gruvbox bg1/fg4.
    pub const GRUVBOX: Self = Self {
        name: "gruvbox-dark",
        palette: Palette {
            dim: rgb(0x7c, 0x6f, 0x64),
            dim_selected: rgb(0xfa, 0xbd, 0x2f),
            highlight_bg: rgb(0x3c, 0x38, 0x36),
            accent: rgb(0xeb, 0xdb, 0xb2),
            bar: rgb(0xb8, 0xbb, 0x26),
            ok: rgb(0xb8, 0xbb, 0x26),
            warn: rgb(0xfa, 0xbd, 0x2f),
            crit: rgb(0xfb, 0x49, 0x34),
            focus: rgb(0xd7, 0x99, 0x21),
            anon: rgb(0xcc, 0x24, 0x1d),
            file: rgb(0x83, 0xa5, 0x98),
            shmem: rgb(0xfa, 0xbd, 0x2f),
            swap: rgb(0xa3, 0xa8, 0xf7),
            swap_pss: rgb(0xa3, 0xa8, 0xf7),
            tile_fg: rgb(0xeb, 0xdb, 0xb2),
            treemap: [
                shade((0xcc, 0x24, 0x1d), 60),
                shade((0x45, 0x85, 0x88), 60),
                shade((0xd7, 0x99, 0x21), 60),
                shade((0x4e, 0x59, 0x00), 60),
                shade((0x3d, 0x40, 0x70), 60),
                shade((0x70, 0x1c, 0x45), 60),
            ],
        },
    };

    /// btop's `kanagawa-wave.theme`; treemap hues are that theme's gradient
    /// starts, darkened so the light tile labels stay readable.
    pub const KANAGAWA: Self = Self {
        name: "kanagawa-wave",
        palette: Palette {
            dim: rgb(0x72, 0x71, 0x69),
            dim_selected: rgb(0xdc, 0xa5, 0x61),
            highlight_bg: rgb(0x22, 0x32, 0x49),
            accent: rgb(0xdc, 0xd7, 0xba),
            bar: rgb(0x98, 0xbb, 0x6c),
            ok: rgb(0x98, 0xbb, 0x6c),
            warn: rgb(0xdc, 0xa5, 0x61),
            crit: rgb(0xe8, 0x24, 0x24),
            focus: rgb(0xc3, 0x40, 0x43),
            anon: rgb(0x7f, 0xb4, 0xca),
            file: rgb(0xff, 0x9e, 0x3b),
            shmem: rgb(0xdc, 0xa5, 0x61),
            swap: rgb(0x95, 0x7f, 0xbb),
            swap_pss: rgb(0x95, 0x7f, 0xbb),
            tile_fg: rgb(0xdc, 0xd7, 0xba),
            treemap: [
                shade((0x65, 0x85, 0x94), 45),
                shade((0xc0, 0xa3, 0x6e), 45),
                shade((0x93, 0x8a, 0xa9), 45),
                shade((0xe8, 0x24, 0x24), 45),
                shade((0x7e, 0x9c, 0xdb), 45),
                shade((0xdc, 0xa5, 0x61), 45),
            ],
        },
    };

    /// For light terminals (dark text on a light background).
    pub const LIGHT: Self = Self {
        name: "light",
        palette: Palette {
            dim: Color::DarkGray,
            dim_selected: Color::Black,
            highlight_bg: Color::Rgb(191, 219, 254),
            accent: Color::Blue,
            bar: Color::Blue,
            ok: Color::Green,
            // Yellow is unreadable on white; dark amber keeps the meaning.
            warn: Color::Rgb(146, 64, 14),
            crit: Color::Red,
            focus: Color::Blue,
            anon: Color::Magenta,
            file: Color::Blue,
            shmem: Color::Rgb(146, 64, 14),
            swap: Color::Cyan,
            swap_pss: Color::Blue,
            tile_fg: Color::Black,
            treemap: [
                Color::Rgb(219, 234, 254),
                Color::Rgb(237, 233, 254),
                Color::Rgb(204, 251, 241),
                Color::Rgb(254, 243, 199),
                Color::Rgb(220, 252, 231),
                Color::Rgb(254, 226, 226),
            ],
        },
    };

    /// Only the 16 ANSI colours: usable on limited terminals and in tmux,
    /// where RGB is quantised unpredictably.
    pub const ANSI16: Self = Self {
        name: "ansi16",
        palette: Palette {
            dim: Color::DarkGray,
            dim_selected: Color::Gray,
            highlight_bg: Color::Blue,
            accent: Color::Cyan,
            bar: Color::Cyan,
            ok: Color::Green,
            warn: Color::Yellow,
            crit: Color::Red,
            focus: Color::Yellow,
            anon: Color::Magenta,
            file: Color::Cyan,
            shmem: Color::Yellow,
            swap: Color::LightBlue,
            swap_pss: Color::Blue,
            tile_fg: Color::Gray,
            treemap: [
                Color::Blue,
                Color::Magenta,
                Color::Cyan,
                Color::Yellow,
                Color::Green,
                Color::Red,
            ],
        },
    };

    pub const ALL: [Self; 7] = [
        Self::DEFAULT,
        Self::BTOP,
        Self::DRACULA,
        Self::GRUVBOX,
        Self::KANAGAWA,
        Self::LIGHT,
        Self::ANSI16,
    ];
    pub const NAMES: &'static str =
        "default, btop, dracula, gruvbox-dark, kanagawa-wave, light, ansi16";

    pub fn by_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|theme| theme.name.eq_ignore_ascii_case(name))
    }

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|theme| theme.name == self.name)
            .unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::{Palette, Theme};
    use ratatui::style::Color;

    #[test]
    fn default_theme_keeps_the_shipped_colours() {
        let p = Theme::DEFAULT.palette;
        assert_eq!(p, Palette::DEFAULT);
        assert_eq!(p.highlight_bg, Color::Rgb(38, 79, 120));
        assert_eq!(p.dim, Color::DarkGray);
        assert_eq!(p.dim_selected, Color::Gray);
        assert_eq!(p.accent, Color::Cyan);
        assert_eq!(p.bar, Color::Cyan);
        assert_eq!(p.focus, Color::Yellow);
        assert_eq!(p.treemap[0], Color::Rgb(46, 64, 94));
        assert_eq!(p.treemap[5], Color::Rgb(78, 47, 58));
    }

    #[test]
    fn theme_lookup_is_case_insensitive_and_rejects_unknown_names() {
        assert_eq!(Theme::by_name("default"), Some(Theme::DEFAULT));
        assert_eq!(Theme::by_name("BTOP"), Some(Theme::BTOP));
        assert_eq!(Theme::by_name("Dracula"), Some(Theme::DRACULA));
        assert_eq!(Theme::by_name("gruvbox-dark"), Some(Theme::GRUVBOX));
        assert_eq!(Theme::by_name("Kanagawa-Wave"), Some(Theme::KANAGAWA));
        assert_eq!(Theme::by_name("Light"), Some(Theme::LIGHT));
        assert_eq!(Theme::by_name("ANSI16"), Some(Theme::ANSI16));
        assert_eq!(Theme::by_name("solarized"), None);
    }

    #[test]
    fn t_cycles_through_every_builtin_and_wraps() {
        for pair in Theme::ALL.windows(2) {
            assert_eq!(pair[0].next(), pair[1], "{} ->", pair[0].name);
        }
        assert_eq!(Theme::ALL[Theme::ALL.len() - 1].next(), Theme::ALL[0]);
    }

    #[test]
    fn names_lists_every_theme_in_order() {
        let joined = Theme::ALL
            .iter()
            .map(|theme| theme.name)
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(joined, Theme::NAMES);
    }

    #[test]
    fn btop_theme_matches_the_btop_default_palette() {
        let p = Theme::BTOP.palette;
        assert_eq!(p.highlight_bg, Color::Rgb(0x6a, 0x2f, 0x2f));
        assert_eq!(p.ok, Color::Rgb(0x77, 0xca, 0x9b));
        assert_eq!(p.warn, Color::Rgb(0xcb, 0xc0, 0x6c));
        assert_eq!(p.crit, Color::Rgb(0xdc, 0x4c, 0x4c));
        assert_eq!(p.anon, Color::Rgb(0xff, 0x47, 0x69));
        assert_eq!(p.treemap[1], Color::Rgb(0x16, 0x33, 0x50));
    }

    #[test]
    fn btop_derived_themes_have_six_distinct_dark_tile_fills() {
        for theme in [Theme::BTOP, Theme::DRACULA, Theme::GRUVBOX, Theme::KANAGAWA] {
            let fills = theme.palette.treemap;
            let distinct: std::collections::HashSet<Color> = fills.iter().copied().collect();
            assert_eq!(distinct.len(), 6, "{} reuses a tile fill", theme.name);
            for fill in fills {
                let Color::Rgb(r, g, b) = fill else {
                    panic!("{} has a non-RGB tile", theme.name)
                };
                // Tile labels are light; keep fills dark enough to read.
                assert!(
                    r.max(g).max(b) < 200,
                    "{} tile too bright: {fill:?}",
                    theme.name
                );
            }
        }
    }

    #[test]
    fn ansi16_uses_no_rgb_or_indexed_colours() {
        let p = Theme::ANSI16.palette;
        let slots = [
            p.dim,
            p.dim_selected,
            p.highlight_bg,
            p.accent,
            p.bar,
            p.ok,
            p.warn,
            p.crit,
            p.focus,
            p.anon,
            p.file,
            p.shmem,
            p.swap,
            p.swap_pss,
            p.tile_fg,
        ];
        for color in slots.into_iter().chain(p.treemap) {
            assert!(
                !matches!(color, Color::Rgb(..) | Color::Indexed(_)),
                "ansi16 theme leaked {color:?}"
            );
        }
    }
}
