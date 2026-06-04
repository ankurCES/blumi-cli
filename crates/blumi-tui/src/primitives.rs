//! Shared TUI design-system primitives — the single visual vocabulary the view
//! builds on (panels, centered overlays, text fitting). Grown over the overhaul
//! so render code composes these instead of re-deriving styling ad hoc.

use crate::theme::Theme;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};

/// A rounded, titled pane block (posting-style): a quiet border when idle; a
/// bright border + `▍` title accent + raised surface fill when focused.
pub(crate) fn panel(title: &str, focused: bool, theme: &Theme) -> Block<'static> {
    let title = if focused {
        Line::from(vec![
            Span::styled("▍", theme.accent()),
            Span::styled(format!("{title} "), theme.panel_focus()),
        ])
    } else {
        Line::from(Span::styled(format!(" {title} "), theme.subtle()))
    };
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            theme.panel_focus()
        } else {
            theme.border()
        })
        .title(title);
    if focused {
        block = block.style(theme.surface());
    }
    block
}

/// A centered rect sized as a percentage of `area` (for modal overlays).
pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

/// Truncate to `max` chars with a leading ellipsis (keeps the tail, e.g. paths).
pub(crate) fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let tail: String = s
            .chars()
            .rev()
            .take(max.saturating_sub(1))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("…{tail}")
    }
}

/// Truncate to `max` chars with a trailing ellipsis.
pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}
