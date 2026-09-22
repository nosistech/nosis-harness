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
use std::time::{Duration, Instant};

pub(super) const CTRL_C_EXIT_WINDOW: Duration = Duration::from_millis(1_500);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum UiAction {
    None,
    Dispatch(String),
    SwitchRoute(String),
    SetEffort(ThinkingEffort),
    SetProfile(String),
    Interrupt,
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
        UiAction::Interrupt => {
            worker.cancel_turn();
            false
        }
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
            } else {
                app.invalidate_prompt_estimate();
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
        && matches!(previous, Status::Working | Status::FinishingInterrupted)
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
    let ctrl_c = key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl_c {
        return reduce_ctrl_c(app);
    }
    app.last_ctrl_c = None;
    if key.code == KeyCode::Esc && app.status.esc_interrupts_turn() {
        if let Overlay::Search {
            original_scroll, ..
        } = &app.overlay
        {
            app.scroll_back = *original_scroll;
        }
        app.overlay = Overlay::None;
        if matches!(app.status, Status::Waiting) {
            app.answer_approval(false);
        }
        app.interrupt_turn();
        return UiAction::Interrupt;
    }
    if matches!(key.code, KeyCode::Char('f' | 'F')) && key.modifiers.contains(KeyModifiers::CONTROL)
    {
        app.open_search();
        return UiAction::None;
    }
    if key.code == KeyCode::F(1) {
        app.overlay = Overlay::Help;
        return UiAction::None;
    }
    if app.overlay != Overlay::None {
        return reduce_overlay_key(app, key);
    }
    if key.code == KeyCode::Char('?')
        && key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
        && app.input.is_empty()
    {
        app.overlay = Overlay::Help;
        return UiAction::None;
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
    if newline_key(key) {
        if insert_composer_char(app, '\n') {
            app.end_prompt_history_recall();
        }
        return UiAction::None;
    }
    if let Some(toward_older) = prompt_history_key(key) {
        if toward_older {
            app.recall_previous_prompt();
        } else {
            app.recall_next_prompt();
        }
        return UiAction::None;
    }
    if word_delete_key(key) {
        if delete_previous_word(app) {
            app.end_prompt_history_recall();
        }
        if app.input.trim().is_empty() {
            app.pending_send = false;
        }
        return UiAction::None;
    }
    if line_delete_key(key) {
        if !app.input.is_empty() {
            app.end_prompt_history_recall();
        }
        app.clear_input();
        app.pending_send = false;
        return UiAction::None;
    }
    if reduce_composer_navigation(app, key) {
        return UiAction::None;
    }
    if matches!(app.status, Status::Working | Status::FinishingInterrupted) {
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
                if delete_before_cursor(app) {
                    app.end_prompt_history_recall();
                }
                if app.input.trim().is_empty() {
                    app.pending_send = false;
                }
            }
            KeyCode::Char(character)
                if !character.is_control()
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    && insert_composer_char(app, character) =>
            {
                app.end_prompt_history_recall();
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
            if delete_before_cursor(app) {
                app.end_prompt_history_recall();
            }
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
            if insert_composer_char(app, character) {
                app.end_prompt_history_recall();
            }
            if app.input.starts_with('/') {
                app.overlay = Overlay::CommandMenu { selected: 0 };
            }
        }
        _ => {}
    }
    UiAction::None
}

fn reduce_ctrl_c(app: &mut App) -> UiAction {
    if matches!(app.status, Status::Working | Status::Waiting) {
        if matches!(app.status, Status::Waiting) {
            app.answer_approval(false);
        }
        app.overlay = Overlay::None;
        app.last_ctrl_c = None;
        app.interrupt_turn();
        return UiAction::Interrupt;
    }
    if !app.input.is_empty() || app.pending_send {
        app.end_prompt_history_recall();
        app.clear_input();
        app.pending_send = false;
        app.overlay = Overlay::None;
        app.last_ctrl_c = None;
        return UiAction::None;
    }

    let now = Instant::now();
    if app
        .last_ctrl_c
        .is_some_and(|previous| now.saturating_duration_since(previous) <= CTRL_C_EXIT_WINDOW)
    {
        return UiAction::Quit;
    }
    app.last_ctrl_c = Some(now);
    app.overlay = Overlay::None;
    app.push_line(
        "press Ctrl+C again to exit",
        crate::state::TranscriptKind::Progress,
    );
    UiAction::None
}

