//! Transcript projection, scrolling, overflow markers, and wrapping.

use crate::session::safe_line;
use crate::state::{App, TranscriptKind};
use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};
use std::collections::VecDeque;

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
pub(crate) struct TranscriptOverflow {
    pub(crate) above: bool,
    pub(crate) below: bool,
}

pub(crate) fn transcript_scroll_state(
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

pub(crate) fn wrapped_rows(lines: &[Line<'_>], width: u16) -> u16 {
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

            while let Some(width) = whitespace.front().copied() {
                if width > remaining {
                    break;
                }
                whitespace.pop_front();
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
