//! Terminal branding: the animated colorful flower banner. The rose art +
//! color ramp live in `blumi-tui` (shared with the TUI) so they stay in sync;
//! this module just drives the CLI animation loop.

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// A one-line hint shown under the banner.
pub fn hint() -> String {
    format!("{DIM}  try: blumi run \"<prompt>\"   ·   blumi login   ·   blumi --help{RESET}")
}

/// The brand splash: the pixel-perfect flower logo, then the bold gradient block
/// wordmark + tagline beneath it. Used as the interactive greeting (no hint).
pub fn greeting() {
    println!();
    print!("{}", blumi_tui::flower_raster_ansi(9));
    println!();
    print!("{}", blumi_tui::wordmark_ansi(0));
    println!("{DIM}  {}{RESET}", blumi_tui::TAGLINE);
    println!();
}

/// Static flower + gradient block wordmark + tagline + hint (non-interactive /
/// `web` / gateways / loop).
pub fn banner() {
    println!();
    print!("{}", blumi_tui::flower_raster_ansi(7));
    println!();
    print!("{}", blumi_tui::wordmark_ansi(0));
    println!("{DIM}  {}{RESET}", blumi_tui::TAGLINE);
    println!();
    println!("{}", hint());
}
