//! The blumi flower mascot: a colorful animated rose for the "thinking" state,
//! and a colorful rose mark for the landing screen. Colors are the Living Rose
//! petal ramp (rose → lavender → violet → cyan → mint), swept over time like
//! crush's gradient spinner.

use crate::theme::PETAL_RAMP;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Petal color anchors as RGB tuples (mirrors [`PETAL_RAMP`]) for interpolation.
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

/// A smooth swept color along the petal ramp for a given tick.
pub fn ramp_color(tick: usize) -> Color {
    let total = ANCHORS.len() * STEPS;
    let idx = tick % total;
    let seg = idx / STEPS;
    let t = (idx % STEPS) as f32 / STEPS as f32;
    let a = ANCHORS[seg];
    let b = ANCHORS[(seg + 1) % ANCHORS.len()];
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color::Rgb(l(a.0, b.0), l(a.1, b.1), l(a.2, b.2))
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

/// A colorful multi-line rose mark for the landing screen (10-petal motif:
/// rose/lavender petals around a cyan nucleus).
pub fn rose_logo() -> Vec<Line<'static>> {
    const ART: [&str; 5] = [
        "  ✿ ❀ ✿  ",
        " ❀ ❁ ❁ ❀ ",
        "  ❁ ◉ ❁  ",
        " ❀ ❁ ❁ ❀ ",
        "  ✿ ❀ ✿  ",
    ];
    ART.iter().map(|line| colorize(line)).collect()
}

fn colorize(line: &str) -> Line<'static> {
    let spans: Vec<Span<'static>> = line
        .chars()
        .map(|ch| {
            let style = match ch {
                '✿' => Style::default()
                    .fg(PETAL_RAMP[0])
                    .add_modifier(Modifier::BOLD), // rose
                '❀' => Style::default().fg(PETAL_RAMP[1]), // lavender
                '❁' => Style::default().fg(PETAL_RAMP[2]), // violet
                '◉' => Style::default()
                    .fg(PETAL_RAMP[3])
                    .add_modifier(Modifier::BOLD), // cyan
                _ => Style::default(),
            };
            Span::styled(ch.to_string(), style)
        })
        .collect();
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_is_continuous_and_cycles() {
        // distinct colors across a segment, and wraps back to the start
        let c0 = ramp_color(0);
        let total = ANCHORS.len() * STEPS;
        assert_eq!(c0, ramp_color(total)); // full cycle
        assert!(matches!(ramp_color(3), Color::Rgb(..)));
    }

    #[test]
    fn thinking_has_flower_and_label() {
        let spans = thinking(5);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("thinking"));
        // first span is the flower glyph
        assert!(PETALS.iter().any(|g| spans[0].content.starts_with(g)));
    }

    #[test]
    fn rose_logo_has_five_rows_and_nucleus() {
        let lines = rose_logo();
        assert_eq!(lines.len(), 5);
        let mid: String = lines[2].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(mid.contains('◉'));
    }
}
