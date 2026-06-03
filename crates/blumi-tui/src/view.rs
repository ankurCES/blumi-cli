//! Rendering.

use crate::model::{fmt_dur, Entry, Focus, Model};
use crate::theme::{icon, Theme};
use blumi_protocol::TodoStatus;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Min terminal width to show the run dashboard sidebar.
const DASHBOARD_MIN_WIDTH: u16 = 92;
const DASHBOARD_WIDTH: u16 = 32;
/// Left sidebar (workspaces + sessions) width.
const SIDEBAR_WIDTH: u16 = 26;

const MAX_CONTENT_WIDTH: u16 = 100;

pub fn render(model: &mut Model, f: &mut Frame) {
    let theme = model.theme;
    let area = f.area();

    let editor_h = (model.input.lines().len().clamp(1, 6) as u16) + 2;
    // header / chat / inforule (context meter + working indicator, next to the
    // input) / editor / status (key hints).
    let [header, chat, inforule, editor, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(editor_h),
        Constraint::Length(1),
    ])
    .areas(area);

    render_header(model, f, header, &theme);

    // Body columns: optional left sidebar (workspaces + sessions) | center chat |
    // optional right dashboard (logo + cost + active agents). The dashboard keeps
    // its established threshold; the sidebar only appears when there's room left
    // over for it (so adding it never displaces the dashboard).
    let show_right = model.show_dashboard && !model.is_empty() && chat.width >= DASHBOARD_MIN_WIDTH;
    let show_left = if show_right {
        chat.width >= DASHBOARD_MIN_WIDTH + SIDEBAR_WIDTH
    } else {
        chat.width >= SIDEBAR_WIDTH + 60
    };

    let mut constraints: Vec<Constraint> = Vec::new();
    if show_left {
        constraints.push(Constraint::Length(SIDEBAR_WIDTH));
    }
    constraints.push(Constraint::Min(0));
    if show_right {
        constraints.push(Constraint::Length(DASHBOARD_WIDTH));
    }
    let cols = Layout::horizontal(constraints).split(chat);
    let mut ci = 0;
    if show_left {
        render_sidebar(model, f, cols[ci], &theme);
        ci += 1;
    } else {
        model.sidebar_list_area = None;
        model.sidebar_tab_area = None;
    }
    let chat = cols[ci];
    ci += 1;
    if show_right {
        render_dashboard(model, f, cols[ci], &theme);
    }

    if model.is_empty() {
        render_landing(model, f, chat, &theme);
    } else {
        render_chat(model, f, chat, &theme);
    }
    render_inforule(model, f, inforule, &theme);
    render_editor(model, f, editor, &theme);
    render_status(model, f, status, &theme);

    if model.pending.is_some() {
        render_approval(model, f, area, &theme);
    }
    if model.plan_review.is_some() {
        render_plan_review(model, f, area, &theme);
    }
    if model.dialog.is_some() {
        render_dialog(model, f, area, &theme);
    } else {
        model.dialog_list_area = None;
    }
    if model.memory_view.is_some() {
        render_memory(model, f, area, &theme);
    }
    if model.usage_view.is_some() {
        render_usage(model, f, area, &theme);
    }
    if model.board_view.is_some() {
        render_board(model, f, area, &theme);
    }
    // Slash-command popup floats just above the editor.
    if model.slash_active() {
        render_slash_popup(model, f, editor, &theme);
    }
}

/// A rounded, titled pane block; border + title brighten when focused.
fn pane_block(title: &str, focused: bool, theme: &Theme) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            theme.bold_primary()
        } else {
            theme.dim()
        })
        .title(Span::styled(
            format!(" {title} "),
            if focused {
                theme.bold_primary()
            } else {
                theme.subtle()
            },
        ))
}

/// Render one selectable list into `inner`, windowed around `sel`. Returns the
/// inner rect (for click mapping). `row` formats each item into spans.
#[allow(clippy::too_many_arguments)]
fn render_list<T>(
    f: &mut Frame,
    inner: Rect,
    items: &[T],
    sel: usize,
    focused: bool,
    theme: &Theme,
    empty: &str,
    row: impl Fn(&T, usize) -> Vec<Span<'static>>,
) {
    if items.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(format!("  {empty}"), theme.dim()))),
            inner,
        );
        return;
    }
    let h = inner.height.max(1) as usize;
    let sel = sel.min(items.len() - 1);
    let start = sel.saturating_sub(h.saturating_sub(1));
    let mut lines: Vec<Line> = Vec::new();
    for (i, it) in items.iter().enumerate().skip(start).take(h) {
        let selected = i == sel;
        let caret = if selected && focused {
            Span::styled("▸", theme.accent())
        } else {
            Span::raw(" ")
        };
        let mut spans = vec![caret];
        spans.extend(row(it, i));
        lines.push(Line::from(spans));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The left explorer: one window with a tab bar (Workspaces / Sessions) and the
/// active tab's list below it — like a proper tabbed panel.
fn render_sidebar(model: &mut Model, f: &mut Frame, area: Rect, theme: &Theme) {
    use crate::model::SidebarTab;
    let focused = model.focus == Focus::Sidebar;
    let block = pane_block("explorer", focused, theme);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Tab bar (row 0) + active list (the rest).
    let [tabs_row, list_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    model.sidebar_tab_area = Some((tabs_row.x, tabs_row.y, tabs_row.width, tabs_row.height));
    model.sidebar_list_area = Some((list_area.x, list_area.y, list_area.width, list_area.height));

    let tab = model.sidebar_tab;
    let chip = |label: &str, active: bool| {
        if active {
            Span::styled(
                format!(" {label} "),
                theme.bold_primary().add_modifier(Modifier::REVERSED),
            )
        } else {
            Span::styled(format!(" {label} "), theme.subtle())
        }
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            chip("Workspaces", tab == SidebarTab::Workspaces),
            Span::raw(" "),
            chip("Sessions", tab == SidebarTab::Sessions),
        ])),
        tabs_row,
    );

    let sel_style = theme.bold_primary();
    let body = theme.body();
    match tab {
        SidebarTab::Workspaces => {
            let name_w = list_area.width.saturating_sub(3) as usize;
            render_list(
                f,
                list_area,
                &model.workspaces,
                model.ws_sel,
                focused,
                theme,
                "(no projects)",
                |ws, i| {
                    let star = if ws.pinned { "★" } else { " " };
                    let style = if i == model.ws_sel { sel_style } else { body };
                    vec![
                        Span::styled(format!("{star} "), theme.accent()),
                        Span::styled(truncate(&ws.name, name_w), style),
                    ]
                },
            );
        }
        SidebarTab::Sessions => {
            let title_w = list_area.width.saturating_sub(2) as usize;
            render_list(
                f,
                list_area,
                &model.recent_sessions,
                model.sess_sel,
                focused,
                theme,
                "(no sessions)",
                |(_, title), i| {
                    let style = if i == model.sess_sel { sel_style } else { body };
                    let t = if title.trim().is_empty() {
                        "(untitled)"
                    } else {
                        title
                    };
                    vec![Span::styled(format!(" {}", truncate(t, title_w)), style)]
                },
            );
        }
    }
}

