//! Pure terminal layout and rendering helpers.

mod transcript;

use transcript::render_transcript;
#[cfg(test)]
pub(super) use transcript::{transcript_scroll_state, wrapped_rows};

use crate::input::command_matches;
use crate::keymap::{key_hint_line_for, visible_key_bindings};
use crate::palette::{filter_palette, trust_dial_lines};
use crate::session::{effort_name, safe_line};
use crate::state::{
    search_match_count, search_match_lines, App, Overlay, PickerKind, PickerRow, Status,
};
use crate::timeline::{timeline_detail_lines_for, timeline_row_for};
use chrono::{DateTime, Utc};
use nh_core::terminal_capability::TerminalCapability;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph},
    Frame,
};

const BLOCKED_REASON_MAX_CHARS: usize = 32;
const HEADER_TITLE_GAP: usize = 1;
const BLOCKED_LABEL: &str = "● BLOCKED";
const ASCII_BORDER_SET: border::Set<'static> = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

fn display_line(app: &App, text: &str) -> String {
    let text = app.terminal_capability.render_text(text);
    safe_line(&app.scrubber, &text)
}

fn apply_border_set(
    block: Block<'static>,
    terminal_capability: TerminalCapability,
) -> Block<'static> {
    if terminal_capability.uses_ascii_fallback() {
        block.border_set(ASCII_BORDER_SET)
    } else {
        block.border_type(BorderType::Plain)
    }
}

pub(super) fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let outer = main_block(app, area.width);
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
    let transcript_area = if matches!(app.overlay, Overlay::Search { .. }) {
        let panel = search_modal_area(area);
        Rect::new(
            regions[0].x,
            regions[0].y,
            regions[0].width,
            panel.y.saturating_sub(regions[0].y).min(regions[0].height),
        )
    } else {
        regions[0]
    };
    render_transcript(frame, app, transcript_area);
    render_key_hints(frame, app, regions[1]);
    render_separator(frame, app, regions[2]);
    render_input(frame, app, regions[3]);
    render_hud(frame, app, regions[4]);
    render_overlay(frame, app);
    app.color_mode.apply(frame.buffer_mut());
}
pub(super) fn main_block(app: &App, width: u16) -> Block<'static> {
    let now = Utc::now();
    let route_label = display_line(
        app,
        &format!(" {} · effort: {} ", app.route.id(), effort_name(app.effort)),
    );
    let blocked_reason_width = blocked_reason_width(width, &route_label, app);
    let (status, status_style) = match (&app.status, &app.active_tool, &app.active_model) {
        (Status::Working, Some(tool), _) => tool_status_chip(&tool.name, tool.started_at, now),
        (Status::Working, None, Some(request)) => model_status_chip(
            &request.route,
            app.working_since.unwrap_or(request.started_at),
            now,
            app.typical_duration_ms,
        ),
        _ => status_chip(
            &app.status,
            app.working_since,
            now,
            blocked_reason_width,
            app.typical_duration_ms,
        ),
    };
    let left_title = left_title(app, &status, status_style);
    let route_title =
        Line::from(Span::styled(route_label, Style::default().fg(Color::Cyan))).right_aligned();

    apply_border_set(
        Block::default().borders(Borders::ALL),
        app.terminal_capability,
    )
    .border_style(Style::default().fg(Color::DarkGray))
    .style(Style::default().bg(Color::Black))
    .title_top(left_title)
    .title_top(route_title)
}

fn left_title(app: &App, status: &str, status_style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            display_line(app, " nosis "),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            display_line(app, "· "),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(display_line(app, status), status_style),
        Span::raw(display_line(app, " ")),
    ])
}

