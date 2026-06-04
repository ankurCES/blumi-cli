//! Render markdown to styled, wrapped ratatui lines. Prose is parsed each
//! frame (cheap); fenced code blocks are syntect-highlighted via the cached
//! [`crate::highlight::highlighter`].

use crate::highlight::highlighter;
use crate::theme::Theme;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

/// Render markdown to wrapped ratatui lines. **Memoized**: the syntect-highlighted
/// output for a given (text, width, theme) is cached, so the ~20fps redraws
/// driven by animation ticks don't re-highlight the whole transcript every
/// frame — only changed messages (e.g. the streaming reply) are recomputed.
/// Keeps pane navigation responsive on large transcripts.
pub fn render_markdown(text: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    let key = (h.finish(), width, theme.name);
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&key)) {
        return (*hit).clone();
    }
    let lines = Rc::new(render_uncached(text, width, theme));
    CACHE.with(|c| c.borrow_mut().put(key, lines.clone()));
    (*lines).clone()
}

/// Uncached render, for content that changes every frame (the in-flight
/// streaming reply): caching its partials would churn the memo cache and evict
/// the static transcript. Render it fresh instead.
pub fn render_markdown_live(text: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
    render_uncached(text, width, theme)
}

fn render_uncached(text: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
    let mut r = Renderer::new(width, theme);
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    for event in Parser::new_ext(text, opts) {
        r.handle(event);
    }
    r.finish()
}

type CacheKey = (u64, usize, &'static str);

/// A tiny FIFO-capped memo cache for rendered markdown (UI thread only).
struct MdCache {
    map: HashMap<CacheKey, Rc<Vec<Line<'static>>>>,
    order: VecDeque<CacheKey>,
}
impl MdCache {
    const CAP: usize = 512;
    fn get(&self, k: &CacheKey) -> Option<Rc<Vec<Line<'static>>>> {
        self.map.get(k).cloned()
    }
    fn put(&mut self, k: CacheKey, v: Rc<Vec<Line<'static>>>) {
        if self.map.insert(k, v).is_none() {
            self.order.push_back(k);
            if self.order.len() > Self::CAP {
                if let Some(old) = self.order.pop_front() {
                    self.map.remove(&old);
                }
            }
        }
    }
}

thread_local! {
    static CACHE: RefCell<MdCache> =
        RefCell::new(MdCache { map: HashMap::new(), order: VecDeque::new() });
}

struct Renderer<'a> {
    width: usize,
    theme: &'a Theme,
    out: Vec<Line<'static>>,
    cur: Vec<Span<'static>>,
    bold: u32,
    italic: u32,
    strike: u32,
    link: bool,
    list_stack: Vec<Option<u64>>,
    in_item: bool,
    line_prefix: String,
    code_lang: Option<String>,
    in_code: bool,
    code_buf: String,
    quote: u32,
    heading: Option<HeadingLevel>,
}

