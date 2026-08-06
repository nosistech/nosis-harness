//! `nh chat` - line REPL with mid-session route switching.
//! The footer and `/price` lines are the first visible cost HUD: one scannable,
//! aligned line each. The peak indicator shows the window boundary in the user's
//! local time ("peak 2x until 22:00"). `/model` and `/provider` keep the session
//! history across the switch (M1 exit criterion); usage accumulates all session.

mod startup;

use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, FixedOffset, Utc};
use nh_core::agent::{AgentLoop, AgentRunError, MAX_TASK_BYTES};
use nh_core::receipt::Receipt;
use nh_core::session_ledger::{RestoredSession, RestoredTurn, SessionEvent, SessionLedger};
use nh_core::wire::{
    ensure_image_capable, ChatClient, ChatMessage, ContentPart, Usage, UsageEvidence,
};
use nh_routes::{
    cache_split_cost_upper_bound, cost_of, money, money_with_gloss, Currency, PriceConfidence,
    Profiles, ResolvedRoute, RouteClass, RouteResolver, LOCAL_METER_COPY,
};
use nh_tools::Tool;
use nh_vault::{Scrubber, SecretRegistry, SecretValue};

use crate::cmd_run::{self, effort_for, DELEGATE_MSG};
use crate::usage_tracker::LastRequestUsage;

/// Builds a wire client + its key literal (for the Scrubber) from a route.
/// Injected so tests drive the REPL with a mock client - no vault, no network.
type ConnectFn =
    Box<dyn Fn(&ResolvedRoute, Option<u64>) -> anyhow::Result<(Box<dyn ChatClient>, SecretValue)>>;

/// One Scrubber shared by every output path; rebuilt whenever a switch adds a key.
type SharedScrubber = Arc<RwLock<Scrubber>>;

#[derive(Debug, PartialEq, Eq)]
enum ChatInput {
    Line(String),
    TooLong,
}

fn read_chat_input(reader: &mut dyn BufRead) -> std::io::Result<Option<ChatInput>> {
    let mut bytes = Vec::new();
    let mut saw_input = false;
    let mut too_long = false;

    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if !saw_input {
                return Ok(None);
            }
            break;
        }
        saw_input = true;

        let newline = available.iter().position(|byte| *byte == b'\n');
        let content_len = newline.unwrap_or(available.len());
        if !too_long {
            let remaining = (MAX_TASK_BYTES + 1).saturating_sub(bytes.len());
            let copy_len = content_len.min(remaining);
            bytes.extend_from_slice(&available[..copy_len]);
            too_long = copy_len < content_len || bytes.len() > MAX_TASK_BYTES;
        }

        let consumed = newline.map_or(available.len(), |index| index + 1);
        let complete = newline.is_some();
        reader.consume(consumed);
        if complete {
            break;
        }
    }

    if too_long {
        return Ok(Some(ChatInput::TooLong));
    }
    let line = String::from_utf8(bytes).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "chat input is not valid UTF-8",
        )
    })?;
    Ok(Some(ChatInput::Line(line)))
}

struct SessionCost {
    currency: Currency,
    amount: f64,
    uncertain: bool,
    upper_bound: bool,
}

/// Everything one chat session owns. History and usage survive route switches.
struct ChatSession {
    resolver: Arc<RouteResolver>,
    route: ResolvedRoute,
    profiles: Profiles,
    active_profile: String,
    agent: AgentLoop,
    law_constitution: String,
    history: Vec<ChatMessage>,
    /// None means no task has run. Once a task is attempted, its typed evidence
    /// is retained even when the provider reported no usable counters.
    session_usage: Option<Usage>,
    /// Provider evidence for the latest request only. Unlike `session_usage`,
    /// context occupancy is not cumulative and may fall after compaction.
    last_request_usage: LastRequestUsage,
    /// Cache evidence from the provider call immediately before the next task.
    /// Unlike `session_usage`, this is not a total: compaction pricing may use
    /// only the preceding call's measured value.
    last_cached_tokens: Option<u64>,
    session_cost: Vec<SessionCost>,
    incomplete_cost_turns: usize,
    /// Every key literal this session has seen - switched-away keys stay scrubbed.
    key_literals: SecretRegistry,
    scrubber: SharedScrubber,
    connect: ConnectFn,
    /// False after a keyless start (stand-in client installed); tasks retry the
    /// real connection first, so `nh key add` works without restarting the chat.
    connected: bool,
    /// Clock and local UTC offset, injected so /price and footer tests are exact.
    now: Box<dyn Fn() -> DateTime<Utc>>,
    local_offset: FixedOffset,
    mcp_warnings: Vec<String>,
    pending_images: Vec<ContentPart>,
    ledger: SessionLedger,
    ledger_failed: bool,
    ledger_notice_shown: bool,
    resumed: bool,
    restored_turns: usize,
    dropped_torn_tail: bool,
    constitution_changed: bool,
    pending_route_context: Option<ChatMessage>,
}

