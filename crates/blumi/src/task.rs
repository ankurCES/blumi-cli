//! `blumi task` — manage the persistent task board the autonomous loop runs on.

use blumi_config::BlumiConfig;
use blumi_task::{TaskBoard, TaskState};
use std::path::PathBuf;
use time::OffsetDateTime;

/// The board lives next to the project (`<cwd>/.blumi/tasks.json`).
pub fn board_path(config: &BlumiConfig) -> PathBuf {
    config.paths.working_dir.join(".blumi").join("tasks.json")
}

pub fn add(
    config: BlumiConfig,
    title: String,
    priority: u8,
    detail: Option<String>,
) -> anyhow::Result<()> {
    if title.trim().is_empty() {
        anyhow::bail!("a task needs a title");
    }
    let mut board = TaskBoard::load(board_path(&config));
    let id = board.add(
        title.trim(),
        detail.as_deref().unwrap_or("").trim(),
        priority,
        OffsetDateTime::now_utc(),
    );
    board.save()?;
    println!("✿ added {id} (P{}): {}", priority.clamp(1, 4), title.trim());
    Ok(())
}

pub fn list(config: BlumiConfig) -> anyhow::Result<()> {
    let board = TaskBoard::load(board_path(&config));
    print_board(&board);
    Ok(())
}

/// Shared rendering used by `list` (and handy for tests/snapshots).
pub fn print_board(board: &TaskBoard) {
    if board.is_empty() {
        println!("no tasks yet — add one:  blumi task add \"build the parser\" -p 1");
        return;
    }
    let c = board.counts();
    let mut summary = format!(
        "tasks — running {} · queued {} · review {} · done {}",
        c.doing, c.todo, c.review, c.done
    );
    if c.cancelled > 0 {
        summary.push_str(&format!(" · cancelled {}", c.cancelled));
    }
    println!("{summary}\n");
    for (i, t) in board.tasks().iter().enumerate() {
        println!(
            "  {:>2}. {} P{}  {}   ({}, {})",
            i + 1,
            t.state.icon(),
            t.priority,
            t.title,
            t.state.label(),
            t.id
        );
    }
}

pub fn transition(config: BlumiConfig, id: String, state: TaskState) -> anyhow::Result<()> {
    let mut board = TaskBoard::load(board_path(&config));
    match board.set_state(&id, state, OffsetDateTime::now_utc()) {
        Some(title) => {
            board.save()?;
            println!("{} {} → {}: {title}", state.icon(), id, state.label());
        }
        None => println!("no task '{id}' (use `blumi task list`)"),
    }
    Ok(())
}

pub fn remove(config: BlumiConfig, id: String) -> anyhow::Result<()> {
    let mut board = TaskBoard::load(board_path(&config));
    if board.remove(&id) {
        board.save()?;
        println!("removed {id}");
    } else {
        println!("no task '{id}' (use `blumi task list`)");
    }
    Ok(())
}
