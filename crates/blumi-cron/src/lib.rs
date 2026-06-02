//! Scheduler → headless sessions → delivery.
//!
//! This crate is the pure scheduling layer: a [`Schedule`] grammar, a
//! [`CronJob`] model, and a JSON-backed [`CronStore`] that decides which jobs
//! are due. Actually *running* a due job (spinning up a headless agent session
//! and delivering its output) lives in the binary, which has the engine — this
//! crate stays dependency-light and easily testable.

mod schedule;

pub use schedule::{ParseError, Schedule};

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

fn default_deliver() -> String {
    "log".into()
}
fn default_true() -> bool {
    true
}

/// A scheduled automation: run `prompt` on `schedule`, deliver the result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub prompt: String,
    /// Raw schedule string (see [`Schedule::parse`]).
    pub schedule: String,
    /// `"log"` (stdout) or `"file:<path>"` (append).
    #[serde(default = "default_deliver")]
    pub deliver: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// RFC3339 creation time.
    pub created_at: String,
    /// RFC3339 last successful run, if any.
    #[serde(default)]
    pub last_run: Option<String>,
}

impl CronJob {
    pub fn parsed_schedule(&self) -> Result<Schedule, ParseError> {
        Schedule::parse(&self.schedule)
    }

    fn created(&self) -> OffsetDateTime {
        OffsetDateTime::parse(&self.created_at, &Rfc3339).unwrap_or(OffsetDateTime::UNIX_EPOCH)
    }

    fn last(&self) -> Option<OffsetDateTime> {
        self.last_run
            .as_ref()
            .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
    }

    /// The next time this job should run, given `now`.
    pub fn next_run(&self, now: OffsetDateTime) -> Option<OffsetDateTime> {
        let sched = self.parsed_schedule().ok()?;
        match (&sched, self.last()) {
            // A fresh interval job runs at the first opportunity.
            (Schedule::Every(_), None) => Some(now),
            (_, base) => sched.next_after(base.unwrap_or_else(|| self.created())),
        }
    }

    /// Whether the job is enabled and due at `now`.
    pub fn is_due(&self, now: OffsetDateTime) -> bool {
        self.enabled && self.next_run(now).is_some_and(|n| n <= now)
    }
}

/// A JSON-file-backed set of cron jobs.
#[derive(Debug, Default)]
pub struct CronStore {
    path: PathBuf,
    jobs: Vec<CronJob>,
}

impl CronStore {
    /// Load jobs from `path` (missing/invalid file → empty set).
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let jobs = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        CronStore { path, jobs }
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(&self.jobs).unwrap_or_else(|_| "[]".into());
        std::fs::write(&self.path, body)
    }

    pub fn jobs(&self) -> &[CronJob] {
        &self.jobs
    }

    /// Validate the schedule and add a job; returns the new id.
    pub fn add(
        &mut self,
        name: &str,
        schedule: &str,
        prompt: &str,
        deliver: &str,
        now: OffsetDateTime,
    ) -> Result<String, ParseError> {
        Schedule::parse(schedule)?;
        let id = format!("job-{}-{}", now.unix_timestamp(), self.jobs.len());
        self.jobs.push(CronJob {
            id: id.clone(),
            name: name.to_string(),
            prompt: prompt.to_string(),
            schedule: schedule.to_string(),
            deliver: deliver.to_string(),
            enabled: true,
            created_at: now.format(&Rfc3339).unwrap_or_default(),
            last_run: None,
        });
        Ok(id)
    }

    /// Remove by id or name; returns whether anything was removed.
    pub fn remove(&mut self, id_or_name: &str) -> bool {
        let before = self.jobs.len();
        self.jobs
            .retain(|j| j.id != id_or_name && j.name != id_or_name);
        self.jobs.len() != before
    }

    /// All jobs currently due at `now`.
    pub fn due(&self, now: OffsetDateTime) -> Vec<CronJob> {
        self.jobs
            .iter()
            .filter(|j| j.is_due(now))
            .cloned()
            .collect()
    }

    /// Record that a job ran at `when`.
    pub fn mark_run(&mut self, id: &str, when: OffsetDateTime) {
        if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
            j.last_run = when.format(&Rfc3339).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn add_validates_and_lists() {
        let mut store = CronStore::default();
        let now = datetime!(2025-01-01 12:00:00 UTC);
        assert!(store.add("bad", "nonsense", "p", "log", now).is_err());
        let id = store
            .add("digest", "daily 09:00", "summarize", "log", now)
            .unwrap();
        assert_eq!(store.jobs().len(), 1);
        assert!(store.remove(&id));
        assert!(store.jobs().is_empty());
    }

    #[test]
    fn interval_job_due_then_waits() {
        let mut store = CronStore::default();
        let t0 = datetime!(2025-01-01 12:00:00 UTC);
        let id = store.add("poll", "every 1h", "check", "log", t0).unwrap();
        // Fresh interval job is due immediately.
        assert_eq!(store.due(t0).len(), 1);
        store.mark_run(&id, t0);
        // 30 min later: not due; 60 min later: due again.
        assert!(store.due(datetime!(2025-01-01 12:30:00 UTC)).is_empty());
        assert_eq!(store.due(datetime!(2025-01-01 13:00:00 UTC)).len(), 1);
    }

    #[test]
    fn disabled_job_never_due() {
        let mut store = CronStore::default();
        let now = datetime!(2025-01-01 12:00:00 UTC);
        store.add("x", "every 1m", "p", "log", now).unwrap();
        store.jobs[0].enabled = false;
        assert!(store.due(now).is_empty());
    }
}
