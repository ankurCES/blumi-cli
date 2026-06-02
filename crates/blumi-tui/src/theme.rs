//! Semantic colors and icons for the TUI.

use ratatui::style::{Color, Modifier, Style};

pub struct Theme {
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
        // A soft "bloom" palette: pink primary, warm accent.
        Theme {
            primary: Color::Indexed(213), // pink
            accent: Color::Indexed(221),  // warm yellow
            fg: Color::Indexed(252),
            fg_subtle: Color::Indexed(245),
            fg_dim: Color::Indexed(240),
            success: Color::Indexed(114),
            error: Color::Indexed(203),
        }
    }
}

impl Theme {
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
