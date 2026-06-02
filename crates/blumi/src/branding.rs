//! Terminal branding: the animated colorful flower banner. The rose art +
//! color ramp live in `blumi-tui` (shared with the TUI) so they stay in sync;
//! this module just drives the CLI animation loop.

use std::io::{IsTerminal, Write};
use std::time::Duration;

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// A one-line hint shown under the banner.
pub fn hint() -> String {
    format!("{DIM}  try: blumi run \"<prompt>\"   ·   blumi login   ·   blumi --help{RESET}")
}

/// The brand splash: the animated rose sweeps in place, then the bold gradient
/// block wordmark + tagline land beneath it. Falls back to a static frame when
/// stdout isn't a TTY. Used as the interactive greeting (no hint).
pub fn greeting() {
    let mut out = std::io::stdout();
    let _ = writeln!(out);

    if out.is_terminal() {
        const FRAMES: usize = 16;
        for t in 0..FRAMES {
            print!("{}", blumi_tui::banner_frame(t * 2));
            let _ = out.flush();
            std::thread::sleep(Duration::from_millis(55));
            if t + 1 < FRAMES {
                // rewind to the top of the rose to overdraw the next frame
                print!("\x1b[{}A", blumi_tui::ROSE_ROWS);
            }
        }
    } else {
        print!("{}", blumi_tui::banner_frame(0));
    }

    println!();
    print!("{}", blumi_tui::wordmark_ansi(0));
    println!("{DIM}  {}{RESET}", blumi_tui::TAGLINE);
    println!();
}

/// Static gradient block wordmark + tagline + hint (non-interactive / `web`).
pub fn banner() {
    println!();
    print!("{}", blumi_tui::wordmark_ansi(0));
    println!("{DIM}  {}{RESET}", blumi_tui::TAGLINE);
    println!();
    println!("{}", hint());
}
