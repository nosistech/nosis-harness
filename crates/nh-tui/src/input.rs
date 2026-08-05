//! Keyboard, paste, command, and overlay input reduction.

mod commands;

#[cfg(test)]
pub(super) use commands::teaching_error;
pub(super) use commands::{command_matches, execute_command_menu, explain_why};
use commands::{resolved_route_action, set_profile};

use crate::palette::filter_palette;
use crate::state::{
    search_match_count, search_match_lines, AgentEvent, App, Overlay, PaletteAction, PaletteEntry,
    PickerKind, PickerRow, Status,
};
use crate::timeline::apply_event;
use crate::worker::{Worker, WorkerCommand};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use nh_core::agent::MAX_TASK_BYTES;
use nh_core::wire::ThinkingEffort;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum UiAction {
    None,
    Dispatch(String),
    SwitchRoute(String),
    SetEffort(ThinkingEffort),
    SetProfile(String),
    Quit,
}
pub(super) fn handle_input_event(app: &mut App, worker: &mut Worker, input: Event) -> bool {
    let action = reduce_input_event(app, input);
    handle_action(app, worker, action)
}

pub(super) fn handle_action(app: &mut App, worker: &mut Worker, action: UiAction) -> bool {
    match action {
        UiAction::None => false,
        UiAction::Quit => true,
        UiAction::Dispatch(task) => {
            if worker.commands.send(WorkerCommand::Task(task)).is_err() {
                apply_event(
                    app,
                    AgentEvent::Failed("agent stopped - retry the task".into()),
                );
            }
            false
        }
        UiAction::SwitchRoute(route_id) => {
            let route = match app.resolver.resolve(&route_id) {
                Ok(route) => route,
                Err(error) => {
                    apply_event(
                        app,
                        AgentEvent::Failed(format!("could not switch route: {error}")),
                    );
                    return false;
                }
            };
            if worker
                .commands
                .send(WorkerCommand::SwitchRoute(Box::new(route.clone())))
                .is_err()
            {
                apply_event(
                    app,
                    AgentEvent::Failed("agent stopped - retry the task".into()),
                );
            } else {
                app.switch_route(route);
            }
            false
        }
        UiAction::SetEffort(effort) => {
            if worker
                .commands
                .send(WorkerCommand::SetEffort(effort))
                .is_err()
            {
                apply_event(
                    app,
                    AgentEvent::Failed("agent stopped - retry the task".into()),
                );
            } else {
                app.set_effort(effort);
            }
            false
        }
        UiAction::SetProfile(profile) => {
            if worker
                .commands
                .send(WorkerCommand::SetProfile(profile))
                .is_err()
            {
                apply_event(
                    app,
                    AgentEvent::Failed("agent stopped - retry the task".into()),
                );
            }
            false
        }
    }
}

#[cfg(test)]
pub(super) fn handle_key(app: &mut App, worker: &mut Worker, key: KeyEvent) -> bool {
    let action = reduce_key(app, key);
    handle_action(app, worker, action)
}

pub(super) fn reduce_input_event(app: &mut App, input: Event) -> UiAction {
    match input {
        Event::Key(key) if key.kind == KeyEventKind::Press => reduce_key(app, key),
        Event::Paste(text) => reduce_paste(app, &text),
        _ => UiAction::None,
    }
}

pub(super) fn reduce_agent_event(app: &mut App, event: AgentEvent) -> (Status, UiAction) {
    let previous = app.status.clone();
    apply_event(app, event);
    let action = if app.pending_send
        && matches!(previous, Status::Working)
        && matches!(app.status, Status::Idle)
    {
        if app.input.starts_with('/') {
            app.pending_send = false;
            execute_command_menu(app)
        } else {
            app.dispatch().map_or(UiAction::None, UiAction::Dispatch)
        }
    } else {
        UiAction::None
    };
    (previous, action)
}

