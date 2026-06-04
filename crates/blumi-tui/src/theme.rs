//! Semantic colors and icons for the TUI.

use ratatui::style::{Color, Modifier, Style};

#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    // Brand + text.
    pub primary: Color,
    pub accent: Color,
    pub fg: Color,
    pub fg_subtle: Color,
    pub fg_dim: Color,
    pub success: Color,
    pub error: Color,
    /// A slightly-raised panel/chip fill (focused panels, footer chips). We never
    /// paint a global background; only panels/chips opt into this.
    pub surface: Color,
    /// Idle panel border (quieter than `fg_dim`).
    pub border: Color,
    // Surface ramp (additive; only `surface` is used as a global-safe fill).
    pub bg: Color,
    pub surface_alt: Color,
    pub selection: Color,
    pub selection_fg: Color,
    // Borders / titles.
    pub border_active: Color,
    pub title: Color,
    pub title_active: Color,
    // Status bar + chips.
    pub statusbar_bg: Color,
    pub statusbar_fg: Color,
    pub chip_key_fg: Color,
    pub chip_label_fg: Color,
    // Badges / alerts.
    pub warn: Color,
    pub warn_fg: Color,
    pub info: Color,
    pub overdue: Color,
    // Diff.
    pub diff_add: Color,
    pub diff_del: Color,
    pub diff_hunk: Color,
    // Gauges / meters (green → amber → orange → red).
    pub gauge_low: Color,
    pub gauge_mid: Color,
    pub gauge_high: Color,
    pub gauge_crit: Color,
    // Syntax accents (for code/markdown highlighting bridges).
    pub syntax_kw: Color,
    pub syntax_str: Color,
    pub syntax_fn: Color,
    pub syntax_num: Color,
    pub syntax_comment: Color,
}

/// The hand-authored core of a theme. Everything else in [`Theme`] is derived
/// from these via [`Theme::from_core`], so a palette stays compact to write and
/// user TOML themes only need to specify the core colors.
struct Core {
    primary: Color,
    accent: Color,
    fg: Color,
    fg_subtle: Color,
    fg_dim: Color,
    success: Color,
    error: Color,
    warn: Color,
    surface: Color,
    surface_alt: Color,
    border: Color,
    bg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::rose()
    }
}

/// All built-in themes, in cycle order. `rose` (the Living Rose palette, from
/// project_mythara) is the default; `spatial`/`aurora` are mythara's Charmtone
/// skins; the rest are popular community palettes.
pub const THEMES: [fn() -> Theme; 11] = [
    Theme::rose,
    Theme::spatial,
    Theme::aurora,
    Theme::bloom,
    Theme::dark,
    Theme::mono,
    Theme::catppuccin,
    Theme::nord,
    Theme::dracula,
    Theme::tokyo_night,
    Theme::gruvbox,
];

/// `0xRRGGBB` → truecolor.
const fn rgb(hex: u32) -> Color {
    Color::Rgb(
        ((hex >> 16) & 0xFF) as u8,
        ((hex >> 8) & 0xFF) as u8,
        (hex & 0xFF) as u8,
    )
}

impl Theme {
    /// Build a full theme from its hand-authored [`Core`], deriving the rest.
    /// Derivations are chosen so existing render paths look identical to before
    /// (e.g. `border_active = primary`, `chip_key_fg = accent`); amber/orange
    /// gauge steps are fixed hues so meters read consistently across palettes.
    fn from_core(name: &'static str, c: Core) -> Self {
        Theme {
            name,
            primary: c.primary,
            accent: c.accent,
            fg: c.fg,
            fg_subtle: c.fg_subtle,
            fg_dim: c.fg_dim,
            success: c.success,
            error: c.error,
            surface: c.surface,
            border: c.border,
            bg: c.bg,
            surface_alt: c.surface_alt,
            selection: c.surface_alt,
            selection_fg: c.fg,
            border_active: c.primary,
            title: c.fg_subtle,
            title_active: c.primary,
            statusbar_bg: c.surface,
            statusbar_fg: c.fg_subtle,
            chip_key_fg: c.accent,
            chip_label_fg: c.fg_subtle,
            warn: c.warn,
            warn_fg: c.bg,
            info: c.accent,
            overdue: c.error,
            diff_add: c.success,
            diff_del: c.error,
            diff_hunk: c.accent,
            gauge_low: c.success,
            gauge_mid: rgb(0xE6C341),  // amber
            gauge_high: rgb(0xE08A3C), // orange
            gauge_crit: c.error,
            syntax_kw: c.primary,
            syntax_str: c.success,
            syntax_fn: c.accent,
            syntax_num: c.warn,
            syntax_comment: c.fg_dim,
        }
    }

