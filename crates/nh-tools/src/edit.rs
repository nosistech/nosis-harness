use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchTier {
    Exact,
    WhitespaceNormalized,
    IndentationFlexible,
}

impl MatchTier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::WhitespaceNormalized => "whitespace-normalized",
            Self::IndentationFlexible => "indentation-flexible",
        }
    }
}

#[derive(Debug)]
pub struct Match {
    pub range: Range<usize>,
    pub replacement: String,
    pub tier: MatchTier,
}

#[derive(Debug)]
pub enum MatchFailure {
    Ambiguous { tier: MatchTier, count: usize },
    NotFound(CandidateRegion),
}

#[derive(Debug)]
pub struct CandidateRegion {
    pub first_line: usize,
    pub last_line: usize,
    pub text: String,
}

struct Normalized {
    text: String,
    source_spans: Vec<Range<usize>>,
}

struct SourceLine<'a> {
    number: usize,
    text: &'a str,
}

pub fn locate(content: &str, old: &str, new: &str) -> Result<Match, MatchFailure> {
    match exact_ranges(content, old).as_slice() {
        [] => {}
        [range] => {
            return Ok(Match {
                range: range.clone(),
                replacement: new.to_string(),
                tier: MatchTier::Exact,
            });
        }
        ranges => {
            return Err(MatchFailure::Ambiguous {
                tier: MatchTier::Exact,
                count: ranges.len(),
            });
        }
    }

    match normalized_ranges(content, old, true).as_slice() {
        [] => {}
        [range] => {
            return Ok(Match {
                range: range.clone(),
                replacement: new.to_string(),
                tier: MatchTier::WhitespaceNormalized,
            });
        }
        ranges => {
            return Err(MatchFailure::Ambiguous {
                tier: MatchTier::WhitespaceNormalized,
                count: ranges.len(),
            });
        }
    }

    match normalized_ranges(content, old, false).as_slice() {
        [] => Err(MatchFailure::NotFound(nearest_candidate(content, old))),
        [range] => Ok(Match {
            replacement: indentation_flexible_replacement(content, old, new, range),
            range: range.clone(),
            tier: MatchTier::IndentationFlexible,
        }),
        ranges => Err(MatchFailure::Ambiguous {
            tier: MatchTier::IndentationFlexible,
            count: ranges.len(),
        }),
    }
}

fn exact_ranges(content: &str, pattern: &str) -> Vec<Range<usize>> {
    content
        .match_indices(pattern)
        .map(|(start, matched)| start..start + matched.len())
        .collect()
}

fn normalized_ranges(
    content: &str,
    pattern: &str,
    preserve_indentation: bool,
) -> Vec<Range<usize>> {
    let haystack = normalize(content, preserve_indentation);
    let needle = normalize(pattern, preserve_indentation).text;
    if needle.is_empty() {
        return Vec::new();
    }

    haystack
        .text
        .match_indices(&needle)
        .filter_map(|(start, matched)| {
            let end = start.checked_add(matched.len())?;
            let first = haystack.source_spans.get(start)?;
            let last = haystack.source_spans.get(end.checked_sub(1)?)?;
            Some(first.start..last.end)
        })
        .collect()
}

fn normalize(input: &str, preserve_indentation: bool) -> Normalized {
    let mut normalized = Normalized {
        text: String::with_capacity(input.len()),
        source_spans: Vec::with_capacity(input.len()),
    };
    let bytes = input.as_bytes();
    let mut line_start = 0;

    while line_start < bytes.len() {
        let newline = bytes[line_start..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|offset| line_start + offset);
        let line_end = newline.unwrap_or(bytes.len());
        let text_end = if line_end > line_start && bytes[line_end - 1] == b'\r' {
            line_end - 1
        } else {
            line_end
        };
        normalize_line(
            input,
            line_start..text_end,
            preserve_indentation,
            &mut normalized,
        );
        if let Some(newline_at) = newline {
            let source_start = if newline_at > line_start && bytes[newline_at - 1] == b'\r' {
                newline_at - 1
            } else {
                newline_at
            };
            push_char(&mut normalized, '\n', source_start..newline_at + 1);
            line_start = newline_at + 1;
        } else {
            break;
        }
    }

    normalized
}

fn normalize_line(
    input: &str,
    range: Range<usize>,
    preserve_indentation: bool,
    output: &mut Normalized,
) {
    let line = &input[range.clone()];
    let indentation_end = line
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(offset, _)| range.start + offset)
        .unwrap_or(range.end);

    if preserve_indentation && indentation_end < range.end {
        for (offset, ch) in input[range.start..indentation_end].char_indices() {
            let start = range.start + offset;
            push_char(output, ch, start..start + ch.len_utf8());
        }
    }

    let mut pending_whitespace: Option<Range<usize>> = None;
    for (offset, ch) in input[indentation_end..range.end].char_indices() {
        let start = indentation_end + offset;
        let end = start + ch.len_utf8();
        if ch.is_whitespace() {
            match &mut pending_whitespace {
                Some(span) => span.end = end,
                None => pending_whitespace = Some(start..end),
            }
            continue;
        }
        if let Some(span) = pending_whitespace.take() {
            push_char(output, ' ', span);
        }
        push_char(output, ch, start..end);
    }
}

