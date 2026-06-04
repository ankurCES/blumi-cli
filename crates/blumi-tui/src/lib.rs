//! Terminal UI for blumi (ratatui).
//!
//! A self-contained MVU-style app that subscribes to a session's event stream
//! and sends it commands. The brand flower logo lives here (see [`LOGO`]) so the
//! landing screen and the CLI banner share one definition.

mod app;
mod commands;
mod cost;
mod dialog;
mod diff;
mod highlight;
mod icons;
mod logo;
mod markdown;
mod mascot;
mod model;
mod motion;
mod primitives;
pub mod theme;
mod update;
mod view;
mod wizard;

pub use app::{run, ModelOptions, ProviderOpt, SessionFactory, TuiConfig};
pub use logo::{BLUMI_BLOCK, BLUMI_BLOCK_WIDTH, LOGO, MARK, PETAL, TAGLINE, WORDMARK};
pub use mascot::{banner_frame, flower_raster_ansi, wordmark_ansi, ROSE_ROWS};
pub use model::Workspace;
pub use wizard::{run_onboarding, ProviderChoice, WizardOutcome};
