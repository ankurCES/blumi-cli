//! The blumi flower mascot: a colorful animated rose for the "thinking" state,
//! the TUI landing, and the CLI banner. Colors are the Living Rose petal ramp
//! (rose → lavender → violet → cyan → mint), swept over time like crush's
//! gradient spinner.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Petal color anchors (RGB) for the swept gradient.
const ANCHORS: [(u8, u8, u8); 5] = [
    (0xFF, 0x4F, 0x87), // rose-pink
    (0x9B, 0x86, 0xFF), // lavender
    (0x6B, 0x50, 0xFF), // violet
    (0x68, 0xFF, 0xD6), // cyan
    (0x4F, 0xE0, 0xA0), // mint
];

/// Frames per ramp segment — higher = slower, smoother color sweep.
const STEPS: usize = 6;

/// Morphing petal glyphs — the flower "blooms"/turns as the tick advances.
const PETALS: [&str; 8] = ["✿", "❀", "❁", "✾", "❃", "❀", "✿", "❋"];

/// The rose mark: rose/lavender petals around a cyan ◉ nucleus (10-petal motif).
const ROSE_ART: [&str; 5] = [
    "  ✿ ❀ ✿  ",
    " ❀ ❁ ❁ ❀ ",
    "  ❁ ◉ ❁  ",
    " ❀ ❁ ❁ ❀ ",
    "  ✿ ❀ ✿  ",
];

/// Number of rows in the rose mark (for the CLI's cursor-rewind animation).
pub const ROSE_ROWS: usize = ROSE_ART.len();

/// A swept RGB color along the petal ramp for a given tick.
fn ramp_rgb(tick: usize) -> (u8, u8, u8) {
    let total = ANCHORS.len() * STEPS;
    let idx = tick % total;
    let seg = idx / STEPS;
    let t = (idx % STEPS) as f32 / STEPS as f32;
    let a = ANCHORS[seg];
    let b = ANCHORS[(seg + 1) % ANCHORS.len()];
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    (l(a.0, b.0), l(a.1, b.1), l(a.2, b.2))
}

/// The swept color as a ratatui [`Color`].
pub fn ramp_color(tick: usize) -> Color {
    let (r, g, b) = ramp_rgb(tick);
    Color::Rgb(r, g, b)
}

/// The tiny animated mascot for the thinking/working state: a morphing,
/// color-sweeping flower followed by "thinking" and growing petals.
pub fn thinking(tick: usize) -> Vec<Span<'static>> {
    let glyph = PETALS[tick % PETALS.len()];
    let flower = ramp_color(tick);
    let label = ramp_color(tick + 4);
    let dots = match (tick / 3) % 4 {
        0 => "",
        1 => "✿",
        2 => "✿✿",
        _ => "✿✿✿",
    };
    vec![
        Span::styled(
            format!("{glyph} "),
            Style::default().fg(flower).add_modifier(Modifier::BOLD),
        ),
        Span::styled("thinking", Style::default().fg(label)),
        Span::styled(dots.to_string(), Style::default().fg(ramp_color(tick + 8))),
    ]
}

/// The animated rose mark for the TUI landing — glyphs flow through the ramp,
/// so it shimmers when the landing is redrawn each tick.
pub fn rose_logo(tick: usize) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut idx = tick;
    for row in ROSE_ART {
        let mut spans = Vec::new();
        for ch in row.chars() {
            if ch == ' ' {
                spans.push(Span::raw(" "));
            } else {
                let style = Style::default()
                    .fg(ramp_color(idx))
                    .add_modifier(Modifier::BOLD);
                spans.push(Span::styled(ch.to_string(), style));
                idx += 1;
            }
        }
        out.push(Line::from(spans));
    }
    out
}

/// One frame of the rose mark as a truecolor ANSI string (for the CLI banner,
/// which is not a ratatui surface). Ends each row with a reset + newline.
pub fn banner_frame(tick: usize) -> String {
    let mut out = String::new();
    let mut idx = tick;
    for row in ROSE_ART {
        for ch in row.chars() {
            if ch == ' ' {
                out.push(' ');
            } else {
                let (r, g, b) = ramp_rgb(idx);
                out.push_str(&format!("\x1b[1;38;2;{r};{g};{b}m{ch}"));
                idx += 1;
            }
        }
        out.push_str("\x1b[0m\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_cycles() {
        let total = ANCHORS.len() * STEPS;
        assert_eq!(ramp_color(0), ramp_color(total));
        assert!(matches!(ramp_color(3), Color::Rgb(..)));
    }

    #[test]
    fn thinking_has_flower_and_label() {
        let spans = thinking(5);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("thinking"));
        assert!(PETALS.iter().any(|g| spans[0].content.starts_with(g)));
    }

    #[test]
    fn rose_logo_has_rows_and_nucleus() {
        let lines = rose_logo(0);
        assert_eq!(lines.len(), ROSE_ROWS);
        let mid: String = lines[2].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(mid.contains('◉'));
    }

    #[test]
    fn banner_frame_is_ansi_and_multiline() {
        let f = banner_frame(0);
        assert_eq!(f.matches('\n').count(), ROSE_ROWS);
        assert!(f.contains("\x1b[1;38;2;")); // truecolor escape
        assert!(f.contains('◉'));
    }
}
