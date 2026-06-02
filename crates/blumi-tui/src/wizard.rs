//! First-run / `blumi login` onboarding: a standalone, menu-based ratatui
//! screen that collects provider → endpoint (Foundry) → API key → model.
//! API-key based (no OAuth, by design — see the project notes).

use crate::app::{setup_terminal, teardown_terminal, Term};
use crate::theme::{icon, Theme};
use blumi_config::ProviderKind;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// A selectable provider in the wizard.
pub struct ProviderChoice {
    pub name: String,
    pub label: String,
    pub kind: ProviderKind,
    /// Pre-set base URL for providers with a fixed host; `None` → ask (Foundry).
    pub fixed_base_url: Option<String>,
    pub needs_key: bool,
    pub needs_endpoint: bool,
    pub model_hint: String,
}

/// The result of a completed wizard.
pub struct WizardOutcome {
    pub provider: String,
    pub kind: ProviderKind,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
    pub model: String,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum Step {
    Provider,
    Endpoint,
    ApiKey,
    Model,
}

struct Wizard {
    choices: Vec<ProviderChoice>,
    sel: usize,
    step: Step,
    endpoint: String,
    api_key: String,
    model: String,
    theme: Theme,
}

enum Flow {
    Continue,
    Cancel,
    Done(Box<WizardOutcome>),
}

/// Run the onboarding wizard on its own full-screen terminal. Returns the
/// chosen setup, or `None` if the user cancelled.
pub async fn run_onboarding(choices: Vec<ProviderChoice>) -> anyhow::Result<Option<WizardOutcome>> {
    if choices.is_empty() {
        return Ok(None);
    }
    let mut term = setup_terminal()?;
    let result = run_loop(&mut term, choices).await;
    let _ = teardown_terminal(&mut term);
    result
}

async fn run_loop(
    term: &mut Term,
    choices: Vec<ProviderChoice>,
) -> anyhow::Result<Option<WizardOutcome>> {
    let mut w = Wizard {
        endpoint: choices[0].fixed_base_url.clone().unwrap_or_default(),
        api_key: String::new(),
        model: String::new(),
        theme: Theme::default(),
        sel: 0,
        step: Step::Provider,
        choices,
    };
    let mut input = EventStream::new();
    loop {
        term.draw(|f| draw(&w, f))?;
        let Some(Ok(ev)) = input.next().await else {
            return Ok(None);
        };
        if let Event::Key(k) = ev {
            if k.kind == KeyEventKind::Release {
                continue;
            }
            if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(None);
            }
            match w.handle(k.code) {
                Flow::Cancel => return Ok(None),
                Flow::Done(outcome) => return Ok(Some(*outcome)),
                Flow::Continue => {}
            }
        }
    }
}

impl Wizard {
    fn cur(&self) -> &ProviderChoice {
        &self.choices[self.sel]
    }

