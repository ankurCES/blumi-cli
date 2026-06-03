//! The blumi flower mascot: a colorful animated rose for the "thinking" state,
//! the TUI landing, and the CLI banner. Colors are the Living Rose petal ramp
//! (rose → lavender → violet → cyan → mint), swept over time like crush's
//! gradient spinner.

use ratatui::layout::Alignment;
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

/// The cyan nucleus at the flower's center (matches the logo PNG's center ring).
const NUCLEUS: Color = Color::Rgb(0x68, 0xFF, 0xD6);

/// Morphing petal glyphs — the flower "blooms"/turns as the tick advances.
const PETALS: [&str; 8] = ["✿", "❀", "❁", "✾", "❃", "❀", "✿", "❋"];

/// The flower mark, mirroring the logo PNG: eight gradient petals (four cardinal
/// + four diagonal) radiating from a cyan `◉` nucleus.
const ROSE_ART: [&str; 5] = [
    "  ✿   ✿  ",
    "    ✿    ",
    " ✿  ◉  ✿ ",
    "    ✿    ",
    "  ✿   ✿  ",
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

/// The pinned right-pane **app logo**: a small rasterized flower bloom above the
/// compact BLUMI gradient block-wordmark, both centered in `inner_w` — the same
/// rasterized flower + BLUMI font as the startup splash, sized down to sit at the
/// top of the dashboard pane like an app icon. `flower_rows` is the bloom height
/// (0 / 1 → wordmark only). Falls back to a single-line "blumi" when the pane is
/// too narrow for the small block font.
pub fn app_logo(tick: usize, flower_rows: usize, inner_w: usize) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    if flower_rows >= 2 {
        out.extend(
            flower_raster(flower_rows)
                .into_iter()
                .map(|l| l.alignment(Alignment::Center)),
        );
    }
    if inner_w >= crate::logo::BLUMI_BLOCK_SMALL_WIDTH as usize {
        out.extend(
            wordmark_small(tick)
                .into_iter()
                .map(|l| l.alignment(Alignment::Center)),
        );
    } else {
        out.push(
            Line::from(Span::styled(
                "blumi".to_string(),
                Style::default()
                    .fg(ramp_color(tick))
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
        );
    }
    out
}

/// The compact block wordmark ([`crate::logo::BLUMI_BLOCK_SMALL`]) with the same
/// vertical rose→cyan gradient as [`wordmark`], for the pinned right-pane logo.
pub fn wordmark_small(tick: usize) -> Vec<Line<'static>> {
    let n = crate::logo::BLUMI_BLOCK_SMALL.len();
    crate::logo::BLUMI_BLOCK_SMALL
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

// ── Pixel-perfect flower (half-block raster of the logo PNG) ───────────────

/// Linear-interpolate two RGB colors.
fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), f: f32) -> (u8, u8, u8) {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f).round() as u8;
    (l(a.0, b.0), l(a.1, b.1), l(a.2, b.2))
}

/// The petal gradient sampled across the flower's bounding box — top-left rose →
/// bottom-right cyan, the diagonal sweep of the logo PNG.
fn petal_color(x: f32, y: f32) -> (u8, u8, u8) {
    const STOPS: [(f32, (u8, u8, u8)); 4] = [
        (0.0, (0xFF, 0x4F, 0x87)),  // rose
        (0.45, (0x9B, 0x86, 0xFF)), // lavender
        (0.75, (0x6B, 0x50, 0xFF)), // violet
        (1.0, (0x68, 0xFF, 0xD6)),  // cyan
    ];
    let t = ((((x + 69.0) / 138.0) + ((y + 69.0) / 138.0)) * 0.5).clamp(0.0, 1.0);
    for w in STOPS.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
            return lerp_rgb(c0, c1, f);
        }
    }
    STOPS[3].1
}

/// The color of the flower at local point (x, y) — flower-space units, centered
/// at the origin, bbox ≈ [-69, 69]². `None` is transparent (background). Mirrors
/// the logo SVG: a dark center dot, a cyan nucleus ring, then eight gradient
/// petals (four cardinal + four diagonal).
fn flower_pixel(x: f32, y: f32) -> Option<(u8, u8, u8)> {
    let dist2 = x * x + y * y;
    if dist2 <= 7.5 * 7.5 {
        return Some((0x0E, 0x11, 0x16)); // center dot
    }
    if dist2 <= 17.0 * 17.0 {
        return Some((0x68, 0xFF, 0xD6)); // cyan nucleus
    }
    // (cx, cy, rx, ry, rotation°)
    const PETALS: [(f32, f32, f32, f32, f32); 8] = [
        (0.0, -36.0, 19.0, 33.0, 0.0),
        (0.0, 36.0, 19.0, 33.0, 0.0),
        (-36.0, 0.0, 33.0, 19.0, 0.0),
        (36.0, 0.0, 33.0, 19.0, 0.0),
        (-26.0, -26.0, 28.0, 15.0, 45.0),
        (26.0, 26.0, 28.0, 15.0, 45.0),
        (26.0, -26.0, 28.0, 15.0, -45.0),
        (-26.0, 26.0, 28.0, 15.0, -45.0),
    ];
    for (cx, cy, rx, ry, deg) in PETALS {
        let (dx, dy) = (x - cx, y - cy);
        let (s, c) = (-deg).to_radians().sin_cos();
        let (xr, yr) = (dx * c - dy * s, dx * s + dy * c);
        if (xr / rx).powi(2) + (yr / ry).powi(2) <= 1.0 {
            return Some(petal_color(x, y));
        }
    }
    None
}

