//! Semantic colors and icons for the TUI.

use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    // Brand + text.
    pub primary: Color,
    pub accent: Color,
    pub fg: Color,
    pub fg_subtle: Color,
    pub fg_dim: Color,
    pub success: Color,
    pub error: Color,
    /// A slightly-raised panel/chip fill (focused panels, footer chips). We never
    /// paint a global background; only panels/chips opt into this.
    pub surface: Color,
    /// Idle panel border (quieter than `fg_dim`).
    pub border: Color,
    // Surface ramp (additive; only `surface` is used as a global-safe fill).
    pub bg: Color,
    pub surface_alt: Color,
    pub selection: Color,
    pub selection_fg: Color,
    // Borders / titles.
    pub border_active: Color,
    pub title: Color,
    pub title_active: Color,
    // Status bar + chips.
    pub statusbar_bg: Color,
    pub statusbar_fg: Color,
    pub chip_key_fg: Color,
    pub chip_label_fg: Color,
    // Badges / alerts.
    pub warn: Color,
    pub warn_fg: Color,
    pub info: Color,
    pub overdue: Color,
    // Diff.
    pub diff_add: Color,
    pub diff_del: Color,
    pub diff_hunk: Color,
    // Gauges / meters (green → amber → orange → red).
    pub gauge_low: Color,
    pub gauge_mid: Color,
    pub gauge_high: Color,
    pub gauge_crit: Color,
    // Syntax accents (for code/markdown highlighting bridges).
    pub syntax_kw: Color,
    pub syntax_str: Color,
    pub syntax_fn: Color,
    pub syntax_num: Color,
    pub syntax_comment: Color,
}

/// The hand-authored core of a theme. Everything else in [`Theme`] is derived
/// from these via [`Theme::from_core`], so a palette stays compact to write and
/// user TOML themes only need to specify the core colors.
struct Core {
    primary: Color,
    accent: Color,
    fg: Color,
    fg_subtle: Color,
    fg_dim: Color,
    success: Color,
    error: Color,
    warn: Color,
    surface: Color,
    surface_alt: Color,
    border: Color,
    bg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::rose()
    }
}

/// All built-in themes, in cycle order. `rose` (the Living Rose palette, from
/// project_mythara) is the default; `spatial`/`aurora` are mythara's Charmtone
/// skins; the rest are popular community palettes.
pub const THEMES: [fn() -> Theme; 11] = [
    Theme::rose,
    Theme::spatial,
    Theme::aurora,
    Theme::bloom,
    Theme::dark,
    Theme::mono,
    Theme::catppuccin,
    Theme::nord,
    Theme::dracula,
    Theme::tokyo_night,
    Theme::gruvbox,
];

