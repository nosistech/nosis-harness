//! User-owned capability mode shared by the UI and worker.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use nh_core::wire::ChatMessage;
use nh_law::Policy;
use nh_tools::{GuardFn, Tool};

pub(super) const PLAN_MODE_NOTICE: &str = "Plan mode is active. Until the user switches back to Change mode you can only read and search files: file edits, shell commands and remote tools are unavailable in this session and will be refused. Answer questions and explain directly. If the user asks for a change, investigate and reply with a concrete plan (the files to change, the steps, and the checks to run) instead of making the change. This overrides any earlier statement that file editing or the shell is available.";
pub(super) const CHANGE_MODE_NOTICE: &str = "Change mode is active and the restrictions of Plan mode no longer apply. File editing is available again, and so is the local shell when this session's policy allows it, subject to the current policy and explicit user approval.";
pub(super) const FINISH_BEFORE_SWITCHING: &str =
    "finish or stop the current turn before switching modes";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Plan,
    #[default]
    Change,
}

impl Mode {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Plan => "Plan",
            Self::Change => "Change",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Plan => "Plan (read-only)",
            Self::Change => "Change",
        }
    }

    pub(super) fn switched_notice(self) -> &'static str {
        match self {
            Self::Plan => "mode: Plan (read-only) - Nosis can read and search files, but cannot edit files, run commands, or use remote tools. Shift+Tab or /change to switch.",
            Self::Change => "mode: Change - Nosis can edit files and run commands. Commands always ask first; edits follow your approval settings. Shift+Tab or /plan to switch.",
        }
    }

    pub(super) fn toggled(self) -> Self {
        match self {
            Self::Plan => Self::Change,
            Self::Change => Self::Plan,
        }
    }

    pub(super) fn tools(self, policy: &Policy) -> Vec<Box<dyn Tool>> {
        match self {
            Self::Plan => nh_tools::read_only_tools(),
            Self::Change => nh_tools::builtin_tools_for_policy(policy),
        }
    }

    pub(super) fn guard(self, policy: Policy) -> GuardFn {
        let guard = nh_tools::policy_guard(policy);
        match self {
            Self::Plan => nh_tools::read_only_guard(guard),
            Self::Change => guard,
        }
    }
}

/// One stored mode, shared rather than mirrored across the UI/worker boundary.
/// Only user input between turns writes it. Worker notifications carry no stale
/// mode payload; each task derives its capabilities from this value at send time.
#[derive(Clone, Default)]
pub(super) struct SharedMode(Arc<AtomicBool>);

impl SharedMode {
    pub(super) fn from_history(history: &[ChatMessage]) -> Self {
        let mode = Self::default();
        mode.set(last_mode(history).unwrap_or_default());
        mode
    }

    pub(super) fn get(&self) -> Mode {
        if self.0.load(Ordering::Acquire) {
            Mode::Plan
        } else {
            Mode::Change
        }
    }

    pub(super) fn set(&self, mode: Mode) {
        self.0.store(mode == Mode::Plan, Ordering::Release);
    }
}

pub(super) fn last_mode(history: &[ChatMessage]) -> Option<Mode> {
    history.iter().rev().find_map(|message| {
        if message.role != "system" {
            return None;
        }
        let content = message.content.as_deref()?;
        match (
            content.rfind(PLAN_MODE_NOTICE),
            content.rfind(CHANGE_MODE_NOTICE),
        ) {
            (Some(plan), Some(change)) if change > plan => Some(Mode::Change),
            (Some(_), _) => Some(Mode::Plan),
            (None, Some(_)) => Some(Mode::Change),
            (None, None) => None,
        }
    })
}

pub(super) fn mode_context_update(history: &[ChatMessage], mode: Mode) -> Option<&'static str> {
    // Never displace history[0]: the agent inserts its constitution only into
    // empty history. An initial Plan notice belongs inside that constitution.
    if history.is_empty() {
        return None;
    }
    match (last_mode(history).unwrap_or_default(), mode) {
        (Mode::Plan, Mode::Change) => Some(CHANGE_MODE_NOTICE),
        (Mode::Change, Mode::Plan) => Some(PLAN_MODE_NOTICE),
        _ => None,
    }
}