/// The agent dashboard: live session state, context usage, tasks, recent tool
/// activity, and recent sessions — the run turned into a terminal cockpit.
fn render_dashboard(model: &Model, f: &mut Frame, area: Rect, theme: &Theme) {
    // A pulsing "live agent" dot: amber while working, green when ready.
    let dot_color = if model.busy {
        crate::mascot::pulse_color(0xFF, 0xC0, 0x4F, model.spinner_frame)
    } else {
        crate::mascot::pulse_color(0x4F, 0xE0, 0xA0, model.spinner_frame)
    };
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(theme.dim())
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", icon::DOT),
                Style::default().fg(dot_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled("agent ", theme.bold_primary()),
        ]));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let w = inner.width.saturating_sub(1) as usize;

    let mut lines: Vec<Line> = Vec::new();
    let model_name = if model.model_name.is_empty() {
        "default"
    } else {
        &model.model_name
    };

    // ── Brand logo crowning the pane: small flower + multicolor wordmark ──
    for line in crate::mascot::brand_logo(model.spinner_frame) {
        lines.push(line);
    }
    lines.push(Line::raw(""));

    // ── Active agents (the team) — directly under the logo ─────
    if !model.agents.is_empty() {
        let working = model
            .agents
            .iter()
            .filter(|a| a.status == crate::model::AgentStatus::Working)
            .count();
        lines.push(section(&format!("Active agents  {working}▸"), theme));
        for a in &model.agents {
            let (glyph, gstyle) = match a.status {
                crate::model::AgentStatus::Working => (
                    crate::mascot::spinner(model.spinner_frame).to_string(),
                    theme.accent(),
                ),
                crate::model::AgentStatus::Done => {
                    (icon::OK.to_string(), Style::default().fg(theme.success))
                }
                crate::model::AgentStatus::Failed => {
                    ("✗".to_string(), Style::default().fg(theme.error))
                }
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{glyph} "), gstyle),
                Span::styled(truncate(&a.role, w.saturating_sub(2)), theme.bold_primary()),
            ]));
            lines.push(Line::from(Span::styled(
                format!("   {}", truncate(&a.task, w.saturating_sub(3))),
                theme.subtle(),
            )));
        }
        lines.push(Line::raw(""));
    }

    // ── Session ───────────────────────────────────────────────
    lines.push(section("Session", theme));
    lines.push(Line::from(vec![
        Span::styled(format!("{:>7} ", "status"), theme.dim()),
        Span::styled(
            format!("{} ", icon::DOT),
            Style::default().fg(dot_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if model.busy { "working" } else { "ready" },
            if model.busy {
                theme.accent()
            } else {
                Style::default().fg(theme.success)
            },
        ),
    ]));
    lines.push(kv(
        "model",
        &truncate(model_name, w.saturating_sub(8)),
        theme,
    ));
    lines.push(kv(
        "persona",
        &truncate(&model.persona, w.saturating_sub(8)),
        theme,
    ));
    lines.push(kv("uptime", &fmt_dur(model.uptime_secs()), theme));
    lines.push(kv("active", &fmt_dur(model.active_ms / 1000), theme));
    lines.push(kv(
        "approve",
        if model.yolo { "auto (yolo)" } else { "ask" },
        theme,
    ));
    if model.brain_mode != "off" {
        lines.push(kv("brain", &model.brain_mode, theme));
    }
    if model.plan_mode {
        lines.push(kv("mode", "plan (read-only)", theme));
    }
    lines.push(kv(
        "autocont",
        &if model.auto_continue == 0 {
            "off".to_string()
        } else {
            format!("≤{}", model.auto_continue)
        },
        theme,
    ));

    // ── Context usage ─────────────────────────────────────────
    lines.push(Line::raw(""));
    lines.push(section("Context", theme));
    let frac = model.context_frac();
    let bar_color = if frac > 0.85 {
        theme.error
    } else if frac > 0.6 {
        Color::Indexed(214) // amber
    } else {
        theme.success
    };
    lines.push(Line::from(Span::styled(
        bar(frac, w),
        Style::default().fg(bar_color),
    )));
    lines.push(Line::from(Span::styled(
        format!(
            "  {} / {} ({}%)",
            fmt_k(model.context_tokens),
            fmt_k(model.context_size),
            (frac * 100.0).round() as u32
        ),
        theme.subtle(),
    )));

    // ── Usage ─────────────────────────────────────────────────
    lines.push(Line::raw(""));
    lines.push(section("Usage", theme));
    lines.push(kv(
        "tokens",
        &format!(
            "↑{} ↓{}",
            fmt_k(model.input_tokens),
            fmt_k(model.output_tokens)
        ),
        theme,
    ));
    lines.push(kv("turns", &model.turn_count.to_string(), theme));
    lines.push(kv("tools", &model.tools_run().to_string(), theme));
    // Estimated spend (list price × billed tokens); "n/a" for unpriced models.
    let cost = if crate::cost::is_priced(&model.model_name) {
        format!("~${:.4}", model.cost_usd)
    } else {
        "n/a".to_string()
    };
    lines.push(kv("cost", &cost, theme));

    // ── Goal (if set) ─────────────────────────────────────────
    if !model.goal.is_empty() {
        lines.push(Line::raw(""));
        lines.push(section("Goal", theme));
        for l in wrap_lines(&model.goal, w, theme.subtle()) {
            lines.push(l);
        }
    }

    // ── Tasks ─────────────────────────────────────────────────
    let total = model.todos.len();
    let done = model
        .todos
        .iter()
        .filter(|t| t.status == TodoStatus::Completed)
        .count();
    lines.push(Line::raw(""));
    lines.push(section(&format!("Tasks  {done}/{total}"), theme));
    if total == 0 {
        lines.push(Line::from(Span::styled("  (none yet)", theme.dim())));
    } else {
        lines.push(Line::from(Span::styled(
            bar(done as f64 / total as f64, w),
            Style::default().fg(theme.success),
        )));
        // Which team members are working right now (shown against the active task).
        let working_roles: Vec<&str> = model
            .agents
            .iter()
            .filter(|a| a.status == crate::model::AgentStatus::Working)
            .map(|a| a.role.as_str())
            .collect();
        for todo in &model.todos {
            let (mark, style) = match todo.status {
                TodoStatus::Completed => (icon::OK.to_string(), Style::default().fg(theme.success)),
                // In-flight tasks get an animated spinner.
                TodoStatus::InProgress => (
                    crate::mascot::spinner(model.spinner_frame).to_string(),
                    theme.accent(),
                ),
                TodoStatus::Pending => ("•".to_string(), theme.subtle()),
            };
            // Tag the active task with the agent(s) working it (team mode).
            let agent_tag = if todo.status == TodoStatus::InProgress && !working_roles.is_empty() {
                format!(" ◐ {}", working_roles.join(", "))
            } else {
                String::new()
            };
            let body_w = w.saturating_sub(2 + agent_tag.chars().count());
            let mut spans = vec![
                Span::styled(format!("{mark} "), style),
                Span::styled(truncate(&todo.content, body_w), theme.body()),
            ];
            if !agent_tag.is_empty() {
                spans.push(Span::styled(agent_tag, theme.accent()));
            }
            lines.push(Line::from(spans));
        }
    }

    // ── Activity (recent tool calls; running ones spin) ───────
    let mut tools: Vec<(&str, Option<bool>)> = model
        .entries
        .iter()
        .rev()
        .filter_map(|e| match e {
            Entry::Tool { name, ok, .. } => Some((name.as_str(), *ok)),
            _ => None,
        })
        .take(5)
        .collect();
    tools.reverse();
    if !tools.is_empty() {
        lines.push(Line::raw(""));
        lines.push(section("Activity", theme));
        for (name, ok) in tools {
            let (mark, style) = match ok {
                Some(true) => (icon::OK.to_string(), Style::default().fg(theme.success)),
                Some(false) => (icon::ERR.to_string(), Style::default().fg(theme.error)),
                None => (
                    crate::mascot::spinner(model.spinner_frame).to_string(),
                    theme.accent(),
                ),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{mark} "), style),
                Span::styled(truncate(name, w.saturating_sub(2)), theme.subtle()),
            ]));
        }
    }

    // ── Sessions (recent history) ─────────────────────────────
    if !model.recent_sessions.is_empty() {
        lines.push(Line::raw(""));
        lines.push(section("Sessions", theme));
        for (_, title) in model.recent_sessions.iter().take(3) {
            let title = if title.is_empty() {
                "(untitled)"
            } else {
                title
            };
            lines.push(Line::from(vec![
                Span::styled("· ", theme.dim()),
                Span::styled(truncate(title, w.saturating_sub(2)), theme.subtle()),
            ]));
        }
    }

    f.render_widget(Paragraph::new(lines), inner);
}

