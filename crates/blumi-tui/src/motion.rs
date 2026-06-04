//! Cinematic motion (powered by `tachyonfx`). We keep it tasteful and optional:
//! a short "scene-in" coalesce when the UI first appears, the session/scene
//! changes, or the theme switches. Honors a reduced-motion setting and a hard
//! off switch (config `/motion` or env `BLUMI_NO_MOTION` / `NO_MOTION`).
//!
//! Effects mutate the rendered buffer in place, so [`Motion::process`] runs as
//! the very last step of `view::render`, after every widget is drawn. While an
//! effect is live we keep marking the UI dirty (see `update::Msg::Tick`) so it
//! animates; when it settles we stop, so idle CPU stays at zero.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::time::{Duration, Instant};
use tachyonfx::{fx, EffectManager};

/// How long the "scene-in" effect runs, in milliseconds.
const SCENE_MS: u32 = 320;
const SCENE_MS_REDUCED: u32 = 120;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MotionLevel {
    Full,
    Reduced,
    Off,
}

pub struct Motion {
    level: MotionLevel,
    mgr: EffectManager<&'static str>,
    last: Option<Instant>,
    /// While `Some(t)` and `now < t`, an effect is animating.
    active_until: Option<Instant>,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            level: MotionLevel::Full,
            mgr: EffectManager::default(),
            last: None,
            active_until: None,
        }
    }
}

impl Motion {
    /// Build from the environment: `BLUMI_NO_MOTION` / `NO_MOTION` force off;
    /// `BLUMI_REDUCED_MOTION` requests reduced.
    pub fn from_env() -> Self {
        let mut m = Self::default();
        if std::env::var_os("BLUMI_NO_MOTION").is_some() || std::env::var_os("NO_MOTION").is_some()
        {
            m.level = MotionLevel::Off;
        } else if std::env::var_os("BLUMI_REDUCED_MOTION").is_some() {
            m.level = MotionLevel::Reduced;
        }
        m
    }

    pub fn level(&self) -> MotionLevel {
        self.level
    }

    /// Set the motion level (the `/motion` command). Cancels any live effect when
    /// turning off.
    pub fn set_level(&mut self, level: MotionLevel) {
        self.level = level;
        if level == MotionLevel::Off {
            self.active_until = None;
            self.mgr = EffectManager::default();
        }
    }

    /// Whether an effect is currently animating (drives redraws).
    pub fn is_active(&self) -> bool {
        self.active_until.is_some_and(|t| Instant::now() < t)
    }

    /// Schedule a short "scene-in" coalesce over the whole frame — the UI
    /// materializes. Used on launch, session/scene change, and theme switch.
    pub fn scene_in(&mut self) {
        let ms = match self.level {
            MotionLevel::Off => return,
            MotionLevel::Reduced => SCENE_MS_REDUCED,
            MotionLevel::Full => SCENE_MS,
        };
        self.mgr.add_effect(fx::coalesce(ms));
        self.active_until = Some(Instant::now() + Duration::from_millis(ms as u64 + 80));
    }

    /// Advance + apply active effects onto `area`. Call last in `view::render`.
    /// A no-op when off or idle (and it resets the clock so the next effect
    /// starts from a fresh delta).
    pub fn process(&mut self, buf: &mut Buffer, area: Rect) {
        if self.level == MotionLevel::Off || !self.is_active() {
            self.last = None;
            return;
        }
        let now = Instant::now();
        let dt = self
            .last
            .replace(now)
            .map(|prev| now.saturating_duration_since(prev))
            .unwrap_or_else(|| Duration::from_millis(16));
        self.mgr.process_effects(dt.into(), buf, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_scene_in_activates() {
        let mut m = Motion::default();
        assert_eq!(m.level(), MotionLevel::Full);
        assert!(!m.is_active());
        m.scene_in();
        assert!(
            m.is_active(),
            "an effect should be live right after scene_in"
        );
    }

    #[test]
    fn off_level_never_activates() {
        let mut m = Motion::default();
        m.set_level(MotionLevel::Off);
        m.scene_in();
        assert!(!m.is_active(), "motion off → no effects scheduled");
    }
}