pub(super) fn reduce_key(app: &mut App, key: KeyEvent) -> UiAction {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(app.status, Status::Waiting) {
            app.answer_approval(false);
        }
        return UiAction::Quit;
    }
    if matches!(key.code, KeyCode::Char('f' | 'F')) && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        app.open_search();
        return UiAction::None;
    }
    if app.overlay != Overlay::None {
        return reduce_overlay_key(app, key);
    }
    if matches!(app.status, Status::Waiting) {
        if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() {
            match key.code {
                KeyCode::Char('y' | 'Y') => app.answer_approval(true),
                KeyCode::Char('a' | 'A') => app.answer_approval_with_rule(true, true),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => app.answer_approval(false),
                _ => {}
            }
        }
        return UiAction::None;
    }
    if matches!(app.status, Status::Working) {
        match key.code {
            KeyCode::Up if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                scroll_transcript(app, 1, true);
            }
            KeyCode::Down if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                scroll_transcript(app, 1, false);
            }
            KeyCode::PageUp if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                scroll_transcript(app, 5, true);
            }
            KeyCode::PageDown if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                scroll_transcript(app, 5, false);
            }
            KeyCode::Enter => {
                app.pending_send = !app.input.trim().is_empty();
            }
            KeyCode::Backspace => {
                app.input.pop();
                if app.input.trim().is_empty() {
                    app.pending_send = false;
                }
            }
            KeyCode::Char(character)
                if !character.is_control()
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                push_input_char(&mut app.input, character);
            }
            _ => {}
        }
        return UiAction::None;
    }
    match key.code {
        KeyCode::Enter => {
            if let Some(task) = app.dispatch() {
                return UiAction::Dispatch(task);
            }
        }
        KeyCode::Backspace => {
            app.input.pop();
            if app.input.trim().is_empty() {
                app.pending_send = false;
            }
        }
        KeyCode::Up => scroll_transcript(app, 1, true),
        KeyCode::Down => scroll_transcript(app, 1, false),
        KeyCode::PageUp => scroll_transcript(app, 5, true),
        KeyCode::PageDown => scroll_transcript(app, 5, false),
        KeyCode::End => app.scroll_back = 0,
        KeyCode::Char(character)
            if !character.is_control()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            push_input_char(&mut app.input, character);
            if app.input.starts_with('/') {
                app.overlay = Overlay::CommandMenu { selected: 0 };
            }
        }
        _ => {}
    }
    UiAction::None
}

pub(super) fn push_input_char(input: &mut String, character: char) -> bool {
    if input.len().saturating_add(character.len_utf8()) > MAX_TASK_BYTES {
        return false;
    }
    input.push(character);
    true
}

pub(super) fn reduce_paste(app: &mut App, text: &str) -> UiAction {
    let working = matches!(app.status, Status::Working);
    if matches!(app.status, Status::Waiting)
        || (working && app.overlay != Overlay::None)
        || (!working && !matches!(app.overlay, Overlay::None | Overlay::CommandMenu { .. }))
    {
        return UiAction::None;
    }

    for character in text.chars().filter_map(|character| match character {
        '\n' | '\r' | '\t' => Some(' '),
        character if character.is_control() => None,
        character => Some(character),
    }) {
        if !push_input_char(&mut app.input, character) {
            break;
        }
    }

    if working {
        return UiAction::None;
    }

    if app.input.starts_with('/') {
        if let Overlay::CommandMenu { selected } = &mut app.overlay {
            *selected = 0;
        } else {
            app.overlay = Overlay::CommandMenu { selected: 0 };
        }
    } else if matches!(app.overlay, Overlay::CommandMenu { .. }) {
        app.overlay = Overlay::None;
    }

    UiAction::None
}

pub(super) fn scroll_transcript(app: &mut App, amount: usize, toward_older: bool) {
    let max_scroll = app.max_scroll.get();
    let current = app.scroll_back.min(max_scroll);
    app.scroll_back = if toward_older {
        current.saturating_add(amount).min(max_scroll)
    } else {
        current.saturating_sub(amount)
    };
}