pub(super) fn render_key_hints(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let hints = display_line(
        app,
        &key_hint_line_for(
            app.terminal_capability,
            app.budget_reached(),
            app.status.esc_interrupts_turn(),
        ),
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
    let rule = display_line(app, &"─".repeat(usize::from(area.width)));
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
    let prompt = display_line(app, "❯ ");
    let queued = if app.pending_send {
        let marker = match &app.status {
            Status::Blocked(_) if app.budget_reached() => "[queued - budget reached] ",
            Status::Blocked(_) => "[queued - press Enter] ",
            _ => "[queued] ",
        };
        display_line(app, marker)
    } else {
        String::new()
    };
    let input = display_line(app, &app.input);
    let mut spans = vec![Span::styled(
        prompt.clone(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )];
    if app.pending_send {
        spans.push(Span::styled(
            queued.clone(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if app.input.is_empty() {
        let placeholder = if app.budget_reached() {
            "budget reached - press Ctrl+C twice to exit"
        } else {
            "type a task and press Enter…"
        };
        spans.push(Span::styled(
            display_line(app, placeholder),
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
        let cursor_width = Line::from(format!("{prompt}{queued}{input}")).width();
        let cursor_x = area.x.saturating_add(
            u16::try_from(cursor_width)
                .unwrap_or(u16::MAX)
                .min(area.width.saturating_sub(1)),
        );
        frame.set_cursor_position((cursor_x, area.y));
    }
}

pub(super) fn render_hud(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let hud = fit_with_ellipsis(&app.hud_line(Utc::now()), usize::from(area.width));
    frame.render_widget(
        Paragraph::new(hud).style(Style::default().fg(Color::Gray)),
        area,
    );
}

pub(super) fn render_overlay(frame: &mut Frame<'_>, app: &App) {
    match &app.overlay {
        Overlay::None => {}
        Overlay::Search {
            query, selected, ..
        } => render_search(
            frame,
            app,
            search_modal_area(frame.area()),
            query,
            *selected,
        ),
        Overlay::CommandMenu { selected } => {
            render_command_menu(frame, app, modal_area(frame.area(), 14), *selected)
        }
        Overlay::Help => render_help(frame, app, modal_area(frame.area(), 18)),
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

pub(super) fn render_help(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let body = render_modal_shell(
        frame,
        app,
        area,
        " Help · read-only ",
        "Keys for the current state",
    );
    let lines: Vec<Line<'static>> =
        visible_key_bindings(app.budget_reached(), app.status.esc_interrupts_turn())
            .map(|binding| {
                Line::from(vec![
                    Span::styled(
                        display_line(
                            app,
                            &format!("{:<12}", binding.display_keys(app.terminal_capability)),
                        ),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        display_line(app, &format!("{}{}", binding.action, binding.detail)),
                        Style::default().fg(Color::White),
                    ),
                ])
            })
            .collect();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(Color::White).bg(Color::Black)),
        body,
    );
}

pub(super) fn render_search(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    query: &str,
    selected: usize,
) {
    let body = render_modal_shell(
        frame,
        app,
        area,
        " Search transcript ",
        "Type literal text · ↑/↓ match · Enter keep · Esc cancel",
    );
    let matches = search_match_lines(&app.transcript, query);
    let match_count = search_match_count(&matches);
    let position = if match_count == 0 {
        "0 matches".to_owned()
    } else {
        format!(
            "match {}/{}",
            selected.min(match_count - 1) + 1,
            match_count
        )
    };
    let lines = vec![
        Line::from(display_line(app, &format!("query: {query}"))),
        Line::from(display_line(app, &position)),
    ];
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(Color::White).bg(Color::Black)),
        body,
    );
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
        .map(|line| Line::from(display_line(app, &line)))
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
        rail.push(Line::from(display_line(app, "no completed turns")));
    } else {
        rail.extend(
            app.timeline
                .iter()
                .enumerate()
                .skip(start)
                .take(visible_rows)
                .map(|(index, entry)| {
                    let marker = if index == selected { "❯ " } else { "  " };
                    let line = display_line(
                        app,
                        &format!(
                            "{marker}{}",
                            timeline_row_for(app.terminal_capability, entry)
                        ),
                    );
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
            display_line(app, note),
            Style::default().fg(Color::DarkGray),
        )));
        detail.push(Line::from(display_line(app, "")));
    }
    if let Some(entry) = app.timeline.get(selected) {
        let raw = if inspecting {
            timeline_detail_lines_for(app.terminal_capability, entry)
        } else {
            vec![
                format!("selected: #{}", entry.turn),
                format!("task: {}", entry.task),
                "press Enter to inspect".into(),
            ]
        };
        detail.extend(
            raw.into_iter()
                .map(|line| Line::from(display_line(app, &line))),
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
        Line::from(display_line(app, &format!("command: /{query}"))),
        Line::from(display_line(app, "")),
    ];
    if filtered.is_empty() {
        lines.push(Line::from(display_line(app, "no matches")));
    } else {
        lines.extend(
            filtered
                .iter()
                .enumerate()
                .skip(start)
                .take(visible_rows)
                .map(|(index, entry)| {
                    let marker = if index == selected { "❯ " } else { "  " };
                    let line = display_line(app, &format!("{marker}{}", entry.line()));
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
        Line::from(display_line(app, &format!("filter: {filter}"))),
        Line::from(display_line(app, "")),
    ];
    if filtered.is_empty() {
        lines.push(Line::from(display_line(app, "no matches")));
    } else {
        lines.extend(
            filtered
                .iter()
                .enumerate()
                .skip(start)
                .take(visible_rows)
                .map(|(index, entry)| {
                    let marker = if index == selected { "❯ " } else { "  " };
                    let line = display_line(app, &format!("{marker}{}", entry.line()));
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
        lines.push(Line::from(display_line(
            app,
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
        vec![Line::from(display_line(app, kind.empty_message()))]
    } else {
        picker_rows
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_rows)
            .map(|(index, row)| {
                let marker = if index == selected { "❯ " } else { "  " };
                let line = display_line(app, &format!("{marker}{}", row.label));
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

pub(super) fn search_modal_area(area: Rect) -> Rect {
    let mut modal = modal_area(area, 8);
    modal.y = area
        .bottom()
        .saturating_sub(modal.height)
        .saturating_sub(u16::from(area.height > modal.height));
    modal
}

pub(super) fn render_modal_shell(
    frame: &mut Frame<'_>,
    app: &App,
    area: Rect,
    title: &str,
    help: &str,
) -> Rect {
    frame.render_widget(Clear, area);
    let block = apply_border_set(
        Block::default().borders(Borders::ALL),
        app.terminal_capability,
    )
    .border_style(Style::default().fg(Color::DarkGray))
    .style(Style::default().fg(Color::White).bg(Color::Black))
    .padding(Padding::horizontal(1))
    .title_top(Line::from(Span::styled(
        display_line(app, title),
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
        Paragraph::new(display_line(app, help)).style(
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
    blocked_reason_width: usize,
    typical_duration_ms: Option<u64>,
) -> (String, Style) {
    let typical = typical_duration_suffix(typical_duration_ms);
    match status {
        Status::Idle => (
            "○ IDLE".into(),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::DIM),
        ),
        Status::Working => (
            working_since.map_or_else(
                || format!("● WORKING{typical}"),
                |since| {
                    let elapsed = now.signed_duration_since(since).num_seconds().max(0);
                    format!("● WORKING · {elapsed}s{typical}")
                },
            ),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Status::FinishingInterrupted => (
            working_since.map_or_else(
                || format!("● WORKING - interrupted turn{typical}"),
                |since| {
                    let elapsed = now.signed_duration_since(since).num_seconds().max(0);
                    format!("● WORKING - interrupted turn · {elapsed}s{typical}")
                },
            ),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Status::Waiting => (
            "● WAITING ON YOU".into(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Status::Blocked(reason) => {
            let reason = reason
                .split(['\n', '\r'])
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or("");
            let mut chars = reason.chars();
            let head: String = chars.by_ref().take(BLOCKED_REASON_MAX_CHARS).collect();
            let capped = if chars.next().is_some() {
                format!("{head}…")
            } else {
                head
            };
            let reason = fit_with_ellipsis(&capped, blocked_reason_width);
            let label = if reason.is_empty() {
                BLOCKED_LABEL.into()
            } else {
                format!("{BLOCKED_LABEL} - {reason}")
            };
            (
                label,
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )
        }
    }
}

fn typical_duration_suffix(duration_ms: Option<u64>) -> String {
    duration_ms.map_or_else(String::new, |duration_ms| {
        let duration = if duration_ms < 1_000 {
            "~<1s".to_owned()
        } else {
            let seconds = duration_ms.saturating_add(500) / 1_000;
            format!("~{seconds}s")
        };
        format!(", typically {duration}")
    })
}

fn blocked_reason_width(width: u16, route_label: &str, app: &App) -> usize {
    let title_width = usize::from(width.saturating_sub(2));
    let fixed_left_width =
        left_title(app, &format!("{BLOCKED_LABEL} - "), Style::default()).width();
    title_width.saturating_sub(
        fixed_left_width
            .saturating_add(Line::from(route_label).width())
            .saturating_add(HEADER_TITLE_GAP),
    )
}

pub(super) fn fit_with_ellipsis(text: &str, max_width: usize) -> String {
    if Line::from(text).width() <= max_width {
        return text.to_owned();
    }
    let ellipsis = "…";
    let ellipsis_width = Line::from(ellipsis).width();
    if max_width < ellipsis_width {
        return String::new();
    }
    let head_width = max_width.saturating_sub(ellipsis_width);
    let source = text.strip_suffix(ellipsis).unwrap_or(text);
    let mut head = String::new();
    for character in source.chars() {
        head.push(character);
        if Line::from(head.as_str()).width() > head_width {
            head.pop();
            break;
        }
    }
    format!("{head}{ellipsis}")
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

pub(super) fn model_status_chip(
    route: &str,
    started_at: DateTime<Utc>,
    now: DateTime<Utc>,
    typical_duration_ms: Option<u64>,
) -> (String, Style) {
    let elapsed = now.signed_duration_since(started_at).num_seconds().max(0);
    let typical = typical_duration_suffix(typical_duration_ms);
    (
        format!("● WAITING {route} · {elapsed}s{typical}"),
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
