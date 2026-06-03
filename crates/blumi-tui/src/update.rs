//! Message handling: keys, terminal events, and core events.

use crate::commands;
use crate::dialog::{Action, Picker};
use crate::model::{Entry, Focus, Model, Msg, PendingApproval, PlanReview};
use blumi_core::SessionHandle;
use blumi_protocol::{ApprovalScope, Command, Decision, Event};
use crossterm::event::{Event as TermEvent, KeyCode, KeyEventKind, KeyModifiers};

pub async fn update(model: &mut Model, msg: Msg, session: &SessionHandle) {
    match msg {
        Msg::Tick => {
            model.spinner_frame = model.spinner_frame.wrapping_add(1);
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
                model.dialog = Some(Picker::session_picker(&model.recent_sessions));
                model.mark_dirty();
                return;
            }

            // Ctrl+Y toggles yolo (auto-approve / skip permissions), crush-style.
            if ctrl && matches!(key.code, KeyCode::Char('y')) {
                commands::toggle_yolo(model, session).await;
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
                    KeyCode::Tab => model.focus = next_focus(model.focus),
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

            match key.code {
                KeyCode::Esc => {
                    if model.busy {
                        let _ = session.send(Command::Cancel).await;
                    } else {
                        model.clear_input();
                    }
                }
                KeyCode::Tab => model.focus = next_focus(model.focus),
                KeyCode::Char('j') if ctrl => model.input.insert_newline(),
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
                    if let Some(p) = model.plan_review.as_mut() {
                        p.scroll = p.scroll.saturating_sub(3);
                    } else if let Some(d) = model.dialog.as_mut() {
                        d.move_up();
                    } else {
                        model.scrollback = model.scrollback.saturating_add(3);
                    }
                    model.mark_dirty();
                }
                MouseEventKind::ScrollDown => {
                    if let Some(p) = model.plan_review.as_mut() {
                        p.scroll = p.scroll.saturating_add(3);
                    } else if let Some(d) = model.dialog.as_mut() {
                        d.move_down();
                    } else {
                        model.scrollback = model.scrollback.saturating_sub(3);
                    }
                    model.mark_dirty();
                }
                // Click a picker row to select + activate it (menu-style).
                MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
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
                        // Click a sidebar row: focus it, select, and activate.
                        sidebar_click(model, me.column, me.row);
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
    // Tab bar: left half → Workspaces, right half → Sessions.
    if let Some((x, y, w, h)) = model.sidebar_tab_area {
        if col >= x && col < x + w && row >= y && row < y + h {
            model.focus = Focus::Sidebar;
            let tab = if col < x + w / 2 {
                SidebarTab::Workspaces
            } else {
                SidebarTab::Sessions
            };
            model.set_sidebar_tab(tab);
            return;
        }
    }
    // List rows (active tab).
    let (sel, len) = match model.sidebar_tab {
        SidebarTab::Workspaces => (model.ws_sel, model.workspaces.len()),
        SidebarTab::Sessions => (model.sess_sel, model.recent_sessions.len()),
    };
    if let Some(idx) = list_click_index(model.sidebar_list_area, sel, len, col, row) {
        model.focus = Focus::Sidebar;
        match model.sidebar_tab {
            SidebarTab::Workspaces => model.ws_sel = idx,
            SidebarTab::Sessions => model.sess_sel = idx,
        }
        model.sidebar_activate();
        model.mark_dirty();
    }
}

/// Cycle keyboard focus: editor → chat → explorer sidebar → editor.
fn next_focus(f: Focus) -> Focus {
    match f {
        Focus::Editor => Focus::Chat,
        Focus::Chat => Focus::Sidebar,
        Focus::Sidebar => Focus::Editor,
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
            model.dialog = Some(Picker::session_picker(&sessions));
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
