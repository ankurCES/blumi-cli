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

/// A braille spinner frame for in-flight work (terminal-friendly).
pub fn spinner(tick: usize) -> char {
    const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    FRAMES[(tick / 2) % FRAMES.len()]
}

/// A brightness-pulsing variant of `(r,g,b)` for a "live" status dot — a
/// triangle wave so it breathes between half and full intensity.
pub fn pulse_color(r: u8, g: u8, b: u8, tick: usize) -> Color {
    const PERIOD: usize = 16;
    let phase = (tick % PERIOD) as f32 / PERIOD as f32;
    let tri = 1.0 - (phase * 2.0 - 1.0).abs(); // 0 → 1 → 0
    let scale = 0.45 + 0.55 * tri;
    Color::Rgb(
        (r as f32 * scale) as u8,
        (g as f32 * scale) as u8,
        (b as f32 * scale) as u8,
    )
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

/// Vertical-gradient base index for row `r` of `n`, spanning rose→mint without
/// wrapping (so the wordmark reads top-pink to bottom-cyan).
fn row_base(r: usize, n: usize) -> usize {
    let span = (ANCHORS.len() - 1) * STEPS; // 0..=24 : rose → mint
    if n <= 1 {
        0
    } else {
        r * span / (n - 1)
    }
}

/// The block wordmark ([`crate::logo::BLUMI_BLOCK`]) with a vertical rose→cyan
/// gradient, gently swept by `tick` so the landing shimmers.
pub fn wordmark(tick: usize) -> Vec<Line<'static>> {
    let n = crate::logo::BLUMI_BLOCK.len();
    crate::logo::BLUMI_BLOCK
        .iter()
        .enumerate()
        .map(|(r, row)| {
            Line::from(Span::styled(
                (*row).to_string(),
                Style::default()
                    .fg(ramp_color(row_base(r, n) + tick / 2))
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect()
}

/// The block wordmark as a truecolor ANSI string (for the CLI banner), with the
/// same vertical rose→cyan gradient. Ends each row with a reset + newline.
pub fn wordmark_ansi(tick: usize) -> String {
    let n = crate::logo::BLUMI_BLOCK.len();
    let mut out = String::new();
    for (r, row) in crate::logo::BLUMI_BLOCK.iter().enumerate() {
        let (rr, gg, bb) = ramp_rgb(row_base(r, n) + tick / 2);
        out.push_str(&format!("\x1b[1;38;2;{rr};{gg};{bb}m{row}\x1b[0m\n"));
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

    #[test]
    fn wordmark_renders_all_block_rows() {
        let lines = wordmark(0);
        assert_eq!(lines.len(), crate::logo::BLUMI_BLOCK.len());
        let ansi = wordmark_ansi(0);
        assert_eq!(ansi.matches('\n').count(), crate::logo::BLUMI_BLOCK.len());
        assert!(ansi.contains("\x1b[1;38;2;"));
    }

    #[test]
    fn wordmark_gradient_top_differs_from_bottom() {
        // Top row should be rosier, bottom row cyaner — distinct colors.
        let lines = wordmark(0);
        let top = lines.first().unwrap().spans[0].style.fg;
        let bottom = lines.last().unwrap().spans[0].style.fg;
        assert_ne!(top, bottom);
    }
}
