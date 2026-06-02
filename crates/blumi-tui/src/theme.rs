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
        Theme::bloom()
    }
}

/// All selectable themes, in cycle order.
pub const THEMES: [fn() -> Theme; 3] = [Theme::bloom, Theme::dark, Theme::mono];

impl Theme {
    /// The soft "bloom" palette: pink primary, warm accent (default).
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

    pub fn primary(&self) -> Style {
        Style::default().fg(self.primary)
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

/// Icons (dingbats render at width 1 in most terminals).
pub mod icon {
    pub const FLOWER: &str = "✿";
    pub const OK: &str = "✓";
    pub const ERR: &str = "✗";
    pub const PENDING: &str = "●";
    pub const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠇"];
}