fn push_char(output: &mut Normalized, ch: char, source: Range<usize>) {
    output.text.push(ch);
    for _ in 0..ch.len_utf8() {
        output.source_spans.push(source.clone());
    }
}

fn indentation_flexible_replacement(
    content: &str,
    old: &str,
    new: &str,
    matched: &Range<usize>,
) -> String {
    let line_start = content[..matched.start]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    let source_base = &content[line_start..matched.start];
    let source_base = if source_base.chars().all(char::is_whitespace) {
        source_base
    } else {
        ""
    };
    let source_lines = content[matched.clone()].split('\n').collect::<Vec<_>>();
    let old_lines = old.split('\n').collect::<Vec<_>>();

    let mut replacement = String::with_capacity(new.len() + source_base.len());
    for (index, line) in new.split_inclusive('\n').enumerate() {
        let (body, newline) = line
            .strip_suffix('\n')
            .map_or((line, ""), |body| (body, "\n"));
        let old_indentation = old_lines.get(index).map_or_else(
            || leading_indentation(old),
            |line| leading_indentation(line),
        );
        let body = body.strip_prefix(old_indentation).unwrap_or(body);
        if index > 0 {
            let indentation = source_lines
                .get(index)
                .map_or(source_base, |line| leading_indentation(line));
            replacement.push_str(indentation);
        }
        replacement.push_str(body);
        replacement.push_str(newline);
    }
    replacement
}

fn leading_indentation(text: &str) -> &str {
    let first_line = text.split('\n').next().unwrap_or_default();
    let end = first_line
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(offset, _)| offset)
        .unwrap_or(first_line.len());
    &first_line[..end]
}

fn nearest_candidate(content: &str, old: &str) -> CandidateRegion {
    let source = source_lines(content);
    if source.is_empty() {
        return CandidateRegion {
            first_line: 1,
            last_line: 1,
            text: String::new(),
        };
    }

    let expected = old.lines().collect::<Vec<_>>();
    let window_len = expected.len().max(1).min(source.len());
    let expected_normalized = expected
        .iter()
        .map(|line| normalize(line, false).text)
        .collect::<Vec<_>>();
    let mut best_start = 0;
    let mut best_score = 0;

    for start in 0..=source.len() - window_len {
        let score = source[start..start + window_len]
            .iter()
            .zip(&expected_normalized)
            .map(|(candidate, wanted)| {
                let candidate = normalize(candidate.text, false).text;
                common_prefix_chars(&candidate, wanted)
                    + shared_exact_words(&candidate, wanted).saturating_mul(4)
            })
            .sum();
        if score > best_score {
            best_score = score;
            best_start = start;
        }
    }

    let selected = &source[best_start..best_start + window_len];
    let mut text = selected
        .iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join("\n");
    const MAX_CANDIDATE_CHARS: usize = 2_000;
    if text.chars().count() > MAX_CANDIDATE_CHARS {
        text = text.chars().take(MAX_CANDIDATE_CHARS).collect();
        text.push_str("\n...[candidate truncated]");
    }
    CandidateRegion {
        first_line: selected.first().map_or(1, |line| line.number),
        last_line: selected.last().map_or(1, |line| line.number),
        text,
    }
}

fn source_lines(content: &str) -> Vec<SourceLine<'_>> {
    if content.is_empty() {
        return Vec::new();
    }
    content
        .split('\n')
        .enumerate()
        .map(|(index, line)| SourceLine {
            number: index + 1,
            text: line.strip_suffix('\r').unwrap_or(line),
        })
        .collect()
}

fn common_prefix_chars(left: &str, right: &str) -> usize {
    left.chars()
        .zip(right.chars())
        .take_while(|(left, right)| left == right)
        .count()
}

fn shared_exact_words(left: &str, right: &str) -> usize {
    let right = right.split_whitespace().collect::<Vec<_>>();
    left.split_whitespace()
        .filter(|word| right.contains(word))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_stops_at_first_unique_tier() {
        let exact = locate("a  b", "a  b", "x").unwrap();
        assert_eq!(exact.tier, MatchTier::Exact);

        let whitespace = locate("a   b", "a b", "x").unwrap();
        assert_eq!(whitespace.tier, MatchTier::WhitespaceNormalized);

        let indentation = locate("    a\n      b", "a\n  b", "x\n  y").unwrap();
        assert_eq!(indentation.tier, MatchTier::IndentationFlexible);
        assert_eq!(indentation.replacement, "x\n      y");
    }

    #[test]
    fn ambiguity_at_a_tier_is_not_resolved_by_a_later_tier() {
        let failure = locate("a  b\na   b", "a b", "x").unwrap_err();
        assert!(matches!(
            failure,
            MatchFailure::Ambiguous {
                tier: MatchTier::WhitespaceNormalized,
                count: 2
            }
        ));
    }

    #[test]
    fn candidate_uses_the_closest_exact_words_and_reports_lines() {
        let candidate = match locate("zero\nalpha beta\nlast", "alpha gamma", "x") {
            Err(MatchFailure::NotFound(candidate)) => candidate,
            _ => panic!("expected not found"),
        };
        assert_eq!(candidate.first_line, 2);
        assert_eq!(candidate.last_line, 2);
        assert_eq!(candidate.text, "alpha beta");
    }
}
