//! Terminal UI for blumi (ratatui).
//!
//! A self-contained MVU-style app that subscribes to a session's event stream
//! and sends it commands. The brand flower logo lives here (see [`LOGO`]) so the
//! landing screen and the CLI banner share one definition.

mod app;
mod dialog;
mod diff;
mod highlight;
mod logo;
mod markdown;
mod mascot;
mod model;
mod theme;
mod update;
mod view;
mod wizard;

pub use app::run;
pub use logo::{LOGO, MARK, PETAL, WORDMARK};
pub use mascot::{banner_frame, ROSE_ROWS};
pub use wizard::{run_onboarding, ProviderChoice, WizardOutcome};
