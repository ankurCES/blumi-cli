//! Semantic colors and icons for the TUI.

use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
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
}

impl Default for Theme {
    fn default() -> Self {
        Theme::rose()
    }
}

/// All selectable themes, in cycle order. `rose` (the Living Rose palette,
/// from project_mythara) is the default; `spatial`/`aurora` are mythara's
/// Charmtone-derived skins.
pub const THEMES: [fn() -> Theme; 6] = [
    Theme::rose,
    Theme::spatial,
    Theme::aurora,
    Theme::bloom,
    Theme::dark,
    Theme::mono,
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
    /// Living Rose — rose-pink brand on a warm palette with a cyan accent,
    /// ported from project_mythara. The colorful default.
    pub fn rose() -> Self {
        Theme {
            name: "rose",
            primary: rgb(0xFF4F87), // rose-pink (Charple)
            accent: rgb(0x68FFD6),  // cyan nucleus (Bok)
            fg: rgb(0xF6E6EC),      // rosy near-white
            fg_subtle: rgb(0xCBA7B4),
            fg_dim: rgb(0x8C6571),
            success: rgb(0x4FE0A0), // Julep
            error: rgb(0xFF5470),   // Sriracha
            surface: rgb(0x2A1722), // raised rose panel
            border: rgb(0x5A3A47),
        }
    }

    /// Spatial — the original Charmtone Pantera (violet brand + cyan), the
    /// look closest to crush's own palette.
    pub fn spatial() -> Self {
        Theme {
            name: "spatial",
            primary: rgb(0x6B50FF), // Charple violet
            accent: rgb(0x68FFD6),  // Bok cyan
            fg: rgb(0xDFDBDD),
            fg_subtle: rgb(0xA8A4AB),
            fg_dim: rgb(0x605F6B),
            success: rgb(0x00FFB2), // Julep
            error: rgb(0xEB4268),   // Sriracha
            surface: rgb(0x1B1630),
            border: rgb(0x39354F),
        }
    }

    /// Aurora — a deep, brighter-violet variant that glows.
    pub fn aurora() -> Self {
        Theme {
            name: "aurora",
            primary: rgb(0x8B6BFF), // brighter violet
            accent: rgb(0x68FFD6),  // cyan
            fg: rgb(0xEDE8F7),
            fg_subtle: rgb(0xB3A8C8),
            fg_dim: rgb(0x6E6488),
            success: rgb(0x00FFB2),
            error: rgb(0xEB4268),
            surface: rgb(0x201A38),
            border: rgb(0x3E3560),
        }
    }

    /// The soft "bloom" palette: pink primary, warm accent.
    pub fn bloom() -> Self {
        Theme {
            name: "bloom",
            primary: Color::Indexed(213), // pink
            accent: Color::Indexed(221),  // warm yellow
            fg: Color::Indexed(252),
            fg_subtle: Color::Indexed(245),
            fg_dim: Color::Indexed(240),
            success: Color::Indexed(114),
            error: Color::Indexed(203),
            surface: Color::Indexed(235),
            border: Color::Indexed(239),
        }
    }

    /// A cool blue/teal dark theme.
    pub fn dark() -> Self {
        Theme {
            name: "dark",
            primary: Color::Indexed(75), // sky blue
            accent: Color::Indexed(80),  // teal
            fg: Color::Indexed(252),
            fg_subtle: Color::Indexed(245),
            fg_dim: Color::Indexed(239),
            success: Color::Indexed(114),
            error: Color::Indexed(203),
            surface: Color::Indexed(235),
            border: Color::Indexed(238),
        }
    }

    /// A restrained monochrome theme.
    pub fn mono() -> Self {
        Theme {
            name: "mono",
            primary: Color::Indexed(254),
            accent: Color::Indexed(250),
            fg: Color::Indexed(250),
            fg_subtle: Color::Indexed(244),
            fg_dim: Color::Indexed(239),
            success: Color::Indexed(246),
            error: Color::Indexed(210),
            surface: Color::Indexed(236),
            border: Color::Indexed(240),
        }
    }

    pub fn by_index(i: usize) -> Self {
        THEMES[i % THEMES.len()]()
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
