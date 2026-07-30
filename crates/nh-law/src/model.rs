//! Compiled immutable session policy and public policy views.

use crate::matcher::{exec_pattern_matches, first_match};
use std::collections::BTreeMap;

/// Session autonomy. Repository law cannot set this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Autonomy {
    #[default]
    Ask,
    Auto,
}

/// Effective decision for one access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Ask,
    Block(String),
}

/// Compiled, immutable policy for one session.
#[derive(Debug, Clone)]
pub struct Policy {
    pub(super) autonomy: Autonomy,
    pub(super) write_auto: Vec<String>,
    pub(super) write_ask: Vec<String>,
    pub(super) write_block: Vec<String>,
    pub(super) read_block: Vec<String>,
    pub(super) send_block: Vec<String>,
    pub(super) credential_audiences: BTreeMap<String, Vec<String>>,
    pub(super) exec_block: Vec<String>,
}

/// Owned, read-only projection of one compiled session policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyView {
    pub autonomy: Autonomy,
    pub auto_paths: Vec<String>,
    pub ask_paths: Vec<String>,
    pub block_paths: Vec<String>,
    pub block_commands: Vec<String>,
}

/// Constitution, policy, and non-fatal source warnings for one session.
#[derive(Debug, Clone)]
pub struct Law {
    pub constitution: String,
    pub policy: Policy,
    pub warnings: Vec<String>,
}

/// Caller-controlled load options.
#[derive(Debug, Clone, Copy)]
pub struct LoadOptions {
    pub cli_autonomy: Option<Autonomy>,
}

/// Inputs to deterministic constitution assembly.
#[derive(Debug, Clone)]
pub struct ConstitutionSources {
    pub bundled: Option<String>,
    pub user_law_text: Option<String>,
    pub repo_law_text: Option<String>,
    pub agents_md: Option<String>,
    pub memory: Option<String>,
}

impl Policy {
    /// Decide whether a normalized, forward-slashed relative path may be written.
    pub fn write_verdict(&self, rel_path: &str) -> Verdict {
        if let Some(pattern) = first_match(&self.write_block, rel_path) {
            return Verdict::Block(format!(
                "protected path ({pattern}) - held even at max autonomy"
            ));
        }
        if first_match(&self.write_ask, rel_path).is_some() {
            return Verdict::Ask;
        }
        if first_match(&self.write_auto, rel_path).is_some() {
            return Verdict::Allow;
        }
        match self.autonomy {
            Autonomy::Ask => Verdict::Ask,
            Autonomy::Auto => Verdict::Allow,
        }
    }

    /// Decide whether a normalized, forward-slashed relative path may be read.
    pub fn read_verdict(&self, rel_path: &str) -> Verdict {
        if let Some(pattern) = first_match(&self.read_block, rel_path) {
            return Verdict::Block(format!("protected read ({pattern})"));
        }
        Verdict::Allow
    }

    /// Decide whether a destination host may receive outbound data.
    pub fn send_verdict(&self, target_host: &str) -> Verdict {
        let normalized = target_host.to_ascii_lowercase();
        let normalized = normalized.strip_suffix('.').unwrap_or(&normalized);
        if let Some(pattern) = first_match(&self.send_block, normalized) {
            return Verdict::Block(format!("blocked destination ({pattern})"));
        }
        Verdict::Allow
    }

    /// Trusted destination hosts for one vault entry.
    pub fn approved_audiences(&self, entry: &str) -> Vec<String> {
        self.credential_audiences
            .get(entry)
            .cloned()
            .unwrap_or_default()
    }

    /// Decide whether a shell command is blocked. Execution is never auto-allowed.
    pub fn exec_verdict(&self, command: &str) -> Verdict {
        if let Some(pattern) = self
            .exec_block
            .iter()
            .find(|pattern| exec_pattern_matches(pattern, command))
        {
            return Verdict::Block(format!("blocked command ({pattern})"));
        }

        Verdict::Ask
    }

    pub fn autonomy(&self) -> Autonomy {
        self.autonomy
    }

    /// Copy the compiled rule classes for read-only user interfaces.
    pub fn view(&self) -> PolicyView {
        PolicyView {
            autonomy: self.autonomy,
            auto_paths: self.write_auto.clone(),
            ask_paths: self.write_ask.clone(),
            block_paths: self.write_block.clone(),
            block_commands: self.exec_block.clone(),
        }
    }
}