pub(super) fn reduce_overlay_key(app: &mut App, key: KeyEvent) -> UiAction {
    if matches!(app.overlay, Overlay::CommandMenu { .. }) {
        return reduce_command_menu_key(app, key);
    }
    if matches!(app.overlay, Overlay::Search { .. }) {
        return reduce_search_key(app, key);
    }
    if key.code == KeyCode::Esc {
        app.overlay = Overlay::None;
        return UiAction::None;
    }
    if matches!(app.overlay, Overlay::TrustDial) {
        return UiAction::None;
    }

    let timeline_len = app.timeline.len();
    if let Overlay::Timeline {
        selected,
        inspecting,
        note,
    } = &mut app.overlay
    {
        timeline_key(timeline_len, selected, inspecting, note, key);
        return UiAction::None;
    }

    let picked = match &mut app.overlay {
        Overlay::Picker {
            kind,
            selected,
            rows,
        } => picker_key(*kind, rows, selected, key),
        _ => None,
    };
    if let Some((kind, value)) = picked {
        app.overlay = Overlay::None;
        return match kind {
            PickerKind::Model => {
                let resolved = app.resolver.resolve(&value);
                resolved_route_action(app, resolved)
            }
            PickerKind::Provider => {
                let resolved = app.resolver.provider_default(&value);
                resolved_route_action(app, resolved)
            }
            PickerKind::Profile => set_profile(app, &value),
        };
    }
    if matches!(app.overlay, Overlay::Picker { .. }) {
        return UiAction::None;
    }

    let activated = match &mut app.overlay {
        Overlay::Palette {
            filter,
            selected,
            detail,
        } => palette_key(&app.palette_entries, filter, selected, detail, key),
        Overlay::None
        | Overlay::Search { .. }
        | Overlay::CommandMenu { .. }
        | Overlay::TrustDial
        | Overlay::Timeline { .. }
        | Overlay::Picker { .. } => None,
    };
    let Some(entry) = activated else {
        return UiAction::None;
    };
    activate_palette_entry(app, entry)
}

pub(super) fn reduce_search_key(app: &mut App, key: KeyEvent) -> UiAction {
    let match_count = match &app.overlay {
        Overlay::Search { query, .. } => {
            search_match_count(&search_match_lines(&app.transcript, query))
        }
        _ => return UiAction::None,
    };
    match key.code {
        KeyCode::Esc => {
            if let Overlay::Search {
                original_scroll, ..
            } = &app.overlay
            {
                app.scroll_back = *original_scroll;
            }
            app.overlay = Overlay::None;
        }
        KeyCode::Enter if match_count > 0 => {
            app.scroll_back = app.search_match_scroll.get();
            app.overlay = Overlay::None;
        }
        KeyCode::Backspace => {
            if let Overlay::Search {
                query, selected, ..
            } = &mut app.overlay
            {
                query.pop();
                *selected = 0;
            }
        }
        KeyCode::Up if match_count > 0 => {
            if let Overlay::Search { selected, .. } = &mut app.overlay {
                *selected = if *selected == 0 {
                    match_count - 1
                } else {
                    (*selected - 1).min(match_count - 1)
                };
            }
        }
        KeyCode::Down if match_count > 0 => {
            if let Overlay::Search { selected, .. } = &mut app.overlay {
                *selected = selected.saturating_add(1) % match_count;
            }
        }
        KeyCode::Char(character)
            if !character.is_control()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            if let Overlay::Search {
                query, selected, ..
            } = &mut app.overlay
            {
                push_input_char(query, character);
                *selected = 0;
            }
        }
        _ => {}
    }
    UiAction::None
}

pub(super) fn picker_key(
    kind: PickerKind,
    rows: &[PickerRow],
    selected: &mut usize,
    key: KeyEvent,
) -> Option<(PickerKind, String)> {
    match key.code {
        KeyCode::Up => *selected = selected.saturating_sub(1),
        KeyCode::Down if !rows.is_empty() => {
            *selected = selected.saturating_add(1).min(rows.len() - 1);
        }
        KeyCode::Enter => {
            return rows.get(*selected).map(|row| (kind, row.value.clone()));
        }
        _ => {}
    }
    None
}

