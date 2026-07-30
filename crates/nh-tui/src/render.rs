//! Pure terminal layout and rendering helpers.

mod transcript;

use transcript::render_transcript;
#[cfg(test)]
pub(super) use transcript::{transcript_scroll_state, wrapped_rows};

use crate::input::command_matches;
use crate::palette::{filter_palette, trust_dial_lines};
use crate::session::{effort_name, safe_line};
use crate::state::{App, Overlay, PickerKind, PickerRow, Status};
use crate::timeline::{timeline_detail_lines, timeline_row};
use chrono::{DateTime, Utc};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph},
    Frame,
};

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
    let now = Utc::now();
    let (status, status_style) = match (&app.status, &app.active_tool) {
        (Status::Working, Some(tool)) => tool_status_chip(&tool.name, tool.started_at, now),
        _ => status_chip(&app.status, app.working_since, now),
    };
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
        Overlay::Picker {
            kind,
            selected,
            rows,
        } => render_picker(
            frame,
            app,
            modal_area(frame.area(), 18),
            *kind,
            *selected,
            rows,
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

pub(super) fn render_picker(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    kind: PickerKind,
    selected: usize,
    picker_rows: &[PickerRow],
) {
    let body = render_modal_shell(
        frame,
        app,
        area,
        kind.title(),
        "↑/↓ move · Enter select · Esc cancel",
    );
    let visible_rows = usize::from(body.height.max(1));
    let selected = selected.min(picker_rows.len().saturating_sub(1));
    let start = selected.saturating_add(1).saturating_sub(visible_rows);
    let lines = if picker_rows.is_empty() {
        vec![Line::from(safe_line(&app.scrubber, kind.empty_message()))]
    } else {
        picker_rows
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_rows)
            .map(|(index, row)| {
                let marker = if index == selected { "❯ " } else { "  " };
                let line = safe_line(&app.scrubber, &format!("{marker}{}", row.label));
                let style = if index == selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                Line::from(Span::styled(line, style))
            })
            .collect()
    };
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

pub(super) fn tool_status_chip(
    name: &str,
    started_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> (String, Style) {
    let elapsed = now.signed_duration_since(started_at).num_seconds().max(0);
    (
        format!("● TOOL {name} · {elapsed}s"),
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )
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