    /// Living Rose — rose-pink brand on a warm palette with a cyan accent,
    /// ported from project_mythara. The colorful default.
    pub fn rose() -> Self {
        Theme::from_core(
            "rose",
            Core {
                primary: rgb(0xFF4F87), // rose-pink (Charple)
                accent: rgb(0x68FFD6),  // cyan nucleus (Bok)
                fg: rgb(0xF6E6EC),      // rosy near-white
                fg_subtle: rgb(0xCBA7B4),
                fg_dim: rgb(0x8C6571),
                success: rgb(0x4FE0A0), // Julep
                error: rgb(0xFF5470),   // Sriracha
                warn: rgb(0xFFC04F),
                surface: rgb(0x2A1722), // raised rose panel
                surface_alt: rgb(0x3A2230),
                border: rgb(0x5A3A47),
                bg: rgb(0x16090E),
            },
        )
    }

    /// Spatial — the original Charmtone Pantera (violet brand + cyan).
    pub fn spatial() -> Self {
        Theme::from_core(
            "spatial",
            Core {
                primary: rgb(0x6B50FF), // Charple violet
                accent: rgb(0x68FFD6),  // Bok cyan
                fg: rgb(0xDFDBDD),
                fg_subtle: rgb(0xA8A4AB),
                fg_dim: rgb(0x605F6B),
                success: rgb(0x00FFB2), // Julep
                error: rgb(0xEB4268),   // Sriracha
                warn: rgb(0xF5A623),
                surface: rgb(0x1B1630),
                surface_alt: rgb(0x252041),
                border: rgb(0x39354F),
                bg: rgb(0x0D0B14),
            },
        )
    }

    /// Aurora — a deep, brighter-violet variant that glows.
    pub fn aurora() -> Self {
        Theme::from_core(
            "aurora",
            Core {
                primary: rgb(0x8B6BFF), // brighter violet
                accent: rgb(0x68FFD6),  // cyan
                fg: rgb(0xEDE8F7),
                fg_subtle: rgb(0xB3A8C8),
                fg_dim: rgb(0x6E6488),
                success: rgb(0x00FFB2),
                error: rgb(0xEB4268),
                warn: rgb(0xF5A623),
                surface: rgb(0x201A38),
                surface_alt: rgb(0x2B2350),
                border: rgb(0x3E3560),
                bg: rgb(0x100B1C),
            },
        )
    }

    /// The soft "bloom" palette: pink primary, warm accent (256-color).
    pub fn bloom() -> Self {
        Theme::from_core(
            "bloom",
            Core {
                primary: Color::Indexed(213), // pink
                accent: Color::Indexed(221),  // warm yellow
                fg: Color::Indexed(252),
                fg_subtle: Color::Indexed(245),
                fg_dim: Color::Indexed(240),
                success: Color::Indexed(114),
                error: Color::Indexed(203),
                warn: Color::Indexed(214),
                surface: Color::Indexed(235),
                surface_alt: Color::Indexed(237),
                border: Color::Indexed(239),
                bg: Color::Indexed(233),
            },
        )
    }

    /// A cool blue/teal dark theme (256-color).
    pub fn dark() -> Self {
        Theme::from_core(
            "dark",
            Core {
                primary: Color::Indexed(75), // sky blue
                accent: Color::Indexed(80),  // teal
                fg: Color::Indexed(252),
                fg_subtle: Color::Indexed(245),
                fg_dim: Color::Indexed(239),
                success: Color::Indexed(114),
                error: Color::Indexed(203),
                warn: Color::Indexed(214),
                surface: Color::Indexed(235),
                surface_alt: Color::Indexed(237),
                border: Color::Indexed(238),
                bg: Color::Indexed(233),
            },
        )
    }

