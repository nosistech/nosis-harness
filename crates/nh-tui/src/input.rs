//! Keyboard, paste, command, and overlay input reduction.

use super::*;

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
                    AgentEvent::Failed("agent stopped — retry the task".into()),
                );
            }
            false
        }
        UiAction::SwitchRoute(route_id) => {
            let route = app
                .resolver
                .resolve(&route_id)
                .expect("a command action only carries a resolved route");
            if worker
                .commands
                .send(WorkerCommand::SwitchRoute(Box::new(route.clone())))
                .is_err()
            {
                apply_event(
                    app,
                    AgentEvent::Failed("agent stopped — retry the task".into()),
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
                    AgentEvent::Failed("agent stopped — retry the task".into()),
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
                    AgentEvent::Failed("agent stopped — retry the task".into()),
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

pub(super) fn reduce_key(app: &mut App, key: KeyEvent) -> UiAction {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        if matches!(app.status, Status::Waiting) {
            app.answer_approval(false);
        }
        return UiAction::Quit;
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
        if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() {
            match key.code {
                KeyCode::Up => scroll_transcript(app, 1, true),
                KeyCode::Down => scroll_transcript(app, 1, false),
                KeyCode::PageUp => scroll_transcript(app, 5, true),
                KeyCode::PageDown => scroll_transcript(app, 5, false),
                _ => {}
            }
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
    if matches!(app.status, Status::Working | Status::Waiting)
        || !matches!(app.overlay, Overlay::None | Overlay::CommandMenu { .. })
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

pub(super) fn scroll_transcript(app: &mut App, amount: u16, toward_older: bool) {
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

    let activated = match &mut app.overlay {
        Overlay::Palette {
            filter,
            selected,
            detail,
        } => palette_key(&app.palette_entries, filter, selected, detail, key),
        Overlay::None
        | Overlay::CommandMenu { .. }
        | Overlay::TrustDial
        | Overlay::Timeline { .. } => None,
    };
    let Some(entry) = activated else {
        return UiAction::None;
    };
    activate_palette_entry(app, entry)
}

pub(super) fn reduce_command_menu_key(app: &mut App, key: KeyEvent) -> UiAction {
    match key.code {
        KeyCode::Esc => {
            app.input.clear();
            app.overlay = Overlay::None;
        }
        KeyCode::Backspace => {
            app.input.pop();
            if app.input.is_empty() {
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

pub(super) fn execute_command_menu(app: &mut App) -> UiAction {
    let command_text = app.input.strip_prefix('/').unwrap_or("");
    let typed = command_text.split_whitespace().next().unwrap_or("");
    let expected = format!("/{typed}");
    let exact = app.palette_entries.iter().any(|entry| {
        entry.kind == "command" && entry.name.split_whitespace().next().unwrap_or("") == expected
    });
    if !command_text.chars().any(char::is_whitespace) && (typed.is_empty() || !exact) {
        let selected = match app.overlay {
            Overlay::CommandMenu { selected } => selected,
            _ => 0,
        };
        if let Some(entry) = command_matches(app).get(selected).copied().cloned() {
            return activate_palette_entry(app, entry);
        }
    }
    execute_command(app)
}

pub(super) fn command_matches(app: &App) -> Vec<&PaletteEntry> {
    let query = app
        .input
        .strip_prefix('/')
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();
    app.palette_entries
        .iter()
        .filter(|entry| {
            entry.kind == "command"
                && (query.is_empty()
                    || entry.name.to_lowercase().contains(&query)
                    || entry.description.to_lowercase().contains(&query))
        })
        .collect()
}

pub(super) fn execute_command(app: &mut App) -> UiAction {
    let input = std::mem::take(&mut app.input);
    app.overlay = Overlay::None;
    let mut parts = input.strip_prefix('/').unwrap_or("").split_whitespace();
    let name = parts.next().unwrap_or("");
    let arg = parts.next();
    match (name, arg) {
        ("help" | "?", _) => {
            app.overlay = Overlay::Palette {
                filter: String::new(),
                selected: 0,
                detail: None,
            };
            UiAction::None
        }
        ("trust", _) => {
            app.overlay = Overlay::TrustDial;
            UiAction::None
        }
        ("timeline", _) => {
            app.overlay = Overlay::Timeline {
                selected: app.timeline.len().saturating_sub(1),
                inspecting: false,
                note: None,
            };
            UiAction::None
        }
        ("why", None) => explain_why(app),
        ("why", Some(_)) => command_error(app, "/why takes no arguments", "run /why by itself"),
        ("profile", Some(name)) => set_profile(app, name),
        ("profile", None) => command_error(
            app,
            "profile name is required",
            "use /profile <frugal|balanced|max-quality>",
        ),
        ("model", Some(id)) => resolved_route_action(app, app.resolver.resolve(id)),
        ("model", None) => command_error(app, "model id is required", "use /model <id>"),
        ("provider", Some(provider)) => {
            resolved_route_action(app, app.resolver.provider_default(provider))
        }
        ("provider", None) => {
            command_error(app, "provider name is required", "use /provider <name>")
        }
        ("effort", Some(value)) => match parse_effort(value) {
            Some(effort) => UiAction::SetEffort(effort),
            None => command_error(
                app,
                "unknown reasoning effort",
                "use /effort <none|low|high|max>",
            ),
        },
        ("effort", None) => command_error(
            app,
            "reasoning effort is required",
            "use /effort <none|low|high|max>",
        ),
        ("quit", _) => UiAction::Quit,
        _ => command_error(app, "unknown command", "type / to see all"),
    }
}

pub(super) fn resolved_route_action(
    app: &mut App,
    resolved: anyhow::Result<ResolvedRoute>,
) -> UiAction {
    match resolved {
        Ok(route) if route.class() == RouteClass::Delegate => command_error(
            app,
            "delegate routes are not available here",
            "pick an api route with /model",
        ),
        Ok(route) => UiAction::SwitchRoute(route.id().to_owned()),
        Err(error) => command_error(app, &error.to_string(), "run /model to list routes"),
    }
}

pub(super) fn teaching_error(cause: &str, next: &str) -> String {
    format!("{cause} — {next}")
}

pub(super) fn command_error(app: &mut App, cause: &str, next: &str) -> UiAction {
    app.push_line(&teaching_error(cause, next), TranscriptKind::Error);
    UiAction::None
}

pub(super) fn set_profile(app: &mut App, name: &str) -> UiAction {
    if !app.profiles.contains(name) {
        return command_error(
            app,
            &format!("unknown profile '{name}'"),
            "use /profile <frugal|balanced|max-quality>",
        );
    }
    let policy = app.profiles.effective(name, &app.route);
    app.active_profile = policy.profile.clone();
    app.effort = effort_for(
        policy.posture,
        app.route.thinking_dialect(),
        app.route.wire(),
    );
    let cap = policy
        .output_cap
        .map_or_else(|| "route default".to_owned(), |cap| cap.to_string());
    app.push_line(
        &format!(
            "profile {} — next turn: thinking {} · max output {}",
            policy.profile,
            effort_name(app.effort),
            cap
        ),
        TranscriptKind::Progress,
    );
    UiAction::SetProfile(policy.profile)
}

pub(super) fn explain_why(app: &mut App) -> UiAction {
    let prompt_est = app
        .timeline
        .last()
        .and_then(|entry| entry.usage.as_ref())
        .map_or(0, |usage| usage.prompt_tokens);
    let cached_est = app
        .timeline
        .last()
        .and_then(|entry| entry.usage.as_ref())
        .and_then(|usage| usage.cached_tokens)
        .unwrap_or(0)
        .min(prompt_est);
    let output_est = 1_024;
    let available = app.resolver.available();
    let allowed: Vec<&str> = available
        .iter()
        .filter(|id| {
            app.resolver
                .resolve(id)
                .is_ok_and(|route| route.class() == RouteClass::Api)
        })
        .map(String::as_str)
        .collect();
    let at = Utc::now();
    let resolved = app
        .resolver
        .resolve_capable(prompt_est, output_est, &allowed, at);
    let (route, trace) = match resolved {
        Ok(result) => result,
        Err(error) => {
            return command_error(
                app,
                &error.to_string(),
                "add a priced api route with enough context",
            )
        }
    };

    app.push_line(
        &format!(
            "route: {} (cheapest capable at ~{} tokens, est)",
            route.id(),
            prompt_est.saturating_add(output_est)
        ),
        TranscriptKind::Progress,
    );
    if let Some(quote) = route.price_at(at) {
        let mut line = match cost_of(&quote, prompt_est, cached_est, output_est) {
            Some(estimate) => format!(
                "  {} this turn (est)",
                money_with_gloss(estimate, quote.currency, app.resolver.fx(), at)
            ),
            None => {
                let _ = apply_event(app, AgentEvent::MeterIncomplete);
                "  unpriced this turn (est) — meter incomplete".into()
            }
        };
        if quote.stale {
            line.push_str(" · *price stale");
        } else if quote.confidence == PriceConfidence::VerifyLive {
            line.push_str(" · *price verify_live");
        }
        app.push_line(&line, TranscriptKind::Progress);
    }
    for line in trace.lines() {
        app.push_line(&line, TranscriptKind::Progress);
    }
    if app.route.id() != route.id() {
        app.push_line(
            &format!(
                "current route {} was selected explicitly; cheapest capable is {}",
                app.route.id(),
                route.id()
            ),
            TranscriptKind::Progress,
        );
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
