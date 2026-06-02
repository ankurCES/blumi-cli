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
}

impl Default for Theme {
    fn default() -> Self {
        Theme::rose()
    }
}

/// All selectable themes, in cycle order. `rose` (the Living Rose palette,
/// from project_mythara) is the default.
pub const THEMES: [fn() -> Theme; 4] = [Theme::rose, Theme::bloom, Theme::dark, Theme::mono];

/// `0xRRGGBB` → truecolor.
const fn rgb(hex: u32) -> Color {
    Color::Rgb(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

/// Living Rose petal colors (from project_mythara's rose), used by the
/// animated flower mascot's color sweep. Anchors: rose → lavender → violet →
/// cyan → mint.
pub const PETAL_RAMP: [Color; 5] = [
    rgb(0xFF4F87), // rose-pink (brand)
    rgb(0x9B86FF), // lavender (small petals)
    rgb(0x6B50FF), // violet (big petals)
    rgb(0x68FFD6), // cyan (nucleus)
    rgb(0x4FE0A0), // mint
];

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
}

/// Icons — crush's glyph alphabet (from charmbracelet/crush styles.go), which
/// project_mythara also adopts. Dingbats render at width 1 in most terminals.
pub mod icon {
    pub const FLOWER: &str = "✿";
    pub const OK: &str = "✓"; // success
    pub const ERR: &str = "×"; // failure (crush uses ×, not ✗)
    pub const PENDING: &str = "●"; // active / running
    pub const BAR: &str = "▌"; // vertical accent (crush's active-section bar)
}
