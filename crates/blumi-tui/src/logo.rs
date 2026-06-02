//! The blumi flower logo, as plain text. Renderers (the binary's ANSI banner,
//! the TUI landing screen) apply their own color/styling so there's one source
//! of truth for the art.

/// The flower glyph used as the brand icon (e.g. in the status bar / titles).
pub const PETAL: &str = "✿";

/// One-line brand mark: flower + wordmark.
pub const MARK: &str = "✿ blumi";

/// The wordmark on its own.
pub const WORDMARK: &str = "blumi";

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
}
