//! Message handling: keys, terminal events, and core events.

use crate::commands;
use crate::dialog::{Action, Picker};
use crate::model::{Entry, Focus, Mode, Model, Msg, PendingApproval, PlanReview};
use blumi_core::SessionHandle;
use blumi_protocol::{ApprovalScope, Command, Decision, Event};
use crossterm::event::{Event as TermEvent, KeyCode, KeyEventKind, KeyModifiers};

pub async fn update(model: &mut Model, msg: Msg, session: &SessionHandle) {
    match msg {
        Msg::Tick => {
            model.spinner_frame = model.spinner_frame.wrapping_add(1);
            model.clear_stale_chord();
            // Keep redrawing while a motion effect is animating (idle otherwise).
            if model.motion.is_active() {
                model.mark_dirty();
            }
            tick_tool_charms(model);
            if model.busy {
                // Accumulate active-with-bot time (tick is ~50ms).
                model.active_ms += 50;
                model.mark_dirty();
            } else if model.is_empty() {
                model.mark_dirty(); // animate the landing rose
            } else if model.spinner_frame % 6 == 0 {
                // ~3fps idle refresh so the uptime/active timers + live dot update.
                model.mark_dirty();
            }
        }
        Msg::Term(ev) => handle_term(model, ev, session).await,
        Msg::Core(env) => {
            handle_core(model, env.event, session).await;
            model.mark_dirty();
        }
        Msg::Bg(u) => {
            model.bg_count = model.bg_count.saturating_sub(1);
            let head = if u.ok { "done" } else { "failed" };
            let body = u.text.trim();
            let body = if body.is_empty() {
                "(no output)".to_string()
            } else if body.chars().count() > 1500 {
                format!("{}…", body.chars().take(1500).collect::<String>())
            } else {
                body.to_string()
            };
            model
                .entries
                .push(Entry::Notice(format!("⬢ [{}] {head}\n{body}", u.id)));
            model.mark_dirty();
        }
    }
}

/// A rotating long-run charm (hermes-style), changing every ~10s.
fn charm_text(secs: u64) -> &'static str {
    const CHARMS: [&str; 4] = [
        "🍵 still cooking…",
        "✨ polishing edges…",
        "🔮 asking the void nicely…",
        "⏳ almost there…",
    ];
    CHARMS[((secs / 10) as usize) % CHARMS.len()]
}

/// Per-tool long-run reassurance: for any tool running ≥8s, post a charm every
/// 10s, capped at 2 per tool (hermes `useLongRunToolCharms`).
fn tick_tool_charms(model: &mut Model) {
    let mut fire: Vec<(String, String, u64)> = Vec::new();
    for (id, run) in model.running_tools.iter() {
        let secs = run.started.elapsed().as_secs();
        if run.charms < 2 && secs >= 8 + run.charms as u64 * 10 {
            fire.push((id.clone(), run.name.clone(), secs));
        }
    }
    for (id, name, secs) in fire {
        if let Some(run) = model.running_tools.get_mut(&id) {
            run.charms += 1;
        }
        model.entries.push(Entry::Notice(format!(
            "{} ({name} · {secs}s)",
            charm_text(secs)
        )));
        model.mark_dirty();
    }
}

