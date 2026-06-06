//! A persistent task board with a small state machine, the backbone of the
//! autonomous `blumi loop` and the TUI/CLI work-progress views.
//!
//! Tasks move `todo → doing → review → done` (or `cancelled`); the loop pulls
//! the highest-priority `todo`, runs it, and advances it. The board is a plain
//! JSON file so it's easy to inspect, edit, and commit alongside a project.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// Where a task sits in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Todo,
    Doing,
    Review,
    Done,
    Cancelled,
}

impl TaskState {
    /// A compact status icon (ORCH/ralph style).
    pub fn icon(&self) -> &'static str {
        match self {
            TaskState::Todo => "○",
            TaskState::Doing => "▶",
            TaskState::Review => "→",
            TaskState::Done => "✓",
            TaskState::Cancelled => "✗",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            TaskState::Todo => "todo",
            TaskState::Doing => "doing",
            TaskState::Review => "review",
            TaskState::Done => "done",
            TaskState::Cancelled => "cancelled",
        }
    }

    /// Parse a state name (for the CLI).
    pub fn parse(s: &str) -> Option<TaskState> {
        match s.to_ascii_lowercase().as_str() {
            "todo" => Some(TaskState::Todo),
            "doing" | "start" | "in_progress" => Some(TaskState::Doing),
            "review" => Some(TaskState::Review),
            "done" => Some(TaskState::Done),
            "cancelled" | "cancel" => Some(TaskState::Cancelled),
            _ => None,
        }
    }
}

/// One unit of work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    /// 1 (highest) .. 4 (lowest).
    pub priority: u8,
    pub state: TaskState,
    pub created_at: String,
    pub updated_at: String,
    /// Grid peer currently executing this task (display name). `None` = local /
    /// unassigned. Backward-compatible: absent in older boards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    /// Tokens billed to this task across every turn it ran (0 until run).
    /// Backward-compatible: absent in older boards.
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    /// Estimated USD cost from the model's list price; `None` for unpriced/local
    /// models or a task that hasn't run (or ran on a remote peer in v1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// Counts by state, for progress summaries.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub todo: usize,
    pub doing: usize,
    pub review: usize,
    pub done: usize,
    pub cancelled: usize,
}

/// The board: a JSON file of tasks.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TaskBoard {
    #[serde(default)]
    tasks: Vec<Task>,
    #[serde(skip)]
    path: PathBuf,
}

impl TaskBoard {
    /// Load the board from `path` (empty board if missing/invalid).
    pub fn load(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let mut board: TaskBoard = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        board.path = path;
        board
    }