    fn handle(&mut self, code: KeyCode) -> Flow {
        match self.step {
            Step::Provider => match code {
                KeyCode::Esc => return Flow::Cancel,
                KeyCode::Up => self.sel = self.sel.saturating_sub(1),
                KeyCode::Down if self.sel + 1 < self.choices.len() => {
                    self.sel += 1;
                }
                KeyCode::Enter => {
                    self.endpoint = self.cur().fixed_base_url.clone().unwrap_or_default();
                    self.api_key.clear();
                    self.model.clear();
                    self.step = if self.cur().needs_endpoint {
                        Step::Endpoint
                    } else if self.cur().needs_key {
                        Step::ApiKey
                    } else {
                        Step::Model
                    };
                }
                _ => {}
            },
            Step::Endpoint => match code {
                KeyCode::Esc => self.step = Step::Provider,
                KeyCode::Enter if !self.endpoint.trim().is_empty() => {
                    self.step = if self.cur().needs_key {
                        Step::ApiKey
                    } else {
                        Step::Model
                    };
                }
                KeyCode::Backspace => {
                    self.endpoint.pop();
                }
                KeyCode::Char(c) => self.endpoint.push(c),
                _ => {}
            },
            Step::ApiKey => match code {
                KeyCode::Esc => {
                    self.step = if self.cur().needs_endpoint {
                        Step::Endpoint
                    } else {
                        Step::Provider
                    };
                }
                KeyCode::Enter if !self.api_key.trim().is_empty() => {
                    self.step = Step::Model;
                }
                KeyCode::Backspace => {
                    self.api_key.pop();
                }
                KeyCode::Char(c) => self.api_key.push(c),
                _ => {}
            },
            Step::Model => match code {
                KeyCode::Esc => {
                    self.step = if self.cur().needs_key {
                        Step::ApiKey
                    } else if self.cur().needs_endpoint {
                        Step::Endpoint
                    } else {
                        Step::Provider
                    };
                }
                KeyCode::Enter if !self.model.trim().is_empty() => {
                    return Flow::Done(Box::new(self.outcome()));
                }
                KeyCode::Backspace => {
                    self.model.pop();
                }
                KeyCode::Char(c) => self.model.push(c),
                _ => {}
            },
        }
        Flow::Continue
    }

    fn outcome(&self) -> WizardOutcome {
        let c = self.cur();
        WizardOutcome {
            provider: c.name.clone(),
            kind: c.kind,
            endpoint: if c.needs_endpoint {
                Some(self.endpoint.trim().to_string())
            } else {
                c.fixed_base_url.clone()
            },
            api_key: if c.needs_key && !self.api_key.trim().is_empty() {
                Some(self.api_key.trim().to_string())
            } else {
                None
            },
            model: self.model.trim().to_string(),
        }
    }
}

fn draw(w: &Wizard, f: &mut Frame) {
    let t = &w.theme;
    let area = centered(64, 20, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(t.primary))
        .title(Span::styled(
            format!(" {} blumi setup ", icon::FLOWER),
            t.bold_primary(),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [body, hint] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);

    let mut lines: Vec<Line> = Vec::new();
    match w.step {
        Step::Provider => {
            lines.push(Line::from(Span::styled("Choose a provider:", t.subtle())));
            lines.push(Line::raw(""));
            for (i, c) in w.choices.iter().enumerate() {
                let selected = i == w.sel;
                let marker = if selected { "❯ " } else { "  " };
                let style = if selected { t.bold_primary() } else { t.body() };
                lines.push(Line::from(vec![
                    Span::styled(marker, t.accent()),
                    Span::styled(c.label.clone(), style),
                ]));
            }
        }
        Step::Endpoint => {
            field_summary(&mut lines, t, w);
            lines.push(Line::from(Span::styled(
                "Azure AI Foundry endpoint:",
                t.subtle(),
            )));
            lines.push(input_line(t, &w.endpoint, false));
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "e.g. https://my-resource.services.ai.azure.com",
                t.dim(),
            )));
        }
        Step::ApiKey => {
            field_summary(&mut lines, t, w);
            lines.push(Line::from(Span::styled("API key:", t.subtle())));
            lines.push(input_line(t, &w.api_key, true));
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                "stored in ~/.blumi/settings.json (0600)",
                t.dim(),
            )));
        }
        Step::Model => {
            field_summary(&mut lines, t, w);
            lines.push(Line::from(Span::styled("Model id:", t.subtle())));
            lines.push(input_line(t, &w.model, false));
            lines.push(Line::raw(""));
            lines.push(Line::from(Span::styled(
                format!("e.g. {}", w.cur().model_hint),
                t.dim(),
            )));
        }
    }
    f.render_widget(Paragraph::new(lines), body);

    let hint_text = match w.step {
        Step::Provider => "↑/↓ select · enter next · esc/ctrl+c cancel",
        _ => "type · enter next · esc back · ctrl+c cancel",
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(hint_text, t.dim()))).alignment(Alignment::Center),
        hint,
    );
}

