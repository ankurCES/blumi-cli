//! Icon set with three modes: **unicode** (the default — widely-supported
//! dingbats/box-drawing, exactly what blumi has always shipped), **nerd**
//! (opt-in Nerd-Font glyphs for users whose terminal font has them), and
//! **ascii** (a pure-ASCII fallback for `TERM=dumb` / no-unicode terminals).
//!
//! The mode is process-global (like the `FILL_PANELS` switch), chosen once at
//! startup from config/env, so render code just calls `icons::ok()` etc. without
//! threading an icon set through every function.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconMode {
    Unicode,
    Nerd,
    Ascii,
}

static MODE: AtomicU8 = AtomicU8::new(0); // 0=Unicode, 1=Nerd, 2=Ascii

pub fn set_mode(m: IconMode) {
    MODE.store(m as u8, Ordering::Relaxed);
}

pub fn mode() -> IconMode {
    match MODE.load(Ordering::Relaxed) {
        1 => IconMode::Nerd,
        2 => IconMode::Ascii,
        _ => IconMode::Unicode,
    }
}

/// Choose the icon mode from config/env. `BLUMI_ICONS=nerd|unicode|ascii` wins;
/// otherwise `BLUMI_NERD_FONT` (any value) requests nerd. Default stays unicode
/// (safe everywhere) when nothing is set.
pub fn init_from_env() {
    let explicit = match std::env::var("BLUMI_ICONS").ok().as_deref() {
        Some("nerd") => Some(IconMode::Nerd),
        Some("ascii") => Some(IconMode::Ascii),
        Some("unicode") => Some(IconMode::Unicode),
        _ => None,
    };
    let chosen = explicit.or_else(|| std::env::var_os("BLUMI_NERD_FONT").map(|_| IconMode::Nerd));
    if let Some(m) = chosen {
        set_mode(m);
    }
}

/// Pick a glyph by the active mode. `uni` MUST match the historically-shipped
/// glyph so default-mode snapshots are unchanged.
#[inline]
fn pick(uni: &'static str, nerd: &'static str, ascii: &'static str) -> &'static str {
    match mode() {
        IconMode::Unicode => uni,
        IconMode::Nerd => nerd,
        IconMode::Ascii => ascii,
    }
}

// ── Brand / roles ──────────────────────────────────────────────────────────
pub fn agent() -> &'static str {
    pick("✿", "\u{f06c}", "*") // nf-fa-leaf as a flower stand-in
}
pub fn user() -> &'static str {
    pick("›", "\u{f007}", ">") // nf-fa-user
}
pub fn tool() -> &'static str {
    pick("▸", "\u{f0ad}", ">") // nf-fa-wrench
}
pub fn ok() -> &'static str {
    pick("✓", "\u{f00c}", "+") // nf-fa-check
}
pub fn err() -> &'static str {
    pick("×", "\u{f00d}", "x") // nf-fa-times
}
/// Agent/sub-agent hard failure (historically a heavier ✗ than [`err`]).
pub fn fail() -> &'static str {
    pick("✗", "\u{f00d}", "x")
}
pub fn dot() -> &'static str {
    pick("●", "\u{f111}", "*") // nf-fa-circle
}
pub fn pin() -> &'static str {
    pick("★", "\u{f005}", "*") // nf-fa-star
}
pub fn remote() -> &'static str {
    pick("☁", "\u{f0c2}", "@") // nf-fa-cloud
}
pub fn local() -> &'static str {
    pick("▪", "\u{f015}", "-") // nf-fa-home
}

// ── Box-drawing (cards) + bars ─────────────────────────────────────────────
// Nerd fonts include box-drawing, so nerd == unicode here; only ascii degrades.
pub fn tl() -> &'static str {
    pick("╭", "╭", "+")
}
pub fn bl() -> &'static str {
    pick("╰", "╰", "+")
}
pub fn h() -> &'static str {
    pick("─", "─", "-")
}
pub fn v() -> &'static str {
    pick("│", "│", "|")
}
pub fn bar_full() -> &'static str {
    pick("█", "█", "#")
}
pub fn bar_empty() -> &'static str {
    pick("░", "░", ".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_default_matches_legacy_glyphs() {
        // Default mode must be byte-identical to what blumi always shipped.
        set_mode(IconMode::Unicode);
        assert_eq!(agent(), "✿");
        assert_eq!(ok(), "✓");
        assert_eq!(tl(), "╭");
        assert_eq!(bar_full(), "█");
    }

    #[test]
    fn ascii_and_nerd_differ() {
        set_mode(IconMode::Ascii);
        assert_eq!(ok(), "+");
        assert_eq!(tl(), "+");
        set_mode(IconMode::Nerd);
        assert_ne!(ok(), "+");
        set_mode(IconMode::Unicode); // restore for other tests
    }
}
