//! Raskolnikov's Psychological Loop (RPL-Judgement): an adversarial,
//! regret-minimizing pre-execution reasoning pass that wraps the agent's tool
//! calls. Before a mutating batch touches the live system it (1) maps the blast
//! radius ("The Hypothesis"), (2) simulates branches and scores their paranoia
//! ("The Fever Dream"), (3) submits the best plan to an adversarial "Porfiry"
//! judge ("Anticipating Judgment"), (4) actuates the survivor ("The Strike"),
//! and (5) writes the predicted-vs-actual Error Delta back to memory
//! ("The Confession"). Dostoevsky by way of control theory: a standard agent
//! maximizes success; an RPL agent *minimizes regret*.
//!
//! This module is the pure, deterministic core — the data types, the
//! blast-radius classifier, the least-regret decision rule, and the Error-Delta
//! computation. The live loop (sub-agent branch simulation, the `Brain`-backed
//! judge, the `SemanticMemory` write) is wired in the agent runner on top of
//! these primitives.

use blumi_protocol::Capability;

/// What a planned action could touch — the surface RPL reasons about in Phase 1
/// ("The Hypothesis": exploration of bounds / the blast radius).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlastRadius {
    pub files_written: Vec<String>,
    pub commands: Vec<String>,
    pub network_hosts: Vec<String>,
    pub vcs_ops: Vec<String>,
    pub spawns: usize,
    /// Any command matched the destructive heuristic (`rm -rf`, force-push, …).
    pub destructive: bool,
}

impl BlastRadius {
    /// Map a flattened capability set (gathered from a batch of tool calls via
    /// [`crate::Tool::required_capabilities`]) to a blast radius. Read-only calls
    /// contribute no capabilities, so a read-only batch yields an empty radius.
    pub fn assess(caps: &[Capability]) -> Self {
        let mut b = BlastRadius::default();
        for c in caps {
            match c {
                Capability::FileRead { .. } => {}
                Capability::FileWrite { path } => {
                    b.files_written.push(path.display().to_string());
                }
                Capability::ProcessExec { command } => {
                    if crate::permissions::is_destructive(command) {
                        b.destructive = true;
                    }
                    b.commands.push(command.clone());
                }
                Capability::NetworkEgress { host } => b.network_hosts.push(host.clone()),
                Capability::VcsMutation { op, .. } => b.vcs_ops.push(op.clone()),
                Capability::AgentSpawn { .. } => b.spawns += 1,
            }
        }
        b
    }

    /// True when the action only reads / spawns — nothing that mutates the world.
    pub fn is_read_only(&self) -> bool {
        self.files_written.is_empty()
            && self.commands.is_empty()
            && self.network_hosts.is_empty()
            && self.vcs_ops.is_empty()
    }

    /// True when every effect is plausibly undoable. File writes are journalled
    /// (`/undo`); destructive commands, network egress, and VCS mutations are not.
    pub fn reversible(&self) -> bool {
        !self.destructive && self.network_hosts.is_empty() && self.vcs_ops.is_empty()
    }

    /// A 0–100 severity used to decide whether the full loop is worth its cost.
    pub fn severity(&self) -> u8 {
        let mut s: u32 = 0;
        if self.destructive {
            s += 60;
        }
        s += (self.commands.len() as u32).min(2) * 20;
        s += (self.vcs_ops.len() as u32).min(2) * 25;
        s += (self.network_hosts.len() as u32).min(2) * 15;
        s += (self.files_written.len() as u32).min(3) * 10;
        if !self.reversible() {
            s += 15;
        }
        s.min(100) as u8
    }

    /// Engage the full RPL loop only when a *mutating* batch clears the severity
    /// `threshold`; read-only / trivial batches keep the cheap path.
    pub fn warrants_review(&self, threshold: u8) -> bool {
        !self.is_read_only() && self.severity() >= threshold
    }

    /// Whether to engage the full loop, given whether *any* tool in the batch is
    /// non-read-only (`any_mutating`, from the registry's `is_read_only` flag).
    /// Beyond declared high-blast batches, this catches **opaque mutations** — a
    /// tool that mutates but declared no [`Capability`]s (e.g. an MCP or
    /// self-management tool), whose blast can't be assessed from capabilities —
    /// so an undeclared side effect is reviewed rather than silently skipped.
    pub fn should_review(&self, any_mutating: bool, threshold: u8) -> bool {
        self.warrants_review(threshold) || (any_mutating && self.is_read_only())
    }