/// Format a token count compactly: `1.2k`, `131k`, `42`.
fn fmt_k(n: u32) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// A dashboard section header.
fn section(title: &str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        theme.accent().add_modifier(Modifier::BOLD),
    ))
}

/// A right-aligned `key  value` dashboard row.
fn kv(key: &str, val: &str, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:>7} "), theme.dim()),
        Span::styled(val.to_string(), theme.body()),
    ])
}

/// The slash-command autocomplete popup, anchored above the editor.
fn render_slash_popup(model: &Model, f: &mut Frame, editor: Rect, theme: &Theme) {
    let matches = crate::commands::matching(&model.input_text());
    if matches.is_empty() {
        return;
    }
    let shown = matches.len().min(8);
    let height = shown as u16 + 2;
    let width = editor.width.min(48);
    let y = editor.y.saturating_sub(height);
    let popup = Rect {
        x: editor.x,
        y,
        width,
        height,
    };
    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary))
        .title(Span::styled(" commands ", theme.subtle()));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let sel = model.slash_sel.min(matches.len().saturating_sub(1));
    let rows: Vec<Line> = matches
        .iter()
        .take(shown)
        .enumerate()
        .map(|(i, c)| {
            let selected = i == sel;
            let marker = if selected { "❯ " } else { "  " };
            let name_style = if selected {
                theme.bold_primary()
            } else {
                theme.body()
            };
            Line::from(vec![
                Span::styled(marker, theme.accent()),
                Span::styled(format!("{:<11}", c.name), name_style),
                Span::styled(c.desc, theme.dim()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(rows), inner);
}

/// The `/board` overlay: the persistent task board (status + counts).
fn render_board(model: &Model, f: &mut Frame, area: Rect, theme: &Theme) {
    let Some(text) = &model.board_view else {
        return;
    };
    let popup = centered_rect(64, 60, area);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary))
        .title(Span::styled(
            " task board — any key to close ",
            theme.bold_primary(),
        ));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let width = inner.width.saturating_sub(1) as usize;
    let mut lines: Vec<Line> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let style = if i == 0 {
            theme.bold_primary() // the running/queued/done summary line
        } else {
            theme.body()
        };
        lines.push(Line::from(Span::styled(truncate(raw, width), style)));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The `/memory` overlay: shows MEMORY.md + USER.md.
fn render_memory(model: &Model, f: &mut Frame, area: Rect, theme: &Theme) {
    let Some(text) = &model.memory_view else {
        return;
    };
    let popup = centered_rect(70, 60, area);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary))
        .title(Span::styled(
            " memory — any key to close ",
            theme.bold_primary(),
        ));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let width = inner.width.saturating_sub(1) as usize;
    let mut lines: Vec<Line> = Vec::new();
    for raw in text.lines() {
        let style = if raw.ends_with(')') && !raw.starts_with(' ') {
            theme.accent() // section headers like "MEMORY.md (agent notes)"
        } else {
            theme.body()
        };
        lines.push(Line::from(Span::styled(truncate(raw, width), style)));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The `/usage` analytics overlay.
fn render_usage(model: &Model, f: &mut Frame, area: Rect, theme: &Theme) {
    let Some(text) = &model.usage_view else {
        return;
    };
    let popup = centered_rect(58, 60, area);
    f.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary))
        .title(Span::styled(
            " usage analytics — any key to close ",
            theme.bold_primary(),
        ));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let width = inner.width.saturating_sub(1) as usize;
    let mut lines: Vec<Line> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let style = if i == 0 {
            theme.bold_primary()
        } else {
            theme.body()
        };
        lines.push(Line::from(Span::styled(truncate(raw, width), style)));
    }
    // A visual context-usage bar.
    lines.push(Line::raw(""));
    let frac = model.context_frac();
    lines.push(Line::from(vec![
        Span::styled("context  ", theme.dim()),
        Span::styled(bar(frac, width.min(36)), theme.accent()),
        Span::styled(
            format!("  {}%", (frac * 100.0).round() as u32),
            theme.subtle(),
        ),
    ]));
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_dialog(model: &mut Model, f: &mut Frame, area: Rect, theme: &Theme) {
    if model.dialog.is_none() {
        return;
    }
    let popup = centered_rect(60, 50, area);
    f.render_widget(Clear, popup);

    let title = model
        .dialog
        .as_ref()
        .map(|d| d.title.clone())
        .unwrap_or_default();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary))
        .title(Span::styled(format!(" {title} "), theme.bold_primary()));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let [filter_area, list_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    // Record the list rect so mouse clicks can hit-test rows.
    model.dialog_list_area = Some((list_area.x, list_area.y, list_area.width, list_area.height));
    let d = model.dialog.as_ref().expect("dialog present");
    let filter_line = Line::from(vec![
        Span::styled("› ", theme.accent()),
        Span::styled(d.filter.clone(), theme.body()),
    ]);
    f.render_widget(Paragraph::new(filter_line), filter_area);

    let rows: Vec<Line> = d
        .rows()
        .into_iter()
        .map(|(label, hint, selected)| {
            let marker = if selected { "❯ " } else { "  " };
            let label_style = if selected {
                theme.bold_primary()
            } else {
                theme.body()
            };
            Line::from(vec![
                Span::styled(marker, theme.accent()),
                Span::styled(label.to_string(), label_style),
                Span::raw("   "),
                Span::styled(hint.to_string(), theme.dim()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(rows), list_area);
}

fn render_header(model: &Model, f: &mut Frame, area: Rect, theme: &Theme) {
    let [left_area, right_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(40)]).areas(area);

    let title = if model.session_title.is_empty() {
        "blumi".to_string()
    } else {
        model.session_title.clone()
    };
    let mut spans = vec![
        Span::styled(format!("{} {title}", icon::FLOWER), theme.bold_primary()),
        Span::styled("  ·  ", theme.dim()),
        Span::styled(
            if model.model_name.is_empty() {
                "default".to_string()
            } else {
                model.model_name.clone()
            },
            theme.accent(),
        ),
    ];
    // Skipping permissions is dangerous — surface it loudly and always, not just
    // in the (hideable) dashboard. A black-on-amber badge right in the header.
    if model.yolo {
        spans.push(Span::styled("  ", theme.dim()));
        spans.push(Span::styled(
            " ⚡ YOLO ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Indexed(214))
                .add_modifier(Modifier::BOLD),
        ));
    }
    // Planning mode: read-only until a plan is approved.
    if model.plan_mode {
        spans.push(Span::styled("  ", theme.dim()));
        spans.push(Span::styled(
            " ◑ PLAN ",
            Style::default()
                .fg(Color::Black)
                .bg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ));
    }
    // When more than the local tab is open, the header shows a tab strip
    // (ralph-style) in place of the working-dir crumb.
    if model.tabs.len() > 1 {
        spans.push(Span::styled("   ", theme.dim()));
        for (i, (name, remote)) in model.tabs.iter().enumerate() {
            let active = i == model.active_tab;
            let glyph = if *remote { "☁" } else { "▪" };
            let chip = format!(" {glyph} {name} ");
            let style = if active {
                theme.bold_primary()
            } else {
                theme.subtle()
            };
            spans.push(Span::styled(chip, style));
        }
    } else {
        spans.push(Span::styled("  ·  ", theme.dim()));
        spans.push(Span::styled(
            shorten(&model.working_dir, 32),
            theme.subtle(),
        ));
        if !model.persona.is_empty() && model.persona != "default" {
            spans.push(Span::styled("  ·  ", theme.dim()));
            spans.push(Span::styled(model.persona.clone(), theme.subtle()));
        }
    }
    f.render_widget(Paragraph::new(Line::from(spans)), left_area);

    // Right: context %, uptime, token meter.
    let frac = model.context_frac();
    let meter = format!(
        "ctx {}% · {} · ↑{} ↓{}",
        (frac * 100.0).round() as u32,
        fmt_dur(model.uptime_secs()),
        fmt_k(model.input_tokens),
        fmt_k(model.output_tokens),
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(meter, theme.dim()))).alignment(Alignment::Right),
        right_area,
    );
}

fn render_landing(model: &Model, f: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines = vec![Line::raw(""), Line::raw("")];
    lines.extend(
        crate::mascot::rose_logo(model.spinner_frame)
            .into_iter()
            .map(|l| l.alignment(Alignment::Center)),
    );
    lines.push(Line::raw(""));
    if area.width >= crate::logo::BLUMI_BLOCK_WIDTH + 4 {
        lines.extend(
            crate::mascot::wordmark(model.spinner_frame)
                .into_iter()
                .map(|l| l.alignment(Alignment::Center)),
        );
    } else {
        lines.push(
            Line::from(Span::styled("blumi", theme.bold_primary())).alignment(Alignment::Center),
        );
    }
    lines.push(Line::raw(""));
    lines.push(
        Line::from(Span::styled(crate::logo::TAGLINE, theme.subtle())).alignment(Alignment::Center),
    );

    // A table of helpful commands, two per row.
    lines.push(Line::raw(""));
    lines.push(
        Line::from(Span::styled("─ quick commands ─", theme.dim())).alignment(Alignment::Center),
    );
    for chunk in LANDING_CMDS.chunks(2) {
        let mut spans = Vec::new();
        for (i, (cmd, desc)) in chunk.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("    "));
            }
            spans.push(Span::styled(format!("{cmd:<9}"), theme.accent()));
            spans.push(Span::styled(format!("{desc:<17}"), theme.dim()));
        }
        lines.push(Line::from(spans).alignment(Alignment::Center));
    }

    lines.push(Line::raw(""));
    lines.push(
        Line::from(Span::styled(
            "type a message and press Enter · / for the command palette",
            theme.dim(),
        ))
        .alignment(Alignment::Center),
    );
    let para = Paragraph::new(lines);
    f.render_widget(para, area);
}

