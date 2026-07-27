//! Pure terminal layout and rendering helpers.

use super::*;

pub(super) fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let outer = main_block(app);
    let inner = outer.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(outer, area);
    let regions = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);
    render_transcript(frame, app, regions[0]);
    render_key_hints(frame, app, regions[1]);
    render_separator(frame, app, regions[2]);
    render_input(frame, app, regions[3]);
    render_hud(frame, app, regions[4]);
    render_overlay(frame, app);
}

pub(super) fn main_block(app: &App) -> Block<'static> {
    let (status, status_style) = status_chip(&app.status, app.working_since, Utc::now());
    let left_title = Line::from(vec![
        Span::styled(
            safe_line(&app.scrubber, " nosis "),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            safe_line(&app.scrubber, "· "),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(safe_line(&app.scrubber, &status), status_style),
        Span::raw(safe_line(&app.scrubber, " ")),
    ]);
    let route_title = Line::from(Span::styled(
        safe_line(
            &app.scrubber,
            &format!(" {} · effort: {} ", app.route.id(), effort_name(app.effort)),
        ),
        Style::default().fg(Color::Cyan),
    ))
    .right_aligned();

    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(Color::DarkGray))
        .style(Style::default().bg(Color::Black))
        .title_top(left_title)
        .title_top(route_title)
}

pub(super) fn render_key_hints(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let hints = safe_line(
        &app.scrubber,
        "/ commands   ↑↓ scroll   Enter send   Ctrl+C quit",
    );
    frame.render_widget(
        Paragraph::new(hints).style(
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ),
        area,
    );
}

pub(super) fn render_separator(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let rule = safe_line(&app.scrubber, &"─".repeat(usize::from(area.width)));
    frame.render_widget(
        Paragraph::new(rule).style(
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ),
        area,
    );
}

