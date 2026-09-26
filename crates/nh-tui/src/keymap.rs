//! Binding table for contextual help plus compact, state-aware base-view hints.

use std::borrow::Cow;

use nh_core::terminal_capability::TerminalCapability;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct KeyBinding {
    pub(super) keys: &'static str,
    pub(super) action: &'static str,
    pub(super) detail: &'static str,
    show_in_hint: bool,
    hide_at_budget: bool,
    working_only: bool,
}

impl KeyBinding {
    pub(super) fn display_keys(self, terminal_capability: TerminalCapability) -> Cow<'static, str> {
        terminal_capability.render_text(self.keys)
    }
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
        detail: " transcript; multiline composer rows",
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
        detail: " transcript on empty input; otherwise line end",
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
        detail: " task; queue while working or waiting for approval",
        show_in_hint: true,
        hide_at_budget: true,
        working_only: false,
    },
    KeyBinding {
        keys: "Shift+Enter/Ctrl+J",
        action: "newline",
        detail: " in the composer without sending",
        show_in_hint: false,
        hide_at_budget: true,
        working_only: false,
    },
    KeyBinding {
        keys: "Left/Right",
        action: "move",
        detail: " the composer cursor",
        show_in_hint: false,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "Home/End",
        action: "move",
        detail: " to current composer line start/end",
        show_in_hint: false,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "Backspace/Delete",
        action: "delete",
        detail: " before/after the composer cursor",
        show_in_hint: false,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "Ctrl+W",
        action: "delete",
        detail: " previous word (Ctrl+Backspace/Ctrl+H aliases)",
        show_in_hint: false,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "Ctrl+U",
        action: "clear",
        detail: " the whole composer",
        show_in_hint: false,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "Ctrl+P",
        action: "recall",
        detail: " previous prompt outside approval",
        show_in_hint: false,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "Ctrl+N",
        action: "recall",
        detail: " next prompt or draft outside approval",
        show_in_hint: false,
        hide_at_budget: false,
        working_only: false,
    },
    KeyBinding {
        keys: "Ctrl+C",
        action: "interrupt / clear / exit",
        detail: " by state; two presses exit",
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
        keys: "F2 / F3 / F4",
        action: "answer",
        detail:
            " approval once / repeat identical shell command this session when offered / decline",
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

pub(super) fn key_hint_line_for(
    terminal_capability: TerminalCapability,
    budget_reached: bool,
    busy: bool,
    approval: bool,
    repeat_approval: bool,
    can_stop: bool,
    width: usize,
) -> String {
    let mut candidates = vec!["F1 help".to_owned()];
    if approval {
        if can_stop {
            candidates.push("Esc stop".to_owned());
        }
        candidates.push("F4 decline".to_owned());
        candidates.push("F2 approve".to_owned());
        if repeat_approval {
            candidates.push("F3 repeat".to_owned());
        }
        candidates.push(format!(
            "{} scroll/edit",
            terminal_capability.render_text("↑↓")
        ));
        candidates.push("PgUp/PgDn scroll".to_owned());
    } else {
        if can_stop {
            candidates.push("Esc stop".to_owned());
        }
        if !budget_reached {
            candidates.push(if busy {
                "Enter queue".to_owned()
            } else {
                "Enter send".to_owned()
            });
        }
        for binding in visible_key_bindings(budget_reached, can_stop).filter(|binding| {
            binding.show_in_hint && !matches!(binding.keys, "Enter" | "?/F1" | "Esc")
        }) {
            let action = if binding.keys == "Ctrl+C" && can_stop {
                "stop/exit"
            } else if binding.keys == "Ctrl+C" {
                "clear/exit"
            } else {
                binding.action
            };
            candidates.push(format!(
                "{} {action}",
                binding.display_keys(terminal_capability)
            ));
        }
    }

    let mut line = String::new();
    for candidate in candidates {
        let separator = if line.is_empty() { "" } else { "   " };
        let fits = line
            .chars()
            .count()
            .saturating_add(separator.len())
            .saturating_add(candidate.chars().count())
            <= width;
        if fits {
            line.push_str(separator);
            line.push_str(&candidate);
        } else if approval {
            break;
        }
    }
    line
}

#[cfg(test)]
pub(super) fn key_hint_line(
    budget_reached: bool,
    busy: bool,
    approval: bool,
    can_stop: bool,
    width: usize,
) -> String {
    key_hint_line_for(
        TerminalCapability::Unicode,
        budget_reached,
        busy,
        approval,
        approval,
        can_stop,
        width,
    )
}
