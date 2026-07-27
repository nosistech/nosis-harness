//! Event reduction, timeline projection, and session cost accounting.

use super::*;

pub(super) fn outcome_name(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Pass => "pass",
        Outcome::Fail => "fail",
        Outcome::Partial => "partial",
        Outcome::Skip => "skip",
        Outcome::Timeout => "timeout",
    }
}

pub(super) fn failure_class_name(class: FailureClass) -> &'static str {
    match class {
        FailureClass::Context => "context",
        FailureClass::Constraint => "constraint",
        FailureClass::Filtered => "filtered",
        FailureClass::Verification => "verification",
        FailureClass::Planning => "planning",
        FailureClass::Unreceipted => "unreceipted",
    }
}

pub(super) fn is_compaction_progress(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("context ") && line.contains('%') && line.contains("compacted")
}

pub(super) fn timeline_row(entry: &TimelineEntry) -> String {
    let (input, output, cached) = entry.tokens();
    let compacted = if entry.compacted { "  [compact]" } else { "" };
    format!(
        "#{}  {}  {input}/{output}/{cached}{compacted}",
        entry.turn,
        outcome_name(entry.outcome)
    )
}

pub(super) fn timeline_detail_lines(entry: &TimelineEntry) -> Vec<String> {
    let (input, output, cached) = entry.tokens();
    let failure = entry
        .failure_class
        .map(failure_class_name)
        .unwrap_or("none");
    vec![
        format!("TURN #{}", entry.turn),
        format!("timestamp: {}", entry.ts_utc),
        format!("model: {}", entry.model_id),
        format!("task: {}", entry.task),
        format!("outcome: {}", outcome_name(entry.outcome)),
        format!("agent turns: {}", entry.turns),
        format!("tool calls: {}", entry.tool_calls),
        format!("failure class: {failure}"),
        format!("tokens: {input} in / {output} out / {cached} cached"),
        format!("compacted: {}", if entry.compacted { "yes" } else { "no" }),
        String::new(),
        format!("answer: {}", entry.answer),
    ]
}

/// Fold one worker event into application state.
pub fn apply_event(app: &mut App, event: AgentEvent) -> &Status {
    match event {
        AgentEvent::Progress(line) => {
            if is_compaction_progress(&line) {
                app.current_task_compacted = true;
            }
            app.push_line(&line, TranscriptKind::Progress);
        }
        AgentEvent::Approval(request) => {
            if app.session_allow.contains(&request.prompt) {
                let _ = request.reply.send(true);
                app.push_line(
                    &format!("auto-approved (session rule): {}", request.prompt),
                    TranscriptKind::Progress,
                );
                app.set_status(Status::Working, Utc::now());
            } else {
                let line = format!("approve: {}   {APPROVAL_LEGEND}", request.prompt);
                app.push_approval_line(&line);
                app.pending_approval = Some(request);
                app.set_status(Status::Waiting, Utc::now());
            }
        }
        AgentEvent::Usage(usage) => {
            app.usage = usage;
            if app.budget_reached() {
                app.set_status(Status::Blocked(BUDGET_REASON.into()), Utc::now());
            }
        }
        AgentEvent::TaskReceipt(summary) => {
            if let (Some(usage), Ok(at)) = (
                summary.receipt.usage.as_ref(),
                DateTime::parse_from_rfc3339(&summary.receipt.ts_utc),
            ) {
                record_turn_cost(app, usage, at.with_timezone(&Utc));
            }
            let turn = app.timeline.len().saturating_add(1);
            let compacted = std::mem::take(&mut app.current_task_compacted);
            app.timeline.push(TimelineEntry::from_receipt(
                turn,
                summary.receipt,
                summary.answer,
                compacted,
            ));
        }
        AgentEvent::Answer(answer) => {
            app.push_text("", &answer, TranscriptKind::Answer);
            let status = if app.budget_reached() {
                Status::Blocked(BUDGET_REASON.into())
            } else {
                Status::Idle
            };
            app.set_status(status, Utc::now());
        }
        AgentEvent::MeterIncomplete => app.has_failed_turn = true,
        AgentEvent::Failed(reason) => {
            let status_reason = safe_line(&app.scrubber, &reason);
            let what = reason
                .lines()
                .next()
                .filter(|line| !line.trim().is_empty())
                .unwrap_or("the task could not finish");
            let what = safe_line(&app.scrubber, what);
            app.push_line(
                &format!("! {what} — retry the task or type /help"),
                TranscriptKind::Error,
            );
            app.set_status(Status::Blocked(status_reason), Utc::now());
        }
    }
    &app.status
}

pub(super) fn record_turn_cost(app: &mut App, usage: &Usage, at: DateTime<Utc>) {
    let Some(quote) = app.route.price_at(at) else {
        return;
    };
    let cached = usage.cached_tokens.unwrap_or(0);
    let Some(actual) = cost_of(&quote, usage.prompt_tokens, cached, usage.completion_tokens) else {
        let _ = apply_event(app, AgentEvent::MeterIncomplete);
        return;
    };
    let uncertain = quote.stale || quote.confidence == PriceConfidence::VerifyLive;
    app.add_session_cost(quote.currency, actual, uncertain);
    for line in savings_lines(&app.resolver, &app.route, usage, at) {
        app.push_line(&line, TranscriptKind::Progress);
    }
}

pub(super) fn savings_lines(
    resolver: &RouteResolver,
    route: &ResolvedRoute,
    usage: &Usage,
    at: DateTime<Utc>,
) -> Vec<String> {
    let Some(quote) = route.price_at(at) else {
        return Vec::new();
    };
    let cached = usage.cached_tokens.unwrap_or(0);
    let Some(actual) = cost_of(&quote, usage.prompt_tokens, cached, usage.completion_tokens) else {
        return vec!["cost unpriced — invalid usage; meter incomplete".into()];
    };
    let mut paid = money_with_gloss(actual, quote.currency, resolver.fx(), at);
    let uncertain = quote.stale || quote.confidence == PriceConfidence::VerifyLive;
    if uncertain {
        paid.push('*');
    }
    let mut headline = format!("cost {paid}");
    let naive = resolver.naive_cost(
        route,
        usage.prompt_tokens,
        cached,
        usage.completion_tokens,
        at,
    );
    if let Some(percent) = naive
        .as_ref()
        .and_then(|costs| saved_pct(actual, costs.no_cache))
    {
        headline.push_str(&format!(" — saved {percent}% vs no-cache"));
    }
    let mut lines = vec![headline];
    if let Some(costs) = naive {
        lines.push(format!(
            "naive: peak {} · no-cache {} · top-tier {}",
            money(costs.peak, costs.currency),
            money(costs.no_cache, costs.currency),
            money(costs.top_tier, costs.currency)
        ));
    }
    if uncertain {
        lines.push(if quote.stale {
            "*price stale".into()
        } else {
            "*price verify_live".into()
        });
    }
    lines
}
