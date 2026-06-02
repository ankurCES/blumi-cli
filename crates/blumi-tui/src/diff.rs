//! Unified-diff rendering: +/- gutter coloring and hunk headers.

use crate::theme::Theme;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// Render a unified diff into colored lines, capped at `max_lines`
/// (a truncation note is appended if it overflows).
pub fn render_unified(diff: &str, theme: &Theme, max_lines: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let total = diff.lines().count();
    for raw in diff.lines().take(max_lines) {
        let style = match raw.as_bytes().first() {
            Some(b'+') if !raw.starts_with("+++") => Style::default().fg(theme.success),
            Some(b'-') if !raw.starts_with("---") => Style::default().fg(theme.error),
            Some(b'@') => theme.accent(),
            Some(b'+') | Some(b'-') => theme.subtle(), // file headers
            _ => theme.dim(),
        };
        out.push(Line::from(Span::styled(raw.to_string(), style)));
    }
    if total > max_lines {
        out.push(Line::from(Span::styled(
            format!("  … {} more lines", total - max_lines),
            theme.dim(),
        )));
    }
    if out.is_empty() {
        out.push(Line::raw(""));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_add_and_remove() {
        let theme = Theme::default();
        let diff = "@@ -1 +1 @@\n-old line\n+new line\n context";
        let lines = render_unified(diff, &theme, 100);
        assert_eq!(lines.len(), 4);
        // added line uses success color
        assert_eq!(lines[2].spans[0].style.fg, Some(theme.success));
        assert_eq!(lines[1].spans[0].style.fg, Some(theme.error));
    }

    #[test]
    fn truncates() {
        let theme = Theme::default();
        let diff = (0..50)
            .map(|i| format!("+line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = render_unified(&diff, &theme, 10);
        assert_eq!(lines.len(), 11); // 10 + truncation note
        assert!(lines.last().unwrap().spans[0]
            .content
            .contains("more lines"));
    }
}
