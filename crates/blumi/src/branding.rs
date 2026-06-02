//! Terminal branding: the animated colorful flower banner. The rose art +
//! color ramp live in `blumi-tui` (shared with the TUI) so they stay in sync;
//! this module just drives the CLI animation loop.

use std::io::{IsTerminal, Write};
use std::time::Duration;

const PINK: &str = "\x1b[1;38;2;255;79;135m"; // rose-pink, bold
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

fn wordmark() -> String {
    format!("{PINK}   b l u m i{RESET}")
}

/// A one-line hint shown under the banner.
pub fn hint() -> String {
    format!("{DIM}  try: blumi run \"<prompt>\"   ·   blumi login   ·   blumi --help{RESET}")
}

/// Animated rose: plays a short color sweep in place (rewinding the cursor each
/// frame), then leaves the final frame + wordmark. Falls back to a single
/// static frame when stdout isn't a TTY. No trailing hint (used as a splash).
pub fn greeting() {
    let mut out = std::io::stdout();
    let _ = writeln!(out);

    if out.is_terminal() {
        const FRAMES: usize = 18;
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

    println!("{}", wordmark());
    println!();
}

/// Static rose + wordmark + hint (non-interactive / `web`).
pub fn banner() {
    println!();
    print!("{}", blumi_tui::banner_frame(0));
    println!("{}", wordmark());
    println!();
    println!("{}", hint());
}