fn control_edit_key(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && key
            .modifiers
            .difference(KeyModifiers::CONTROL | KeyModifiers::SHIFT)
            .is_empty()
}

fn word_delete_key(key: KeyEvent) -> bool {
    control_edit_key(key)
        && matches!(
            key.code,
            KeyCode::Backspace | KeyCode::Char('w' | 'W' | 'h' | 'H')
        )
}

fn line_delete_key(key: KeyEvent) -> bool {
    control_edit_key(key) && matches!(key.code, KeyCode::Char('u' | 'U'))
}

fn prompt_history_key(key: KeyEvent) -> Option<bool> {
    if !control_edit_key(key) {
        return None;
    }
    match key.code {
        KeyCode::Char('p' | 'P') => Some(true),
        KeyCode::Char('n' | 'N') => Some(false),
        _ => None,
    }
}

fn newline_key(key: KeyEvent) -> bool {
    (key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::SHIFT))
        || (matches!(key.code, KeyCode::Char('j' | 'J'))
            && key.modifiers.contains(KeyModifiers::CONTROL))
}

fn previous_boundary(input: &str, index: usize) -> Option<usize> {
    input[..index]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
}

fn next_boundary(input: &str, index: usize) -> Option<usize> {
    input[index..]
        .chars()
        .next()
        .map(|character| index + character.len_utf8())
}

fn insert_composer_char(app: &mut App, character: char) -> bool {
    if app.input.len().saturating_add(character.len_utf8()) > MAX_TASK_BYTES {
        return false;
    }
    let cursor = app.input_cursor_index();
    app.input.insert(cursor, character);
    app.set_input_cursor(cursor + character.len_utf8());
    true
}

fn delete_before_cursor(app: &mut App) -> bool {
    let cursor = app.input_cursor_index();
    let Some(previous) = previous_boundary(&app.input, cursor) else {
        return false;
    };
    app.input.drain(previous..cursor);
    app.set_input_cursor(previous);
    true
}

fn delete_at_cursor(app: &mut App) -> bool {
    let cursor = app.input_cursor_index();
    let Some(next) = next_boundary(&app.input, cursor) else {
        return false;
    };
    app.input.drain(cursor..next);
    app.set_input_cursor(cursor);
    true
}

fn delete_previous_word(app: &mut App) -> bool {
    let cursor = app.input_cursor_index();
    let mut start = cursor;
    while let Some(previous) = previous_boundary(&app.input, start) {
        let character = app.input[previous..start]
            .chars()
            .next()
            .expect("one character boundary");
        if !character.is_whitespace() {
            break;
        }
        start = previous;
    }
    while let Some(previous) = previous_boundary(&app.input, start) {
        let character = app.input[previous..start]
            .chars()
            .next()
            .expect("one character boundary");
        if character.is_whitespace() {
            break;
        }
        start = previous;
    }
    if start == cursor {
        return false;
    }
    app.input.drain(start..cursor);
    app.set_input_cursor(start);
    true
}

fn reduce_composer_navigation(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Left if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
            let cursor = app.input_cursor_index();
            if let Some(previous) = previous_boundary(&app.input, cursor) {
                app.set_input_cursor(previous);
            }
            true
        }
        KeyCode::Right if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
            let cursor = app.input_cursor_index();
            if let Some(next) = next_boundary(&app.input, cursor) {
                app.set_input_cursor(next);
            }
            true
        }
        KeyCode::Home if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
            if app.input.is_empty() {
                return false;
            }
            let cursor = app.input_cursor_index();
            let start = app.input[..cursor].rfind('\n').map_or(0, |index| index + 1);
            app.set_input_cursor(start);
            true
        }
        KeyCode::End if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
            if app.input.is_empty() {
                return false;
            }
            let cursor = app.input_cursor_index();
            let end = app.input[cursor..]
                .find('\n')
                .map_or(app.input.len(), |index| cursor + index);
            app.set_input_cursor(end);
            true
        }
        KeyCode::Delete if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
            if delete_at_cursor(app) {
                app.end_prompt_history_recall();
                if app.input.trim().is_empty() {
                    app.pending_send = false;
                }
            }
            true
        }
        KeyCode::Up
            if app.input.contains('\n')
                && key.modifiers.difference(KeyModifiers::SHIFT).is_empty() =>
        {
            move_cursor_line(app, true);
            true
        }
        KeyCode::Down
            if app.input.contains('\n')
                && key.modifiers.difference(KeyModifiers::SHIFT).is_empty() =>
        {
            move_cursor_line(app, false);
            true
        }
        _ => false,
    }
}