/// Helpful commands shown on the landing screen.
const LANDING_CMDS: [(&str, &str); 8] = [
    ("/help", "list commands"),
    ("/persona", "switch persona"),
    ("/model", "switch model"),
    ("/usage", "usage analytics"),
    ("/theme", "change theme"),
    ("/tasks", "toggle dashboard"),
    ("/memory", "view memory"),
    ("/quit", "exit blumi"),
];

fn render_chat(model: &Model, f: &mut Frame, area: Rect, theme: &Theme) {
    let width = area.width.min(MAX_CONTENT_WIDTH).saturating_sub(2) as usize;
    let lines = build_lines(model, width, theme);
    let total = lines.len() as u16;
    let height = area.height;
    let max_scroll = total.saturating_sub(height);
    let scroll_y = max_scroll.saturating_sub(model.scrollback.min(max_scroll));
    let para = Paragraph::new(lines).scroll((scroll_y, 0));
    f.render_widget(para, area);
}

fn build_lines(model: &Model, width: usize, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let inner = width.saturating_sub(2).max(8); // content width inside the gutter

    for entry in &model.entries {
        match entry {
            Entry::User(t) => {
                let content = wrap_lines(t, inner, theme.body());
                push_card(&mut lines, icon::USER, "you", theme.accent, content, width);
            }
            Entry::Assistant(t) => {
                let content = crate::markdown::render_markdown(t, inner, theme);
                push_card(
                    &mut lines,
                    icon::FLOWER,
                    "blumi",
                    theme.primary,
                    content,
                    width,
                );
            }
            Entry::Tool {
                name,
                summary,
                ok,
                preview,
                diff_stat,
                diff,
                ..
            } => {
                let (mark, color) = match ok {
                    None => (
                        crate::mascot::spinner(model.spinner_frame).to_string(),
                        theme.accent,
                    ),
                    Some(true) => (icon::OK.to_string(), theme.success),
                    Some(false) => (icon::ERR.to_string(), theme.error),
                };
                let mut label = format!("{} {name}", icon::TOOL);
                if let Some(d) = diff_stat {
                    label.push_str(&format!("  {d}"));
                }
                let mut content = Vec::new();
                if !summary.is_empty() {
                    content.push(Line::from(Span::styled(
                        truncate(summary, inner),
                        theme.subtle(),
                    )));
                }
                if let Some(p) = preview {
                    if !p.is_empty() {
                        content.push(Line::from(Span::styled(truncate(p, inner), theme.dim())));
                    }
                }
                if let Some(d) = diff {
                    content.extend(crate::diff::render_unified(d, theme, 14));
                }
                if content.is_empty() {
                    content.push(Line::from(Span::styled("…", theme.dim())));
                }
                push_card(&mut lines, &mark, &label, color, content, width);
            }
            Entry::Notice(t) => {
                lines.push(Line::from(Span::styled(
                    format!("  · {t}"),
                    theme.dim().add_modifier(Modifier::ITALIC),
                )));
            }
        }
        lines.push(Line::raw(""));
    }

    // Animated mascot while the agent works but hasn't produced visible output.
    if model.busy && model.streaming.is_none() {
        lines.push(Line::from(crate::mascot::thinking(model.spinner_frame)));
        if model.show_reasoning {
            if let Some(th) = &model.thinking {
                if !th.trim().is_empty() {
                    for l in wrap_lines(th, inner, theme.dim().add_modifier(Modifier::ITALIC)) {
                        let mut spans = vec![Span::styled("  ", theme.dim())];
                        spans.extend(l.spans);
                        lines.push(Line::from(spans));
                    }
                }
            }
        }
    }
    if let Some(s) = &model.streaming {
        let content = crate::markdown::render_markdown(s, inner, theme);
        push_card(
            &mut lines,
            icon::FLOWER,
            "blumi",
            theme.primary,
            content,
            width,
        );
    }
    lines
}