async fn handle_term(model: &mut Model, ev: TermEvent, session: &SessionHandle) {
    match ev {
        TermEvent::Resize(..) => model.mark_dirty(),
        TermEvent::Paste(s) => {
            model.input.insert_str(&s);
            model.mark_dirty();
        }
        TermEvent::Key(key) if key.kind != KeyEventKind::Release => {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

            // Ctrl+C always quits.
            if ctrl && matches!(key.code, KeyCode::Char('c')) {
                model.should_quit = true;
                return;
            }

            // Overlays close on any key.
            if model.memory_view.is_some() {
                model.memory_view = None;
                model.mark_dirty();
                return;
            }
            if model.usage_view.is_some() {
                model.usage_view = None;
                model.mark_dirty();
                return;
            }
            if model.board_view.is_some() {
                model.board_view = None;
                model.mark_dirty();
                return;
            }
            if model.grid_view.is_some() {
                model.grid_view = None;
                model.mark_dirty();
                return;
            }

            // The /dashboard + /help modals: scrollable overlays; esc/q closes.
            if model.dash_modal || model.help_modal {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {
                        model.dash_modal = false;
                        model.help_modal = false;
                    }
                    KeyCode::Up | KeyCode::Char('k') => model.modal_pane.scroll_by(-1),
                    KeyCode::Down | KeyCode::Char('j') => model.modal_pane.scroll_by(1),
                    KeyCode::PageUp => model.modal_pane.scroll_by(-10),
                    KeyCode::PageDown => model.modal_pane.scroll_by(10),
                    KeyCode::Home | KeyCode::Char('g') => model.modal_pane.scroll_by(isize::MIN),
                    KeyCode::End | KeyCode::Char('G') => model.modal_pane.scroll_by(isize::MAX),
                    _ => {}
                }
                model.mark_dirty();
                return;
            }

            // A dialog (command palette) captures all keys.
            if model.dialog.is_some() {
                handle_dialog_key(model, key.code, session).await;
                model.mark_dirty();
                return;
            }

            // Capturing an API key for a provider switch.
            if model.provider_key_prompt.is_some() {
                match key.code {
                    KeyCode::Enter => {
                        let api_key = model.input_text();
                        let provider = model.provider_key_prompt.take().unwrap_or_default();
                        model.clear_input(); // a fresh editor drops the mask
                        if api_key.trim().is_empty() {
                            model
                                .entries
                                .push(Entry::Notice("provider switch cancelled (no key)".into()));
                        } else {
                            model.request_provider(provider, Some(api_key));
                        }
                    }
                    KeyCode::Esc => model.cancel_key_prompt(),
                    _ => {
                        model.input.input(key);
                    }
                }
                model.mark_dirty();
                return;
            }

            // A plan-review modal captures all keys (scroll + approve/reject).
            if model.plan_review.is_some() {
                handle_plan_key(model, key, session).await;
                model.mark_dirty();
                return;
            }

            // A pending approval captures all keys.
            if model.pending.is_some() {
                handle_approval_key(model, key.code, session).await;
                model.mark_dirty();
                return;
            }

            // Ctrl+P opens the command palette.
            if ctrl && matches!(key.code, KeyCode::Char('p')) {
                model.dialog = Some(Picker::command_palette());
                model.mark_dirty();
                return;
            }

            // Ctrl+S opens the session switcher.
            if ctrl && matches!(key.code, KeyCode::Char('s')) {
                model.dialog = Some(Picker::session_picker(
                    &model.recent_sessions,
                    &model.remotes,
                ));
                model.mark_dirty();
                return;
            }

            // Ctrl+Y toggles yolo (auto-approve / skip permissions), crush-style.
            if ctrl && matches!(key.code, KeyCode::Char('y')) {
                commands::toggle_yolo(model, session).await;
                return;
            }

            // Ctrl+B / Ctrl+J collapse-toggle the left explorer / right agent rails.
            if ctrl && matches!(key.code, KeyCode::Char('b')) {
                model.toggle_explorer();
                return;
            }
            if ctrl && matches!(key.code, KeyCode::Char('j')) {
                model.toggle_dashboard();
                return;
            }

            // Slash-command popup navigation (while typing a "/..." command).
            if model.slash_active() && handle_slash_key(model, key, session).await {
                model.mark_dirty();
                return;
            }

            // Left explorer sidebar focus: the active tab's list captures nav;
            // ←/→ (or [/]) switch tabs, Enter activates.
            if model.focus == Focus::Sidebar {
                match key.code {
                    KeyCode::Tab => model.focus = next_focus(model),
                    KeyCode::Esc => model.focus = Focus::Editor,
                    KeyCode::Up | KeyCode::Char('k') => model.sidebar_move(-1),
                    KeyCode::Down | KeyCode::Char('j') => model.sidebar_move(1),
                    KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Char('h')
                    | KeyCode::Char('l')
                    | KeyCode::Char('[')
                    | KeyCode::Char(']') => model.toggle_sidebar_tab(),
                    KeyCode::Enter => model.sidebar_activate(),
                    _ => {}
                }
                model.mark_dirty();
                return;
            }

            // Right dashboard focus: ←/→ pick the sub-panel (agents / tasks),
            // the rest scroll the selected one independently.
            if model.focus == Focus::Dashboard {
                match key.code {
                    KeyCode::Tab => model.focus = next_focus(model),
                    KeyCode::Esc => model.focus = Focus::Editor,
                    KeyCode::Left
                    | KeyCode::Right
                    | KeyCode::Char('h')
                    | KeyCode::Char('l')
                    | KeyCode::Char('[')
                    | KeyCode::Char(']') => model.cycle_dash_panel(),
                    KeyCode::Up | KeyCode::Char('k') => model.scroll_dashboard(-1),
                    KeyCode::Down | KeyCode::Char('j') => model.scroll_dashboard(1),
                    KeyCode::PageUp => model.scroll_dashboard(-10),
                    KeyCode::PageDown => model.scroll_dashboard(10),
                    KeyCode::Home | KeyCode::Char('g') => model.scroll_dashboard(isize::MIN),
                    KeyCode::End | KeyCode::Char('G') => model.scroll_dashboard(isize::MAX),
                    _ => {}
                }
                model.mark_dirty();
                return;
            }

            // Nav mode (Focus::Editor): drive the transcript with vim keys without
            // the editor capturing text; any printable / i / Enter returns to Insert.
            if model.focus == Focus::Editor && model.mode == Mode::Nav {
                match key.code {
                    KeyCode::Tab => model.focus = next_focus(model),
                    KeyCode::Char('j') | KeyCode::Down => {
                        model.scrollback = model.scrollback.saturating_sub(1)
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        model.scrollback = model.scrollback.saturating_add(1)
                    }
                    KeyCode::PageDown => model.scrollback = model.scrollback.saturating_sub(5),
                    KeyCode::PageUp => model.scrollback = model.scrollback.saturating_add(5),
                    KeyCode::Char('g') if model.chord('g') => {
                        model.scrollback = u16::MAX; // gg → oldest
                    }
                    KeyCode::Char('g') => {} // first g of a chord; wait for the second
                    KeyCode::Char('G') => model.scrollback = 0, // latest
                    KeyCode::Char('i') | KeyCode::Enter => model.set_mode(Mode::Insert),
                    KeyCode::Esc => {}
                    KeyCode::Char(c) => {
                        model.set_mode(Mode::Insert);
                        model.input.insert_char(c);
                    }
                    _ => {}
                }
                model.mark_dirty();
                return;
            }

            match key.code {
                KeyCode::Esc => {
                    if model.busy {
                        let _ = session.send(Command::Cancel).await;
                    } else if !model.input_text().is_empty() {
                        model.clear_input();
                    } else {
                        // Empty + idle editor: drop into Nav mode (vim-style).
                        model.set_mode(crate::model::Mode::Nav);
                    }
                }
                KeyCode::Tab => model.focus = next_focus(model),
                KeyCode::Enter
                    if key
                        .modifiers
                        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
                {
                    model.input.insert_newline();
                }
                KeyCode::Enter => {
                    let text = model.input_text();
                    let trimmed = text.trim().to_string();
                    if trimmed.is_empty() {
                        // nothing to do
                    } else if trimmed.starts_with('/') {
                        commands::run(model, session, &trimmed).await;
                    } else if !model.busy {
                        send_message(model, session, text).await;
                    }
                }
                KeyCode::Up if model.focus == Focus::Editor => history_prev(model),
                KeyCode::Down if model.focus == Focus::Editor => history_next(model),
                KeyCode::PageUp => model.scrollback = model.scrollback.saturating_add(5),
                KeyCode::PageDown => model.scrollback = model.scrollback.saturating_sub(5),
                _ => {
                    if model.focus == Focus::Editor {
                        model.input.input(key);
                        if model.slash_active() {
                            model.slash_sel = 0;
                        }
                    } else {
                        match key.code {
                            KeyCode::Char('k') | KeyCode::Up => {
                                model.scrollback = model.scrollback.saturating_add(1)
                            }
                            KeyCode::Char('j') | KeyCode::Down => {
                                model.scrollback = model.scrollback.saturating_sub(1)
                            }
                            _ => {}
                        }
                    }
                }
            }
            model.mark_dirty();
        }
        TermEvent::Mouse(me) => {
            use crossterm::event::MouseEventKind;
            match me.kind {
                MouseEventKind::ScrollUp => {
                    scroll_panes(model, me.column, me.row, -1);
                    model.mark_dirty();
                }
                MouseEventKind::ScrollDown => {
                    scroll_panes(model, me.column, me.row, 1);
                    model.mark_dirty();
                }
                // Click a picker row to select + activate it (menu-style).
                MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                    // Header tab strip: click a chip to switch to that tab.
                    let tab_hit = model
                        .header_tab_areas
                        .iter()
                        .find(|&&(x, y, w, _)| me.column >= x && me.column < x + w && me.row == y)
                        .map(|&(_, _, _, idx)| idx);
                    if let Some(idx) = tab_hit {
                        model.request_tab(idx);
                        model.mark_dirty();
                        return;
                    }
                    let row = model.dialog_list_area.and_then(|(x, y, w, h)| {
                        let hit = model.dialog.is_some()
                            && me.column >= x
                            && me.column < x + w
                            && me.row >= y
                            && me.row < y + h;
                        hit.then(|| (me.row - y) as usize)
                    });
                    if let Some(row) = row {
                        let action = model.dialog.as_mut().and_then(|d| {
                            (row < d.filtered.len()).then(|| {
                                d.selected = row;
                                d.selected_action()
                            })?
                        });
                        if let Some(a) = action {
                            model.dialog = None;
                            perform_action(model, a, session).await;
                        }
                        model.mark_dirty();
                    } else if model.dialog.is_none() {
                        let inside = |a: Option<(u16, u16, u16, u16)>| {
                            a.is_some_and(|(x, y, w, h)| {
                                me.column >= x && me.column < x + w && me.row >= y && me.row < y + h
                            })
                        };
                        // Rail title rows collapse the rail; the editor focuses +
                        // returns to Insert; dashboard sub-panels focus for scroll;
                        // else fall through to a sidebar click.
                        if inside(model.explorer_title_area) {
                            model.toggle_explorer();
                        } else if inside(model.agent_title_area) {
                            model.toggle_dashboard();
                        } else if model.agents_pane.hit(me.column, me.row) {
                            model.focus = Focus::Dashboard;
                            model.dash_panel = crate::model::DashPanel::Agents;
                            model.mark_dirty();
                        } else if model.tasks_pane.hit(me.column, me.row) {
                            model.focus = Focus::Dashboard;
                            model.dash_panel = crate::model::DashPanel::Tasks;
                            model.mark_dirty();
                        } else if inside(model.editor_area) {
                            model.focus = Focus::Editor;
                            model.set_mode(crate::model::Mode::Insert);
                            model.mark_dirty();
                        } else {
                            sidebar_click(model, me.column, me.row);
                        }
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

/// Handle a key while the slash-command popup is open. Returns true if it was
/// consumed (navigation / run / cancel); false to let normal editing proceed.
async fn handle_slash_key(
    model: &mut Model,
    key: crossterm::event::KeyEvent,
    session: &SessionHandle,
) -> bool {
    match key.code {
        KeyCode::Up => {
            model.slash_sel = model.slash_sel.saturating_sub(1);
            true
        }
        KeyCode::Down => {
            let n = commands::matching(&model.input_text()).len();
            if n > 0 && model.slash_sel + 1 < n {
                model.slash_sel += 1;
            }
            true
        }
        KeyCode::Tab => {
            let typed = model.input_text();
            let matches = commands::matching(&typed);
            if let Some(c) = matches.get(model.slash_sel).or_else(|| matches.first()) {
                model.set_input(&format!("{} ", c.name));
            }
            true
        }
        KeyCode::Enter
            if !key
                .modifiers
                .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
        {
            let typed = model.input_text();
            // A bare command word → run the highlighted match; with args → run as typed.
            let line = if typed.split_whitespace().count() <= 1 {
                commands::matching(&typed)
                    .get(model.slash_sel)
                    .map(|c| c.name.to_string())
                    .unwrap_or(typed)
            } else {
                typed
            };
            commands::run(model, session, &line).await;
            true
        }
        KeyCode::Esc => {
            model.clear_input();
            true
        }
        _ => false,
    }
}

async fn send_message(model: &mut Model, session: &SessionHandle, text: String) {
    model.entries.push(Entry::User(text.clone()));
    model.history.push(text.clone());
    model.history_pos = None;
    model.draft.clear();
    model.clear_input();
    model.busy = true;
    model.busy_since = Some(std::time::Instant::now());
    model.scrollback = 0;
    let _ = session
        .send(Command::UserMessage {
            text,
            attachments: vec![],
            stream_id: None,
        })
        .await;
}

async fn handle_approval_key(model: &mut Model, code: KeyCode, session: &SessionHandle) {
    let decision = match code {
        KeyCode::Char('a') => Some((Decision::Allow, ApprovalScope::Once)),
        KeyCode::Char('s') => Some((Decision::Allow, ApprovalScope::Session)),
        KeyCode::Char('d') | KeyCode::Char('n') | KeyCode::Esc => {
            Some((Decision::Deny, ApprovalScope::Once))
        }
        _ => None,
    };
    if let Some((decision, scope)) = decision {
        if let Some(p) = model.pending.take() {
            let _ = session
                .send(Command::ApproveTool {
                    request_id: p.request_id,
                    decision,
                    scope,
                })
                .await;
        }
    }
}

/// Keys while the plan-review modal is open: scroll, or approve/reject.
async fn handle_plan_key(
    model: &mut Model,
    key: crossterm::event::KeyEvent,
    session: &SessionHandle,
) {
    match key.code {
        KeyCode::Char('a') | KeyCode::Enter => resolve_plan(model, session, true).await,
        KeyCode::Char('d') | KeyCode::Char('r') | KeyCode::Esc => {
            resolve_plan(model, session, false).await
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let Some(p) = model.plan_review.as_mut() {
                p.scroll = p.scroll.saturating_sub(1);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let Some(p) = model.plan_review.as_mut() {
                p.scroll = p.scroll.saturating_add(1);
            }
        }
        KeyCode::PageUp => {
            if let Some(p) = model.plan_review.as_mut() {
                p.scroll = p.scroll.saturating_sub(10);
            }
        }
        KeyCode::PageDown => {
            if let Some(p) = model.plan_review.as_mut() {
                p.scroll = p.scroll.saturating_add(10);
            }
        }
        _ => {}
    }
}

/// Resolve a plan review: approve (proceed) or reject (revise). On approve the
/// core also exits plan mode; we mirror the flag locally for the indicator.
async fn resolve_plan(model: &mut Model, session: &SessionHandle, approve: bool) {
    let Some(p) = model.plan_review.take() else {
        return;
    };
    let decision = if approve {
        Decision::Allow
    } else {
        Decision::Deny
    };
    let _ = session
        .send(Command::ApproveTool {
            request_id: p.request_id,
            decision,
            scope: ApprovalScope::Once,
        })
        .await;
    if approve {
        model.plan_mode = false;
        model
            .entries
            .push(Entry::Notice("✓ plan approved — proceeding".into()));
    } else {
        model.entries.push(Entry::Notice(
            "✗ plan rejected — still in plan mode; tell blumi what to change".into(),
        ));
    }
}

async fn handle_core(model: &mut Model, event: Event, session: &SessionHandle) {
    match event {
        Event::AssistantStarted { .. } => model.streaming = Some(String::new()),
        Event::Token { text } => {
            model
                .streaming
                .get_or_insert_with(String::new)
                .push_str(&text);
            model.scrollback = 0;
        }
        Event::Thinking { text } => {
            model
                .thinking
                .get_or_insert_with(String::new)
                .push_str(&text);
        }
        Event::AssistantFinished { .. } => commit_streaming(model),
        Event::ToolStart {
            id, name, summary, ..
        } => {
            // Track the running tool so long ones get "still working" charms.
            model.running_tools.insert(
                id.as_str().to_string(),
                crate::model::ToolRun {
                    started: std::time::Instant::now(),
                    name: name.clone(),
                    charms: 0,
                },
            );
            model.entries.push(Entry::Tool {
                id,
                name,
                summary,
                ok: None,
                preview: None,
                diff_stat: None,
                diff: None,
            });
        }
        Event::ToolResult {
            id, ok, preview, ..
        } => {
            model.running_tools.remove(id.as_str());
            if let Some(Entry::Tool {
                ok: o, preview: p, ..
            }) = find_tool(model, &id)
            {
                *o = Some(ok);
                *p = Some(first_line(&preview));
            }
        }
        Event::Diff {
            id,
            unified,
            additions,
            deletions,
            ..
        } => {
            if let Some(Entry::Tool {
                diff_stat, diff, ..
            }) = find_tool(model, &id)
            {
                *diff_stat = Some(format!("+{additions} -{deletions}"));
                *diff = Some(unified);
            }
        }
        Event::ApprovalRequest {
            request_id,
            tool,
            summary,
            dangerous,
            diff,
            advice,
        } => {
            model.pending = Some(PendingApproval {
                request_id,
                tool,
                summary,
                dangerous,
                diff,
                advice,
            });
        }
        Event::PlanReview { request_id, plan } => {
            model.plan_review = Some(PlanReview {
                request_id,
                plan,
                scroll: 0,
            });
        }
        Event::ClarifyRequest {
            request_id,
            question,
            ..
        } => {
            model
                .entries
                .push(Entry::Notice(format!("clarify: {question}")));
            let _ = session
                .send(Command::AnswerClarify {
                    request_id,
                    value: String::new(),
                })
                .await;
        }
        Event::AgentStart {
            id,
            agent_type,
            task,
        } => model.agent_started(id, agent_type, task),
        Event::AgentDone {
            id, ok, summary, ..
        } => model.agent_finished(&id, ok, summary),
        Event::TodoUpdate { items } => model.todos = items,
        Event::Usage {
            input,
            output,
            context,
            ..
        } => {
            // `context` = the full prompt (uncached input + cache read + write).
            // Display that as input so the ↑ meter isn't ~0 once prompt caching
            // kicks in (`input` alone counts only the uncached remainder). Cost
            // still uses the billed (uncached) `input`.
            let prompt = if context > 0 { context } else { input };
            model.input_tokens += prompt;
            model.output_tokens += output;
            model.cost_usd += crate::cost::estimate(&model.model_name, input, output);
            model.context_tokens = prompt;
        }
        Event::Compaction {
            messages_compressed,
            tokens_after,
            ..
        } => {
            // Reset the context meter immediately to the post-compaction size
            // (otherwise it stays pinned near 100% until the next request).
            if tokens_after > 0 {
                model.context_tokens = tokens_after;
            }
            model.entries.push(Entry::Notice(format!(
                "context compacted ({messages_compressed} messages)"
            )));
        }
        Event::TurnDone { .. } => {
            commit_streaming(model);
            model.thinking = None;
            model.busy = false;
            model.busy_since = None;
            model.running_tools.clear();
            model.turn_count += 1;
        }
        Event::Notice { message } => {
            model.entries.push(Entry::Notice(message));
        }
        Event::Reload { reason } => {
            // The agent asked to reload itself; the app loop performs the
            // in-place rebuild once the turn goes idle (keeps the transcript).
            model.request_reload(reason);
        }
        Event::Error { message, .. } => {
            model
                .entries
                .push(Entry::Notice(format!("error: {message}")));
        }
        _ => {}
    }
}

fn commit_streaming(model: &mut Model) {
    if let Some(s) = model.streaming.take() {
        if !s.trim().is_empty() {
            model.entries.push(Entry::Assistant(s));
        }
    }
}

fn find_tool<'a>(model: &'a mut Model, id: &blumi_protocol::ToolCallId) -> Option<&'a mut Entry> {
    model
        .entries
        .iter_mut()
        .rev()
        .find(|e| matches!(e, Entry::Tool { id: tid, .. } if tid == id))
}

/// Route a mouse-wheel tick to whichever pane the cursor is over, so each side
/// pane scrolls on its own. `dir` is -1 for up, +1 for down. Overlays (plan /
/// dialog) capture the wheel wherever the cursor is.
fn scroll_panes(model: &mut Model, col: u16, row: u16, dir: i32) {
    let step = (dir * 3) as isize;
    // The /dashboard modal captures the wheel while open.
    if model.dash_modal {
        model.modal_pane.scroll_by(step);
        return;
    }
    if let Some(p) = model.plan_review.as_mut() {
        p.scroll = if dir < 0 {
            p.scroll.saturating_sub(3)
        } else {
            p.scroll.saturating_add(3)
        };
        return;
    }
    if let Some(d) = model.dialog.as_mut() {
        if dir < 0 {
            d.move_up();
        } else {
            d.move_down();
        }
        return;
    }
    // Each dashboard sub-panel scrolls on its own when hovered.
    if model.agents_pane.hit(col, row) {
        model.agents_pane.scroll_by(step);
    } else if model.tasks_pane.hit(col, row) {
        model.tasks_pane.scroll_by(step);
    } else if model
        .sidebar_list_area
        .is_some_and(|(x, y, w, h)| col >= x && col < x + w && row >= y && row < y + h)
    {
        // Left explorer pane: scroll the active list via its selection.
        model.sidebar_move(dir as isize * 3);
    } else {
        // Default: the chat transcript.
        model.scrollback = if dir < 0 {
            model.scrollback.saturating_add(3)
        } else {
            model.scrollback.saturating_sub(3)
        };
    }
}

/// Map a left-click in a sidebar list to a row index (accounting for the same
/// bottom-anchored scroll window the renderer uses).
fn list_click_index(
    area: Option<(u16, u16, u16, u16)>,
    sel: usize,
    len: usize,
    col: u16,
    row: u16,
) -> Option<usize> {
    let (x, y, w, h) = area?;
    if len == 0 || col < x || col >= x + w || row < y || row >= y + h {
        return None;
    }
    let h = h as usize;
    let sel = sel.min(len - 1);
    let start = sel.saturating_sub(h.saturating_sub(1));
    let idx = start + (row - y) as usize;
    (idx < len).then_some(idx)
}

/// Handle a left-click in the left explorer sidebar: the tab row switches tabs;
/// a list row selects + activates.
fn sidebar_click(model: &mut Model, col: u16, row: u16) {
    use crate::model::SidebarTab;
    // Tab bar (2 rows): a click anywhere on it focuses the explorer and cycles to
    // the next tab.
    if let Some((x, y, w, h)) = model.sidebar_tab_area {
        if col >= x && col < x + w && row >= y && row < y + h {
            model.focus = Focus::Sidebar;
            model.toggle_sidebar_tab();
            return;
        }
    }
    // List rows (active tab).
    let (sel, len) = match model.sidebar_tab {
        SidebarTab::Workspaces => (model.ws_sel, model.workspaces.len()),
        SidebarTab::Sessions => (model.sess_sel, model.session_entries().len()),
        SidebarTab::Skills => (model.skill_sel, model.skills.len()),
    };
    if let Some(idx) = list_click_index(model.sidebar_list_area, sel, len, col, row) {
        model.focus = Focus::Sidebar;
        match model.sidebar_tab {
            SidebarTab::Workspaces => model.ws_sel = idx,
            SidebarTab::Sessions => model.sess_sel = idx,
            SidebarTab::Skills => model.skill_sel = idx,
        }
        model.sidebar_activate();
        model.mark_dirty();
    }
}

/// Cycle keyboard focus: editor → chat → explorer → dashboard → editor,
/// skipping panes that aren't currently on screen (their areas are only
/// recorded while rendered).
fn next_focus(model: &Model) -> Focus {
    let sidebar = model.sidebar_list_area.is_some();
    let dash = model.agents_pane.area.is_some() || model.tasks_pane.area.is_some();
    match model.focus {
        Focus::Editor => Focus::Chat,
        Focus::Chat if sidebar => Focus::Sidebar,
        Focus::Chat if dash => Focus::Dashboard,
        Focus::Chat => Focus::Editor,
        Focus::Sidebar if dash => Focus::Dashboard,
        Focus::Sidebar => Focus::Editor,
        Focus::Dashboard => Focus::Editor,
    }
}

fn history_prev(model: &mut Model) {
    if model.history.is_empty() {
        return;
    }
    let pos = match model.history_pos {
        None => {
            model.draft = model.input_text();
            model.history.len() - 1
        }
        Some(0) => 0,
        Some(p) => p - 1,
    };
    model.history_pos = Some(pos);
    let entry = model.history[pos].clone();
    model.set_input(&entry);
}

fn history_next(model: &mut Model) {
    match model.history_pos {
        None => {}
        Some(p) if p + 1 < model.history.len() => {
            model.history_pos = Some(p + 1);
            let entry = model.history[p + 1].clone();
            model.set_input(&entry);
        }
        Some(_) => {
            model.history_pos = None;
            let draft = model.draft.clone();
            model.set_input(&draft);
        }
    }
}

async fn handle_dialog_key(model: &mut Model, code: KeyCode, session: &SessionHandle) {
    match code {
        KeyCode::Esc => model.dialog = None,
        KeyCode::Up => {
            if let Some(d) = model.dialog.as_mut() {
                d.move_up();
            }
        }
        KeyCode::Down => {
            if let Some(d) = model.dialog.as_mut() {
                d.move_down();
            }
        }
        KeyCode::Backspace => {
            if let Some(d) = model.dialog.as_mut() {
                d.pop_char();
            }
        }
        KeyCode::Char(c) => {
            if let Some(d) = model.dialog.as_mut() {
                d.push_char(c);
            }
        }
        KeyCode::Enter => {
            let action = model.dialog.as_ref().and_then(|d| d.selected_action());
            model.dialog = None;
            if let Some(a) = action {
                perform_action(model, a, session).await;
            }
        }
        _ => {}
    }
}

async fn perform_action(model: &mut Model, action: Action, session: &SessionHandle) {
    match action {
        Action::Quit => model.should_quit = true,
        Action::ClearTranscript => model.clear_transcript(),
        Action::CycleTheme => model.cycle_theme(),
        Action::NewSession => model.request_new_session(),
        Action::ResumeSession(id) => model.request_resume(id),
        Action::AttachRemote(name) => model.request_remote(name),
        Action::SetModel(m) => {
            model.model_name = m.clone();
            model.model_options.model = m.clone();
            let _ = session.send(Command::SetModel { model: m.clone() }).await;
            model.entries.push(Entry::Notice(format!("model → {m}")));
        }
        // Menu hub: open a focused sub-picker (clone first to avoid borrowing
        // `model` while assigning `model.dialog`).
        Action::OpenSessions => {
            let sessions = model.recent_sessions.clone();
            model.dialog = Some(Picker::session_picker(&sessions, &model.remotes));
        }
        Action::OpenModels => {
            let models = model.model_options.models.clone();
            let current = model.model_options.model.clone();
            model.dialog = Some(Picker::model_picker(&models, &current));
        }
        Action::OpenProviders => {
            let providers = model.model_options.providers.clone();
            let current = model.model_options.provider.clone();
            model.dialog = Some(Picker::provider_picker(&providers, &current));
        }
        Action::ToggleYolo => commands::toggle_yolo(model, session).await,
        Action::SetProvider(name) => {
            if name == model.model_options.provider {
                return; // already active
            }
            let ready = model
                .model_options
                .providers
                .iter()
                .any(|p| p.name == name && p.ready);
            if ready {
                model.request_provider(name.clone(), None);
                model
                    .entries
                    .push(Entry::Notice(format!("switching to {name}…")));
            } else {
                // Unready → capture an API key inline (masked editor).
                model.entries.push(Entry::Notice(format!(
                    "enter the {name} API key, then Enter (Esc to cancel)"
                )));
                model.start_key_prompt(name);
            }
        }
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    /// A turn runner that does nothing — just to get a `SessionHandle` in tests.
    struct NoopRunner;
    #[async_trait::async_trait]
    impl blumi_core::TurnRunner for NoopRunner {
        async fn run_turn(
            &self,
            _state: Arc<Mutex<blumi_core::SessionState>>,
            _ctx: blumi_core::TurnContext,
            _ct: CancellationToken,
        ) -> blumi_protocol::DoneReason {
            blumi_protocol::DoneReason::Completed
        }
    }
    fn test_session() -> SessionHandle {
        blumi_core::spawn_session(blumi_protocol::SessionId::new(), "m", Arc::new(NoopRunner))
    }

    async fn press(m: &mut Model, s: &SessionHandle, code: KeyCode, mods: KeyModifiers) {
        let ev = crossterm::event::KeyEvent::new(code, mods);
        update(m, Msg::Term(TermEvent::Key(ev)), s).await;
    }

    #[tokio::test]
    async fn esc_enters_nav_then_key_returns_insert() {
        let mut m = Model::new("m".into(), "/tmp".into());
        let s = test_session();
        assert_eq!(m.mode, Mode::Insert);
        // Esc on an empty idle editor drops into Nav.
        press(&mut m, &s, KeyCode::Esc, KeyModifiers::NONE).await;
        assert_eq!(m.mode, Mode::Nav);
        // `i` returns to Insert.
        press(&mut m, &s, KeyCode::Char('i'), KeyModifiers::NONE).await;
        assert_eq!(m.mode, Mode::Insert);
    }

    #[tokio::test]
    async fn ctrl_b_and_ctrl_j_toggle_rails() {
        let mut m = Model::new("m".into(), "/tmp".into());
        let s = test_session();
        assert!(m.explorer_open && m.show_dashboard);
        press(&mut m, &s, KeyCode::Char('b'), KeyModifiers::CONTROL).await;
        assert!(!m.explorer_open, "ctrl+b hides explorer");
        press(&mut m, &s, KeyCode::Char('j'), KeyModifiers::CONTROL).await;
        assert!(!m.show_dashboard, "ctrl+j hides agent rail");
    }

    #[tokio::test]
    async fn nav_gg_and_shift_g_scroll() {
        let mut m = Model::new("m".into(), "/tmp".into());
        let s = test_session();
        m.mode = Mode::Nav;
        press(&mut m, &s, KeyCode::Char('g'), KeyModifiers::NONE).await; // first g: waits
        assert_eq!(m.scrollback, 0);
        press(&mut m, &s, KeyCode::Char('g'), KeyModifiers::NONE).await; // gg → oldest
        assert_eq!(m.scrollback, u16::MAX);
        press(&mut m, &s, KeyCode::Char('G'), KeyModifiers::NONE).await; // G → latest
        assert_eq!(m.scrollback, 0);
    }

    #[test]
    fn dashboard_keyboard_scroll_clamps() {
        let mut m = Model::new("m".into(), "/tmp".into());
        // The selected sub-panel (Tasks) scrolls, clamped to its content.
        m.dash_panel = crate::model::DashPanel::Tasks;
        m.tasks_pane.record(50, 2, 30, 10, 25); // height 10, 25 lines → max 15
        m.scroll_dashboard(100); // PgDn past the end → clamps
        assert_eq!(m.tasks_pane.scroll, 15);
        m.scroll_dashboard(isize::MIN); // Home → top
        assert_eq!(m.tasks_pane.scroll, 0);
        m.scroll_dashboard(isize::MAX); // End → bottom
        assert_eq!(m.tasks_pane.scroll, 15);
    }

    #[test]
    fn wheel_scrolls_pane_under_cursor() {
        let mut m = Model::new("m".into(), "/tmp".into());
        m.agents_pane.record(50, 2, 30, 10, 100);
        m.tasks_pane.record(50, 14, 30, 10, 100);
        // Over the agents sub-panel → pans only it.
        scroll_panes(&mut m, 60, 5, 1);
        assert_eq!(m.agents_pane.scroll, 3);
        assert_eq!(m.tasks_pane.scroll, 0);
        assert_eq!(m.scrollback, 0);
        // Over the tasks sub-panel → pans only it.
        scroll_panes(&mut m, 60, 16, 1);
        assert_eq!(m.tasks_pane.scroll, 3);
        // Elsewhere (the chat area) → scrolls the transcript.
        scroll_panes(&mut m, 10, 5, -1);
        assert_eq!(m.scrollback, 3);
    }

    #[test]
    fn long_running_tool_posts_a_charm() {
        let mut m = Model::new("m".into(), "/tmp".into());
        m.running_tools.insert(
            "c1".into(),
            crate::model::ToolRun {
                started: std::time::Instant::now() - std::time::Duration::from_secs(9),
                name: "Bash".into(),
                charms: 0,
            },
        );
        tick_tool_charms(&mut m);
        // First charm posted at ≥8s; the per-tool counter advances (so it won't spam).
        assert_eq!(m.running_tools["c1"].charms, 1);
        assert!(m
            .entries
            .iter()
            .any(|e| matches!(e, Entry::Notice(n) if n.contains("Bash") && n.contains("9s"))));
        // A second immediate tick must NOT post again (next charm is ~10s later).
        tick_tool_charms(&mut m);
        assert_eq!(m.running_tools["c1"].charms, 1);
    }

    #[tokio::test]
    async fn cycle_theme_changes_name() {
        let mut m = Model::new("x".into(), "/".into());
        let s = test_session();
        let first = m.theme.name;
        perform_action(&mut m, Action::CycleTheme, &s).await;
        assert_ne!(m.theme.name, first);
        assert_eq!(m.theme_idx, 1);
    }

    #[tokio::test]
    async fn clear_transcript_empties() {
        let mut m = Model::new("x".into(), "/".into());
        let s = test_session();
        m.entries.push(Entry::User("hi".into()));
        m.streaming = Some("partial".into());
        perform_action(&mut m, Action::ClearTranscript, &s).await;
        assert!(m.entries.is_empty());
        assert!(m.streaming.is_none());
    }

    #[tokio::test]
    async fn set_provider_ready_requests_switch_unready_prompts_key() {
        let mut m = Model::new("x".into(), "/".into());
        let s = test_session();
        m.model_options.providers = vec![
            crate::app::ProviderOpt {
                name: "openai".into(),
                label: "OpenAI".into(),
                ready: true,
            },
            crate::app::ProviderOpt {
                name: "groq".into(),
                label: "Groq".into(),
                ready: false,
            },
        ];
        perform_action(&mut m, Action::SetProvider("openai".into()), &s).await;
        assert_eq!(
            m.take_provider_request(),
            Some(("openai".to_string(), None))
        );
        // Unready → starts an inline key prompt instead.
        perform_action(&mut m, Action::SetProvider("groq".into()), &s).await;
        assert_eq!(m.provider_key_prompt.as_deref(), Some("groq"));
    }
}
