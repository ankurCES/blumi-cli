//! Agent personas: configurable top-level personalities/roles.
//!
//! A persona layers extra instructions onto the base system prompt and may
//! suggest a model + temperature, shaping how the top-level agent behaves
//! (e.g. a terse "reviewer", a careful "architect", a fast "pair"). Personas
//! are selected at startup and switchable at runtime (`Command::SetPersona`).
//! Distinct from sub-agents ([`crate::AgentDef`]), which are delegated workers.

/// A configurable agent personality.
#[derive(Debug, Clone, PartialEq)]
pub struct Persona {
    pub name: String,
    pub description: String,
    /// Appended to the base system prompt to shape behavior (may be empty).
    pub instructions: String,
    /// Optional model id to switch to when this persona activates.
    pub model: Option<String>,
    /// Optional sampling temperature override.
    pub temperature: Option<f32>,
}

impl Persona {
    /// A no-op persona (no extra instructions / overrides).
    pub fn plain(name: impl Into<String>) -> Self {
        Persona {
            name: name.into(),
            description: String::new(),
            instructions: String::new(),
            model: None,
            temperature: None,
        }
    }
}

impl Default for Persona {
    fn default() -> Self {
        Persona::plain("default")
    }
}

impl Persona {
    fn new(name: &str, description: &str, instructions: &str, temperature: Option<f32>) -> Self {
        Persona {
            name: name.to_string(),
            description: description.to_string(),
            instructions: instructions.to_string(),
            model: None,
            temperature,
        }
    }
}

/// The built-in persona roster. The engine merges any configured personas over
/// these (by name), and `default` is the no-op baseline.
pub fn builtin_personas() -> Vec<Persona> {
    vec![
        Persona::new("default", "Balanced general coding assistant.", "", None),
        Persona::new(
            "architect",
            "Designs before coding; weighs trade-offs.",
            "Adopt an architect's mindset. Before large changes, investigate the code and \
             propose a short plan, calling out trade-offs, risks, and alternatives. Prefer \
             clear, maintainable designs over clever ones.",
            Some(0.5),
        ),
        Persona::new(
            "pair",
            "Fast, terse pair programmer.",
            "Act as a fast pair programmer. Make the smallest change that works, run the \
             relevant check, and report briefly. Skip preamble; bias to action.",
            Some(0.7),
        ),
        Persona::new(
            "reviewer",
            "Critical code reviewer; finds bugs.",
            "Act as a meticulous code reviewer. Read carefully and point out bugs, edge \
             cases, security issues, and style problems with concrete, minimal fixes. Do not \
             modify files unless explicitly asked — review first.",
            Some(0.3),
        ),
        Persona::new(
            "explainer",
            "Teaches and explains as it works.",
            "Explain your reasoning clearly as you work, with small concrete examples. Favor \
             readable prose and short snippets so the user learns along the way.",
            Some(0.6),
        ),
        Persona::new(
            "team",
            "Lead that assembles a team of specialist agents.",
            "You are the LEAD of an agent team. Don't do the hands-on work yourself — \
             orchestrate it. For any non-trivial task:\n\
             1. Break the task into clear sub-tasks.\n\
             2. Delegate each sub-task to the fitting specialist with the `delegate` tool — \
             `Explore` for read-only investigation, `Plan` for design, `Coder` for \
             implementation, `Verify` for running checks/tests. Delegate one sub-task per \
             call with focused, self-contained instructions.\n\
             3. Review what each member reports; delegate follow-ups as needed.\n\
             4. When the team's work is done, synthesize a concise final summary.\n\
             Prefer delegation over doing it yourself; reserve direct tool use for quick glue \
             work. Keep the user informed of the plan and who is working on what.",
            Some(0.4),
        ),
    ]
}