/// Wrap plain text into styled lines at `width`.
fn wrap_lines(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let wrapped = textwrap::wrap(para, width.max(8));
        if wrapped.is_empty() {
            out.push(Line::raw(""));
            continue;
        }
        for piece in wrapped {
            out.push(Line::from(Span::styled(piece.to_string(), style)));
        }
    }
    out
}

/// Push a titled, left-accented card — a header rule, a coloured left gutter
/// around `content`, and a bottom rule. Reads as a coloured box.
fn push_card(
    out: &mut Vec<Line<'static>>,
    glyph: &str,
    label: &str,
    color: Color,
    content: Vec<Line<'static>>,
    width: usize,
) {
    let head = format!("{} {glyph} {label} ", icon::TL);
    let used = head.chars().count();
    let rule = icon::H.repeat(width.saturating_sub(used).max(1));
    out.push(Line::from(Span::styled(
        format!("{head}{rule}"),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )));
    let gutter = Style::default().fg(color);
    for l in content {
        let mut spans = vec![Span::styled(format!("{} ", icon::V), gutter)];
        spans.extend(l.spans);
        out.push(Line::from(spans));
    }
    out.push(Line::from(Span::styled(
        format!(
            "{}{}",
            icon::BL,
            icon::H.repeat(width.saturating_sub(1).max(1))
        ),
        gutter,
    )));
}

/// A textual progress bar like `███████░░░`.
fn bar(frac: f64, width: usize) -> String {
    let w = width.max(1);
    let filled = (frac.clamp(0.0, 1.0) * w as f64).round() as usize;
    format!(
        "{}{}",
        icon::BAR_FULL.repeat(filled),
        icon::BAR_EMPTY.repeat(w - filled)
    )
}

fn push_wrapped(
    out: &mut Vec<Line<'static>>,
    text: &str,
    width: usize,
    style: Style,
    prefix: &str,
) {
    let w = width.saturating_sub(prefix.len()).max(8);
    let indent = " ".repeat(prefix.len());
    let mut first = true;
    for para in text.split('\n') {
        let wrapped = textwrap::wrap(para, w);
        if wrapped.is_empty() {
            out.push(Line::raw(""));
            continue;
        }
        for piece in wrapped {
            let pfx = if first { prefix } else { indent.as_str() };
            out.push(Line::from(vec![
                Span::raw(pfx.to_string()),
                Span::styled(piece.to_string(), style),
            ]));
            first = false;
        }
    }
}