    /// A restrained monochrome theme (256-color).
    pub fn mono() -> Self {
        Theme::from_core(
            "mono",
            Core {
                primary: Color::Indexed(254),
                accent: Color::Indexed(250),
                fg: Color::Indexed(250),
                fg_subtle: Color::Indexed(244),
                fg_dim: Color::Indexed(239),
                success: Color::Indexed(246),
                error: Color::Indexed(210),
                warn: Color::Indexed(248),
                surface: Color::Indexed(236),
                surface_alt: Color::Indexed(238),
                border: Color::Indexed(240),
                bg: Color::Indexed(233),
            },
        )
    }

    /// Catppuccin Mocha.
    pub fn catppuccin() -> Self {
        Theme::from_core(
            "catppuccin",
            Core {
                primary: rgb(0xCBA6F7),     // mauve
                accent: rgb(0x94E2D5),      // teal
                fg: rgb(0xCDD6F4),          // text
                fg_subtle: rgb(0xBAC2DE),   // subtext1
                fg_dim: rgb(0x6C7086),      // overlay0
                success: rgb(0xA6E3A1),     // green
                error: rgb(0xF38BA8),       // red
                warn: rgb(0xF9E2AF),        // yellow
                surface: rgb(0x1E1E2E),     // base
                surface_alt: rgb(0x313244), // surface0
                border: rgb(0x45475A),      // surface1
                bg: rgb(0x181825),          // mantle
            },
        )
    }

    /// Nord.
    pub fn nord() -> Self {
        Theme::from_core(
            "nord",
            Core {
                primary: rgb(0x88C0D0), // frost
                accent: rgb(0x8FBCBB),
                fg: rgb(0xECEFF4),
                fg_subtle: rgb(0xD8DEE9),
                fg_dim: rgb(0x4C566A),
                success: rgb(0xA3BE8C),
                error: rgb(0xBF616A),
                warn: rgb(0xEBCB8B),
                surface: rgb(0x2E3440),
                surface_alt: rgb(0x3B4252),
                border: rgb(0x434C5E),
                bg: rgb(0x242933),
            },
        )
    }

    /// Dracula.
    pub fn dracula() -> Self {
        Theme::from_core(
            "dracula",
            Core {
                primary: rgb(0xBD93F9), // purple
                accent: rgb(0x8BE9FD),  // cyan
                fg: rgb(0xF8F8F2),
                fg_subtle: rgb(0xC8C8D0),
                fg_dim: rgb(0x6272A4), // comment
                success: rgb(0x50FA7B),
                error: rgb(0xFF5555),
                warn: rgb(0xF1FA8C),
                surface: rgb(0x282A36),
                surface_alt: rgb(0x343746),
                border: rgb(0x44475A),
                bg: rgb(0x21222C),
            },
        )
    }

    /// Tokyo Night.
    pub fn tokyo_night() -> Self {
        Theme::from_core(
            "tokyo-night",
            Core {
                primary: rgb(0x7AA2F7), // blue
                accent: rgb(0x7DCFFF),  // cyan
                fg: rgb(0xC0CAF5),
                fg_subtle: rgb(0xA9B1D6),
                fg_dim: rgb(0x565F89),
                success: rgb(0x9ECE6A),
                error: rgb(0xF7768E),
                warn: rgb(0xE0AF68),
                surface: rgb(0x1A1B26),
                surface_alt: rgb(0x24283B),
                border: rgb(0x33467C),
                bg: rgb(0x16161E),
            },
        )
    }

    /// Gruvbox (dark).
    pub fn gruvbox() -> Self {
        Theme::from_core(
            "gruvbox",
            Core {
                primary: rgb(0xFE8019), // orange
                accent: rgb(0x8EC07C),  // aqua
                fg: rgb(0xEBDBB2),
                fg_subtle: rgb(0xD5C4A1),
                fg_dim: rgb(0x928374),
                success: rgb(0xB8BB26),
                error: rgb(0xFB4934),
                warn: rgb(0xFABD2F),
                surface: rgb(0x3C3836),
                surface_alt: rgb(0x504945),
                border: rgb(0x665C54),
                bg: rgb(0x282828),
            },
        )
    }

    pub fn accent(&self) -> Style {
        Style::default().fg(self.accent)
    }
    pub fn dim(&self) -> Style {
        Style::default().fg(self.fg_dim)
    }
    pub fn subtle(&self) -> Style {
        Style::default().fg(self.fg_subtle)
    }
    pub fn body(&self) -> Style {
        Style::default().fg(self.fg)
    }
    pub fn bold_primary(&self) -> Style {
        Style::default()
            .fg(self.primary)
            .add_modifier(Modifier::BOLD)
    }

