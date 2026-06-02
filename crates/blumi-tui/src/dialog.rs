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

    /// A picker over recent sessions (+ a "new session" entry on top).
    pub fn session_picker(sessions: &[(String, String)]) -> Self {
        let mut items = vec![PickerItem {
            label: "✿ New session".into(),
            hint: "fresh".into(),
            action: Action::NewSession,
        }];
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
        assert_eq!(p.filtered.len(), 3);
        p.push_char('t');
        p.push_char('h');
        // "theme" should rank in; "Quit" should drop out
        let labels: Vec<&str> = p.rows().iter().map(|(l, _, _)| *l).collect();
        assert!(labels.iter().any(|l| l.contains("theme")));
        assert!(!labels.contains(&"Quit"));
    }

    #[test]
    fn selection_action() {
        let p = Picker::command_palette();
        assert_eq!(p.selected_action(), Some(Action::ClearTranscript));
    }
}
