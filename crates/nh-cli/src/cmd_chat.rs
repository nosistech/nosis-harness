//! `nh chat` - line REPL with mid-session route switching (CONTRACTS_M1.md §4).
//! The footer and `/price` lines are the first visible cost HUD: one scannable,
//! aligned line each. The peak indicator shows the window boundary in the user's
//! local time ("peak 2x until 22:00"). `/model` and `/provider` keep the session
//! history across the switch (M1 exit criterion); usage accumulates all session.

mod startup;

use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, FixedOffset, Utc};
use nh_core::agent::{AgentLoop, MAX_TASK_BYTES};
use nh_core::session_ledger::{RestoredSession, RestoredTurn, SessionEvent, SessionLedger};
use nh_core::wire::{
    cache_hit_pct, ensure_image_capable, ChatClient, ChatMessage, ContentPart, Usage,
};
use nh_routes::{
    cost_of, money, money_with_gloss, Currency, PriceConfidence, Profiles, ResolvedRoute,
    RouteClass, RouteResolver, LOCAL_METER_COPY,
};
use nh_tools::Tool;
use nh_vault::{Scrubber, SecretRegistry, SecretValue};

use crate::cmd_run::{self, effort_for, DELEGATE_MSG};

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
}

/// Everything one chat session owns. History and usage survive route switches.
struct ChatSession {
    resolver: RouteResolver,
    route: ResolvedRoute,
    profiles: Profiles,
    active_profile: String,
    agent: AgentLoop,
    law_constitution: String,
    history: Vec<ChatMessage>,
    session_in: u64,
    session_out: u64,
    session_cached: Option<u64>,
    session_cost: Vec<SessionCost>,
    unpriced_turns: usize,
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
    let image_parts = std::mem::take(&mut s.pending_images);
    let result = if image_parts.is_empty() {
        s.agent.run_with_persistent_history(&mut s.history, task)
    } else {
        s.agent
            .run_with_persistent_history_and_parts(&mut s.history, task, image_parts)
    };
    let at = (s.now)();
    let usage = result
        .as_ref()
        .ok()
        .and_then(|(_, receipt)| receipt.usage.clone());
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
            let is_local = s.route.class() == RouteClass::Local;
            if let Some(u) = &receipt.usage {
                if add_session_usage(s, u) {
                    add_session_cost(s, u, at);
                    if !is_local {
                        if let Some(line) = cmd_run::turn_cost_line(&s.resolver, &s.route, u, at) {
                            let _ = writeln!(err, "{}", scrub_line(&s.scrubber, &line));
                        }
                    }
                }
            }
            if is_local {
                let _ = writeln!(err, "{LOCAL_METER_COPY}");
            }
            let _ = writeln!(err, "{}", scrub_line(&s.scrubber, &footer(s)));
        }
        Err(e) => print_err(s, err, &e.to_string()),
    }
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
    s.agent.client = client;
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
    let line = format!(
        "{} | {} | in {:.4} hit / {:.4} miss | out {:.4} | {}/M tokens | confidence {} | session {}",
        s.route.id(),
        s.route.peak_status(now, s.local_offset),
        quote.cache_hit,
        quote.cache_miss,
        quote.output,
        quote.currency,
        quote.confidence,
        session_money(s, now)
    );
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
/// `deepseek-v4-flash | peak 2x until 22:00 | session ¥0.11 | tokens 812 in / 340 out / 512 cached | cache 63%`.
fn footer(s: &ChatSession) -> String {
    let now = (s.now)();
    let mut line = format!(
        "{} | {} | session {} | tokens {} in / {} out",
        s.route.id(),
        s.route.peak_status(now, s.local_offset),
        session_money(s, now),
        s.session_in,
        s.session_out
    );
    if let (Some(cached), Some(pct)) = (
        s.session_cached,
        cache_hit_pct(s.session_in, s.session_cached),
    ) {
        line.push_str(&format!(" / {cached} cached | cache {pct:.0}%"));
    }
    if s.resumed {
        line.push_str(" | resumed");
    }
    line
}

fn add_session_usage(s: &mut ChatSession, usage: &nh_core::wire::Usage) -> bool {
    let Some(session_in) = s.session_in.checked_add(usage.prompt_tokens) else {
        s.unpriced_turns = s.unpriced_turns.saturating_add(1);
        return false;
    };
    let Some(session_out) = s.session_out.checked_add(usage.completion_tokens) else {
        s.unpriced_turns = s.unpriced_turns.saturating_add(1);
        return false;
    };
    let session_cached = match (s.session_cached, usage.cached_tokens) {
        (Some(session_cached), Some(cached)) => {
            let Some(session_cached) = session_cached.checked_add(cached) else {
                s.unpriced_turns = s.unpriced_turns.saturating_add(1);
                return false;
            };
            Some(session_cached)
        }
        _ => None,
    };

    s.session_in = session_in;
    s.session_out = session_out;
    s.session_cached = session_cached;
    true
}

fn add_session_cost(s: &mut ChatSession, usage: &nh_core::wire::Usage, at: DateTime<Utc>) {
    let route = s.route.clone();
    add_route_cost(s, &route, usage, at);
}

fn add_route_cost(s: &mut ChatSession, route: &ResolvedRoute, usage: &Usage, at: DateTime<Utc>) {
    if route.class() == RouteClass::Local {
        return;
    }
    let Some(quote) = route.price_at(at) else {
        s.unpriced_turns = s.unpriced_turns.saturating_add(1);
        return;
    };
    let Some(amount) = cost_of(
        &quote,
        usage.prompt_tokens,
        usage.cached_tokens.unwrap_or(0),
        usage.completion_tokens,
    ) else {
        s.unpriced_turns = s.unpriced_turns.saturating_add(1);
        return;
    };
    let uncertain = quote.confidence == PriceConfidence::VerifyLive;
    if let Some(total) = s
        .session_cost
        .iter_mut()
        .find(|total| total.currency == quote.currency)
    {
        total.amount += amount;
        total.uncertain |= uncertain;
    } else {
        s.session_cost.push(SessionCost {
            currency: quote.currency,
            amount,
            uncertain,
        });
    }
}

fn restore_session_totals(s: &mut ChatSession, turns: &[RestoredTurn]) -> anyhow::Result<()> {
    let mut replay = Vec::new();
    for turn in turns {
        let Some(usage) = &turn.usage else {
            continue;
        };
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
        replay.push((route, usage.clone(), at));
    }
    for (route, usage, at) in replay {
        if add_session_usage(s, &usage) {
            add_route_cost(s, &route, &usage, at);
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
        let mixed = s.session_cost.len() > 1;
        [Currency::Cny, Currency::Usd]
            .into_iter()
            .filter_map(|currency| {
                s.session_cost
                    .iter()
                    .find(|total| total.currency == currency)
                    .map(|total| {
                        let mut display = if mixed {
                            money(total.amount, total.currency)
                        } else {
                            money_with_gloss(total.amount, total.currency, s.resolver.fx(), at)
                        };
                        if total.uncertain {
                            display.push('*');
                        }
                        display
                    })
            })
            .collect::<Vec<_>>()
            .join(" · ")
    };
    if s.unpriced_turns > 0 {
        let noun = if s.unpriced_turns == 1 {
            "turn"
        } else {
            "turns"
        };
        display.push_str(&format!(
            " (incomplete - {} unpriced {noun})",
            s.unpriced_turns
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