    /// A raised panel/chip fill. Honors the `FILL_PANELS` kill switch so
    /// transparency-loving terminals can opt out (env `BLUMI_NO_FILL`).
    pub fn surface(&self) -> Style {
        if FILL_PANELS.load(std::sync::atomic::Ordering::Relaxed) {
            Style::default().bg(self.surface)
        } else {
            Style::default()
        }
    }
    /// Style for an idle (unfocused) panel border + its title.
    pub fn border(&self) -> Style {
        Style::default().fg(self.border)
    }
    /// Style for a focused panel border + its title accent.
    pub fn panel_focus(&self) -> Style {
        Style::default()
            .fg(self.primary)
            .add_modifier(Modifier::BOLD)
    }
    /// A footer key-chip: the keycap half (bright accent on surface).
    pub fn chip_key(&self) -> Style {
        self.surface().fg(self.accent).add_modifier(Modifier::BOLD)
    }
    /// A footer key-chip: the label half (subtle on surface).
    pub fn chip_label(&self) -> Style {
        self.surface().fg(self.fg_subtle)
    }

    /// A selected-row style — surface fill + selection fg when fills are on,
    /// else a reversed primary (so it stays visible on transparent terminals).
    pub fn selection(&self) -> Style {
        if FILL_PANELS.load(std::sync::atomic::Ordering::Relaxed) {
            Style::default().bg(self.selection).fg(self.selection_fg)
        } else {
            Style::default()
                .fg(self.primary)
                .add_modifier(Modifier::REVERSED)
        }
    }
    /// A warning/alert badge: dark text on the warn (amber) ground.
    pub fn warn_badge(&self) -> Style {
        Style::default()
            .fg(self.warn_fg)
            .bg(self.warn)
            .add_modifier(Modifier::BOLD)
    }
    /// The meter/gauge color for a fill fraction (green → amber → orange → red).
    pub fn gauge(&self, frac: f64) -> Color {
        let pct = frac * 100.0;
        if pct >= 95.0 {
            self.gauge_crit
        } else if pct > 80.0 {
            self.gauge_high
        } else if pct >= 50.0 {
            self.gauge_mid
        } else {
            self.gauge_low
        }
    }
    pub fn diff_add_style(&self) -> Style {
        Style::default().fg(self.diff_add)
    }
    pub fn diff_del_style(&self) -> Style {
        Style::default().fg(self.diff_del)
    }
    pub fn diff_hunk_style(&self) -> Style {
        Style::default().fg(self.diff_hunk)
    }
}

/// A registry of selectable themes — the built-ins plus any user themes loaded
/// from `~/.blumi/themes/*.toml`. Theme selection (cycle / by-name / by-index)
/// routes through here so user themes participate alongside the built-ins. Held
/// in the `Model`; built once at startup.
#[derive(Clone)]
pub struct ThemeRegistry {
    themes: Vec<Theme>,
}

impl Default for ThemeRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl ThemeRegistry {
    /// Just the built-in palettes, in cycle order (`rose` first).
    pub fn builtin() -> Self {
        Self {
            themes: THEMES.iter().map(|f| f()).collect(),
        }
    }

    /// The theme at `idx` (wrapping); falls back to the default if empty.
    pub fn get(&self, idx: usize) -> Theme {
        if self.themes.is_empty() {
            Theme::default()
        } else {
            self.themes[idx % self.themes.len()]
        }
    }
    /// Index of a theme by name (case-insensitive).
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.themes
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(name))
    }
    /// The next index after `cur`, wrapping (for `/theme` cycling).
    pub fn next_index(&self, cur: usize) -> usize {
        if self.themes.is_empty() {
            0
        } else {
            (cur + 1) % self.themes.len()
        }
    }
    /// Append user themes after the built-ins. A user theme whose name matches a
    /// built-in (case-insensitive) overrides it in place; otherwise it's appended.
    pub fn with_user(mut self, user: Vec<Theme>) -> Self {
        for t in user {
            if let Some(slot) = self
                .themes
                .iter_mut()
                .find(|b| b.name.eq_ignore_ascii_case(t.name))
            {
                *slot = t;
            } else {
                self.themes.push(t);
            }
        }
        self
    }
    /// Theme names in cycle order (for `/theme` listing and pickers).
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.themes.iter().map(|t| t.name)
    }
}

