//! The blumi flower logo, as plain text. Renderers (the binary's ANSI banner,
//! the TUI landing screen) apply their own color/styling so there's one source
//! of truth for the art.

/// The flower glyph used as the brand icon (e.g. in the status bar / titles).
pub const PETAL: &str = "✿";

/// One-line brand mark: flower + wordmark.
pub const MARK: &str = "✿ blumi";

/// The wordmark on its own.
pub const WORDMARK: &str = "blumi";

/// A tagline shown beneath the wordmark on the splash / banner.
pub const TAGLINE: &str = "the local-first agentic coding companion";

/// Block-letter wordmark ("BLUMI", ANSI-Shadow figlet) for the landing splash
/// and the CLI banner. Rendered with a vertical rose→cyan gradient by the
/// mascot module — the bold gradient-block style, à la hermes' logo.
pub const BLUMI_BLOCK: [&str; 6] = [
    "██████╗ ██╗     ██╗   ██╗███╗   ███╗██╗",
    "██╔══██╗██║     ██║   ██║████╗ ████║██║",
    "██████╔╝██║     ██║   ██║██╔████╔██║██║",
    "██╔══██╗██║     ██║   ██║██║╚██╔╝██║██║",
    "██████╔╝███████╗╚██████╔╝██║ ╚═╝ ██║██║",
    "╚═════╝ ╚══════╝ ╚═════╝ ╚═╝     ╚═╝╚═╝",
];

/// Visible width (columns) of every row in [`BLUMI_BLOCK`].
pub const BLUMI_BLOCK_WIDTH: u16 = 39;

/// Multi-line flower splash for the landing/onboarding screen.
///
/// A four-petal bloom around a center, with the wordmark beneath:
/// ```text
///       ✿
///     ❀ ◉ ❀
///       ✿
///    b l u m i
/// ```
pub const LOGO: &str = "      ✿
    ❀ ◉ ❀
      ✿
   b l u m i";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_has_four_lines_and_wordmark() {
        let lines: Vec<&str> = LOGO.lines().collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[3].replace(' ', "").contains("blumi"));
        assert_eq!(MARK, "✿ blumi");
    }

    #[test]
    fn block_wordmark_rows_are_uniform_width() {
        for row in BLUMI_BLOCK {
            assert_eq!(
                row.chars().count(),
                BLUMI_BLOCK_WIDTH as usize,
                "row: {row}"
            );
        }
    }
}