fn field_summary(lines: &mut Vec<Line<'static>>, t: &Theme, w: &Wizard) {
    lines.push(Line::from(vec![
        Span::styled("provider: ", t.dim()),
        Span::styled(w.cur().label.clone(), t.accent()),
    ]));
    lines.push(Line::raw(""));
}

fn input_line(t: &Theme, value: &str, masked: bool) -> Line<'static> {
    let shown = if masked {
        "•".repeat(value.chars().count())
    } else {
        value.to_string()
    };
    Line::from(vec![
        Span::styled("  ", t.dim()),
        Span::styled(shown, t.body()),
        Span::styled("▏", t.accent()), // cursor
    ])
}

fn centered(w: u16, h: u16, area: Rect) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices() -> Vec<ProviderChoice> {
        vec![
            ProviderChoice {
                name: "anthropic".into(),
                label: "Anthropic".into(),
                kind: ProviderKind::Anthropic,
                fixed_base_url: Some("https://api.anthropic.com".into()),
                needs_key: true,
                needs_endpoint: false,
                model_hint: "claude-…".into(),
            },
            ProviderChoice {
                name: "azure-foundry".into(),
                label: "Azure AI Foundry".into(),
                kind: ProviderKind::AnthropicFoundry,
                fixed_base_url: None,
                needs_key: true,
                needs_endpoint: true,
                model_hint: "your-deployment".into(),
            },
            ProviderChoice {
                name: "local".into(),
                label: "Local (llama.cpp)".into(),
                kind: ProviderKind::OpenaiCompat,
                fixed_base_url: Some("http://localhost:7474/v1".into()),
                needs_key: false,
                needs_endpoint: false,
                model_hint: "".into(),
            },
        ]
    }

    #[test]
    fn anthropic_flow_skips_endpoint() {
        let mut w = Wizard {
            endpoint: String::new(),
            api_key: String::new(),
            model: String::new(),
            theme: Theme::default(),
            sel: 0,
            step: Step::Provider,
            choices: choices(),
        };
        // select Anthropic
        matches!(w.handle(KeyCode::Enter), Flow::Continue);
        assert_eq!(w.step, Step::ApiKey); // endpoint skipped
        for c in "sk-test".chars() {
            w.handle(KeyCode::Char(c));
        }
        w.handle(KeyCode::Enter);
        assert_eq!(w.step, Step::Model);
        for c in "claude-x".chars() {
            w.handle(KeyCode::Char(c));
        }
        let done = w.handle(KeyCode::Enter);
        match done {
            Flow::Done(o) => {
                assert_eq!(o.provider, "anthropic");
                assert_eq!(o.api_key.as_deref(), Some("sk-test"));
                assert_eq!(o.model, "claude-x");
                assert_eq!(o.endpoint.as_deref(), Some("https://api.anthropic.com"));
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn foundry_flow_asks_endpoint() {
        let mut w = Wizard {
            endpoint: String::new(),
            api_key: String::new(),
            model: String::new(),
            theme: Theme::default(),
            sel: 0,
            step: Step::Provider,
            choices: choices(),
        };
        w.handle(KeyCode::Down); // → azure-foundry
        w.handle(KeyCode::Enter);
        assert_eq!(w.step, Step::Endpoint);
        for c in "https://r.services.ai.azure.com".chars() {
            w.handle(KeyCode::Char(c));
        }
        w.handle(KeyCode::Enter);
        assert_eq!(w.step, Step::ApiKey);
    }

    #[test]
    fn local_flow_skips_key_and_endpoint() {
        let mut w = Wizard {
            endpoint: String::new(),
            api_key: String::new(),
            model: String::new(),
            theme: Theme::default(),
            sel: 0,
            step: Step::Provider,
            choices: choices(),
        };
        w.handle(KeyCode::Down);
        w.handle(KeyCode::Down); // → local
        w.handle(KeyCode::Enter);
        assert_eq!(w.step, Step::Model); // key + endpoint skipped
    }
}