// ── User themes (TOML) ──────────────────────────────────────────────────────

/// A user theme file (`~/.blumi/themes/<name>.toml`). Every field is optional and
/// falls back to a neutral dark base, so a minimal file (just `primary`/`accent`/
/// `fg`) still yields a coherent theme — the rest is derived by [`Theme::from_core`].
/// Colors are `"#rrggbb"` (truecolor) or `"@N"` (a 0–255 palette index).
#[derive(serde::Deserialize, Default)]
struct ThemeFile {
    name: Option<String>,
    primary: Option<String>,
    accent: Option<String>,
    fg: Option<String>,
    fg_subtle: Option<String>,
    fg_dim: Option<String>,
    success: Option<String>,
    error: Option<String>,
    warn: Option<String>,
    surface: Option<String>,
    surface_alt: Option<String>,
    border: Option<String>,
    bg: Option<String>,
}

impl Core {
    /// A neutral dark base that fills any color a user theme omits.
    fn base() -> Core {
        Core {
            primary: rgb(0x7AA2F7),
            accent: rgb(0x7DCFFF),
            fg: rgb(0xC0CAF5),
            fg_subtle: rgb(0x9AA5CE),
            fg_dim: rgb(0x565F89),
            success: rgb(0x9ECE6A),
            error: rgb(0xF7768E),
            warn: rgb(0xE0AF68),
            surface: rgb(0x1F2335),
            surface_alt: rgb(0x292E42),
            border: rgb(0x3B4261),
            bg: rgb(0x16161E),
        }
    }
}

/// Parse a single `#rrggbb` (or `@N` 256-palette index) color.
fn parse_color(s: &str) -> Result<Color, String> {
    let s = s.trim();
    if let Some(idx) = s.strip_prefix('@') {
        let n: u8 = idx
            .parse()
            .map_err(|_| format!("invalid palette index '{s}' (want @0..@255)"))?;
        return Ok(Color::Indexed(n));
    }
    let h = s.strip_prefix('#').unwrap_or(s);
    if h.len() != 6 {
        return Err(format!("invalid color '{s}' (want #rrggbb or @N)"));
    }
    let v = u32::from_str_radix(h, 16).map_err(|_| format!("invalid hex color '{s}'"))?;
    Ok(rgb(v))
}

fn opt_color(o: &Option<String>, base: Color) -> Result<Color, String> {
    match o {
        Some(s) => parse_color(s),
        None => Ok(base),
    }
}

/// Leak a user-theme name to `&'static str` so [`Theme`] stays `Copy`. Themes are
/// few and live for the whole process, so the tiny leak is acceptable.
fn intern(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

fn theme_from_file(file: ThemeFile, stem: &str) -> Result<Theme, String> {
    let b = Core::base();
    let c = Core {
        primary: opt_color(&file.primary, b.primary)?,
        accent: opt_color(&file.accent, b.accent)?,
        fg: opt_color(&file.fg, b.fg)?,
        fg_subtle: opt_color(&file.fg_subtle, b.fg_subtle)?,
        fg_dim: opt_color(&file.fg_dim, b.fg_dim)?,
        success: opt_color(&file.success, b.success)?,
        error: opt_color(&file.error, b.error)?,
        warn: opt_color(&file.warn, b.warn)?,
        surface: opt_color(&file.surface, b.surface)?,
        surface_alt: opt_color(&file.surface_alt, b.surface_alt)?,
        border: opt_color(&file.border, b.border)?,
        bg: opt_color(&file.bg, b.bg)?,
    };
    let name = file.name.unwrap_or_else(|| stem.to_string());
    Ok(Theme::from_core(intern(name), c))
}

/// Load user themes from `dir` (`~/.blumi/themes/*.toml`), sorted by filename.
/// Unreadable / malformed files are skipped with a warning — never fatal.
pub fn load_user_themes(dir: &std::path::Path) -> Vec<Theme> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut files: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("toml"))
        })
        .collect();
    files.sort();
    for path in files {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parsed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| toml::from_str::<ThemeFile>(&s).map_err(|e| e.to_string()))
            .and_then(|f| theme_from_file(f, &stem));
        match parsed {
            Ok(t) => out.push(t),
            Err(e) => tracing::warn!("skipping theme {}: {e}", path.display()),
        }
    }
    out
}

