//! Transcript projection, scrolling, overflow markers, and wrapping.

use super::fit_with_ellipsis;
use crate::session::safe_line;
use crate::state::{
    search_match_lines, search_match_position, App, Overlay, SearchMatchLine, TranscriptKind,
};
use crate::APPROVAL_LEGEND;
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
        app.max_scroll.set(0);
        app.search_match_scroll.set(0);
        render_empty_state(frame, app, area);
        return;
    }
    let (search_matches, selected_match) = active_search_matches(app);
    let projection = chat_projection(app, &search_matches, selected_match, area.width);
    let requested_scroll = selected_match.map_or(app.scroll_back, |(source_index, _)| {
        scroll_back_for_source(&projection, source_index, area)
    });
    let (scroll, max_scroll, overflow) =
        projection_scroll_state(&projection.lines, area, requested_scroll);
    app.max_scroll.set(max_scroll);
    app.search_match_scroll
        .set(requested_scroll.min(max_scroll));
    let (first_line, paragraph_scroll) = paragraph_window(&projection.lines, area.width, scroll);
    render_projected_lines(
        frame,
        &projection.lines[first_line..],
        area,
        paragraph_scroll,
    );
    render_overflow_markers(frame, app, area, overflow);
}

fn active_search_matches(app: &App) -> (Vec<SearchMatchLine>, Option<(usize, usize)>) {
    let Overlay::Search {
        query, selected, ..
    } = &app.overlay
    else {
        return (Vec::new(), None);
    };
    let matches = search_match_lines(&app.transcript, query);
    let selected_match = search_match_position(&matches, *selected);
    (matches, selected_match)
}

struct ChatProjection {
    lines: Vec<ProjectedLine>,
    source_positions: Vec<usize>,
}

struct ProjectedLine {
    content: Line<'static>,
    hanging_prefix: Option<Line<'static>>,
}

impl ProjectedLine {
    fn plain(content: Line<'static>) -> Self {
        Self {
            content,
            hanging_prefix: None,
        }
    }

    fn hanging(prefix: Span<'static>, content: Line<'static>) -> Self {
        Self {
            content,
            hanging_prefix: Some(Line::from(prefix)),
        }
    }

    fn indent_width(&self, width: u16) -> u16 {
        self.hanging_prefix.as_ref().map_or(0, |prefix| {
            u16::try_from(prefix.width())
                .unwrap_or(u16::MAX)
                .min(width.saturating_sub(1))
        })
    }

    fn wrap_width(&self, width: u16) -> u16 {
        width.saturating_sub(self.indent_width(width)).max(1)
    }

    fn wrapped_rows(&self, width: u16) -> usize {
        wrapped_rows(std::slice::from_ref(&self.content), self.wrap_width(width)).max(1)
    }
}

fn scroll_back_for_source(projection: &ChatProjection, source_index: usize, area: Rect) -> usize {
    let total_rows = projection_wrapped_rows(&projection.lines, area.width);
    let max_scroll = total_rows.saturating_sub(usize::from(area.height));
    let target_line = projection
        .source_positions
        .get(source_index)
        .copied()
        .unwrap_or(0);
    let target_row = projection_wrapped_rows(&projection.lines[..target_line], area.width);
    // Keep the selected result above the search panel so its distinct style is visible.
    let viewport_top = target_row
        .saturating_sub(usize::from(area.height) / 4)
        .min(max_scroll);
    max_scroll.saturating_sub(viewport_top)
}

fn paragraph_window(lines: &[ProjectedLine], width: u16, scroll: usize) -> (usize, u16) {
    let mut remaining = scroll;
    for (index, line) in lines.iter().enumerate() {
        let rows = line.wrapped_rows(width);
        if remaining < rows {
            return (index, u16::try_from(remaining).unwrap_or(u16::MAX));
        }
        remaining = remaining.saturating_sub(rows);
    }
    (lines.len(), 0)
}

fn projection_wrapped_rows(lines: &[ProjectedLine], width: u16) -> usize {
    lines.iter().fold(0_usize, |rows, line| {
        rows.saturating_add(line.wrapped_rows(width))
    })
}

