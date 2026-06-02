//! Message handling: keys, terminal events, and core events.

use crate::dialog::{Action, Picker};
use crate::model::{Entry, Focus, Model, Msg, PendingApproval};
use crate::theme::Theme;
use blumi_core::SessionHandle;
use blumi_protocol::{ApprovalScope, Command, Decision, Event};
use crossterm::event::{Event as TermEvent, KeyCode, KeyEventKind, KeyModifiers};

pub async fn update(model: &mut Model, msg: Msg, session: &SessionHandle) {
    match msg {
        Msg::Tick => {
            if model.busy {
                model.spinner_frame = model.spinner_frame.wrapping_add(1);
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

            // A dialog (command palette) captures all keys.
            if model.dialog.is_some() {
                handle_dialog_key(model, key.code);
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

            match key.code {
                KeyCode::Esc => {
                    if model.busy {
                        let _ = session.send(Command::Cancel).await;
                    } else {
                        model.clear_input();
                    }
                }
                KeyCode::Tab => {
                    model.focus = match model.focus {
                        Focus::Editor => Focus::Chat,
                        Focus::Chat => Focus::Editor,
                    };
                }
                // Ctrl+J or Shift/Alt+Enter inserts a newline; plain Enter sends.
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
                        handle_slash(model, session, &trimmed).await;
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
                    } else {
                        // chat-focused scrolling
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
        _ => {}
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

async fn handle_core(model: &mut Model, event: Event, session: &SessionHandle) {
    match event {
        Event::AssistantStarted { .. } => {
            model.streaming = Some(String::new());
        }
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
        Event::AssistantFinished { .. } => {
            commit_streaming(model);
        }
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
        } => {
            model.pending = Some(PendingApproval {
                request_id,
                tool,
                summary,
                dangerous,
                diff,
            });
        }
        Event::ClarifyRequest {
            request_id,
            question,
            ..
        } => {
            // MVP: surface the question and answer empty so the turn proceeds.
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
        Event::Usage { input, output, .. } => {
            model.input_tokens += input;
            model.output_tokens += output;
        }
        Event::Compaction {
            messages_compressed,
            ..
        } => {
            model.entries.push(Entry::Notice(format!(
                "context compacted ({messages_compressed} messages)"
            )));
        }
        Event::TurnDone { .. } => {
            commit_streaming(model);
            model.thinking = None;
            model.busy = false;
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

fn handle_dialog_key(model: &mut Model, code: KeyCode) {
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
                perform_action(model, a);
            }
        }
        _ => {}
    }
}

fn perform_action(model: &mut Model, action: Action) {
    match action {
        Action::Quit => model.should_quit = true,
        Action::ClearTranscript => clear_transcript(model),
        Action::CycleTheme => cycle_theme(model),
    }
}

fn clear_transcript(model: &mut Model) {
    model.entries.clear();
    model.streaming = None;
    model.thinking = None;
    model.scrollback = 0;
}

fn cycle_theme(model: &mut Model) {
    model.theme_idx = (model.theme_idx + 1) % crate::theme::THEMES.len();
    model.theme = Theme::by_index(model.theme_idx);
    model
        .entries
        .push(Entry::Notice(format!("theme: {}", model.theme.name)));
}

async fn handle_slash(model: &mut Model, session: &SessionHandle, input: &str) {
    model.clear_input();
    let mut parts = input.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim();
    match cmd {
        "/help" => model.entries.push(Entry::Notice(
            "commands: /help · /clear · /theme [name] · /model <id> · /login · /quit   \
             (ctrl+p palette · tab focus · esc cancel)"
                .into(),
        )),
        "/clear" => clear_transcript(model),
        "/quit" => model.should_quit = true,
        "/theme" => {
            if arg.is_empty() {
                cycle_theme(model);
            } else if let Some(i) =
                (0..crate::theme::THEMES.len()).find(|&i| Theme::by_index(i).name == arg)
            {
                model.theme_idx = i;
                model.theme = Theme::by_index(i);
                model.entries.push(Entry::Notice(format!("theme: {arg}")));
            } else {
                model
                    .entries
                    .push(Entry::Notice(format!("unknown theme '{arg}'")));
            }
        }
        "/model" => {
            if arg.is_empty() {
                model
                    .entries
                    .push(Entry::Notice("usage: /model <id>".into()));
            } else {
                model.model_name = arg.to_string();
                let _ = session
                    .send(Command::SetModel {
                        model: arg.to_string(),
                    })
                    .await;
                model.entries.push(Entry::Notice(format!("model → {arg}")));
            }
        }
        "/login" => model.entries.push(Entry::Notice(
            "run `blumi login` from a shell to add/switch providers".into(),
        )),
        other => model.entries.push(Entry::Notice(format!(
            "unknown command '{other}' (try /help)"
        ))),
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_theme_changes_name() {
        let mut m = Model::new("x".into(), "/".into());
        let first = m.theme.name;
        perform_action(&mut m, Action::CycleTheme);
        assert_ne!(m.theme.name, first);
        assert_eq!(m.theme_idx, 1);
    }

    #[test]
    fn clear_transcript_empties() {
        let mut m = Model::new("x".into(), "/".into());
        m.entries.push(Entry::User("hi".into()));
        m.streaming = Some("partial".into());
        perform_action(&mut m, Action::ClearTranscript);
        assert!(m.entries.is_empty());
        assert!(m.streaming.is_none());
    }
}