fn render_editor(model: &mut Model, f: &mut Frame, area: Rect, theme: &Theme) {
    let border = if model.focus == Focus::Editor {
        theme.primary
    } else {
        theme.fg_dim
    };
    let title = if model.busy {
        " working… (esc to cancel) "
    } else {
        " message "
    };
    model.input.set_block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border))
            .title(Span::styled(title, theme.subtle())),
    );
    model.input.set_cursor_line_style(Style::default());
    f.render_widget(&model.input, area);
}

/// A 10-cell block context bar, hermes-style: `████░░░░░░`.
fn ctx_bar(frac: f64, w: usize) -> String {
    let p = (frac.clamp(0.0, 1.0) * w as f64).round() as usize;
    format!("{}{}", "█".repeat(p), "░".repeat(w.saturating_sub(p)))
}

/// Bar color by fill, hermes thresholds: green → gold → orange → red.
fn ctx_bar_color(frac: f64, theme: &Theme) -> Color {
    let pct = frac * 100.0;
    if pct >= 95.0 {
        theme.error
    } else if pct > 80.0 {
        Color::Indexed(208) // orange
    } else if pct >= 50.0 {
        Color::Indexed(214) // amber/gold
    } else {
        theme.success
    }
}

/// A rotating "still working" charm for long-running turns (hermes-style),
/// changing every ~10s so the user knows it's alive.
fn long_run_charm(secs: u64) -> &'static str {
    const CHARMS: [&str; 4] = [
        "still cooking…",
        "polishing edges…",
        "asking the void nicely…",
        "almost there…",
    ];
    CHARMS[((secs / 10) as usize) % CHARMS.len()]
}

/// The status rule directly above the input (hermes-style): the live working
/// indicator while busy, plus the model + context meter + tokens + cost.
fn render_inforule(model: &Model, f: &mut Frame, area: Rect, theme: &Theme) {
    let frac = model.context_frac();
    let mut spans: Vec<Span> = vec![Span::styled("─ ", theme.dim())];

    if model.busy {
        // Working indicator: spinner · elapsed (· charm once it's been a while).
        let secs = model.busy_secs();
        spans.push(Span::styled(
            format!("{} ", crate::mascot::spinner(model.spinner_frame)),
            theme.accent(),
        ));
        let work = if secs >= 8 {
            format!("working · {} · {}", fmt_dur(secs), long_run_charm(secs))
        } else if secs > 0 {
            format!("working · {}", fmt_dur(secs))
        } else {
            "working".to_string()
        };
        spans.push(Span::styled(work, theme.accent()));
    } else {
        let model_name = if model.model_name.is_empty() {
            "default"
        } else {
            &model.model_name
        };
        spans.push(Span::styled(model_name.to_string(), theme.subtle()));
    }

    // Context meter: used/max [bar] pct, colored by fill.
    spans.push(Span::styled(" │ ", theme.dim()));
    spans.push(Span::styled(
        format!(
            "{}/{} ",
            fmt_k(model.context_tokens),
            fmt_k(model.context_size)
        ),
        theme.subtle(),
    ));
    let bar_color = ctx_bar_color(frac, theme);
    spans.push(Span::styled(
        format!("[{}]", ctx_bar(frac, 10)),
        Style::default().fg(bar_color),
    ));
    spans.push(Span::styled(
        format!(" {}%", (frac * 100.0).round() as u32),
        Style::default().fg(bar_color),
    ));

    // Tokens ↑↓ and (if any) cost.
    spans.push(Span::styled(
        format!(
            " │ ↑{} ↓{}",
            fmt_k(model.input_tokens),
            fmt_k(model.output_tokens)
        ),
        theme.dim(),
    ));
    if model.cost_usd > 0.0 {
        spans.push(Span::styled(
            format!(" │ ${:.4}", model.cost_usd),
            theme.dim(),
        ));
    }

    // A single-row Paragraph clips to the area width.
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_status(model: &Model, f: &mut Frame, area: Rect, theme: &Theme) {
    // The autonomous loop owns the status line while running/paused (ralph-style).
    if model.loop_active || model.loop_current.is_some() {
        let cur = model
            .loop_current
            .as_ref()
            .map(|(_, t)| t.as_str())
            .unwrap_or("");
        let label = if model.loop_active {
            format!(
                "⟳ loop · iter {} · {cur}   (/loop to pause)",
                model.loop_iter
            )
        } else {
            format!(
                "⏸ loop paused · iter {}   (/loop to resume)",
                model.loop_iter
            )
        };
        let width = area.width as usize;
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                truncate(&label, width),
                theme.bold_primary(),
            ))),
            area,
        );
        return;
    }
    let hint = if model.dialog.is_some() {
        "type to filter · ↑/↓ move · enter select · esc close"
    } else if model.pending.is_some() {
        "[a] allow once   [s] allow session   [d] deny"
    } else if model.busy {
        "esc cancel · ctrl+c quit"
    } else {
        "enter send · / commands · ctrl+p palette · tab focus · ctrl+c quit"
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(hint, theme.dim()))),
        area,
    );
}

/// The scrollable plan-review modal (the `ExitPlanMode` approval popup).
fn render_plan_review(model: &Model, f: &mut Frame, area: Rect, theme: &Theme) {
    let Some(p) = &model.plan_review else { return };
    let popup = centered_rect(80, 80, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.primary))
        .title(Span::styled(" ◑ plan review ", theme.bold_primary()));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let [body, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);

    let width = body.width.saturating_sub(1) as usize;
    let lines = crate::markdown::render_markdown(&p.plan, width, theme);
    let max_scroll = (lines.len() as u16).saturating_sub(body.height);
    let scroll = p.scroll.min(max_scroll);
    f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), body);

    let hint = Line::from(vec![
        Span::styled(" [a]", theme.accent()),
        Span::styled(" approve  ", theme.dim()),
        Span::styled("[d]", theme.accent()),
        Span::styled(" reject  ", theme.dim()),
        Span::styled("↑/↓", theme.accent()),
        Span::styled(" scroll  ", theme.dim()),
        Span::styled("esc", theme.accent()),
        Span::styled(" reject", theme.dim()),
    ]);
    f.render_widget(Paragraph::new(hint), footer);
}