impl<'a> Renderer<'a> {
    fn new(width: usize, theme: &'a Theme) -> Self {
        Renderer {
            width: width.max(16),
            theme,
            out: Vec::new(),
            cur: Vec::new(),
            bold: 0,
            italic: 0,
            strike: 0,
            link: false,
            list_stack: Vec::new(),
            in_item: false,
            line_prefix: String::new(),
            code_lang: None,
            in_code: false,
            code_buf: String::new(),
            quote: 0,
            heading: None,
        }
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                if self.in_code {
                    self.code_buf.push_str(&t);
                } else {
                    let style = self.inline_style();
                    self.cur.push(Span::styled(t.into_string(), style));
                }
            }
            Event::Code(t) => {
                self.cur
                    .push(Span::styled(t.into_string(), self.code_inline_style()));
            }
            Event::SoftBreak if !self.in_code => {
                self.cur.push(Span::raw(" "));
            }
            Event::HardBreak => self.flush_inline(),
            Event::Rule => {
                self.ensure_blank();
                self.out.push(Line::from(Span::styled(
                    "─".repeat(self.width.min(80)),
                    self.theme.dim(),
                )));
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph if !self.in_item => {
                self.ensure_blank();
            }
            Tag::Heading { level, .. } => {
                self.ensure_blank();
                self.heading = Some(level);
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { .. } => self.link = true,
            Tag::List(start) => {
                if self.list_stack.is_empty() {
                    self.ensure_blank();
                }
                self.list_stack.push(start);
            }
            Tag::Item => {
                let depth = self.list_stack.len().max(1);
                let indent = "  ".repeat(depth - 1);
                let marker = match self.list_stack.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}. ");
                        *self.list_stack.last_mut().unwrap() = Some(*n + 1);
                        m
                    }
                    _ => "• ".to_string(),
                };
                self.line_prefix = format!("{indent}{marker}");
                self.in_item = true;
            }
            Tag::CodeBlock(kind) => {
                self.ensure_blank();
                self.in_code = true;
                self.code_buf.clear();
                self.code_lang = match kind {
                    CodeBlockKind::Fenced(l) if !l.is_empty() => Some(l.into_string()),
                    _ => None,
                };
            }
            Tag::BlockQuote(_) => {
                self.ensure_blank();
                self.quote += 1;
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph if !self.in_item => {
                self.flush_inline();
            }
            TagEnd::Heading(_) => self.flush_heading(),
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => self.link = false,
            TagEnd::List(_) => {
                self.list_stack.pop();
            }
            TagEnd::Item => {
                self.flush_inline();
                self.in_item = false;
                self.line_prefix.clear();
            }
            TagEnd::CodeBlock => self.flush_code_block(),
            TagEnd::BlockQuote(_) => self.quote = self.quote.saturating_sub(1),
            _ => {}
        }
    }

    fn inline_style(&self) -> Style {
        let mut s = self.theme.body();
        if self.bold > 0 {
            s = s.add_modifier(Modifier::BOLD);
        }
        if self.italic > 0 {
            s = s.add_modifier(Modifier::ITALIC);
        }
        if self.strike > 0 {
            s = s.add_modifier(Modifier::CROSSED_OUT);
        }
        if self.link {
            s = s.fg(self.theme.accent).add_modifier(Modifier::UNDERLINED);
        }
        s
    }

    fn code_inline_style(&self) -> Style {
        Style::default().fg(Color::Indexed(216))
    }

    fn quote_prefix(&self) -> String {
        "│ ".repeat(self.quote as usize)
    }

    fn ensure_blank(&mut self) {
        if !self.out.is_empty() && !self.last_is_blank() {
            self.out.push(Line::raw(""));
        }
    }

    fn last_is_blank(&self) -> bool {
        self.out
            .last()
            .map(|l| l.spans.iter().all(|s| s.content.trim().is_empty()))
            .unwrap_or(true)
    }

    /// Wrap and emit the current inline run as a paragraph.
    fn flush_inline(&mut self) {
        if self.cur.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.cur);
        let prefix = if !self.line_prefix.is_empty() {
            self.line_prefix.clone()
        } else {
            self.quote_prefix()
        };
        let style = if self.quote > 0 {
            self.theme.subtle()
        } else {
            self.theme.body()
        };
        let wrapped = wrap_spans(spans, self.width, &prefix, style);
        self.out.extend(wrapped);
    }

    fn flush_heading(&mut self) {
        if self.cur.is_empty() {
            self.heading = None;
            return;
        }
        let level = self.heading.take().unwrap_or(HeadingLevel::H3);
        let style = match level {
            HeadingLevel::H1 => self.theme.bold_primary().add_modifier(Modifier::UNDERLINED),
            HeadingLevel::H2 => self.theme.bold_primary(),
            _ => Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        };
        // Re-style all heading text spans uniformly.
        let text: String = self.cur.drain(..).map(|s| s.content.into_owned()).collect();
        let wrapped = wrap_spans(vec![Span::styled(text, style)], self.width, "", style);
        self.out.extend(wrapped);
        self.out.push(Line::raw(""));
    }

    fn flush_code_block(&mut self) {
        let code = std::mem::take(&mut self.code_buf);
        let lang = self.code_lang.take();
        let highlighted = highlighter().highlight_block(&code, lang.as_deref());
        // Indent code by two spaces, with a subtle left rule.
        for line in highlighted {
            let mut spans = vec![Span::styled("▏ ", self.theme.dim())];
            spans.extend(line.spans);
            self.out.push(Line::from(spans));
        }
        self.in_code = false;
        self.out.push(Line::raw(""));
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_inline();
        // Trim a trailing blank line.
        while self
            .out
            .last()
            .map(|l| l.spans.iter().all(|s| s.content.trim().is_empty()))
            .unwrap_or(false)
        {
            self.out.pop();
        }
        if self.out.is_empty() {
            self.out.push(Line::raw(""));
        }
        self.out
    }
}