    /// Predicted risk fed to the Confession's [`ErrorDelta`]: the declared
    /// `severity`, but **floored to `threshold`** for an opaque mutation (no
    /// declared capabilities) so an unknown effect is never recorded as "safe".
    pub fn predicted_risk(&self, any_mutating: bool, threshold: u8) -> u8 {
        if any_mutating && self.is_read_only() {
            self.severity().max(threshold)
        } else {
            self.severity()
        }
    }

    /// A one-line declaration of assumed risk (Raskolnikov's "Extraordinary Man"
    /// theory): what this action does and what protections it bypasses — for the
    /// trace and the Porfiry judge's prompt.
    pub fn declaration(&self) -> String {
        let mut parts = Vec::new();
        if self.destructive {
            parts.push("DESTRUCTIVE command".to_string());
        }
        if !self.commands.is_empty() {
            parts.push(format!("{} command(s)", self.commands.len()));
        }
        if !self.files_written.is_empty() {
            parts.push(format!("{} file write(s)", self.files_written.len()));
        }
        if !self.vcs_ops.is_empty() {
            parts.push(format!("vcs {}", self.vcs_ops.join("/")));
        }
        if !self.network_hosts.is_empty() {
            parts.push(format!("network {}", self.network_hosts.join("/")));
        }
        if parts.is_empty() {
            return "no mutating effects".to_string();
        }
        format!(
            "blast radius (severity {}, {}): {}",
            self.severity(),
            if self.reversible() {
                "reversible"
            } else {
                "IRREVERSIBLE"
            },
            parts.join("; ")
        )
    }
}

/// Per-branch worst-case assessment from the Fever-Dream simulation (Phase 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParanoiaScore {
    /// The branch index this score belongs to (stable across reordering).
    pub branch: usize,
    /// 0–100; higher = more dangerous / more likely to be regretted.
    pub risk: u8,
    pub reversible: bool,
    /// The predicted worst-case systemic failure for this branch.
    pub worst_case: String,
}

/// Choose the least-regret branch: lowest risk, then most reversible, then the
/// earliest branch (the agent's first instinct) as a stable tiebreak. Returns
/// the winning branch index, or `None` if there were no branches.
pub fn choose_least_regret(scores: &[ParanoiaScore]) -> Option<usize> {
    scores
        .iter()
        .min_by(|a, b| {
            a.risk
                .cmp(&b.risk)
                .then(b.reversible.cmp(&a.reversible)) // reversible (true) ranks first
                .then(a.branch.cmp(&b.branch))
        })
        .map(|s| s.branch)
}

/// The adversarial judge's ruling on the surviving plan (Phase 3, "Porfiry").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PorfiryVerdict {
    pub approved: bool,
    /// The flaw / ignored edge case Porfiry found (set when not approved).
    pub flaw: Option<String>,
}

/// The Confession (Phase 5): the gap between what Phase-2 predicted and what
/// actually happened — the "guilt"/regret signal written back to memory and fed
/// to value-based fitness (memory-fix 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorDelta {
    pub predicted_risk: u8,
    pub actual_failed: bool,
    /// 0–100: how far reality diverged from the sterile simulation.
    pub magnitude: u8,
}

impl ErrorDelta {
    /// Compute the delta from the chosen branch's predicted risk and the observed
    /// outcome. A clean success at low predicted risk ⇒ ~0 regret; a failure the
    /// simulation rated "safe" ⇒ high regret (the surprise most worth learning).
    pub fn compute(predicted_risk: u8, actual_failed: bool) -> Self {
        let magnitude = if actual_failed {
            // Failed despite a low predicted risk = maximal surprise; even an
            // anticipated failure carries a floor of regret.
            (100u8).saturating_sub(predicted_risk).max(40)
        } else {
            // Succeeded; small residual regret proportional to how scared we were.
            predicted_risk / 4
        };
        ErrorDelta {
            predicted_risk,
            actual_failed,
            magnitude,
        }
    }

