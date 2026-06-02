//! Rendering.

use crate::model::{Entry, Focus, Model};
use crate::theme::{icon, Theme};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

const MAX_CONTENT_WIDTH: u16 = 100;

pub fn render(model: &mut Model, f: &mut Frame) {
    let theme = model.theme;
    let area = f.area();

    let editor_h = (model.input.lines().len().clamp(1, 6) as u16) + 2;
    let [header, chat, editor, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(editor_h),
        Constraint::Length(1),
    ])
    .areas(area);

    render_header(model, f, header, &theme);
    if model.is_empty() {
        render_landing(f, chat, &theme);
    } else {
        render_chat(model, f, chat, &theme);
    }
    render_editor(model, f, editor, &theme);
    render_status(model, f, status, &theme);

    if model.pending.is_some() {
        render_approval(model, f, area, &theme);
    }
    if model.dialog.is_some() {
        render_dialog(model, f, area, &theme);
    }
}

fn render_dialog(model: &Model, f: &mut Frame, area: Rect, theme: &Theme) {
    let Some(d) = &model.dialog else { return };
    let popup = centered_rect(60, 50, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.primary))
        .title(Span::styled(format!(" {} ", d.title), theme.bold_primary()));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let [filter_area, list_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
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
        Layout::horizontal([Constraint::Min(0), Constraint::Length(20)]).areas(area);

    let mut spans = vec![
        Span::styled(format!("{} blumi", icon::FLOWER), theme.bold_primary()),
        Span::styled("  ·  ", theme.dim()),
        Span::styled(
            if model.model_name.is_empty() {
                "default".to_string()
            } else {
                model.model_name.clone()
            },
            theme.accent(),
        ),
        Span::styled("  ·  ", theme.dim()),
        Span::styled(shorten(&model.working_dir, 36), theme.subtle()),
    ];
    if model.busy {
        spans.push(Span::raw("   "));
        spans.extend(crate::mascot::thinking(model.spinner_frame));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), left_area);

    let meter = format!("↑{} ↓{}", model.input_tokens, model.output_tokens);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(meter, theme.dim()))).alignment(Alignment::Right),
        right_area,
    );
}

fn render_landing(f: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines = vec![Line::raw(""), Line::raw("")];
    lines.extend(
        crate::mascot::rose_logo()
            .into_iter()
            .map(|l| l.alignment(Alignment::Center)),
    );
    lines.push(Line::raw(""));
    lines
        .push(Line::from(Span::styled("blumi", theme.bold_primary())).alignment(Alignment::Center));
    lines.push(Line::raw(""));
    lines.push(
        Line::from(Span::styled(
            "a local-first agentic coding assistant",
            theme.subtle(),
        ))
        .alignment(Alignment::Center),
    );
    lines.push(Line::raw(""));
    lines.push(
        Line::from(Span::styled(
            "type a message below and press Enter",
            theme.dim(),
        ))
        .alignment(Alignment::Center),
    );
    let para = Paragraph::new(lines);
    f.render_widget(para, area);
}

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
    for entry in &model.entries {
        match entry {
            Entry::User(t) => {
                push_wrapped(
                    &mut lines,
                    t,
                    width,
                    theme.bold_primary(),
                    &format!("{} ", icon::BAR),
                );
            }
            Entry::Assistant(t) => {
                lines.extend(crate::markdown::render_markdown(t, width, theme));
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
                let (mark, style) = match ok {
                    None => (icon::PENDING, theme.accent()),
                    Some(true) => (icon::OK, Style::default().fg(theme.success)),
                    Some(false) => (icon::ERR, Style::default().fg(theme.error)),
                };
                let mut header = format!("{} {name}", mark);
                if !summary.is_empty() {
                    header.push_str(&format!(": {}", truncate(summary, width.saturating_sub(6))));
                }
                if let Some(d) = diff_stat {
                    header.push_str(&format!("  ({d})"));
                }
                lines.push(Line::from(Span::styled(header, style)));
                if let Some(p) = preview {
                    if !p.is_empty() {
                        lines.push(Line::from(Span::styled(
                            format!("    {}", truncate(p, width.saturating_sub(4))),
                            theme.dim(),
                        )));
                    }
                }
                if let Some(d) = diff {
                    for dl in crate::diff::render_unified(d, theme, 14) {
                        let mut spans = vec![Span::raw("    ")];
                        spans.extend(dl.spans);
                        lines.push(Line::from(spans));
                    }
                }
            }
            Entry::Notice(t) => {
                lines.push(Line::from(Span::styled(
                    format!("— {t}"),
                    theme.dim().add_modifier(Modifier::ITALIC),
                )));
            }
        }
        lines.push(Line::raw(""));
    }

    // Animated mascot while the agent works but hasn't produced output yet.
    if model.busy && model.streaming.is_none() && model.thinking.is_none() {
        lines.push(Line::from(crate::mascot::thinking(model.spinner_frame)));
    }
    if let Some(th) = &model.thinking {
        if !th.trim().is_empty() {
            push_wrapped(&mut lines, th, width, theme.dim(), "  ");
        }
    }
    if let Some(s) = &model.streaming {
        lines.extend(crate::markdown::render_markdown(s, width, theme));
    }
    lines
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
            .border_style(Style::default().fg(border))
            .title(Span::styled(title, theme.subtle())),
    );
    model.input.set_cursor_line_style(Style::default());
    f.render_widget(&model.input, area);
}

fn render_status(model: &Model, f: &mut Frame, area: Rect, theme: &Theme) {
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
    use crate::model::{Entry, Model, PendingApproval};
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
        let out = render_to_string(&mut model, 80, 24);
        assert!(out.contains("blumi"), "header wordmark");
        assert!(out.contains('✿'), "flower glyph");
        assert!(out.contains("local-first"), "landing tagline");
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
        });
        let out = render_to_string(&mut model, 80, 24);
        assert!(out.contains("permission"));
        assert!(out.contains("rm -rf build"));
        assert!(out.contains("allow once"));
    }
}