/// `0xRRGGBB` → truecolor.
const fn rgb(hex: u32) -> Color {
    Color::Rgb(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

impl Theme {
    /// Build a full theme from its hand-authored [`Core`], deriving the rest.
    /// Derivations are chosen so existing render paths look identical to before
    /// (e.g. `border_active = primary`, `chip_key_fg = accent`); amber/orange
    /// gauge steps are fixed hues so meters read consistently across palettes.
    fn from_core(name: &'static str, c: Core) -> Self {
        Theme {
            name,
            primary: c.primary,
            accent: c.accent,
            fg: c.fg,
            fg_subtle: c.fg_subtle,
            fg_dim: c.fg_dim,
            success: c.success,
            error: c.error,
            surface: c.surface,
            border: c.border,
            bg: c.bg,
            surface_alt: c.surface_alt,
            selection: c.surface_alt,
            selection_fg: c.fg,
            border_active: c.primary,
            title: c.fg_subtle,
            title_active: c.primary,
            statusbar_bg: c.surface,
            statusbar_fg: c.fg_subtle,
            chip_key_fg: c.accent,
            chip_label_fg: c.fg_subtle,
            warn: c.warn,
            warn_fg: c.bg,
            info: c.accent,
            overdue: c.error,
            diff_add: c.success,
            diff_del: c.error,
            diff_hunk: c.accent,
            gauge_low: c.success,
            gauge_mid: rgb(0xE6C341),  // amber
            gauge_high: rgb(0xE08A3C), // orange
            gauge_crit: c.error,
            syntax_kw: c.primary,
            syntax_str: c.success,
            syntax_fn: c.accent,
            syntax_num: c.warn,
            syntax_comment: c.fg_dim,
        }
    }

    /// Living Rose — rose-pink brand on a warm palette with a cyan accent,
    /// ported from project_mythara. The colorful default.
    pub fn rose() -> Self {
        Theme::from_core(
            "rose",
            Core {
                primary: rgb(0xFF4F87), // rose-pink (Charple)
                accent: rgb(0x68FFD6),  // cyan nucleus (Bok)
                fg: rgb(0xF6E6EC),      // rosy near-white
                fg_subtle: rgb(0xCBA7B4),
                fg_dim: rgb(0x8C6571),
                success: rgb(0x4FE0A0), // Julep
                error: rgb(0xFF5470),   // Sriracha
                warn: rgb(0xFFC04F),
                surface: rgb(0x2A1722), // raised rose panel
                surface_alt: rgb(0x3A2230),
                border: rgb(0x5A3A47),
                bg: rgb(0x16090E),
            },
        )
    }

    /// Spatial — the original Charmtone Pantera (violet brand + cyan).
    pub fn spatial() -> Self {
        Theme::from_core(
            "spatial",
            Core {
                primary: rgb(0x6B50FF), // Charple violet
                accent: rgb(0x68FFD6),  // Bok cyan
                fg: rgb(0xDFDBDD),
                fg_subtle: rgb(0xA8A4AB),
                fg_dim: rgb(0x605F6B),
                success: rgb(0x00FFB2), // Julep
                error: rgb(0xEB4268),   // Sriracha
                warn: rgb(0xF5A623),
                surface: rgb(0x1B1630),
                surface_alt: rgb(0x252041),
                border: rgb(0x39354F),
                bg: rgb(0x0D0B14),
            },
        )
    }

    /// Aurora — a deep, brighter-violet variant that glows.
    pub fn aurora() -> Self {
        Theme::from_core(
            "aurora",
            Core {
                primary: rgb(0x8B6BFF), // brighter violet
                accent: rgb(0x68FFD6),  // cyan
                fg: rgb(0xEDE8F7),
                fg_subtle: rgb(0xB3A8C8),
                fg_dim: rgb(0x6E6488),
                success: rgb(0x00FFB2),
                error: rgb(0xEB4268),
                warn: rgb(0xF5A623),
                surface: rgb(0x201A38),
                surface_alt: rgb(0x2B2350),
                border: rgb(0x3E3560),
                bg: rgb(0x100B1C),
            },
        )
    }

    /// The soft "bloom" palette: pink primary, warm accent (256-color).
    pub fn bloom() -> Self {
        Theme::from_core(
            "bloom",
            Core {
                primary: Color::Indexed(213), // pink
                accent: Color::Indexed(221),  // warm yellow
                fg: Color::Indexed(252),
                fg_subtle: Color::Indexed(245),
                fg_dim: Color::Indexed(240),
                success: Color::Indexed(114),
                error: Color::Indexed(203),
                warn: Color::Indexed(214),
                surface: Color::Indexed(235),
                surface_alt: Color::Indexed(237),
                border: Color::Indexed(239),
                bg: Color::Indexed(233),
            },
        )
    }

    /// A cool blue/teal dark theme (256-color).
    pub fn dark() -> Self {
        Theme::from_core(
            "dark",
            Core {
                primary: Color::Indexed(75), // sky blue
                accent: Color::Indexed(80),  // teal
                fg: Color::Indexed(252),
                fg_subtle: Color::Indexed(245),
                fg_dim: Color::Indexed(239),
                success: Color::Indexed(114),
                error: Color::Indexed(203),
                warn: Color::Indexed(214),
                surface: Color::Indexed(235),
                surface_alt: Color::Indexed(237),
                border: Color::Indexed(238),
                bg: Color::Indexed(233),
            },
        )
    }

    /// A restrained monochrome theme (256-color).
    pub fn mono() -> Self {
        Theme::from_core(
            "mono",
            Core {
                primary: Color::Indexed(254),
                accent: Color::Indexed(250),
                fg: Color::Indexed(250),
                fg_subtle: Color::Indexed(244),
                fg_dim: Color::Indexed(239),
                success: Color::Indexed(246),
                error: Color::Indexed(210),
                warn: Color::Indexed(248),
                surface: Color::Indexed(236),
                surface_alt: Color::Indexed(238),
                border: Color::Indexed(240),
                bg: Color::Indexed(233),
            },
        )
    }

    /// Catppuccin Mocha.
    pub fn catppuccin() -> Self {
        Theme::from_core(
            "catppuccin",
            Core {
                primary: rgb(0xCBA6F7),     // mauve
                accent: rgb(0x94E2D5),      // teal
                fg: rgb(0xCDD6F4),          // text
                fg_subtle: rgb(0xBAC2DE),   // subtext1
                fg_dim: rgb(0x6C7086),      // overlay0
                success: rgb(0xA6E3A1),     // green
                error: rgb(0xF38BA8),       // red
                warn: rgb(0xF9E2AF),        // yellow
                surface: rgb(0x1E1E2E),     // base
                surface_alt: rgb(0x313244), // surface0
                border: rgb(0x45475A),      // surface1
                bg: rgb(0x181825),          // mantle
            },
        )
    }

    /// Nord.
    pub fn nord() -> Self {
        Theme::from_core(
            "nord",
            Core {
                primary: rgb(0x88C0D0), // frost
                accent: rgb(0x8FBCBB),
                fg: rgb(0xECEFF4),
                fg_subtle: rgb(0xD8DEE9),
                fg_dim: rgb(0x4C566A),
                success: rgb(0xA3BE8C),
                error: rgb(0xBF616A),
                warn: rgb(0xEBCB8B),
                surface: rgb(0x2E3440),
                surface_alt: rgb(0x3B4252),
                border: rgb(0x434C5E),
                bg: rgb(0x242933),
            },
        )
    }

    /// Dracula.
    pub fn dracula() -> Self {
        Theme::from_core(
            "dracula",
            Core {
                primary: rgb(0xBD93F9), // purple
                accent: rgb(0x8BE9FD),  // cyan
                fg: rgb(0xF8F8F2),
                fg_subtle: rgb(0xC8C8D0),
                fg_dim: rgb(0x6272A4), // comment
                success: rgb(0x50FA7B),
                error: rgb(0xFF5555),
                warn: rgb(0xF1FA8C),
                surface: rgb(0x282A36),
                surface_alt: rgb(0x343746),
                border: rgb(0x44475A),
                bg: rgb(0x21222C),
            },
        )
    }

    /// Tokyo Night.
    pub fn tokyo_night() -> Self {
        Theme::from_core(
            "tokyo-night",
            Core {
                primary: rgb(0x7AA2F7), // blue
                accent: rgb(0x7DCFFF),  // cyan
                fg: rgb(0xC0CAF5),
                fg_subtle: rgb(0xA9B1D6),
                fg_dim: rgb(0x565F89),
                success: rgb(0x9ECE6A),
                error: rgb(0xF7768E),
                warn: rgb(0xE0AF68),
                surface: rgb(0x1A1B26),
                surface_alt: rgb(0x24283B),
                border: rgb(0x33467C),
                bg: rgb(0x16161E),
            },
        )
    }

    /// Gruvbox (dark).
    pub fn gruvbox() -> Self {
        Theme::from_core(
            "gruvbox",
            Core {
                primary: rgb(0xFE8019), // orange
                accent: rgb(0x8EC07C),  // aqua
                fg: rgb(0xEBDBB2),
                fg_subtle: rgb(0xD5C4A1),
                fg_dim: rgb(0x928374),
                success: rgb(0xB8BB26),
                error: rgb(0xFB4934),
                warn: rgb(0xFABD2F),
                surface: rgb(0x3C3836),
                surface_alt: rgb(0x504945),
                border: rgb(0x665C54),
                bg: rgb(0x282828),
            },
        )
    }

    pub fn accent(&self) -> Style {
        Style::default().fg(self.accent)
    }
    pub fn dim(&self) -> Style {
        Style::default().fg(self.fg_dim)
    }
    pub fn subtle(&self) -> Style {
        Style::default().fg(self.fg_subtle)
    }
    pub fn body(&self) -> Style {
        Style::default().fg(self.fg)
    }
    pub fn bold_primary(&self) -> Style {
        Style::default()
            .fg(self.primary)
            .add_modifier(Modifier::BOLD)
    }

    /// A raised panel/chip fill. Honors the `FILL_PANELS` kill switch so
    /// transparency-loving terminals can opt out (env `BLUMI_NO_FILL`).
    pub fn surface(&self) -> Style {
        if FILL_PANELS.load(std::sync::atomic::Ordering::Relaxed) {
            Style::default().bg(self.surface)
        } else {
            Style::default()
        }
    }
    /// Style for an idle (unfocused) panel border + its title.
    pub fn border(&self) -> Style {
        Style::default().fg(self.border)
    }
    /// Style for a focused panel border + its title accent.
    pub fn panel_focus(&self) -> Style {
        Style::default()
            .fg(self.primary)
            .add_modifier(Modifier::BOLD)
    }
    /// A footer key-chip: the keycap half (bright accent on surface).
    pub fn chip_key(&self) -> Style {
        self.surface().fg(self.accent).add_modifier(Modifier::BOLD)
    }
    /// A footer key-chip: the label half (subtle on surface).
    pub fn chip_label(&self) -> Style {
        self.surface().fg(self.fg_subtle)
    }

    /// A selected-row style — surface fill + selection fg when fills are on,
    /// else a reversed primary (so it stays visible on transparent terminals).
    pub fn selection(&self) -> Style {
        if FILL_PANELS.load(std::sync::atomic::Ordering::Relaxed) {
            Style::default().bg(self.selection).fg(self.selection_fg)
        } else {
            Style::default()
                .fg(self.primary)
                .add_modifier(Modifier::REVERSED)
        }
    }
    /// A warning/alert badge: dark text on the warn (amber) ground.
    pub fn warn_badge(&self) -> Style {
        Style::default()
            .fg(self.warn_fg)
            .bg(self.warn)
            .add_modifier(Modifier::BOLD)
    }
    /// The meter/gauge color for a fill fraction (green → amber → orange → red).
    pub fn gauge(&self, frac: f64) -> Color {
        let pct = frac * 100.0;
        if pct >= 95.0 {
            self.gauge_crit
        } else if pct > 80.0 {
            self.gauge_high
        } else if pct >= 50.0 {
            self.gauge_mid
        } else {
            self.gauge_low
        }
    }
    pub fn diff_add_style(&self) -> Style {
        Style::default().fg(self.diff_add)
    }
    pub fn diff_del_style(&self) -> Style {
        Style::default().fg(self.diff_del)
    }
    pub fn diff_hunk_style(&self) -> Style {
        Style::default().fg(self.diff_hunk)
    }
}

/// A registry of selectable themes — the built-ins plus any user themes loaded
/// from `~/.blumi/themes/*.toml`. Theme selection (cycle / by-name / by-index)
/// routes through here so user themes participate alongside the built-ins. Held
/// in the `Model`; built once at startup.
#[derive(Clone)]
pub struct ThemeRegistry {
    themes: Vec<Theme>,
}

impl Default for ThemeRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl ThemeRegistry {
    /// Just the built-in palettes, in cycle order (`rose` first).
    pub fn builtin() -> Self {
        Self {
            themes: THEMES.iter().map(|f| f()).collect(),
        }
    }

    /// The theme at `idx` (wrapping); falls back to the default if empty.
    pub fn get(&self, idx: usize) -> Theme {
        if self.themes.is_empty() {
            Theme::default()
        } else {
            self.themes[idx % self.themes.len()]
        }
    }
    /// Index of a theme by name (case-insensitive).
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.themes
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(name))
    }
    /// The next index after `cur`, wrapping (for `/theme` cycling).
    pub fn next_index(&self, cur: usize) -> usize {
        if self.themes.is_empty() {
            0
        } else {
            (cur + 1) % self.themes.len()
        }
    }
}