    /// Compact episode line for memory (`kind="rpl_delta"`, `agent` namespace).
    pub fn episode_text(&self, action: &str) -> String {
        format!(
            "rpl action={action} predicted_risk={} outcome={} regret={}",
            self.predicted_risk,
            if self.actual_failed { "failed" } else { "ok" },
            self.magnitude
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_batch_is_low_blast() {
        let caps = vec![
            Capability::file_read("/x/a.rs"),
            Capability::file_read("/x/b.rs"),
        ];
        let b = BlastRadius::assess(&caps);
        assert!(b.is_read_only());
        assert!(b.reversible());
        assert_eq!(b.severity(), 0);
        assert!(!b.warrants_review(40));
    }

    #[test]
    fn destructive_command_is_high_blast() {
        let caps = vec![Capability::process_exec("rm -rf /tmp/stuff")];
        let b = BlastRadius::assess(&caps);
        assert!(b.destructive);
        assert!(!b.is_read_only());
        assert!(!b.reversible());
        assert!(b.severity() >= 60, "got {}", b.severity());
        assert!(b.warrants_review(40));
        assert!(b.declaration().contains("IRREVERSIBLE"));
    }

    #[test]
    fn file_writes_are_reversible_but_reviewable() {
        let caps = vec![
            Capability::file_write("/proj/a.rs"),
            Capability::file_write("/proj/b.rs"),
        ];
        let b = BlastRadius::assess(&caps);
        assert!(!b.is_read_only());
        assert!(b.reversible());
        assert_eq!(b.severity(), 20);
    }

    #[test]
    fn network_and_vcs_are_irreversible() {
        let caps = vec![
            Capability::NetworkEgress {
                host: "example.com".into(),
            },
            Capability::VcsMutation {
                repo: "r".into(),
                op: "push".into(),
            },
        ];
        let b = BlastRadius::assess(&caps);
        assert!(!b.reversible());
        assert!(b.warrants_review(30));
    }

    #[test]
    fn opaque_mutation_is_reviewed_with_floored_risk() {
        // A tool that mutates but declared no capabilities ⇒ empty blast radius.
        let empty = BlastRadius::assess(&[]);
        assert!(empty.is_read_only());
        // Genuinely read-only batch (nothing mutating) ⇒ skip the loop.
        assert!(!empty.should_review(false, 40));
        // Opaque mutation (a non-read-only tool with no declared caps) ⇒ review,
        // and its predicted risk is floored to the threshold (not "safe").
        assert!(empty.should_review(true, 40));
        assert_eq!(empty.predicted_risk(true, 40), 40);
        // A declared high-blast batch reviews regardless, at its real severity.
        let d = BlastRadius::assess(&[Capability::process_exec("rm -rf /x")]);
        assert!(d.should_review(true, 40));
        assert!(d.predicted_risk(false, 40) >= 60);
    }

    #[test]
    fn least_regret_prefers_low_risk_then_reversible_then_first() {
        let scores = vec![
            ParanoiaScore {
                branch: 0,
                risk: 50,
                reversible: true,
                worst_case: "a".into(),
            },
            ParanoiaScore {
                branch: 1,
                risk: 20,
                reversible: false,
                worst_case: "b".into(),
            },
            ParanoiaScore {
                branch: 2,
                risk: 20,
                reversible: true,
                worst_case: "c".into(),
            },
        ];
        // branch 1 and 2 tie on risk (20); 2 is reversible → wins.
        assert_eq!(choose_least_regret(&scores), Some(2));
        assert_eq!(choose_least_regret(&[]), None);
    }

    #[test]
    fn error_delta_punishes_surprise_failures() {
        // Failed though the sim thought it was safe ⇒ large regret.
        let surprise = ErrorDelta::compute(10, true);
        assert_eq!(surprise.magnitude, 90);
        // Anticipated failure still carries a floor.
        assert_eq!(ErrorDelta::compute(80, true).magnitude, 40);
        // Clean success ⇒ small residual regret.
        assert_eq!(ErrorDelta::compute(40, false).magnitude, 10);
        assert!(surprise.episode_text("file_write").contains("regret=90"));
    }
}
