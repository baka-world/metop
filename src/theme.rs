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

/// How many colours the terminal can actually display.
///
/// Themes are authored in 24-bit RGB; [`Theme::rendered`] converts each slot to
/// the terminal's depth, so a 256-colour terminal gets `38;5;n` instead of RGB
/// SGR it may not parse, and a 16-colour terminal gets named ANSI colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSupport {
    TrueColor,
    Ansi256,
    Ansi16,
}

impl ColorSupport {
    /// Best-effort detection from `COLORTERM`/`TERM`, falling back to
    /// crossterm's count. Checking both matters: crossterm prefers an empty
    /// `COLORTERM` over a perfectly good `TERM=...-256color`.
    pub fn detect() -> Self {
        if let Some(value) = std::env::var_os("COLORTERM") {
            let value = value.to_string_lossy();
            if value.contains("truecolor") || value.contains("24bit") {
                return Self::TrueColor;
            }
        }
        if let Some(value) = std::env::var_os("TERM") {
            let value = value.to_string_lossy();
            if value.contains("256color") {
                return Self::Ansi256;
            }
            if value.contains("direct") {
                return Self::TrueColor;
            }
        }
        match ratatui::crossterm::style::available_color_count() {
            u16::MAX => Self::TrueColor,
            256 => Self::Ansi256,
            _ => Self::Ansi16,
        }
    }

    pub const NAMES: &'static str = "auto, truecolor, 256, 16";

    /// Parse a `--color` value; `auto` performs detection.
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Some(Self::detect()),
            "truecolor" | "24bit" => Some(Self::TrueColor),
            "256" | "ansi256" => Some(Self::Ansi256),
            "16" | "ansi16" => Some(Self::Ansi16),
            _ => None,
        }
    }
}

// PSS composition colours are fixed rather than themed: anon/file/shmem/swap
// must mean the same thing in every theme. Okabe-Ito hues, with a darker
// variant for light terminals and ANSI names for 16-colour terminals.
const MIX_ANON_DARK: Color = rgb(0x56, 0xb4, 0xe9);
const MIX_FILE_DARK: Color = rgb(0x00, 0x9e, 0x73);
const MIX_SHMEM_DARK: Color = rgb(0xe6, 0x9f, 0x00);
const MIX_SWAP_DARK: Color = rgb(0xcc, 0x79, 0xa7);

const MIX_ANON_LIGHT: Color = rgb(0x00, 0x72, 0xb2);
const MIX_FILE_LIGHT: Color = rgb(0x00, 0x7a, 0x5e);
const MIX_SHMEM_LIGHT: Color = rgb(0x9a, 0x5b, 0x00);
const MIX_SWAP_LIGHT: Color = rgb(0x8e, 0x45, 0x85);