/// Render the flower as a `rows`-tall half-block raster: each cell packs two
/// vertical pixels (`▀` = top fg / bottom bg), giving a smooth, full-color image
/// of the logo PNG in the terminal. Cell aspect ~1:2 makes the doubled-height
/// pixel grid square, so the bloom stays round.
pub fn flower_raster(rows: usize) -> Vec<Line<'static>> {
    let cols = rows * 2; // square pixel grid (half-blocks double the height)
    let (pw, ph) = (cols as f32, (rows * 2) as f32);
    let (cx, cy) = (pw / 2.0, ph / 2.0);
    let scale = (pw / 2.0) / 74.0; // flower radius ≈ 69 + a little margin
    let sample = |px: usize, py: usize| -> Option<Color> {
        let x = (px as f32 + 0.5 - cx) / scale;
        let y = (py as f32 + 0.5 - cy) / scale;
        flower_pixel(x, y).map(|(r, g, b)| Color::Rgb(r, g, b))
    };
    (0..rows)
        .map(|row| {
            let spans = (0..cols)
                .map(
                    |col| match (sample(col, row * 2), sample(col, row * 2 + 1)) {
                        (None, None) => Span::raw(" "),
                        (Some(t), None) => Span::styled("▀".to_string(), Style::default().fg(t)),
                        (None, Some(b)) => Span::styled("▄".to_string(), Style::default().fg(b)),
                        (Some(t), Some(b)) => {
                            Span::styled("▀".to_string(), Style::default().fg(t).bg(b))
                        }
                    },
                )
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

/// The same pixel-perfect flower as [`flower_raster`], but as a truecolor ANSI
/// string for non-ratatui surfaces (the CLI startup splash / banners). Each row
/// ends with a reset + newline.
pub fn flower_raster_ansi(rows: usize) -> String {
    let cols = rows * 2;
    let (pw, ph) = (cols as f32, (rows * 2) as f32);
    let (cx, cy) = (pw / 2.0, ph / 2.0);
    let scale = (pw / 2.0) / 74.0;
    let px = |c: usize, r: usize| {
        let x = (c as f32 + 0.5 - cx) / scale;
        let y = (r as f32 + 0.5 - cy) / scale;
        flower_pixel(x, y)
    };
    let mut out = String::new();
    for row in 0..rows {
        for col in 0..cols {
            match (px(col, row * 2), px(col, row * 2 + 1)) {
                (None, None) => out.push(' '),
                (Some((r, g, b)), None) => {
                    out.push_str(&format!("\x1b[38;2;{r};{g};{b}m\x1b[49m▀"))
                }
                (None, Some((r, g, b))) => {
                    out.push_str(&format!("\x1b[38;2;{r};{g};{b}m\x1b[49m▄"))
                }
                (Some((tr, tg, tb)), Some((br, bg, bb))) => {
                    out.push_str(&format!("\x1b[38;2;{tr};{tg};{tb};48;2;{br};{bg};{bb}m▀"))
                }
            }
        }
        out.push_str("\x1b[0m\n");
    }
    out
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
            } else if ch == '◉' {
                // The nucleus is always cyan (the PNG's center ring).
                spans.push(Span::styled(
                    "◉".to_string(),
                    Style::default().fg(NUCLEUS).add_modifier(Modifier::BOLD),
                ));
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
            } else if ch == '◉' {
                out.push_str("\x1b[1;38;2;104;255;214m◉"); // cyan nucleus
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
    fn app_logo_has_flower_and_small_wordmark() {
        // Roomy pane: flower bloom (5 rows) + the 4-row small block wordmark.
        let lines = app_logo(0, 5, 24);
        assert_eq!(lines.len(), 9, "5 flower rows + 4 wordmark rows");
        let all: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(all.contains('▀') || all.contains('▄'), "rasterized flower");
        // The small wordmark uses the block figlet, gradient-colored.
        assert!(
            lines.iter().all(|l| l.alignment == Some(Alignment::Center)),
            "logo is centered"
        );

        // Narrow pane: falls back to a single-line "blumi" wordmark.
        let narrow = app_logo(0, 0, 8);
        let txt: String = narrow
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(txt.contains("blumi"), "narrow fallback wordmark");
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
        assert!(ansi.contains("\x1b[1;38;2;")); // truecolor escape
    }

    #[test]
    fn flower_raster_paints_png_regions() {
        let lines = flower_raster(11);
        assert_eq!(lines.len(), 11);
        let (mut cyan, mut dark, mut blocks) = (false, false, false);
        for l in &lines {
            for s in &l.spans {
                if s.content == "▀" || s.content == "▄" {
                    blocks = true;
                }
                for col in [s.style.fg, s.style.bg].into_iter().flatten() {
                    if col == Color::Rgb(0x68, 0xFF, 0xD6) {
                        cyan = true;
                    }
                    if col == Color::Rgb(0x0E, 0x11, 0x16) {
                        dark = true;
                    }
                }
            }
        }
        assert!(blocks, "rendered with half-block pixels");
        assert!(cyan, "cyan nucleus present");
        assert!(dark, "dark center dot present");
    }

    #[test]
    fn rose_logo_nucleus_is_cyan() {
        // The center ◉ is rendered in the fixed cyan nucleus color, not swept.
        let lines = rose_logo(0);
        let nucleus = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content == "◉")
            .expect("a nucleus glyph");
        assert_eq!(nucleus.style.fg, Some(NUCLEUS));
    }
}
