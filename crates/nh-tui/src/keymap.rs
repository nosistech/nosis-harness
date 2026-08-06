//! One source of truth for the base-view key hints and contextual help.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct KeyBinding {
    pub(super) keys: &'static str,
    pub(super) action: &'static str,
    pub(super) detail: &'static str,
    show_in_hint: bool,
    hide_at_budget: bool,
    working_only: bool,
}

pub(super) const KEY_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        keys: "/",
        action: "commands",
        detail: " and tools",
        show_in_hint: true,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "↑↓",
        action: "scroll",
        detail: " transcript",
        show_in_hint: true,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "PgUp/PgDn",
        action: "page",
        detail: " through transcript",
        show_in_hint: false,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "End",
        action: "latest",
        detail: " transcript position (when idle)",
        show_in_hint: false,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "Ctrl+F",
        action: "search",
        detail: " displayed transcript",
        show_in_hint: true,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "Enter",
        action: "send",
        detail: " task; queue while working (not during approval)",
        show_in_hint: true,
        hide_at_budget: true,
        working_only: false,
    },
    KeyBinding {
        keys: "Ctrl+W",
        action: "delete",
        detail: " previous word (Ctrl+Backspace / Ctrl+H aliases)",
        show_in_hint: false,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "Ctrl+U",
        action: "clear",
        detail: " input line",
        show_in_hint: false,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "Ctrl+C",
        action: "interrupt / clear / exit",
        detail: " by state; exit needs two presses",
        show_in_hint: true,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "?/F1",
        action: "help",
        detail: " (F1 always; ? needs an empty input)",
        show_in_hint: true,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "Esc",
        action: "interrupt",
        detail: " the turn; close overlays; decline approvals",
        show_in_hint: true,
        hide_at_budget: false,
        working_only: true,
    },
    KeyBinding {
        keys: "y / a / n",
        action: "answer",
        detail: " approval yes / always / no",
        show_in_hint: false,
        hide_at_budget: false,
        working_only: false,
    },
];

pub(super) fn visible_key_bindings(
    budget_reached: bool,
    working: bool,
) -> impl Iterator<Item = &'static KeyBinding> {
    KEY_BINDINGS
        .iter()
        .filter(move |binding| !budget_reached || !binding.hide_at_budget)
        .filter(move |binding| working || !binding.working_only)
}

pub(super) fn key_hint_line(budget_reached: bool, working: bool) -> String {
    visible_key_bindings(budget_reached, working)
        .filter(|binding| binding.show_in_hint)
        .map(|binding| format!("{} {}", binding.keys, binding.action))
        .collect::<Vec<_>>()
        .join("   ")
}