pub(super) fn reduce_command_menu_key(app: &mut App, key: KeyEvent) -> UiAction {
    match key.code {
        KeyCode::Esc => {
            app.input.clear();
            app.pending_send = false;
            app.overlay = Overlay::None;
        }
        KeyCode::Backspace => {
            app.input.pop();
            if app.input.is_empty() {
                app.pending_send = false;
                app.overlay = Overlay::None;
            } else if let Overlay::CommandMenu { selected } = &mut app.overlay {
                *selected = 0;
            }
        }
        KeyCode::Up => {
            if let Overlay::CommandMenu { selected } = &mut app.overlay {
                *selected = selected.saturating_sub(1);
            }
        }
        KeyCode::Down => {
            let count = command_matches(app).len();
            if let Overlay::CommandMenu { selected } = &mut app.overlay {
                if count > 0 {
                    *selected = selected.saturating_add(1).min(count - 1);
                }
            }
        }
        KeyCode::Enter => return execute_command_menu(app),
        KeyCode::Char(character)
            if !character.is_control()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            push_input_char(&mut app.input, character);
            if let Overlay::CommandMenu { selected } = &mut app.overlay {
                *selected = 0;
            }
        }
        _ => {}
    }
    UiAction::None
}

pub(super) fn timeline_key(
    entry_count: usize,
    selected: &mut usize,
    inspecting: &mut bool,
    note: &mut Option<String>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Up => {
            *selected = selected.saturating_sub(1);
            *inspecting = false;
            *note = None;
        }
        KeyCode::Down => {
            if entry_count > 0 {
                *selected = selected.saturating_add(1).min(entry_count - 1);
            }
            *inspecting = false;
            *note = None;
        }
        KeyCode::Enter if entry_count > 0 => {
            *selected = (*selected).min(entry_count - 1);
            *inspecting = true;
            *note = None;
        }
        _ => {}
    }
}

pub(super) fn palette_key(
    entries: &[PaletteEntry],
    filter: &mut String,
    selected: &mut usize,
    detail: &mut Option<String>,
    key: KeyEvent,
) -> Option<PaletteEntry> {
    match key.code {
        KeyCode::Backspace => {
            filter.pop();
            *selected = 0;
            *detail = None;
        }
        KeyCode::Up => {
            *selected = selected.saturating_sub(1);
            *detail = None;
        }
        KeyCode::Down => {
            let count = filter_palette(entries, filter).len();
            if count > 0 {
                *selected = selected.saturating_add(1).min(count - 1);
            }
            *detail = None;
        }
        KeyCode::Enter => {
            return filter_palette(entries, filter)
                .get(*selected)
                .map(|entry| (*entry).clone());
        }
        KeyCode::Char(character)
            if !character.is_control()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            filter.push(character);
            *selected = 0;
            *detail = None;
        }
        _ => {}
    }
    None
}

pub(super) fn activate_palette_entry(app: &mut App, entry: PaletteEntry) -> UiAction {
    match entry.action {
        PaletteAction::Quit => UiAction::Quit,
        PaletteAction::Search => {
            app.open_search();
            UiAction::None
        }
        PaletteAction::TrustDial => {
            app.overlay = Overlay::TrustDial;
            UiAction::None
        }
        PaletteAction::Timeline => {
            app.overlay = Overlay::Timeline {
                selected: app.timeline.len().saturating_sub(1),
                inspecting: false,
                note: None,
            };
            UiAction::None
        }
        PaletteAction::Why => explain_why(app),
        PaletteAction::Palette => {
            app.overlay = Overlay::Palette {
                filter: String::new(),
                selected: 0,
                detail: None,
            };
            UiAction::None
        }
        PaletteAction::Prefill(command) => {
            app.input = command.into();
            app.overlay = Overlay::CommandMenu { selected: 0 };
            UiAction::None
        }
        PaletteAction::Describe => {
            if let Overlay::Palette { detail, .. } = &mut app.overlay {
                *detail = Some(entry.description);
            }
            UiAction::None
        }
    }
}
