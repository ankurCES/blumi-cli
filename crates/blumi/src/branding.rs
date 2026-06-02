//! Terminal branding: render the blumi flower logo with ANSI color. The art
//! itself lives in `blumi-tui` so the TUI and CLI stay in sync.

const PINK: &str = "\x1b[38;5;213m";
const YELLOW: &str = "\x1b[38;5;221m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// The flower splash with petals in pink, center in yellow, wordmark bold.
pub fn logo_banner() -> String {
    let mut out = String::from("\n");
    for line in blumi_tui::LOGO.lines() {
        if line.replace(' ', "").starts_with('b') {
            // the wordmark line
            out.push_str(&format!("{BOLD}{PINK}{line}{RESET}\n"));
        } else {
            // petal/center line: color the center glyph yellow, petals pink
            let colored = line.replace('◉', &format!("{YELLOW}◉{PINK}"));
            out.push_str(&format!("{PINK}{colored}{RESET}\n"));
        }
    }
    out
}

/// A one-line hint shown under the logo for the no-command / tui-stub cases.
pub fn hint() -> String {
    format!("{DIM}  try: blumi run \"<prompt>\"   ·   blumi session list   ·   blumi --help{RESET}")
}