const CHAT_HELP: &str = "commands: /image <path> (PNG or JPEG; max 4 for the next message), \
                         /model <id>, /provider <name>, /price, /tools, /quit";

pub fn run(model: &str, profile: &str) -> anyhow::Result<()> {
    run_session(startup::open(model, profile)?)
}

pub(crate) fn resume(restored: RestoredSession) -> anyhow::Result<()> {
    run_session(startup::reopen(restored)?)
}

fn run_session(mut session: ChatSession) -> anyhow::Result<()> {
    if session.resumed {
        eprintln!(
            "resumed {} - {} turns restored on {}",
            scrub_line(&session.scrubber, session.ledger.session_id()),
            session.restored_turns,
            scrub_line(&session.scrubber, session.route.id())
        );
        if session.dropped_torn_tail {
            eprintln!("last session record was incomplete and was dropped - continuing safely");
        }
        if session.constitution_changed {
            eprintln!(
                "session kept its original constitution - start a new session to use the current one"
            );
        }
    } else {
        eprintln!(
            "chat started: {} on {} - /help lists commands",
            scrub_line(&session.scrubber, session.ledger.session_id()),
            scrub_line(&session.scrubber, session.route.id())
        );
    }
    let mut next_line = || {
        let stdin = std::io::stdin();
        read_chat_input(&mut stdin.lock())
    };
    chat_loop(
        &mut session,
        &mut next_line,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

/// Stand-in client for a keyless start: every task fails with the stored
/// (already friendly) vault error; the REPL itself stays fully usable.
struct NotConnected {
    msg: String,
}

impl ChatClient for NotConnected {
    fn complete(
        &self,
        _req: &nh_core::wire::ChatRequest,
    ) -> anyhow::Result<nh_core::wire::ChatResponse> {
        anyhow::bail!("{}", self.msg)
    }
}

#[derive(PartialEq)]
enum Flow {
    Continue,
    Quit,
}

/// The REPL. Prompt on stderr; stdout carries only answers and command output.
/// EOF or /quit exits cleanly; blank lines re-prompt.
fn chat_loop(
    s: &mut ChatSession,
    next_line: &mut dyn FnMut() -> std::io::Result<Option<ChatInput>>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> anyhow::Result<()> {
    report_persistence_failure(s, err);
    loop {
        let _ = write!(err, "nh> ");
        let _ = err.flush();
        let Some(input) = next_line()? else {
            end_session(s, err);
            return Ok(());
        };
        let raw = match input {
            ChatInput::Line(raw) => raw,
            ChatInput::TooLong => {
                let _ = writeln!(
                    err,
                    "error: input is too large - maximum is {MAX_TASK_BYTES} bytes"
                );
                continue;
            }
        };
        // Windows shells prefix piped input with a UTF-8 BOM; strip it so
        // `echo "/price" | nh chat` sees the command, not a strange task.
        let line = raw.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
        if line.is_empty() {
            continue;
        }
        match line.strip_prefix('/') {
            Some(cmd) => {
                if handle_command(s, cmd, out, err) == Flow::Quit {
                    end_session(s, err);
                    return Ok(());
                }
            }
            None => run_task(s, line, out, err),
        }
    }
}

fn handle_command(
    s: &mut ChatSession,
    cmd: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Flow {
    let mut parts = cmd.split_whitespace();
    let name = parts.next().unwrap_or("");
    let arg = parts.next();
    match (name, arg) {
        ("quit", _) => return Flow::Quit,
        ("help", _) => {
            let _ = writeln!(out, "{CHAT_HELP}");
        }
        ("model", Some(id)) => match s.resolver.resolve(id) {
            Ok(route) => switch_to(s, route, out, err),
            Err(e) => print_err(s, err, &e.to_string()),
        },
        ("model", None) => print_err(s, err, "usage: /model <id>"),
        ("provider", Some(p)) => match s.resolver.provider_default(p) {
            Ok(route) => switch_to(s, route, out, err),
            Err(e) => print_err(s, err, &e.to_string()),
        },
        ("provider", None) => print_err(s, err, "usage: /provider <name>"),
        ("image", _) => {
            let path = cmd
                .strip_prefix(name)
                .map(str::trim)
                .filter(|path| !path.is_empty());
            match path {
                Some(path) => attach_image(s, path, out, err),
                None => print_err(s, err, "usage: /image <path> (PNG or JPEG; max 4)"),
            }
        }
        ("price", _) => print_price(s, out),
        ("tools", _) => print_tools(s, out, err),
        _ => print_err(s, err, "unknown command - type /help"),
    }
    Flow::Continue
}

/// Run one task through the shared session history; answer to stdout, footer to
/// stderr. A failed call is one friendly line and the session keeps going.
fn run_task(s: &mut ChatSession, task: &str, out: &mut dyn Write, err: &mut dyn Write) {
    // Keyless start? Retry the real connection now - a key added mid-session
    // (`nh key add <provider>` in another terminal) works without restarting.
    if !s.connected {
        let policy = s.profiles.effective(&s.active_profile, &s.route);
        match (s.connect)(&s.route, policy.output_cap) {
            Ok((client, literal)) => install_client(s, client, literal),
            Err(e) => {
                print_err(s, err, &e.to_string());
                return;
            }
        }
    }
    let history_before = s.history.len();
    if let Some(message) = s.pending_route_context.take() {
        s.history.push(message);
    }
    refresh_progress_meter(s);
    let image_parts = std::mem::take(&mut s.pending_images);
    let result = if image_parts.is_empty() {
        s.agent.run_with_persistent_history_and_cache(
            &mut s.history,
            task,
            &mut s.last_cached_tokens,
        )
    } else {
        s.agent.run_with_persistent_history_and_parts_and_cache(
            &mut s.history,
            task,
            image_parts,
            &mut s.last_cached_tokens,
        )
    };
    if result.is_err() {
        s.last_cached_tokens = None;
    }
    let at = (s.now)();
    let receipt = match &result {
        Ok((_, receipt)) => Some(receipt),
        Err(error) => error
            .downcast_ref::<AgentRunError>()
            .map(AgentRunError::receipt),
    };
    let usage = receipt.and_then(|receipt| receipt.usage.clone());
    let messages = s
        .history
        .get(history_before..)
        .map_or_else(Vec::new, <[ChatMessage]>::to_vec);
    append_session_event(
        s,
        &SessionEvent::Turn {
            ts_utc: session_timestamp(at),
            route_id: s.route.id().to_owned(),
            messages,
            usage,
        },
        err,
    );
    match result {
        Ok((answer, receipt)) => {
            let _ = writeln!(out, "{}", scrub_text(&s.scrubber, &answer));
            project_turn_meter(s, Some(&receipt), at, err);
        }
        Err(error) => {
            print_err(s, err, &error.to_string());
            let receipt = error
                .downcast_ref::<AgentRunError>()
                .map(AgentRunError::receipt);
            project_turn_meter(s, receipt, at, err);
        }
    }
}

fn project_turn_meter(
    s: &mut ChatSession,
    receipt: Option<&Receipt>,
    at: DateTime<Utc>,
    err: &mut dyn Write,
) {
    let usage = receipt.and_then(|receipt| receipt.usage.as_ref());
    let route = s.route.clone();
    if add_session_usage(s, usage) {
        add_session_cost(s, usage, at);
    }

    if route.class() == RouteClass::Local {
        let _ = writeln!(err, "{LOCAL_METER_COPY}");
    } else {
        let line = usage.map_or_else(
            || "cost unknown - tokens not reported by provider".to_owned(),
            |usage| {
                cmd_run::turn_cost_line(&s.resolver, &route, usage, at)
                    .unwrap_or_else(|| "cost unknown".to_owned())
            },
        );
        let _ = writeln!(err, "{}", scrub_line(&s.scrubber, &line));
    }
    if let Some(line) = receipt.and_then(|receipt| {
        cmd_run::compaction_meter_line(&s.resolver, &route, &receipt.compaction)
    }) {
        let _ = writeln!(err, "{}", scrub_line(&s.scrubber, &line));
    }
    let _ = writeln!(err, "{}", scrub_line(&s.scrubber, &footer(s)));
}

fn refresh_progress_meter(s: &mut ChatSession) {
    if s.agent.on_event.is_none() {
        return;
    }
    let resolver = Arc::clone(&s.resolver);
    let route = s.route.clone();
    let scrubber = Arc::clone(&s.scrubber);
    s.agent.on_event = Some(Box::new(move |core_line| {
        let line = cmd_run::progress_meter_line(&resolver, &route, core_line);
        eprintln!("  {}", scrub_line(&scrubber, &line));
    }));
}

fn attach_image(s: &mut ChatSession, path: &str, out: &mut dyn Write, err: &mut dyn Write) {
    if s.pending_images.len() >= nh_tools::MAX_IMAGES_PER_MESSAGE {
        print_err(
            s,
            err,
            "a message can attach at most 4 images - send the pending message first",
        );
        return;
    }
    if let Err(error) = ensure_image_capable(&s.route, &s.resolver) {
        print_err(s, err, &error.to_string());
        return;
    }
    match cmd_run::image_part(path, &s.agent.ctx) {
        Ok(part) => {
            s.pending_images.push(part);
            let line = format!(
                "attached {path} for next message ({}/{})",
                s.pending_images.len(),
                nh_tools::MAX_IMAGES_PER_MESSAGE
            );
            let _ = writeln!(out, "{}", scrub_line(&s.scrubber, &line));
        }
        Err(error) => print_err(s, err, &error.to_string()),
    }
}

/// Swap in a live client. Its key literal joins the Scrubber on every output
/// path (stdout, stderr closures, receipts) before the client ever runs.
fn install_client(s: &mut ChatSession, client: Box<dyn ChatClient>, literal: SecretValue) {
    s.key_literals.insert(literal);
    let registry = s.key_literals.scrubber();
    match s.scrubber.write() {
        Ok(mut guard) => *guard = registry.clone(),
        Err(poisoned) => *poisoned.into_inner() = registry.clone(),
    }
    // Refresh the tool-boundary scrubber too, so a route switch cannot leave the
    // newly-active credential unredacted at tool egress, same as the TUI.
    s.agent.ctx.scrubber = registry.clone();
    s.agent.receipts.replace_scrubber(registry.clone());
    s.ledger.replace_scrubber(registry);
    s.agent.client = s.last_request_usage.wrap(client);
    s.connected = true;
}

/// Switch the live route, keeping history and session usage.
fn switch_to(s: &mut ChatSession, route: ResolvedRoute, out: &mut dyn Write, err: &mut dyn Write) {
    if route.class() == RouteClass::Delegate {
        print_err(s, err, DELEGATE_MSG);
        return;
    }
    let execution_policy = s.profiles.effective(&s.active_profile, &route);
    match (s.connect)(&route, execution_policy.output_cap) {
        Ok((client, literal)) => {
            install_client(s, client, literal);
            s.agent.model_id = route.model_id().to_owned();
            s.agent.thinking = effort_for(
                None,
                execution_policy.posture,
                route.thinking_dialect(),
                route.wire(),
            );
            s.agent.profile = Some(execution_policy.profile.clone());
            s.active_profile = execution_policy.profile;
            // Preserve the sealed prefix. The next task records the new route
            // context as part of that turn's append-only history delta.
            let constitution = cmd_run::agent_constitution(&s.law_constitution, &route);
            s.pending_route_context =
                (!s.history.is_empty()).then(|| route_context_message(constitution.clone()));
            s.agent.constitution = Some(constitution);
            s.agent.context_limit = route.context();
            s.route = route;
            s.last_cached_tokens = None;
            let event = SessionEvent::RouteSwitched {
                ts_utc: session_timestamp((s.now)()),
                route_id: s.route.id().to_owned(),
                model_id: s.route.model_id().to_owned(),
                profile: s.active_profile.clone(),
            };
            append_session_event(s, &event, err);
            let _ = writeln!(out, "switched to {}", scrub_line(&s.scrubber, s.route.id()));
        }
        // Unknown key, delegate, whatever - keep the current route, say why.
        Err(e) => print_err(s, err, &e.to_string()),
    }
}

/// `/price` - the cost HUD quote for this instant, one aligned line.
fn print_price(s: &ChatSession, out: &mut dyn Write) {
    if s.route.class() == RouteClass::Local {
        let _ = writeln!(out, "{LOCAL_METER_COPY}");
        return;
    }
    let now = (s.now)();
    let Some(quote) = s.route.price_at(now) else {
        let line = format!(
            "no price data for {id} - add a [routes.{id}.price] table to catalog.toml",
            id = s.route.id()
        );
        let _ = writeln!(out, "{}", scrub_line(&s.scrubber, &line));
        return;
    };
    let mut segments = vec![s.route.id().to_owned()];
    if let Some(peak_status) = s.route.peak_status(now, s.local_offset) {
        segments.push(peak_status);
    }
    segments.push(format!(
        "in {} hit / {} miss",
        money(quote.cache_hit, quote.currency),
        money(quote.cache_miss, quote.currency)
    ));
    segments.push(format!("out {}", money(quote.output, quote.currency)));
    segments.push("per million tokens".to_owned());
    segments.push(format!("confidence {}", quote.confidence));
    segments.push(format!("session {}", session_money(s, now)));
    let line = segments.join(" | ");
    let _ = writeln!(out, "{}", scrub_line(&s.scrubber, &line));
}

/// `/tools` - builtin tools first, then MCP tools, one line each; MCP warnings
/// go to stderr after the list.
fn print_tools(s: &ChatSession, out: &mut dyn Write, err: &mut dyn Write) {
    for tool in &s.agent.tools {
        let spec = tool.spec();
        let first = spec.description.lines().next().unwrap_or("");
        let _ = writeln!(
            out,
            "{}",
            scrub_line(&s.scrubber, &format!("{} - {first}", spec.name))
        );
    }
    for w in &s.mcp_warnings {
        let _ = writeln!(err, "warning: {}", scrub_line(&s.scrubber, w));
    }
}

/// Footer: the always-on cost HUD line, e.g.
/// `deepseek-v4-flash | peak 2x until 22:00 | session ¥0.11 | tokens 812 in / 340 out / 512 cached | cache 63% | ctx 41%`.
fn footer(s: &ChatSession) -> String {
    let now = (s.now)();
    let mut segments = vec![s.route.id().to_owned()];
    if let Some(peak_status) = s.route.peak_status(now, s.local_offset) {
        segments.push(peak_status);
    }
    segments.push(format!("session {}", session_money(s, now)));
    segments.push(s.session_usage.as_ref().map_or_else(
        || "tokens 0 in / 0 out".to_owned(),
        |usage| cmd_run::usage_token_summary(Some(usage)),
    ));
    let last_request_usage = s.last_request_usage.snapshot();
    if let Some(context) = cmd_run::context_window_summary(&s.route, last_request_usage.as_ref()) {
        segments.push(context);
    }
    if s.resumed {
        segments.push("resumed".to_owned());
    }
    segments.join(" | ")
}

fn unknown_usage() -> Usage {
    Usage {
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_tokens: None,
        evidence: UsageEvidence::Unknown,
    }
}

fn add_session_usage(s: &mut ChatSession, usage: Option<&Usage>) -> bool {
    let usage = usage.cloned().unwrap_or_else(unknown_usage);
    match &mut s.session_usage {
        Some(total) => {
            if total.checked_add_assign(&usage) {
                true
            } else {
                total.evidence = UsageEvidence::Partial;
                s.incomplete_cost_turns = s.incomplete_cost_turns.saturating_add(1);
                false
            }
        }
        None => {
            s.session_usage = Some(usage);
            true
        }
    }
}

fn add_session_cost(s: &mut ChatSession, usage: Option<&Usage>, at: DateTime<Utc>) {
    let route = s.route.clone();
    add_route_cost(s, &route, usage, at);
}

fn add_route_cost(
    s: &mut ChatSession,
    route: &ResolvedRoute,
    usage: Option<&Usage>,
    at: DateTime<Utc>,
) {
    if route.class() == RouteClass::Local {
        return;
    }
    let Some(usage) = usage.filter(|usage| usage.evidence == UsageEvidence::Measured) else {
        s.incomplete_cost_turns = s.incomplete_cost_turns.saturating_add(1);
        return;
    };
    let Some(quote) = route.price_at(at) else {
        s.incomplete_cost_turns = s.incomplete_cost_turns.saturating_add(1);
        return;
    };
    let amount = usage.cached_tokens.map_or_else(
        || cache_split_cost_upper_bound(&quote, usage.prompt_tokens, usage.completion_tokens),
        |cached| cost_of(&quote, usage.prompt_tokens, cached, usage.completion_tokens),
    );
    let Some(amount) = amount else {
        s.incomplete_cost_turns = s.incomplete_cost_turns.saturating_add(1);
        return;
    };
    let uncertain = quote.confidence == PriceConfidence::VerifyLive;
    let upper_bound = usage.cached_tokens.is_none();
    if let Some(index) = s
        .session_cost
        .iter()
        .position(|total| total.currency == quote.currency)
    {
        let total = &mut s.session_cost[index];
        let sum = total.amount + amount;
        if !sum.is_finite() {
            s.incomplete_cost_turns = s.incomplete_cost_turns.saturating_add(1);
            return;
        }
        total.amount = sum;
        total.uncertain |= uncertain;
        total.upper_bound |= upper_bound;
    } else {
        s.session_cost.push(SessionCost {
            currency: quote.currency,
            amount,
            uncertain,
            upper_bound,
        });
    }
}

fn restore_session_totals(s: &mut ChatSession, turns: &[RestoredTurn]) -> anyhow::Result<()> {
    let mut replay = Vec::new();
    for turn in turns {
        let route = s.resolver.resolve(&turn.route_id).map_err(|_| {
            anyhow::anyhow!(
                "session route {} is no longer available - restore it in catalog.toml, then retry",
                turn.route_id
            )
        })?;
        let at = DateTime::parse_from_rfc3339(&turn.ts_utc)
            .map_err(|_| {
                anyhow::anyhow!("session has an invalid turn timestamp - inspect its ledger")
            })?
            .with_timezone(&Utc);
        replay.push((route, turn.usage.clone(), at));
    }
    for (route, usage, at) in replay {
        if add_session_usage(s, usage.as_ref()) {
            add_route_cost(s, &route, usage.as_ref(), at);
        }
    }
    Ok(())
}

fn route_context_message(content: String) -> ChatMessage {
    ChatMessage {
        role: "system".to_owned(),
        content: Some(content),
        parts: None,
        tool_calls: None,
        tool_call_id: None,
        reasoning_content: None,
    }
}

fn session_timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn append_session_event(s: &mut ChatSession, event: &SessionEvent, err: &mut dyn Write) {
    if s.ledger_failed {
        report_persistence_failure(s, err);
        return;
    }
    if s.ledger.append(event).is_err() {
        s.ledger_failed = true;
        report_persistence_failure(s, err);
    }
}

fn report_persistence_failure(s: &mut ChatSession, err: &mut dyn Write) {
    if s.ledger_failed && !s.ledger_notice_shown {
        let line = "session persistence is off - copy anything you need before quitting";
        let _ = writeln!(err, "{}", scrub_line(&s.scrubber, line));
        s.ledger_notice_shown = true;
    }
}

fn end_session(s: &mut ChatSession, err: &mut dyn Write) {
    let event = SessionEvent::Ended {
        ts_utc: session_timestamp((s.now)()),
    };
    append_session_event(s, &event, err);
}

fn session_money(s: &ChatSession, at: DateTime<Utc>) -> String {
    let incomplete = s.incomplete_cost_turns > 0;
    if incomplete
        && (s.session_cost.iter().any(|total| total.upper_bound)
            || s.session_cost.is_empty()
            || s.session_cost
                .iter()
                .all(|total| total.amount.abs() <= f64::EPSILON))
    {
        let noun = if s.incomplete_cost_turns == 1 {
            "turn"
        } else {
            "turns"
        };
        return format!("unknown (incomplete - {} {noun})", s.incomplete_cost_turns);
    }
    let mut display = if s.session_cost.is_empty() {
        if s.route.class() == RouteClass::Local {
            "no billed tokens".into()
        } else {
            s.route.price_at(at).map_or_else(
                || "-".into(),
                |quote| {
                    let mut display = money_with_gloss(0.0, quote.currency, s.resolver.fx(), at);
                    if quote.confidence == PriceConfidence::VerifyLive {
                        display.push('*');
                    }
                    display
                },
            )
        }
    } else {
        let visible_totals = s
            .session_cost
            .iter()
            .filter(|total| !incomplete || total.amount.abs() > f64::EPSILON)
            .count();
        let mixed = visible_totals > 1;
        [Currency::Cny, Currency::Usd]
            .into_iter()
            .filter_map(|currency| {
                s.session_cost
                    .iter()
                    .find(|total| total.currency == currency)
                    .filter(|total| !incomplete || total.amount.abs() > f64::EPSILON)
                    .map(|total| {
                        let mut display = if mixed {
                            money(total.amount, total.currency)
                        } else {
                            money_with_gloss(total.amount, total.currency, s.resolver.fx(), at)
                        };
                        if total.uncertain {
                            display.push('*');
                        }
                        if total.upper_bound {
                            display.insert_str(0, "at most ");
                        }
                        if incomplete {
                            display.insert(0, '~');
                        }
                        display
                    })
            })
            .collect::<Vec<_>>()
            .join(" · ")
    };
    if incomplete {
        let noun = if s.incomplete_cost_turns == 1 {
            "turn"
        } else {
            "turns"
        };
        display.push_str(&format!(
            " (lower bound - {} incomplete {noun})",
            s.incomplete_cost_turns
        ));
    }
    display
}

/// Load MCP tools from repository and user-global config when either exists.
/// Any failure becomes a warning line, never an error.
fn load_mcp(
    root: &Path,
    home: Option<&Path>,
    policy: &nh_law::Policy,
    warnings: &mut Vec<String>,
) -> Vec<Box<dyn Tool>> {
    let configs = cmd_run::load_and_vet_mcp_configs(root, home, policy, warnings);
    let send_allowed = |host: &str| !matches!(policy.send_verdict(host), nh_law::Verdict::Block(_));
    let set = nh_tools::mcp::mcp_tools(&configs, &send_allowed);
    warnings.extend(set.warnings);
    set.tools
}

/// Scrub + control-char-escape one display line via the shared Scrubber.
fn scrub_line(scrubber: &SharedScrubber, text: &str) -> String {
    match scrubber.read() {
        Ok(guard) => cmd_run::safe_line(&guard, text),
        Err(poisoned) => cmd_run::safe_line(&poisoned.into_inner(), text),
    }
}

/// Scrub and control-escape each answer line while preserving newlines.
fn scrub_text(scrubber: &SharedScrubber, text: &str) -> String {
    match scrubber.read() {
        Ok(guard) => cmd_run::safe_text(&guard, text),
        Err(poisoned) => cmd_run::safe_text(&poisoned.into_inner(), text),
    }
}

fn print_err(s: &ChatSession, err: &mut dyn Write, msg: &str) {
    let _ = writeln!(err, "{}", scrub_line(&s.scrubber, msg));
}

#[cfg(test)]
mod tests;
