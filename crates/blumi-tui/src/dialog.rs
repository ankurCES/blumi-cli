//! A fuzzy-filterable picker dialog, used for the command palette. (A model
//! picker will reuse this once a provider model catalog exists — Phase 3.)

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// An action a palette entry performs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    ClearTranscript,
    CycleTheme,
    NewSession,
    ResumeSession(String),
    /// Attach live to a configured remote/gateway by name (same as `/remote`).
    AttachRemote(String),
    SetModel(String),
    SetProvider(String),
    // Menu entries that open a focused sub-picker.
    OpenSessions,
    OpenModels,
    OpenProviders,
    ToggleYolo,
}

pub struct PickerItem {
    pub label: String,
    pub hint: String,
    pub action: Action,
}

/// A filterable list dialog.
pub struct Picker {
    pub title: String,
    items: Vec<PickerItem>,
    pub filter: String,
    pub filtered: Vec<usize>,
    pub selected: usize,
}

impl Picker {
    pub fn command_palette() -> Self {
        let items = vec![
            PickerItem {
                label: "Switch session".into(),
                hint: "ctrl+s".into(),
                action: Action::OpenSessions,
            },
            PickerItem {
                label: "New session".into(),
                hint: "/new".into(),
                action: Action::NewSession,
            },
            PickerItem {
                label: "Pick model".into(),
                hint: "/model".into(),
                action: Action::OpenModels,
            },
            PickerItem {
                label: "Pick provider".into(),
                hint: "/provider".into(),
                action: Action::OpenProviders,
            },
            PickerItem {
                label: "Toggle auto-approve (yolo)".into(),
                hint: "ctrl+y".into(),
                action: Action::ToggleYolo,
            },
            PickerItem {
                label: "Clear transcript".into(),
                hint: "/clear".into(),
                action: Action::ClearTranscript,
            },
            PickerItem {
                label: "Cycle theme".into(),
                hint: "/theme".into(),
                action: Action::CycleTheme,
            },
            PickerItem {
                label: "Quit".into(),
                hint: "ctrl+c".into(),
                action: Action::Quit,
            },
        ];
        let mut p = Picker {
            title: "Commands".into(),
            items,
            filter: String::new(),
            filtered: Vec::new(),
            selected: 0,
        };
        p.refilter();
        p
    }

    /// A picker over recent sessions, with a "new session" entry on top and any
    /// configured live gateways/remotes listed right below it (hint `"live"`, so
    /// the view marks them with a blinking dot) for one-tap attach.
    pub fn session_picker(sessions: &[(String, String)], remotes: &[String]) -> Self {
        let mut items = vec![PickerItem {
            label: "✿ New session".into(),
            hint: "fresh".into(),
            action: Action::NewSession,
        }];
        for name in remotes {
            items.push(PickerItem {
                label: name.clone(),
                hint: "live".into(),
                action: Action::AttachRemote(name.clone()),
            });
        }
        for (id, title) in sessions {
            let label = if title.trim().is_empty() {
                "(untitled)".to_string()
            } else {
                title.clone()
            };
            items.push(PickerItem {
                label,
                hint: id.clone(),
                action: Action::ResumeSession(id.clone()),
            });
        }
        let mut p = Picker {
            title: "Sessions".into(),
            items,
            filter: String::new(),
            filtered: Vec::new(),
            selected: 0,
        };
        p.refilter();
        p
    }

    /// A picker over suggested models for the active provider.
    pub fn model_picker(models: &[String], current: &str) -> Self {
        let items = models
            .iter()
            .map(|m| PickerItem {
                label: m.clone(),
                hint: if m == current {
                    "active".into()
                } else {
                    String::new()
                },
                action: Action::SetModel(m.clone()),
            })
            .collect();
        let mut p = Picker {
            title: "Model".into(),
            items,
            filter: String::new(),
            filtered: Vec::new(),
            selected: 0,
        };
        p.refilter();
        p
    }

    /// A picker over providers (unready ones are marked "add key").
    pub fn provider_picker(providers: &[crate::app::ProviderOpt], current: &str) -> Self {
        let items = providers
            .iter()
            .map(|p| PickerItem {
                label: p.label.clone(),
                hint: if p.name == current {
                    "active".into()
                } else if !p.ready {
                    "add key".into()
                } else {
                    String::new()
                },
                action: Action::SetProvider(p.name.clone()),
            })
            .collect();
        let mut p = Picker {
            title: "Provider".into(),
            items,
            filter: String::new(),
            filtered: Vec::new(),
            selected: 0,
        };
        p.refilter();
        p
    }

    pub fn refilter(&mut self) {
        let labels = self.items.iter().map(|i| i.label.as_str());
        self.filtered = fuzzy_filter(labels, &self.filter);
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }

    pub fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    pub fn push_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected = 0;
        self.refilter();
    }

    pub fn pop_char(&mut self) {
        self.filter.pop();
        self.selected = 0;
        self.refilter();
    }

    pub fn selected_action(&self) -> Option<Action> {
        self.filtered
            .get(self.selected)
            .map(|&i| self.items[i].action.clone())
    }

    /// (label, hint, is_selected) for each visible row.
    pub fn rows(&self) -> Vec<(&str, &str, bool)> {
        self.filtered
            .iter()
            .enumerate()
            .map(|(row, &i)| {
                (
                    self.items[i].label.as_str(),
                    self.items[i].hint.as_str(),
                    row == self.selected,
                )
            })
            .collect()
    }
}

fn fuzzy_filter<'a>(labels: impl Iterator<Item = &'a str>, query: &str) -> Vec<usize> {
    let labels: Vec<&str> = labels.collect();
    if query.trim().is_empty() {
        return (0..labels.len()).collect();
    }
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut buf = Vec::new();
    let mut scored: Vec<(usize, u32)> = labels
        .iter()
        .enumerate()
        .filter_map(|(i, label)| {
            pattern
                .score(Utf32Str::new(label, &mut buf), &mut matcher)
                .map(|s| (i, s))
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.into_iter().map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_filters_fuzzily() {
        let mut p = Picker::command_palette();
        assert_eq!(p.filtered.len(), 8);
        p.push_char('t');
        p.push_char('h');
        p.push_char('e');
        p.push_char('m');
        // "theme" should rank in; "Quit" should drop out
        let labels: Vec<&str> = p.rows().iter().map(|(l, _, _)| *l).collect();
        assert!(labels.iter().any(|l| l.contains("theme")));
        assert!(!labels.contains(&"Quit"));
    }

    #[test]
    fn selection_action() {
        let p = Picker::command_palette();
        assert_eq!(p.selected_action(), Some(Action::OpenSessions));
    }

    #[test]
    fn model_picker_yields_set_model() {
        let models = vec!["gpt-4o".to_string(), "o4-mini".to_string()];
        let p = Picker::model_picker(&models, "gpt-4o");
        assert_eq!(p.selected_action(), Some(Action::SetModel("gpt-4o".into())));
    }

    #[test]
    fn provider_picker_yields_set_provider() {
        let provs = vec![crate::app::ProviderOpt {
            name: "openai".into(),
            label: "OpenAI".into(),
            ready: false,
        }];
        let p = Picker::provider_picker(&provs, "anthropic");
        assert_eq!(
            p.selected_action(),
            Some(Action::SetProvider("openai".into()))
        );
    }
}