/// Global kill-switch for panel/chip background fills. Defaults on; set the env
/// var `BLUMI_NO_FILL` (any value) to render borderless/transparent instead —
/// helps terminals that ignore or mangle background colors.
pub static FILL_PANELS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Initialize [`FILL_PANELS`] from the environment. Call once at startup.
pub fn init_fill_from_env() {
    if std::env::var_os("BLUMI_NO_FILL").is_some() {
        FILL_PANELS.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Icons — crush's glyph alphabet (from charmbracelet/crush styles.go), which
/// project_mythara also adopts. Dingbats render at width 1 in most terminals.
pub mod icon {
    pub const FLOWER: &str = "✿"; // the brand mark / agent
    pub const USER: &str = "›"; // user prompt marker
    pub const TOOL: &str = "▸"; // a tool call
    pub const OK: &str = "✓"; // success
    pub const ERR: &str = "×"; // failure (crush uses ×, not ✗)
    pub const DOT: &str = "●"; // status / live dot
                               // Rounded box-drawing for cards (widely supported in modern terminals).
    pub const TL: &str = "╭";
    pub const BL: &str = "╰";
    pub const H: &str = "─";
    pub const V: &str = "│";
    // Progress-bar cells.
    pub const BAR_FULL: &str = "█";
    pub const BAR_EMPTY: &str = "░";
}