pub(super) fn render_input(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let prompt = safe_line(&app.scrubber, "❯ ");
    let input = safe_line(&app.scrubber, &app.input);
    let mut spans = vec![Span::styled(
        prompt.clone(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    if app.input.is_empty() {
        spans.push(Span::styled(
            safe_line(&app.scrubber, "type a task and press Enter…"),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ));
    } else {
        spans.push(Span::styled(
            input.clone(),
            Style::default().fg(Color::White),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    if app.overlay == Overlay::None && area.width > 0 && area.height > 0 {
        let cursor_width = Line::from(format!("{prompt}{input}")).width();
        let cursor_x = area.x.saturating_add(
            u16::try_from(cursor_width)
                .unwrap_or(u16::MAX)
                .min(area.width.saturating_sub(1)),
        );
        frame.set_cursor_position((cursor_x, area.y));
    }
}

pub(super) fn render_hud(frame: &mut Frame<'_>, app: &App, area: Rect) {
    frame.render_widget(
        Paragraph::new(app.hud_line(Utc::now())).style(Style::default().fg(Color::Gray)),
        area,
    );
}

pub(super) fn render_overlay(frame: &mut Frame<'_>, app: &App) {
    match &app.overlay {
        Overlay::None => {}
        Overlay::CommandMenu { selected } => {
            render_command_menu(frame, app, modal_area(frame.area(), 14), *selected)
        }
        Overlay::TrustDial => {
            let desired = u16::try_from(trust_dial_lines(&app.policy_view).len())
                .unwrap_or(u16::MAX)
                .saturating_add(3)
                .max(8);
            render_trust_dial(frame, app, modal_area(frame.area(), desired));
        }
        Overlay::Timeline {
            selected,
            inspecting,
            note,
        } => render_timeline(
            frame,
            app,
            modal_area(frame.area(), 20),
            *selected,
            *inspecting,
            note.as_deref(),
        ),
        Overlay::Palette {
            filter,
            selected,
            detail,
        } => render_palette(
            frame,
            app,
            modal_area(frame.area(), 18),
            filter,
            *selected,
            detail.as_deref(),
        ),
    }
}

pub(super) fn render_trust_dial(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let body = render_modal_shell(
        frame,
        app,
        area,
        " Trust Dial · read-only ",
        "Read-only policy view · Esc close",
    );
    let lines: Vec<Line<'static>> = trust_dial_lines(&app.policy_view)
        .into_iter()
        .map(|line| Line::from(safe_line(&app.scrubber, &line)))
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(Color::White).bg(Color::Black)),
        body,
    );
}

pub(super) fn render_timeline(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    selected: usize,
    inspecting: bool,
    note: Option<&str>,
) {
    let body = render_modal_shell(
        frame,
        app,
        area,
        " Timeline ",
        "↑/↓ move · Enter inspect · Esc close",
    );
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(body);
    let visible_rows = usize::from(columns[0].height.max(1));
    let start = selected.saturating_add(1).saturating_sub(visible_rows);
    let mut rail = Vec::new();
    if app.timeline.is_empty() {
        rail.push(Line::from(safe_line(&app.scrubber, "no completed turns")));
    } else {
        rail.extend(
            app.timeline
                .iter()
                .enumerate()
                .skip(start)
                .take(visible_rows)
                .map(|(index, entry)| {
                    let marker = if index == selected { "❯ " } else { "  " };
                    let line =
                        safe_line(&app.scrubber, &format!("{marker}{}", timeline_row(entry)));
                    let style = if index == selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(Span::styled(line, style))
                }),
        );
    }

    let mut detail = Vec::new();
    if let Some(note) = note {
        detail.push(Line::from(Span::styled(
            safe_line(&app.scrubber, note),
            Style::default().fg(Color::DarkGray),
        )));
        detail.push(Line::from(safe_line(&app.scrubber, "")));
    }
    if let Some(entry) = app.timeline.get(selected) {
        let raw = if inspecting {
            timeline_detail_lines(entry)
        } else {
            vec![
                format!("selected: #{}", entry.turn),
                format!("task: {}", entry.task),
                "press Enter to inspect".into(),
            ]
        };
        detail.extend(
            raw.into_iter()
                .map(|line| Line::from(safe_line(&app.scrubber, &line))),
        );
    }

    let panel_style = Style::default().fg(Color::White).bg(Color::Black);
    frame.render_widget(Paragraph::new(rail).style(panel_style), columns[0]);
    frame.render_widget(Paragraph::new(detail).style(panel_style), columns[1]);
}

pub(super) fn render_command_menu(frame: &mut Frame<'_>, app: &App, area: Rect, selected: usize) {
    let body = render_modal_shell(
        frame,
        app,
        area,
        " Commands ",
        "Type a command · ↑/↓ browse · Enter run · Esc close",
    );
    let filtered = command_matches(app);
    let visible_rows = usize::from(body.height.saturating_sub(2).max(1));
    let selected = selected.min(filtered.len().saturating_sub(1));
    let start = selected.saturating_add(1).saturating_sub(visible_rows);
    let query = app.input.strip_prefix('/').unwrap_or("");
    let mut lines = vec![
        Line::from(safe_line(&app.scrubber, &format!("command: /{query}"))),
        Line::from(safe_line(&app.scrubber, "")),
    ];
    if filtered.is_empty() {
        lines.push(Line::from(safe_line(&app.scrubber, "no matches")));
    } else {
        lines.extend(
            filtered
                .iter()
                .enumerate()
                .skip(start)
                .take(visible_rows)
                .map(|(index, entry)| {
                    let marker = if index == selected { "❯ " } else { "  " };
                    let line = safe_line(&app.scrubber, &format!("{marker}{}", entry.line()));
                    let style = if index == selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(Span::styled(line, style))
                }),
        );
    }
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(Color::White).bg(Color::Black)),
        body,
    );
}

pub(super) fn render_palette(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    filter: &str,
    selected: usize,
    detail: Option<&str>,
) {
    let body = render_modal_shell(
        frame,
        app,
        area,
        " Commands + Tools ",
        "Type to filter · ↑/↓ move · Enter select · Esc close",
    );
    let filtered = filter_palette(&app.palette_entries, filter);
    let visible_rows = usize::from(body.height.saturating_sub(2).max(1));
    let start = selected.saturating_add(1).saturating_sub(visible_rows);
    let mut lines = vec![
        Line::from(safe_line(&app.scrubber, &format!("filter: {filter}"))),
        Line::from(safe_line(&app.scrubber, "")),
    ];
    if filtered.is_empty() {
        lines.push(Line::from(safe_line(&app.scrubber, "no matches")));
    } else {
        lines.extend(
            filtered
                .iter()
                .enumerate()
                .skip(start)
                .take(visible_rows)
                .map(|(index, entry)| {
                    let marker = if index == selected { "❯ " } else { "  " };
                    let line = safe_line(&app.scrubber, &format!("{marker}{}", entry.line()));
                    let style = if index == selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(Span::styled(line, style))
                }),
        );
    }
    if let Some(detail) = detail {
        lines.push(Line::from(safe_line(
            &app.scrubber,
            &format!("selected: {detail}"),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(Color::White).bg(Color::Black)),
        body,
    );
}

pub(super) fn modal_area(area: Rect, desired_height: u16) -> Rect {
    let width = if area.width > 4 {
        area.width.saturating_sub(4).min(76)
    } else {
        area.width
    };
    let max_height = if area.height > 2 {
        area.height.saturating_sub(2)
    } else {
        area.height
    };
    let height = desired_height.clamp(6, 20).min(max_height);
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

pub(super) fn render_modal_shell(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    title: &str,
    help: &str,
) -> Rect {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(Color::DarkGray))
        .style(Style::default().fg(Color::White).bg(Color::Black))
        .padding(Padding::horizontal(1))
        .title_top(Line::from(Span::styled(
            safe_line(&app.scrubber, title),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(safe_line(&app.scrubber, help)).style(
            Style::default()
                .fg(Color::DarkGray)
                .bg(Color::Black)
                .add_modifier(Modifier::DIM),
        ),
        rows[0],
    );
    rows[1]
}

pub(super) fn render_transcript(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if app.transcript.is_empty() {
        render_empty_state(frame, app, area);
        return;
    }
    let lines = chat_lines(app);
    let (scroll, max_scroll, overflow) = transcript_scroll_state(&lines, area, app.scroll_back);
    app.max_scroll.set(max_scroll);
    let transcript = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(transcript, area);
    render_overflow_markers(frame, app, area, overflow);
}

pub(super) fn render_empty_state(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let height = area.height.min(4);
    let welcome_area = Rect::new(
        area.x,
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        area.width,
        height,
    );
    let lines = vec![
        Line::from(Span::styled(
            safe_line(&app.scrubber, "Welcome to nosis."),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            safe_line(&app.scrubber, "Type a task and press Enter."),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            safe_line(&app.scrubber, "e.g. \"fix the failing test in this repo\""),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        )),
        Line::from(Span::styled(
            safe_line(&app.scrubber, "Type / to see everything nosis can do."),
            Style::default().fg(Color::Gray),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        welcome_area,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TranscriptOverflow {
    pub(super) above: bool,
    pub(super) below: bool,
}

pub(super) fn transcript_scroll_state(
    lines: &[Line<'_>],
    area: Rect,
    scroll_back: u16,
) -> (u16, u16, TranscriptOverflow) {
    let rows = wrapped_rows(lines, area.width.max(1));
    let max_scroll = rows.saturating_sub(area.height);
    let scroll = max_scroll.saturating_sub(scroll_back.min(max_scroll));
    (
        scroll,
        max_scroll,
        TranscriptOverflow {
            above: scroll > 0,
            below: scroll < max_scroll,
        },
    )
}

pub(super) fn render_overflow_markers(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    overflow: TranscriptOverflow,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let marker_width = area.width.min(6);
    let marker_x = area.right().saturating_sub(marker_width);
    let style = Style::default()
        .fg(Color::DarkGray)
        .bg(Color::Black)
        .add_modifier(Modifier::DIM);
    if overflow.above {
        let marker = safe_line(&app.scrubber, "↑ more");
        frame.render_widget(
            Paragraph::new(marker)
                .style(style)
                .alignment(Alignment::Right),
            Rect::new(marker_x, area.y, marker_width, 1),
        );
    }
    if overflow.below {
        let marker = safe_line(&app.scrubber, "↓ more");
        frame.render_widget(
            Paragraph::new(marker)
                .style(style)
                .alignment(Alignment::Right),
            Rect::new(marker_x, area.bottom().saturating_sub(1), marker_width, 1),
        );
    }
}

pub(super) fn chat_lines(app: &App) -> Vec<Line<'static>> {
    let mut rendered = Vec::new();
    let mut previous = None;
    for item in &app.transcript {
        let starts_group = previous != Some(item.kind);
        if starts_group && !rendered.is_empty() {
            rendered.push(Line::from(""));
        }
        match item.kind {
            TranscriptKind::Task => {
                if starts_group {
                    rendered.push(Line::from(Span::styled(
                        "❯ you",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )));
                }
                rendered.push(Line::from(Span::styled(
                    format!("   {}", item.text),
                    Style::default().fg(Color::White),
                )));
            }
            TranscriptKind::Answer => {
                if starts_group {
                    rendered.push(Line::from(Span::styled(
                        "◆ nosis",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )));
                }
                rendered.push(Line::from(Span::styled(
                    format!("   {}", item.text),
                    Style::default().fg(Color::White),
                )));
            }
            TranscriptKind::Progress => rendered.push(Line::from(Span::styled(
                format!("· {}", item.text),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM),
            ))),
            TranscriptKind::Approval => rendered.push(Line::from(Span::styled(
                format!(" {} ", item.text),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED),
            ))),
            TranscriptKind::Error => rendered.push(Line::from(Span::styled(
                item.text.clone(),
                Style::default().fg(Color::Red),
            ))),
        }
        previous = Some(item.kind);
    }
    rendered
}

pub(super) fn wrapped_rows(lines: &[Line<'_>], width: u16) -> u16 {
    lines.iter().fold(0_u16, |rows, line| {
        rows.saturating_add(word_wrapped_line_rows(line, width.max(1)))
    })
}

pub(super) fn word_wrapped_line_rows(line: &Line<'_>, width: u16) -> u16 {
    let limit = usize::from(width);
    let mut rows = 0_u16;
    let mut committed_width = 0_usize;
    let mut committed_symbols = 0_usize;
    let mut word_width = 0_usize;
    let mut word_symbols = 0_usize;
    let mut whitespace_width = 0_usize;
    let mut whitespace = VecDeque::new();
    let mut previous_was_word = false;

    for grapheme in line.styled_graphemes(Style::default()) {
        let is_whitespace = grapheme.is_whitespace();
        let symbol_width = Line::from(grapheme.symbol).width();
        if symbol_width > limit {
            continue;
        }

        let word_finished = previous_was_word && is_whitespace;
        let first_segment_overflow = committed_symbols == 0
            && word_width
                .saturating_add(whitespace_width)
                .saturating_add(symbol_width)
                > limit;
        if word_finished || first_segment_overflow {
            committed_width = committed_width
                .saturating_add(whitespace_width)
                .saturating_add(word_width);
            committed_symbols = committed_symbols
                .saturating_add(whitespace.len())
                .saturating_add(word_symbols);
            whitespace.clear();
            whitespace_width = 0;
            word_width = 0;
            word_symbols = 0;
        }

        let line_is_full = committed_width >= limit;
        let pending_word_overflows = symbol_width > 0
            && committed_width
                .saturating_add(whitespace_width)
                .saturating_add(word_width)
                >= limit;
        if line_is_full || pending_word_overflows {
            rows = rows.saturating_add(1);
            let mut remaining = limit.saturating_sub(committed_width);
            committed_width = 0;
            committed_symbols = 0;

            while whitespace.front().is_some_and(|width| *width <= remaining) {
                let width = whitespace.pop_front().expect("front was present");
                whitespace_width = whitespace_width.saturating_sub(width);
                remaining = remaining.saturating_sub(width);
            }
            if is_whitespace && whitespace.is_empty() {
                continue;
            }
        }

        if is_whitespace {
            whitespace.push_back(symbol_width);
            whitespace_width = whitespace_width.saturating_add(symbol_width);
        } else {
            word_width = word_width.saturating_add(symbol_width);
            word_symbols = word_symbols.saturating_add(1);
        }
        previous_was_word = !is_whitespace;
    }

    committed_symbols = committed_symbols
        .saturating_add(whitespace.len())
        .saturating_add(word_symbols);
    if committed_symbols > 0 {
        rows = rows.saturating_add(1);
    }
    rows.max(1)
}

pub(super) fn status_chip(
    status: &Status,
    working_since: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> (String, Style) {
    match status {
        Status::Idle => (
            "○ IDLE".into(),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ),
        Status::Working => (
            working_since.map_or_else(
                || "● WORKING".into(),
                |since| {
                    let elapsed = now.signed_duration_since(since).num_seconds().max(0);
                    format!("● WORKING · {elapsed}s")
                },
            ),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Status::Waiting => (
            "● WAITING ON YOU".into(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Status::Blocked(_) => (
            "● BLOCKED".into(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
    }
}

pub(super) fn budget_bar(used: u64, limit: u64) -> String {
    const WIDTH: u64 = 7;
    let filled = used
        .saturating_mul(WIDTH)
        .saturating_add(limit.saturating_sub(1))
        .checked_div(limit)
        .unwrap_or(WIDTH)
        .min(WIDTH);
    format!(
        "[{}{}]",
        "#".repeat(filled as usize),
        "-".repeat((WIDTH - filled) as usize)
    )
}