fn move_cursor_line(app: &mut App, upward: bool) {
    let cursor = app.input_cursor_index();
    let current_start = app.input[..cursor].rfind('\n').map_or(0, |index| index + 1);
    let current_end = app.input[cursor..]
        .find('\n')
        .map_or(app.input.len(), |index| cursor + index);
    let column = app.input[current_start..cursor].chars().count();
    let (target_start, target_end) = if upward {
        if current_start == 0 {
            return;
        }
        let target_end = current_start - 1;
        let target_start = app.input[..target_end]
            .rfind('\n')
            .map_or(0, |index| index + 1);
        (target_start, target_end)
    } else {
        if current_end == app.input.len() {
            return;
        }
        let target_start = current_end + 1;
        let target_end = app.input[target_start..]
            .find('\n')
            .map_or(app.input.len(), |index| target_start + index);
        (target_start, target_end)
    };
    let offset = app.input[target_start..target_end]
        .char_indices()
        .nth(column)
        .map_or(target_end - target_start, |(index, _)| index);
    app.set_input_cursor(target_start + offset);
}

pub(super) fn push_input_char(input: &mut String, character: char) -> bool {
    if input.len().saturating_add(character.len_utf8()) > MAX_TASK_BYTES {
        return false;
    }
    input.push(character);
    true
}

pub(super) fn reduce_paste(app: &mut App, text: &str) -> UiAction {
    let working = matches!(app.status, Status::Working | Status::FinishingInterrupted);
    if matches!(app.status, Status::Waiting)
        || (working && app.overlay != Overlay::None)
        || (!working && !matches!(app.overlay, Overlay::None | Overlay::CommandMenu { .. }))
    {
        return UiAction::None;
    }

    let original = app.input.clone();
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        let character = match character {
            '\r' => {
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                '\n'
            }
            '\n' => '\n',
            '\t' => ' ',
            character if character.is_control() => continue,
            character => character,
        };
        if !insert_composer_char(app, character) {
            break;
        }
    }
    if app.input != original {
        app.end_prompt_history_recall();
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
    if matches!(app.overlay, Overlay::Help | Overlay::TrustDial) {
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
        | Overlay::Help
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
    if newline_key(key) {
        return UiAction::None;
    }
    if word_delete_key(key) {
        if delete_previous_word(app) {
            app.end_prompt_history_recall();
        }
        if app.input.trim().is_empty() {
            app.pending_send = false;
            app.overlay = Overlay::None;
        } else if let Overlay::CommandMenu { selected } = &mut app.overlay {
            *selected = 0;
        }
        return UiAction::None;
    }
    if line_delete_key(key) {
        if !app.input.is_empty() {
            app.end_prompt_history_recall();
        }
        app.clear_input();
        app.pending_send = false;
        app.overlay = Overlay::None;
        return UiAction::None;
    }
    match key.code {
        KeyCode::Esc => {
            if !app.input.is_empty() {
                app.end_prompt_history_recall();
            }
            app.clear_input();
            app.pending_send = false;
            app.overlay = Overlay::None;
        }
        KeyCode::Backspace => {
            if delete_before_cursor(app) {
                app.end_prompt_history_recall();
            }
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
        KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End | KeyCode::Delete => {
            reduce_composer_navigation(app, key);
            if let Overlay::CommandMenu { selected } = &mut app.overlay {
                *selected = 0;
            }
        }
        KeyCode::Enter => return execute_command_menu(app),
        KeyCode::Char(character)
            if !character.is_control()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            if insert_composer_char(app, character) {
                app.end_prompt_history_recall();
            }
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
            app.replace_input(command.into());
            app.end_prompt_history_recall();
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
