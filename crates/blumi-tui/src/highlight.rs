//! Syntax highlighting for fenced code blocks and diffs, via syntect.
//! A single lazily-initialised highlighter with a per-code-block cache keeps
//! re-renders cheap (markdown prose is re-parsed each frame; code is not).

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style as SynStyle, Theme as SynTheme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

pub struct Highlighter {
    syntaxes: SyntaxSet,
    theme: SynTheme,
    cache: Mutex<HashMap<u64, Vec<Line<'static>>>>,
}

impl Highlighter {
    fn new() -> Self {
        let syntaxes = SyntaxSet::load_defaults_newlines();
        let ts = ThemeSet::load_defaults();
        let theme = ts
            .themes
            .get("base16-ocean.dark")
            .or_else(|| ts.themes.values().next())
            .cloned()
            .expect("at least one default theme");
        Highlighter {
            syntaxes,
            theme,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Highlight a code block into styled lines (cached by content + language).
    pub fn highlight_block(&self, code: &str, lang: Option<&str>) -> Vec<Line<'static>> {
        let key = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            code.hash(&mut h);
            lang.unwrap_or("").hash(&mut h);
            h.finish()
        };
        if let Some(cached) = self.cache.lock().unwrap().get(&key) {
            return cached.clone();
        }

        let syntax = lang
            .and_then(|l| self.syntaxes.find_syntax_by_token(l))
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());
        let mut hl = HighlightLines::new(syntax, &self.theme);

        let mut out = Vec::new();
        for line in LinesWithEndings::from(code) {
            let ranges = hl.highlight_line(line, &self.syntaxes).unwrap_or_default();
            let spans: Vec<Span<'static>> = ranges
                .iter()
                .map(|(style, text)| {
                    Span::styled(
                        text.trim_end_matches('\n').to_string(),
                        syn_to_ratatui(*style),
                    )
                })
                .collect();
            out.push(Line::from(spans));
        }
        if out.is_empty() {
            out.push(Line::raw(""));
        }
        self.cache.lock().unwrap().insert(key, out.clone());
        out
    }
}

/// The process-wide highlighter.
pub fn highlighter() -> &'static Highlighter {
    static H: OnceLock<Highlighter> = OnceLock::new();
    H.get_or_init(Highlighter::new)
}

fn syn_to_ratatui(s: SynStyle) -> Style {
    let mut style = Style::default().fg(Color::Rgb(s.foreground.r, s.foreground.g, s.foreground.b));
    if s.font_style.contains(FontStyle::BOLD) {
        style = style.add_modifier(Modifier::BOLD);
    }
    if s.font_style.contains(FontStyle::ITALIC) {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if s.font_style.contains(FontStyle::UNDERLINE) {
        style = style.add_modifier(Modifier::UNDERLINED);
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_rust_and_caches() {
        let h = highlighter();
        let lines = h.highlight_block("fn main() {}\n", Some("rust"));
        assert!(!lines.is_empty());
        // second call hits cache and is identical
        let again = h.highlight_block("fn main() {}\n", Some("rust"));
        assert_eq!(lines.len(), again.len());
    }

    #[test]
    fn unknown_language_falls_back() {
        let h = highlighter();
        let lines = h.highlight_block("just text\n", Some("not-a-lang"));
        assert_eq!(lines.len(), 1);
    }
}