fn render_approval(model: &Model, f: &mut Frame, area: Rect, theme: &Theme) {
    let Some(p) = &model.pending else { return };
    let popup = centered_rect(70, 50, area);
    f.render_widget(Clear, popup);

    let title_style = if p.dangerous {
        Style::default()
            .fg(theme.error)
            .add_modifier(Modifier::BOLD)
    } else {
        theme.bold_primary()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if p.dangerous {
            theme.error
        } else {
            theme.primary
        }))
        .title(Span::styled(
            if p.dangerous {
                " permission — dangerous "
            } else {
                " permission "
            },
            title_style,
        ));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let width = inner.width.saturating_sub(1) as usize;
    let mut lines = vec![
        Line::from(Span::styled(p.tool.clone(), theme.accent())),
        Line::raw(""),
    ];
    push_wrapped(&mut lines, &p.summary, width, theme.body(), "");
    // Brain recommendation (advisory mode / auto-mode escalation).
    if let Some(advice) = &p.advice {
        lines.push(Line::raw(""));
        push_wrapped(&mut lines, advice, width, theme.accent(), "");
    }
    if let Some(diff) = &p.diff {
        lines.push(Line::raw(""));
        lines.extend(crate::diff::render_unified(diff, theme, 12));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "[a] allow once    [s] allow for session    [d] deny",
        theme.subtle(),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let tail: String = s
            .chars()
            .rev()
            .take(max.saturating_sub(1))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("…{tail}")
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entry, Model, PendingApproval, PlanReview};
    use blumi_protocol::{RequestId, ToolCallId};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn render_to_string(model: &mut Model, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| render(model, f)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn landing_shows_flower_and_header() {
        let mut model = Model::new("test-model".into(), "/tmp/proj".into());
        let out = render_to_string(&mut model, 90, 30);
        eprintln!("\n{out}");
        assert!(out.contains("blumi"), "header wordmark");
        assert!(out.contains('✿'), "flower glyph");
        assert!(out.contains("local-first"), "landing tagline");
        assert!(out.contains("quick commands"), "command table heading");
        assert!(out.contains("/help"), "command table entry");
    }

    #[test]
    fn header_shows_remote_tabs() {
        let mut model = Model::new("m".into(), "/tmp".into());
        model.entries.push(Entry::User("hi".into())); // non-empty so header renders
        model.tabs = vec![("local".into(), false), ("prod-box".into(), true)];
        model.active_tab = 1;
        let out = render_to_string(&mut model, 100, 24);
        assert!(out.contains("local"), "local tab shown");
        assert!(out.contains("prod-box"), "remote tab shown");
        assert!(out.contains('☁'), "remote glyph shown");
    }

    #[test]
    fn sidebar_and_active_agents_render() {
        use crate::model::{AgentCard, AgentStatus, Workspace};
        let mut model = Model::new("m".into(), "/tmp/proj".into());
        model.show_dashboard = true;
        model.entries.push(Entry::User("hi".into())); // non-empty → dashboard shows
        model.workspaces = vec![
            Workspace {
                name: "blumi-cli".into(),
                path: "/x/blumi-cli".into(),
                pinned: true,
            },
            Workspace {
                name: "mono".into(),
                path: "/x/mono".into(),
                pinned: false,
            },
        ];
        model.recent_sessions = vec![("s1".into(), "fix parser".into())];
        model.agents = vec![
            AgentCard {
                id: "a1".into(),
                role: "Coder".into(),
                task: "edit src/x.rs".into(),
                status: AgentStatus::Working,
            },
            AgentCard {
                id: "a2".into(),
                role: "Verify".into(),
                task: "tests pass".into(),
                status: AgentStatus::Done,
            },
        ];
        // Wide enough for both side panes. Default tab = Workspaces.
        let out = render_to_string(&mut model, 130, 30);
        assert!(out.contains("explorer"), "explorer pane");
        assert!(out.contains("Workspaces"), "workspaces tab");
        assert!(out.contains("Sessions"), "sessions tab");
        assert!(out.contains("blumi-cli"), "workspace entry (active tab)");
        assert!(out.contains("Active agents"), "active-agents section");
        assert!(out.contains("Coder"), "agent role");

        // Switch to the Sessions tab → the session list shows.
        model.sidebar_tab = crate::model::SidebarTab::Sessions;
        let out = render_to_string(&mut model, 130, 30);
        assert!(out.contains("fix parser"), "session entry (sessions tab)");
    }

    #[test]
    fn inforule_shows_context_meter_and_working() {
        let mut model = Model::new("claude-sonnet".into(), "/tmp".into());
        model.entries.push(Entry::User("hi".into()));
        model.context_size = 1000;
        model.context_tokens = 500;
        let out = render_to_string(&mut model, 100, 24);
        assert!(out.contains('█'), "context bar has filled cells");
        assert!(out.contains("50%"), "context percent");
        assert!(out.contains("claude-sonnet"), "model name shown when idle");

        // Busy → the working indicator replaces the model label.
        model.busy = true;
        model.busy_since = Some(std::time::Instant::now());
        let out = render_to_string(&mut model, 100, 24);
        assert!(out.contains("working"), "working indicator while busy");
    }

    #[test]
    fn status_shows_loop_indicator() {
        let mut model = Model::new("m".into(), "/tmp".into());
        model.loop_active = true;
        model.loop_iter = 3;
        model.loop_current = Some(("t1".into(), "ship parser".into()));
        let out = render_to_string(&mut model, 80, 24);
        assert!(out.contains("⟳ loop"), "active loop badge");
        assert!(out.contains("iter 3"), "iteration counter");
        assert!(out.contains("ship parser"), "current task title");

        // Paused: current task retained but inactive.
        model.loop_active = false;
        let out = render_to_string(&mut model, 80, 24);
        assert!(out.contains("⏸ loop paused"), "paused loop badge");
    }

    #[test]
    fn header_shows_yolo_badge_when_on() {
        let mut model = Model::new("m".into(), "/tmp".into());
        // Off: no badge.
        assert!(!render_to_string(&mut model, 90, 24).contains("YOLO"));
        // On: a persistent header badge (the ⚡ is a wide glyph, so assert the
        // pieces rather than an exact substring — TestBackend pads wide cells).
        model.yolo = true;
        let out = render_to_string(&mut model, 90, 24);
        assert!(out.contains("YOLO"), "yolo badge label");
        assert!(out.contains('⚡'), "yolo badge glyph");
    }

    #[test]
    fn plan_badge_and_scrollable_modal() {
        let mut model = Model::new("m".into(), "/tmp".into());
        model.plan_mode = true;
        let out = render_to_string(&mut model, 90, 24);
        assert!(out.contains("PLAN"), "plan-mode header badge");

        model.plan_review = Some(PlanReview {
            request_id: RequestId::new(),
            plan: "# Plan\n\n1. first step\n2. second step\n".into(),
            scroll: 0,
        });
        let out = render_to_string(&mut model, 90, 30);
        assert!(out.contains("plan review"), "plan modal title");
        assert!(out.contains("first step"), "plan body");
        assert!(out.contains("approve"), "plan modal footer");
    }

    #[test]
    fn renders_conversation_and_tool() {
        let mut model = Model::new("m".into(), "/tmp".into());
        model.entries.push(Entry::User("hello there".into()));
        model
            .entries
            .push(Entry::Assistant("hi! how can I help?".into()));
        model.entries.push(Entry::Tool {
            id: ToolCallId::from("c1"),
            name: "Bash".into(),
            summary: "ls".into(),
            ok: Some(true),
            preview: Some("a.txt".into()),
            diff_stat: None,
            diff: None,
        });
        let out = render_to_string(&mut model, 80, 24);
        assert!(out.contains("hello there"));
        assert!(out.contains("how can I help"));
        assert!(out.contains("Bash"));
    }

    #[test]
    fn renders_markdown_assistant() {
        let mut model = Model::new("m".into(), "/tmp".into());
        model
            .entries
            .push(Entry::Assistant("# Title\n\nSome **bold** text.".into()));
        let out = render_to_string(&mut model, 80, 24);
        assert!(out.contains("Title"));
        assert!(out.contains("bold"));
    }

    #[test]
    fn renders_command_palette() {
        let mut model = Model::new("m".into(), "/tmp".into());
        model.dialog = Some(crate::dialog::Picker::command_palette());
        let out = render_to_string(&mut model, 80, 24);
        assert!(out.contains("Commands"));
        assert!(out.contains("Cycle theme"));
        assert!(out.contains("Quit"));
    }

    #[test]
    fn renders_tool_diff() {
        let mut model = Model::new("m".into(), "/tmp".into());
        model.entries.push(Entry::Tool {
            id: ToolCallId::from("c2"),
            name: "FileEdit".into(),
            summary: "edit a.rs".into(),
            ok: Some(true),
            preview: None,
            diff_stat: Some("+1 -1".into()),
            diff: Some("@@ -1 +1 @@\n-old\n+new".into()),
        });
        let out = render_to_string(&mut model, 80, 24);
        assert!(out.contains("FileEdit"));
        assert!(out.contains("new"));
    }

    #[test]
    fn renders_approval_modal() {
        let mut model = Model::new("m".into(), "/tmp".into());
        model.busy = true;
        model.pending = Some(PendingApproval {
            request_id: RequestId::from("r1"),
            tool: "Bash".into(),
            summary: "rm -rf build".into(),
            dangerous: true,
            diff: None,
            advice: Some("🧠 brain suggests deny — destroys build dir".into()),
        });
        let out = render_to_string(&mut model, 80, 24);
        assert!(out.contains("permission"));
        assert!(out.contains("rm -rf build"));
        assert!(out.contains("brain suggests deny"), "advice shown on card");
        assert!(out.contains("allow once"));
    }

    #[test]
    fn session_picker_renders() {
        let mut model = Model::new("m".into(), "/tmp".into());
        model.recent_sessions = vec![
            ("sess_abc".into(), "refactor parser".into()),
            ("sess_def".into(), "fix the web build".into()),
        ];
        model.dialog = Some(crate::dialog::Picker::session_picker(
            &model.recent_sessions,
        ));
        let out = render_to_string(&mut model, 90, 24);
        eprintln!("\n{out}");
        assert!(out.contains("Sessions"), "picker title");
        assert!(out.contains("New session"), "new-session entry");
        assert!(out.contains("refactor parser"), "a session title");
        assert!(out.contains('╭'), "rounded editor/border");
    }

    #[test]
    fn dashboard_and_cards_look() {
        use blumi_protocol::{Todo, TodoStatus};
        let mut model = Model::new(
            "claude-sonnet".into(),
            "/Users/ankur/AI_EXPERIMENTS/blumi-cli".into(),
        );
        model.context_size = 200_000;
        model.context_tokens = 84_000;
        model.input_tokens = 84_000;
        model.output_tokens = 5_200;
        model.turn_count = 3;
        model.persona = "architect".into();
        model.busy = true;
        model
            .entries
            .push(Entry::User("refactor the parser and add tests".into()));
        model.entries.push(Entry::Assistant(
            "Here's the plan:\n\n1. extract `tokenize`\n2. add `parse_expr`\n\n\
             ```rust\nfn parse() {}\n```"
                .into(),
        ));
        model.entries.push(Entry::Tool {
            id: ToolCallId::from("c1"),
            name: "FileEdit".into(),
            summary: "src/parser.rs".into(),
            ok: Some(true),
            preview: Some("Edited src/parser.rs (2 replacements)".into()),
            diff_stat: Some("+8 -2".into()),
            diff: None,
        });
        model.entries.push(Entry::Tool {
            id: ToolCallId::from("c2"),
            name: "Bash".into(),
            summary: "cargo test".into(),
            ok: None,
            preview: None,
            diff_stat: None,
            diff: None,
        });
        model
            .entries
            .push(Entry::Notice("context compacted (6 messages)".into()));
        model.todos = vec![
            Todo {
                id: "1".into(),
                content: "extract tokenize".into(),
                status: TodoStatus::Completed,
            },
            Todo {
                id: "2".into(),
                content: "add parse_expr".into(),
                status: TodoStatus::InProgress,
            },
            Todo {
                id: "3".into(),
                content: "write tests".into(),
                status: TodoStatus::Pending,
            },
        ];
        let out = render_to_string(&mut model, 110, 40);
        eprintln!("\n{out}");
        assert!(out.contains('╭'), "card top border");
        assert!(out.contains("agent"), "dashboard agent panel");
        assert!(out.contains("Context"), "context section");
        assert!(out.contains("Tasks"), "tasks section");
    }
}