    /// Persist atomically.
    pub fn save(&self) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, body.as_bytes())?;
        std::fs::rename(&tmp, &self.path)
    }

    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Add a task; returns its id. Priority is clamped to 1..=4.
    pub fn add(&mut self, title: &str, detail: &str, priority: u8, now: OffsetDateTime) -> String {
        let ts = now.format(&Rfc3339).unwrap_or_default();
        let id = format!("t{}-{}", now.unix_timestamp(), self.tasks.len() + 1);
        self.tasks.push(Task {
            id: id.clone(),
            title: title.to_string(),
            detail: detail.to_string(),
            priority: priority.clamp(1, 4),
            state: TaskState::Todo,
            created_at: ts.clone(),
            updated_at: ts,
            owner: None,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: None,
        });
        id
    }

    /// Resolve a task by exact id or 1-based position (as shown in `list`).
    fn index_of(&self, id_or_pos: &str) -> Option<usize> {
        if let Some(i) = self.tasks.iter().position(|t| t.id == id_or_pos) {
            return Some(i);
        }
        id_or_pos
            .parse::<usize>()
            .ok()
            .filter(|n| *n >= 1 && *n <= self.tasks.len())
            .map(|n| n - 1)
    }

    /// Set a task's state; returns the updated task's title, or `None` if not found.
    pub fn set_state(
        &mut self,
        id_or_pos: &str,
        state: TaskState,
        now: OffsetDateTime,
    ) -> Option<String> {
        let i = self.index_of(id_or_pos)?;
        self.tasks[i].state = state;
        self.tasks[i].updated_at = now.format(&Rfc3339).unwrap_or_default();
        Some(self.tasks[i].title.clone())
    }

    /// Like [`Self::set_state`] but stamps the current time (convenience for
    /// callers that don't depend on `time`).
    pub fn set_state_now(&mut self, id_or_pos: &str, state: TaskState) -> Option<String> {
        self.set_state(id_or_pos, state, OffsetDateTime::now_utc())
    }

    /// Set (or clear) the grid peer executing a task. Returns the task's title,
    /// or `None` if not found.
    pub fn set_owner(&mut self, id_or_pos: &str, owner: Option<String>) -> Option<String> {
        let i = self.index_of(id_or_pos)?;
        self.tasks[i].owner = owner;
        self.tasks[i].updated_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default();
        Some(self.tasks[i].title.clone())
    }

    /// Accumulate a token delta on a task and recompute its USD estimate from
    /// `model`'s list price (`None` for unpriced/local models). A task may run
    /// across several turns; deltas accumulate. Returns the task title.
    pub fn record_cost(
        &mut self,
        id_or_pos: &str,
        model: &str,
        input: u64,
        output: u64,
    ) -> Option<String> {
        let i = self.index_of(id_or_pos)?;
        let t = &mut self.tasks[i];
        t.input_tokens += input;
        t.output_tokens += output;
        t.cost_usd = blumi_config::pricing::is_priced(model)
            .then(|| blumi_config::pricing::estimate(model, t.input_tokens, t.output_tokens));
        t.updated_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default();
        Some(t.title.clone())
    }

    /// Remove a task; returns whether anything was removed.
    pub fn remove(&mut self, id_or_pos: &str) -> bool {
        match self.index_of(id_or_pos) {
            Some(i) => {
                self.tasks.remove(i);
                true
            }
            None => false,
        }
    }

    /// The next task to work: highest priority (lowest number) `todo`, oldest first.
    pub fn next_todo(&self) -> Option<&Task> {
        self.tasks
            .iter()
            .filter(|t| t.state == TaskState::Todo)
            .min_by(|a, b| {
                a.priority
                    .cmp(&b.priority)
                    .then(a.created_at.cmp(&b.created_at))
            })
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for t in &self.tasks {
            match t.state {
                TaskState::Todo => c.todo += 1,
                TaskState::Doing => c.doing += 1,
                TaskState::Review => c.review += 1,
                TaskState::Done => c.done += 1,
                TaskState::Cancelled => c.cancelled += 1,
            }
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
    }

    #[test]
    fn add_lists_and_counts() {
        let mut b = TaskBoard::default();
        b.add("ship parser", "", 2, now());
        b.add("fix bug", "", 1, now());
        assert_eq!(b.tasks().len(), 2);
        let c = b.counts();
        assert_eq!(c.todo, 2);
        // next_todo picks priority 1 ("fix bug").
        assert_eq!(b.next_todo().unwrap().title, "fix bug");
    }

    #[test]
    fn state_transitions_and_resolve_by_position() {
        let mut b = TaskBoard::default();
        let id = b.add("task", "", 3, now());
        assert_eq!(
            b.set_state(&id, TaskState::Doing, now()).as_deref(),
            Some("task")
        );
        // resolve by 1-based position too
        assert_eq!(
            b.set_state("1", TaskState::Done, now()).as_deref(),
            Some("task")
        );
        assert_eq!(b.counts().done, 1);
        assert!(b.next_todo().is_none());
    }

    #[test]
    fn priority_is_clamped() {
        let mut b = TaskBoard::default();
        b.add("x", "", 9, now());
        assert_eq!(b.tasks()[0].priority, 4);
    }

    #[test]
    fn remove_and_state_parse() {
        let mut b = TaskBoard::default();
        b.add("x", "", 1, now());
        assert!(b.remove("1"));
        assert!(b.is_empty());
        assert_eq!(TaskState::parse("start"), Some(TaskState::Doing));
        assert_eq!(TaskState::parse("nope"), None);
    }

    #[test]
    fn record_cost_accumulates_and_prices() {
        let mut b = TaskBoard::default();
        let id = b.add("run", "", 1, now());
        // 1M in + 1M out of sonnet = $3 + $15 = $18.
        b.record_cost(&id, "claude-sonnet-4-5", 1_000_000, 1_000_000);
        let t = &b.tasks()[0];
        assert_eq!(t.input_tokens, 1_000_000);
        assert!((t.cost_usd.unwrap() - 18.0).abs() < 1e-9);
        // A second turn accumulates tokens + cost.
        b.record_cost(&id, "claude-sonnet-4-5", 1_000_000, 0);
        let t = &b.tasks()[0];
        assert_eq!(t.input_tokens, 2_000_000);
        assert!((t.cost_usd.unwrap() - 21.0).abs() < 1e-9);
    }

    #[test]
    fn record_cost_unpriced_model_is_none() {
        let mut b = TaskBoard::default();
        let id = b.add("run", "", 1, now());
        b.record_cost(&id, "some-local-llama", 1_000_000, 1_000_000);
        let t = &b.tasks()[0];
        assert_eq!(t.output_tokens, 1_000_000); // tokens still tracked
        assert!(t.cost_usd.is_none()); // but no misleading $
    }

    #[test]
    fn old_board_json_without_cost_fields_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        // An older board JSON with no token/cost fields.
        std::fs::write(
            &path,
            r#"{"tasks":[{"id":"t1","title":"old","priority":1,"state":"todo","created_at":"x","updated_at":"x"}]}"#,
        )
        .unwrap();
        let b = TaskBoard::load(&path);
        assert_eq!(b.tasks().len(), 1);
        assert_eq!(b.tasks()[0].input_tokens, 0);
        assert!(b.tasks()[0].cost_usd.is_none());
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");
        let mut b = TaskBoard::load(&path);
        b.add("persisted", "with detail", 1, now());
        b.save().unwrap();
        let b2 = TaskBoard::load(&path);
        assert_eq!(b2.tasks().len(), 1);
        assert_eq!(b2.tasks()[0].title, "persisted");
        assert_eq!(b2.tasks()[0].detail, "with detail");
    }
}