const MIX_ANON_ANSI: Color = Color::Blue;
const MIX_FILE_ANSI: Color = Color::Cyan;
const MIX_SHMEM_ANSI: Color = Color::Yellow;
const MIX_SWAP_ANSI: Color = Color::Magenta;

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
    /// PSS mix: anonymous pages. Fixed across themes (see `MIX_*`).
    pub anon: Color,
    /// PSS mix: file-backed pages. Fixed across themes.
    pub file: Color,
    /// PSS mix: shared-memory pages. Fixed across themes.
    pub shmem: Color,
    /// PSS mix: PSS-weighted swapped-out pages. Fixed across themes.
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
        anon: MIX_ANON_DARK,
        file: MIX_FILE_DARK,
        shmem: MIX_SHMEM_DARK,
        swap: MIX_SWAP_DARK,
        swap_pss: MIX_SWAP_DARK,
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

    /// Convert every RGB slot to the terminal's colour depth.
    pub fn rendered(self, support: ColorSupport) -> Self {
        let (anon, file, shmem, swap, swap_pss) = if support == ColorSupport::Ansi16 {
            // Nearest-RGB would map the green/blue categories onto similar
            // ANSI hues; use the fixed 16-colour set instead.
            (
                MIX_ANON_ANSI,
                MIX_FILE_ANSI,
                MIX_SHMEM_ANSI,
                MIX_SWAP_ANSI,
                MIX_SWAP_ANSI,
            )
        } else {
            (
                render_color(self.anon, support),
                render_color(self.file, support),
                render_color(self.shmem, support),
                render_color(self.swap, support),
                render_color(self.swap_pss, support),
            )
        };
        Self {
            dim: render_color(self.dim, support),
            dim_selected: render_color(self.dim_selected, support),
            highlight_bg: render_color(self.highlight_bg, support),
            accent: render_color(self.accent, support),
            bar: render_color(self.bar, support),
            ok: render_color(self.ok, support),
            warn: render_color(self.warn, support),
            crit: render_color(self.crit, support),
            focus: render_color(self.focus, support),
            anon,
            file,
            shmem,
            swap,
            swap_pss,
            tile_fg: render_color(self.tile_fg, support),
            treemap: self.treemap.map(|color| render_color(color, support)),
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

/// Convert one RGB colour to what `support` can display.
fn render_color(color: Color, support: ColorSupport) -> Color {
    match (support, color) {
        (ColorSupport::TrueColor, color) => color,
        (ColorSupport::Ansi256, Color::Rgb(r, g, b)) => Color::Indexed(rgb_to_256(r, g, b)),
        (ColorSupport::Ansi16, Color::Rgb(r, g, b)) => nearest_ansi16(r, g, b),
        (_, color) => color,
    }
}

/// btop's truecolor->256 rule: the 232-255 grey ramp when the channels are
/// close, otherwise the 6x6x6 colour cube.
fn rgb_to_256(r: u8, g: u8, b: u8) -> u8 {
    let (rf, gf, bf) = (r as f64, g as f64, b as f64);
    if (rf / 11.0).round() == (gf / 11.0).round() && (gf / 11.0).round() == (bf / 11.0).round() {
        (232.0 + (rf / 11.0).round()).min(255.0) as u8
    } else {
        (16.0 + (rf / 51.0).round() * 36.0 + (gf / 51.0).round() * 6.0 + (bf / 51.0).round()) as u8
    }
}

#[rustfmt::skip]
const ANSI16_RGB: [(u8, u8, u8); 16] = [
    (0, 0, 0), (128, 0, 0), (0, 128, 0), (128, 128, 0),
    (0, 0, 128), (128, 0, 128), (0, 128, 128), (192, 192, 192),
    (85, 85, 85), (255, 0, 0), (0, 255, 0), (255, 255, 0),
    (0, 0, 255), (255, 0, 255), (0, 255, 255), (255, 255, 255),
];

#[rustfmt::skip]
const ANSI16_COLORS: [Color; 16] = [
    Color::Black, Color::Red, Color::Green, Color::Yellow,
    Color::Blue, Color::Magenta, Color::Cyan, Color::Gray,
    Color::DarkGray, Color::LightRed, Color::LightGreen, Color::LightYellow,
    Color::LightBlue, Color::LightMagenta, Color::LightCyan, Color::White,
];

fn nearest_ansi16(r: u8, g: u8, b: u8) -> Color {
    let mut best = 0;
    let mut best_distance = u32::MAX;
    for (index, (r2, g2, b2)) in ANSI16_RGB.into_iter().enumerate() {
        let distance = (i32::from(r) - i32::from(r2)).pow(2) as u32
            + (i32::from(g) - i32::from(g2)).pow(2) as u32
            + (i32::from(b) - i32::from(b2)).pow(2) as u32;
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    ANSI16_COLORS[best]
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
            anon: MIX_ANON_DARK,
            file: MIX_FILE_DARK,
            shmem: MIX_SHMEM_DARK,
            swap: MIX_SWAP_DARK,
            swap_pss: MIX_SWAP_DARK,
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
            anon: MIX_ANON_DARK,
            file: MIX_FILE_DARK,
            shmem: MIX_SHMEM_DARK,
            swap: MIX_SWAP_DARK,
            swap_pss: MIX_SWAP_DARK,
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
            anon: MIX_ANON_DARK,
            file: MIX_FILE_DARK,
            shmem: MIX_SHMEM_DARK,
            swap: MIX_SWAP_DARK,
            swap_pss: MIX_SWAP_DARK,
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
            anon: MIX_ANON_DARK,
            file: MIX_FILE_DARK,
            shmem: MIX_SHMEM_DARK,
            swap: MIX_SWAP_DARK,
            swap_pss: MIX_SWAP_DARK,
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
            anon: MIX_ANON_LIGHT,
            file: MIX_FILE_LIGHT,
            shmem: MIX_SHMEM_LIGHT,
            swap: MIX_SWAP_LIGHT,
            swap_pss: MIX_SWAP_LIGHT,
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
            anon: MIX_ANON_ANSI,
            file: MIX_FILE_ANSI,
            shmem: MIX_SHMEM_ANSI,
            swap: MIX_SWAP_ANSI,
            swap_pss: MIX_SWAP_ANSI,
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

    /// Apply terminal colour-depth conversion while keeping the theme name.
    pub fn rendered(self, support: ColorSupport) -> Self {
        Self {
            name: self.name,
            palette: self.palette.rendered(support),
        }
    }

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
    fn default_theme_keeps_the_shipped_chrome_colours() {
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
    fn ansi256_conversion_uses_the_cube_and_grey_ramp() {
        assert_eq!(super::rgb_to_256(255, 0, 0), 196);
        assert_eq!(super::rgb_to_256(0, 255, 0), 46);
        assert_eq!(super::rgb_to_256(0, 0, 255), 21);
        assert_eq!(super::rgb_to_256(0, 0, 0), 232);
        assert_eq!(super::rgb_to_256(255, 255, 255), 255);
        assert_eq!(super::rgb_to_256(128, 128, 128), 244);
    }

    #[test]
    fn rendered_256_maps_rgb_to_indexed_and_keeps_the_name() {
        let theme = Theme::BTOP.rendered(super::ColorSupport::Ansi256);
        assert_eq!(theme.name, "btop");
        assert_eq!(
            theme.palette.highlight_bg,
            Color::Indexed(super::rgb_to_256(0x6a, 0x2f, 0x2f))
        );
        assert!(matches!(theme.palette.treemap[0], Color::Indexed(_)));
    }

    #[test]
    fn rendered_16_uses_named_ansi_colours() {
        use super::ColorSupport;
        assert_eq!(
            super::render_color(Color::Rgb(255, 0, 0), ColorSupport::Ansi16),
            Color::LightRed
        );
        assert_eq!(
            super::render_color(Color::Rgb(0, 0, 0), ColorSupport::Ansi16),
            Color::Black
        );
        assert_eq!(
            super::render_color(Color::Rgb(255, 255, 255), ColorSupport::Ansi16),
            Color::White
        );
        // Dark grey must not collapse to black on a dark terminal.
        assert_eq!(
            super::render_color(Color::Rgb(0x44, 0x47, 0x5a), ColorSupport::Ansi16),
            Color::DarkGray
        );
        // Named colours from the ansi16 theme are already terminal indices.
        assert_eq!(
            super::render_color(Color::Cyan, ColorSupport::Ansi16),
            Color::Cyan
        );
    }

    #[test]
    fn truecolor_rendering_is_a_no_op() {
        assert_eq!(
            Theme::DRACULA.rendered(super::ColorSupport::TrueColor),
            Theme::DRACULA
        );
    }

    #[test]
    fn pss_mix_colours_are_fixed_across_themes_and_distinct() {
        use super::{
            MIX_ANON_ANSI, MIX_ANON_DARK, MIX_ANON_LIGHT, MIX_FILE_ANSI, MIX_FILE_DARK,
            MIX_FILE_LIGHT, MIX_SHMEM_ANSI, MIX_SHMEM_DARK, MIX_SHMEM_LIGHT, MIX_SWAP_ANSI,
            MIX_SWAP_DARK, MIX_SWAP_LIGHT,
        };

        let dark = [MIX_ANON_DARK, MIX_FILE_DARK, MIX_SHMEM_DARK, MIX_SWAP_DARK];
        for theme in [
            Theme::DEFAULT,
            Theme::BTOP,
            Theme::DRACULA,
            Theme::GRUVBOX,
            Theme::KANAGAWA,
        ] {
            let p = theme.palette;
            assert_eq!([p.anon, p.file, p.shmem, p.swap], dark, "{}", theme.name);
            assert_eq!(p.swap_pss, p.swap, "{}", theme.name);
        }

        let light = [
            MIX_ANON_LIGHT,
            MIX_FILE_LIGHT,
            MIX_SHMEM_LIGHT,
            MIX_SWAP_LIGHT,
        ];
        let p = Theme::LIGHT.palette;
        assert_eq!([p.anon, p.file, p.shmem, p.swap], light);

        let ansi = [MIX_ANON_ANSI, MIX_FILE_ANSI, MIX_SHMEM_ANSI, MIX_SWAP_ANSI];
        let p = Theme::ANSI16.palette;
        assert_eq!([p.anon, p.file, p.shmem, p.swap], ansi);

        for set in [dark, light, ansi] {
            let distinct: std::collections::HashSet<Color> = set.into_iter().collect();
            assert_eq!(distinct.len(), 4, "mix colours collide: {set:?}");
        }

        // Even a truecolor theme must switch to the fixed ANSI set on a
        // 16-colour terminal.
        let p = Theme::DRACULA.rendered(super::ColorSupport::Ansi16).palette;
        assert_eq!([p.anon, p.file, p.shmem, p.swap], ansi);
    }

    #[test]
    fn every_theme_has_distinct_fills_and_no_tile_fg_collision() {
        for theme in Theme::ALL {
            let palette = theme.palette;
            let distinct: std::collections::HashSet<Color> =
                palette.treemap.iter().copied().collect();
            assert_eq!(distinct.len(), 6, "{} reuses a tile fill", theme.name);
            assert!(
                !distinct.contains(&palette.tile_fg),
                "{} tile_fg is also a tile fill",
                theme.name
            );
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