/// Global kill-switch for panel/chip background fills. Defaults on; set the env
/// var `BLUMI_NO_FILL` (any value) to render borderless/transparent instead —
/// helps terminals that ignore or mangle background colors.
pub static FILL_PANELS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Initialize [`FILL_PANELS`] from the environment. Call once at startup.
pub fn init_fill_from_env() {
    if std::env::var_os("BLUMI_NO_FILL").is_some() {
        FILL_PANELS.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

// Icons moved to the `crate::icons` module (unicode / nerd / ascii modes).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eleven_builtins_with_rose_default() {
        assert_eq!(THEMES.len(), 11);
        let reg = ThemeRegistry::builtin();
        assert_eq!(reg.get(0).name, "rose");
        assert_eq!(reg.index_of("catppuccin"), Some(6));
        assert_eq!(reg.index_of("TOKYO-NIGHT"), Some(9)); // case-insensitive
        assert_eq!(reg.next_index(10), 0); // wraps
    }

    #[test]
    fn parse_color_hex_and_index() {
        assert_eq!(
            parse_color("#FF8800").unwrap(),
            Color::Rgb(0xFF, 0x88, 0x00)
        );
        assert_eq!(parse_color("aabbcc").unwrap(), Color::Rgb(0xAA, 0xBB, 0xCC));
        assert_eq!(parse_color("@213").unwrap(), Color::Indexed(213));
        assert!(parse_color("#xyz").is_err());
        assert!(parse_color("#1234").is_err());
        assert!(parse_color("@999").is_err());
    }

    #[test]
    fn theme_from_file_defaults_and_derives() {
        // A minimal file: only a couple of core colors; the rest fall back.
        let f = ThemeFile {
            name: Some("Solar".into()),
            primary: Some("#FF0000".into()),
            accent: Some("@40".into()),
            ..Default::default()
        };
        let t = theme_from_file(f, "ignored-stem").unwrap();
        assert_eq!(t.name, "Solar");
        assert_eq!(t.primary, Color::Rgb(0xFF, 0, 0));
        assert_eq!(t.accent, Color::Indexed(40));
        // Derived: border_active follows primary, chip_key_fg follows accent.
        assert_eq!(t.border_active, t.primary);
        assert_eq!(t.chip_key_fg, t.accent);
        // Omitted fg falls back to the base (not the default sentinel).
        assert_eq!(t.fg, Core::base().fg);
        // Name falls back to the file stem when unset.
        let t2 = theme_from_file(ThemeFile::default(), "my-theme").unwrap();
        assert_eq!(t2.name, "my-theme");
    }

    #[test]
    fn registry_with_user_overrides_then_appends() {
        let user = vec![
            theme_from_file(
                ThemeFile {
                    name: Some("rose".into()),
                    primary: Some("#010203".into()),
                    ..Default::default()
                },
                "rose",
            )
            .unwrap(),
            theme_from_file(
                ThemeFile {
                    name: Some("custom".into()),
                    ..Default::default()
                },
                "custom",
            )
            .unwrap(),
        ];
        let reg = ThemeRegistry::builtin().with_user(user);
        // "rose" overridden in place (still index 0) with the user's primary.
        assert_eq!(reg.get(0).name, "rose");
        assert_eq!(reg.get(0).primary, Color::Rgb(1, 2, 3));
        // "custom" appended after the 11 built-ins.
        assert_eq!(reg.index_of("custom"), Some(11));
    }

    #[test]
    fn load_user_themes_skips_malformed_and_sorts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("aaa.toml"),
            "name = \"Aaa\"\nprimary = \"#112233\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("bad.toml"), "primary = \"#nothex\"\n").unwrap();
        std::fs::write(dir.path().join("notatheme.txt"), "ignored").unwrap();
        let themes = load_user_themes(dir.path());
        assert_eq!(themes.len(), 1, "malformed + non-toml skipped");
        assert_eq!(themes[0].name, "Aaa");
        // Missing dir → empty, never panics.
        assert!(load_user_themes(&dir.path().join("nope")).is_empty());
    }
}