/// Greedy word-wrap a run of styled spans to `width`, prefixing the first line
/// with `prefix` and continuation lines with equivalent indentation. Collapses
/// inter-word whitespace (markdown semantics).
fn wrap_spans(
    spans: Vec<Span<'static>>,
    width: usize,
    prefix: &str,
    _base: Style,
) -> Vec<Line<'static>> {
    let prefix_w = prefix.chars().count();
    let indent: String = " ".repeat(prefix_w);
    let avail = width.saturating_sub(prefix_w).max(8);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut cur: Vec<Span<'static>> = Vec::new();
    let mut cur_w = 0usize;
    let mut first = true;

    let flush = |cur: &mut Vec<Span<'static>>, first: &mut bool, lines: &mut Vec<Line<'static>>| {
        let mut spans = Vec::new();
        let pfx = if *first { prefix } else { indent.as_str() };
        if !pfx.is_empty() {
            spans.push(Span::raw(pfx.to_string()));
        }
        spans.append(cur);
        lines.push(Line::from(spans));
        *first = false;
    };

    for span in spans {
        let style = span.style;
        for word in span.content.split_whitespace() {
            let wl = word.chars().count();
            if !cur.is_empty() && cur_w + 1 + wl > avail {
                flush(&mut cur, &mut first, &mut lines);
                cur_w = 0;
            }
            if !cur.is_empty() {
                cur.push(Span::raw(" "));
                cur_w += 1;
            }
            cur.push(Span::styled(word.to_string(), style));
            cur_w += wl;
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        flush(&mut cur, &mut first, &mut lines);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_heading_and_paragraph() {
        let theme = Theme::default();
        let lines = render_markdown("# Title\n\nHello **world**.", 40, &theme);
        let s = text_of(&lines);
        assert!(s.contains("Title"));
        assert!(s.contains("world"));
    }

    #[test]
    fn wraps_long_paragraph() {
        let theme = Theme::default();
        let long = "word ".repeat(40);
        let lines = render_markdown(&long, 20, &theme);
        assert!(lines.len() > 1, "should wrap to multiple lines");
        for l in &lines {
            let w: usize = l.spans.iter().map(|s| s.content.chars().count()).sum();
            assert!(w <= 20, "line within width: {w}");
        }
    }

    #[test]
    fn renders_code_block() {
        let theme = Theme::default();
        let md = "```rust\nfn main() {}\n```";
        let lines = render_markdown(md, 40, &theme);
        let s = text_of(&lines);
        assert!(s.contains("fn main"));
    }

    #[test]
    fn renders_bullets() {
        let theme = Theme::default();
        let lines = render_markdown("- one\n- two", 40, &theme);
        let s = text_of(&lines);
        assert!(s.contains("• one"));
        assert!(s.contains("• two"));
    }
}