fn projection_scroll_state(
    lines: &[ProjectedLine],
    area: Rect,
    scroll_back: usize,
) -> (usize, usize, TranscriptOverflow) {
    let rows = projection_wrapped_rows(lines, area.width);
    let max_scroll = rows.saturating_sub(usize::from(area.height));
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

fn render_projected_lines(
    frame: &mut Frame<'_>,
    lines: &[ProjectedLine],
    area: Rect,
    first_line_scroll: u16,
) {
    let mut y = area.y;
    let mut remaining_height = area.height;
    let mut line_scroll = usize::from(first_line_scroll);
    for line in lines {
        if remaining_height == 0 {
            break;
        }
        let rows = line.wrapped_rows(area.width);
        let visible_rows = rows
            .saturating_sub(line_scroll)
            .min(usize::from(remaining_height));
        if visible_rows == 0 {
            line_scroll = 0;
            continue;
        }
        let height = u16::try_from(visible_rows).unwrap_or(remaining_height);
        let indent = line.indent_width(area.width);
        if line_scroll == 0 {
            if let Some(prefix) = &line.hanging_prefix {
                frame.render_widget(
                    Paragraph::new(prefix.clone()),
                    Rect::new(area.x, y, indent, 1),
                );
            }
        }
        frame.render_widget(
            Paragraph::new(line.content.clone())
                .wrap(Wrap { trim: false })
                .scroll((u16::try_from(line_scroll).unwrap_or(u16::MAX), 0)),
            Rect::new(
                area.x.saturating_add(indent),
                y,
                area.width.saturating_sub(indent),
                height,
            ),
        );
        y = y.saturating_add(height);
        remaining_height = remaining_height.saturating_sub(height);
        line_scroll = 0;
    }
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

#[cfg(test)]
pub(crate) fn transcript_scroll_state(
    lines: &[Line<'_>],
    area: Rect,
    scroll_back: usize,
) -> (usize, usize, TranscriptOverflow) {
    let rows = wrapped_rows(lines, area.width.max(1));
    let max_scroll = rows.saturating_sub(usize::from(area.height));
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

fn chat_projection(
    app: &App,
    search_matches: &[SearchMatchLine],
    selected_match: Option<(usize, usize)>,
    width: u16,
) -> ChatProjection {
    let mut rendered = Vec::new();
    let mut source_positions = Vec::with_capacity(app.transcript.len());
    let mut previous = None;
    let mut search_index = 0;
    for (source_index, item) in app.transcript.iter().enumerate() {
        let matched = search_matches
            .get(search_index)
            .filter(|matched| matched.line_index == source_index);
        if matched.is_some() {
            search_index = search_index.saturating_add(1);
        }
        let ranges = matched.map_or(&[][..], |matched| matched.ranges.as_slice());
        let selected_range = selected_match
            .filter(|(line_index, _)| *line_index == source_index)
            .map(|(_, range_index)| range_index);
        let starts_group = previous != Some(item.kind);
        if starts_group && !rendered.is_empty() {
            rendered.push(ProjectedLine::plain(Line::from("")));
        }
        match item.kind {
            TranscriptKind::Task => {
                if starts_group {
                    rendered.push(ProjectedLine::plain(Line::from(Span::styled(
                        "❯ you",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ))));
                }
                source_positions.push(rendered.len());
                let style = Style::default().fg(Color::White);
                rendered.push(ProjectedLine::hanging(
                    Span::styled("   ", style),
                    highlighted_transcript_line("", &item.text, "", style, ranges, selected_range),
                ));
            }
            TranscriptKind::Answer => {
                if starts_group {
                    rendered.push(ProjectedLine::plain(Line::from(Span::styled(
                        "◆ nosis",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ))));
                }
                source_positions.push(rendered.len());
                let style = Style::default().fg(Color::White);
                rendered.push(ProjectedLine::hanging(
                    Span::styled("   ", style),
                    highlighted_transcript_line("", &item.text, "", style, ranges, selected_range),
                ));
            }
            TranscriptKind::Progress => {
                source_positions.push(rendered.len());
                let style = Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::DIM);
                rendered.push(ProjectedLine::hanging(
                    Span::styled("· ", style),
                    highlighted_transcript_line("", &item.text, "", style, ranges, selected_range),
                ));
            }
            TranscriptKind::Approval => {
                source_positions.push(rendered.len());
                let display = approval_display(
                    &item.text,
                    usize::from(width.saturating_sub(2)),
                    ranges,
                    selected_range,
                );
                rendered.push(ProjectedLine::plain(highlighted_transcript_line(
                    " ",
                    &display.text,
                    " ",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED),
                    &display.ranges,
                    display.selected_range,
                )));
            }
            TranscriptKind::Error => {
                source_positions.push(rendered.len());
                rendered.push(ProjectedLine::plain(highlighted_transcript_line(
                    "",
                    &item.text,
                    "",
                    Style::default().fg(Color::Red),
                    ranges,
                    selected_range,
                )));
            }
        }
        previous = Some(item.kind);
    }
    ChatProjection {
        lines: rendered,
        source_positions,
    }
}

const APPROVAL_LABEL: &str = "approve: ";
const APPROVAL_GAP: &str = "   ";

struct ApprovalDisplay {
    text: String,
    ranges: Vec<std::ops::Range<usize>>,
    selected_range: Option<usize>,
}

fn approval_display(
    text: &str,
    max_width: usize,
    ranges: &[std::ops::Range<usize>],
    selected_range: Option<usize>,
) -> ApprovalDisplay {
    if Line::from(text).width() <= max_width {
        return ApprovalDisplay {
            text: text.to_owned(),
            ranges: ranges.to_vec(),
            selected_range,
        };
    }

    let suffix = format!("{APPROVAL_GAP}{APPROVAL_LEGEND}");
    let Some(action) = text
        .strip_suffix(&suffix)
        .and_then(|head| head.strip_prefix(APPROVAL_LABEL))
    else {
        let fitted = fit_with_ellipsis(text, max_width);
        let retained = retained_source_bytes(text, &fitted);
        let (ranges, selected_range) = map_highlight_ranges(ranges, selected_range, |range| {
            (range.end <= retained).then(|| range.clone())
        });
        return ApprovalDisplay {
            text: fitted,
            ranges,
            selected_range,
        };
    };

    let fixed = format!("{APPROVAL_LABEL}{APPROVAL_GAP}{APPROVAL_LEGEND}");
    let fixed_width = Line::from(fixed.as_str()).width();
    let original_suffix_start = APPROVAL_LABEL.len().saturating_add(action.len());
    if fixed_width > max_width {
        let fitted = fit_with_ellipsis(APPROVAL_LEGEND, max_width);
        let retained = retained_source_bytes(APPROVAL_LEGEND, &fitted);
        let original_legend_start = original_suffix_start.saturating_add(APPROVAL_GAP.len());
        let (ranges, selected_range) = map_highlight_ranges(ranges, selected_range, |range| {
            if range.start < original_legend_start {
                return None;
            }
            let start = range.start - original_legend_start;
            let end = range.end - original_legend_start;
            (end <= retained).then_some(start..end)
        });
        return ApprovalDisplay {
            text: fitted,
            ranges,
            selected_range,
        };
    }

    let fitted_action = fit_with_ellipsis(action, max_width - fixed_width);
    let retained_action = retained_source_bytes(action, &fitted_action);
    let display_suffix_start = APPROVAL_LABEL.len().saturating_add(fitted_action.len());
    let visible_original_end = APPROVAL_LABEL.len().saturating_add(retained_action);
    let (ranges, selected_range) = map_highlight_ranges(ranges, selected_range, |range| {
        if range.end <= visible_original_end {
            Some(range.clone())
        } else if range.start >= original_suffix_start {
            Some(
                display_suffix_start.saturating_add(range.start - original_suffix_start)
                    ..display_suffix_start.saturating_add(range.end - original_suffix_start),
            )
        } else {
            None
        }
    });
    ApprovalDisplay {
        text: format!("{APPROVAL_LABEL}{fitted_action}{APPROVAL_GAP}{APPROVAL_LEGEND}"),
        ranges,
        selected_range,
    }
}

fn retained_source_bytes(source: &str, fitted: &str) -> usize {
    source
        .char_indices()
        .map(|(start, character)| start.saturating_add(character.len_utf8()))
        .take_while(|end| fitted.starts_with(&source[..*end]))
        .last()
        .unwrap_or(0)
}

fn map_highlight_ranges(
    ranges: &[std::ops::Range<usize>],
    selected_range: Option<usize>,
    mut map: impl FnMut(&std::ops::Range<usize>) -> Option<std::ops::Range<usize>>,
) -> (Vec<std::ops::Range<usize>>, Option<usize>) {
    let mut mapped = Vec::new();
    let mut mapped_selection = None;
    for (range_index, range) in ranges.iter().enumerate() {
        let Some(range) = map(range) else {
            continue;
        };
        if selected_range == Some(range_index) {
            mapped_selection = Some(mapped.len());
        }
        mapped.push(range);
    }
    (mapped, mapped_selection)
}

fn highlighted_transcript_line(
    prefix: &str,
    text: &str,
    suffix: &str,
    base_style: Style,
    ranges: &[std::ops::Range<usize>],
    selected_range: Option<usize>,
) -> Line<'static> {
    let mut spans = vec![Span::styled(prefix.to_owned(), base_style)];
    let mut cursor = 0;
    for (range_index, range) in ranges.iter().enumerate() {
        if range.start < cursor
            || range.end > text.len()
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            continue;
        }
        if cursor < range.start {
            spans.push(Span::styled(
                text[cursor..range.start].to_owned(),
                base_style,
            ));
        }
        let style = if selected_range == Some(range_index) {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        };
        spans.push(Span::styled(text[range.clone()].to_owned(), style));
        cursor = range.end;
    }
    if cursor < text.len() {
        spans.push(Span::styled(text[cursor..].to_owned(), base_style));
    }
    if !suffix.is_empty() {
        spans.push(Span::styled(suffix.to_owned(), base_style));
    }
    Line::from(spans)
}

pub(crate) fn wrapped_rows(lines: &[Line<'_>], width: u16) -> usize {
    lines.iter().fold(0_usize, |rows, line| {
        rows.saturating_add(word_wrapped_line_rows(line, width.max(1)))
    })
}

pub(super) fn word_wrapped_line_rows(line: &Line<'_>, width: u16) -> usize {
    let limit = usize::from(width);
    let mut rows = 0_usize;
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
