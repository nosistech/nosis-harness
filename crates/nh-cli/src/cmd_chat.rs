//! `nh chat` — line REPL with mid-session route switching (CONTRACTS_M1.md §4).
//! The footer and `/price` lines are the first visible cost HUD: one scannable,
//! aligned line each. The peak indicator shows the window boundary in the user's
//! local time ("peak 2x until 22:00"). `/model` and `/provider` keep the session
//! history across the switch (M1 exit criterion); usage accumulates all session.

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, RwLock};

use chrono::{DateTime, FixedOffset, Utc};
use nh_core::agent::AgentLoop;
use nh_core::receipt::ReceiptWriter;
use nh_core::wire::{cache_hit_pct, make_client, ChatClient, ChatMessage};
use nh_law::LoadOptions;
use nh_routes::{
    cost_of, money, money_with_gloss, Currency, PriceConfidence, ResolvedRoute, RouteClass,
    RouteResolver,
};
use nh_tools::{builtin_tools, Access, Tool, ToolCtx};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber};

use crate::guard_from;
use crate::cmd_run::{self, effort_for, DELEGATE_MSG};

/// Builds a wire client + its key literal (for the Scrubber) from a route.
/// Injected so tests drive the REPL with a mock client — no vault, no network.
type ConnectFn = Box<dyn Fn(&ResolvedRoute) -> anyhow::Result<(Box<dyn ChatClient>, String)>>;

/// One Scrubber shared by every output path; rebuilt whenever a switch adds a key.
type SharedScrubber = Arc<RwLock<Scrubber>>;

struct SessionCost {
    currency: Currency,
    amount: f64,
    uncertain: bool,
}

/// Everything one chat session owns. History and usage survive route switches.
struct ChatSession {
    resolver: RouteResolver,
    route: ResolvedRoute,
    agent: AgentLoop,
    law_constitution: String,
    history: Vec<ChatMessage>,
    session_in: u64,
    session_out: u64,
    session_cached: u64,
    session_cost: Vec<SessionCost>,
    /// Every key literal this session has seen — switched-away keys stay scrubbed.
    key_literals: Vec<String>,
    /// Every catalog vault entry, refreshed into the scrubber after reconnects.
    vault_entries: Vec<String>,
    scrubber: SharedScrubber,
    connect: ConnectFn,
    /// False after a keyless start (stand-in client installed); tasks retry the
    /// real connection first, so `nh key add` works without restarting the chat.
    connected: bool,
    /// Clock and local UTC offset, injected so /price and footer tests are exact.
    now: Box<dyn Fn() -> DateTime<Utc>>,
    local_offset: FixedOffset,
    mcp_warnings: Vec<String>,
}

pub fn run(model: &str) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (root, catalog) = cmd_run::find_catalog(&cwd)?;
    let law = nh_law::load(&root, &LoadOptions { cli_autonomy: None });
    let warning_scrubber = Scrubber::new(Vec::new());
    for warning in &law.warnings {
        eprintln!("warning: {}", cmd_run::safe_line(&warning_scrubber, warning));
    }
    let resolver = RouteResolver::from_toml(&catalog)?;
    let route = resolver.resolve(model)?;
    if route.class == RouteClass::Delegate {
        anyhow::bail!("{DELEGATE_MSG}");
    }

    let connect_policy = law.policy.clone();
    let connect: ConnectFn = Box::new(move |route| {
        let vault = EnvFallbackVault { inner: KeyringVault };
        let key = nh_vault::get_scoped(
            &vault,
            &route.vault_entry,
            cmd_run::host_of(&route.base_url),
            &connect_policy.approved_audiences(&route.vault_entry),
        )?;
        let literal = key.as_str().to_owned();
        Ok((make_client(route, key), literal))
    });
    // No key yet? Chat still starts (§4: EOF or /quit → exit 0): warn once and
    // install a stand-in client that re-surfaces this error only when a task runs.
    // Commands (/model, /provider, /price, /tools, /quit) all work keyless.
    let (client, key_literals, connected) = match connect(&route) {
        Ok((client, literal)) => (client, vec![literal], true),
        Err(e) if e.to_string().starts_with("refused:") => return Err(e),
        Err(e) => {
            eprintln!("warning: {e}");
            (Box::new(NotConnected { msg: e.to_string() }) as Box<dyn ChatClient>, Vec::new(), false)
        }
    };
    let vault_entries = cmd_run::catalog_vault_entries(&resolver);
    let vault = EnvFallbackVault { inner: KeyringVault };
    let registry_scrubber = nh_vault::from_vault(&vault, &vault_entries);
    let scrubber: SharedScrubber = Arc::new(RwLock::new(registry_scrubber.clone()));

    // MCP tools load at chat start when .nosis/mcp.toml exists; a broken file is
    // one warning line and the chat continues without MCP — never a hard failure.
    let mut mcp_warnings = Vec::new();
    let mut tools = builtin_tools();
    tools.extend(load_mcp(&root, &law.policy, &mut mcp_warnings));
    for w in &mcp_warnings {
        eprintln!("warning: {}", scrub_line(&scrubber, w));
    }

    let approve_scrubber = Arc::clone(&scrubber);
    let event_scrubber = Arc::clone(&scrubber);
    let policy = law.policy.clone();
    let law_constitution = law.constitution;
    let agent = AgentLoop {
        client,
        tools,
        ctx: ToolCtx::new(
            cwd,
            Box::new(move |action| {
                cmd_run::approve_on_stdin(&scrub_line(&approve_scrubber, action))
            }),
        )
        .with_guard(Box::new(move |access| match access {
            Access::Read(path) => guard_from(policy.read_verdict(path)),
            Access::Write(path) => guard_from(policy.write_verdict(path)),
            Access::Exec(command) => guard_from(policy.exec_verdict(command)),
            Access::Send(target) => guard_from(policy.send_verdict(target)),
        })),
        receipts: ReceiptWriter {
            path: root.join(".nosis").join("receipts.jsonl"),
            scrubber: registry_scrubber,
        },
        model_id: route.model_id.clone(),
        max_turns: 20,
        thinking: effort_for(None, route.thinking_dialect),
        // Honest identity: name the real route + forbid claiming to be Claude/GPT.
        constitution: Some(nh_tui::identity_constitution(&law_constitution, &route)),
        context_limit: route.context,
        on_event: Some(Box::new(move |line| {
            eprintln!("  {}", scrub_line(&event_scrubber, line));
        })),
    };
    let mut session = ChatSession {
        resolver,
        route,
        agent,
        law_constitution,
        history: Vec::new(),
        session_in: 0,
        session_out: 0,
        session_cached: 0,
        session_cost: Vec::new(),
        key_literals,
        vault_entries,
        scrubber,
        connect,
        connected,
        now: Box::new(Utc::now),
        local_offset: *chrono::Local::now().offset(),
        mcp_warnings,
    };

    eprintln!(
        "chat started on {} — /model <id>, /provider <name>, /price, /tools, /quit",
        session.route.id
    );
    let mut next_line = || {
        let mut buf = String::new();
        match std::io::stdin().read_line(&mut buf) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(buf),
        }
    };
    chat_loop(&mut session, &mut next_line, &mut std::io::stdout(), &mut std::io::stderr())
}

/// Stand-in client for a keyless start: every task fails with the stored
/// (already friendly) vault error; the REPL itself stays fully usable.
struct NotConnected {
    msg: String,
}

impl ChatClient for NotConnected {
    fn complete(&self, _req: &nh_core::wire::ChatRequest) -> anyhow::Result<nh_core::wire::ChatResponse> {
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
    next_line: &mut dyn FnMut() -> Option<String>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> anyhow::Result<()> {
    loop {
        let _ = write!(err, "nh> ");
        let _ = err.flush();
        let Some(raw) = next_line() else { return Ok(()) };
        // Windows shells prefix piped input with a UTF-8 BOM; strip it so
        // `echo "/price" | nh chat` sees the command, not a strange task.
        let line = raw.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
        if line.is_empty() {
            continue;
        }
        match line.strip_prefix('/') {
            Some(cmd) => {
                if handle_command(s, cmd, out, err) == Flow::Quit {
                    return Ok(());
                }
            }
            None => run_task(s, line, out, err),
        }
    }
}

fn handle_command(s: &mut ChatSession, cmd: &str, out: &mut dyn Write, err: &mut dyn Write) -> Flow {
    let mut parts = cmd.split_whitespace();
    let name = parts.next().unwrap_or("");
    let arg = parts.next();
    match (name, arg) {
        ("quit", _) => return Flow::Quit,
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
        ("price", _) => print_price(s, out),
        ("tools", _) => print_tools(s, out, err),
        _ => print_err(
            s,
            err,
            "unknown command — try /model <id>, /provider <name>, /price, /tools, /quit",
        ),
    }
    Flow::Continue
}

/// Run one task through the shared session history; answer to stdout, footer to
/// stderr. A failed call is one friendly line and the session keeps going.
fn run_task(s: &mut ChatSession, task: &str, out: &mut dyn Write, err: &mut dyn Write) {
    // Keyless start? Retry the real connection now — a key added mid-session
    // (`nh key add <provider>` in another terminal) works without restarting.
    if !s.connected {
        match (s.connect)(&s.route) {
            Ok((client, literal)) => install_client(s, client, literal),
            Err(e) => {
                print_err(s, err, &e.to_string());
                return;
            }
        }
    }
    match s.agent.run_with_history(&mut s.history, task) {
        Ok((answer, receipt)) => {
            let _ = writeln!(out, "{}", scrub_text(&s.scrubber, &answer));
            if let Some(u) = &receipt.usage {
                s.session_in += u.prompt_tokens;
                s.session_out += u.completion_tokens;
                s.session_cached += u.cached_tokens.unwrap_or(0);
                let at = (s.now)();
                add_session_cost(s, u, at);
                if let Some(line) = cmd_run::turn_cost_line(&s.resolver, &s.route, u, at) {
                    let _ = writeln!(err, "{}", scrub_line(&s.scrubber, &line));
                }
            }
            let _ = writeln!(err, "{}", scrub_line(&s.scrubber, &footer(s)));
        }
        Err(e) => print_err(s, err, &e.to_string()),
    }
}

/// Swap in a live client. Its key literal joins the Scrubber on every output
/// path (stdout, stderr closures, receipts) before the client ever runs.
fn install_client(s: &mut ChatSession, client: Box<dyn ChatClient>, literal: String) {
    if !literal.is_empty() && !s.key_literals.contains(&literal) {
        s.key_literals.push(literal);
    }
    let registry = if s.vault_entries.is_empty() {
        Scrubber::new(s.key_literals.clone())
    } else {
        let vault = EnvFallbackVault { inner: KeyringVault };
        nh_vault::from_vault(&vault, &s.vault_entries)
    };
    match s.scrubber.write() {
        Ok(mut guard) => *guard = registry.clone(),
        Err(poisoned) => *poisoned.into_inner() = registry.clone(),
    }
    s.agent.receipts.scrubber = registry;
    s.agent.client = client;
    s.connected = true;
}

/// Switch the live route, keeping history and session usage.
fn switch_to(s: &mut ChatSession, route: ResolvedRoute, out: &mut dyn Write, err: &mut dyn Write) {
    if route.class == RouteClass::Delegate {
        print_err(s, err, DELEGATE_MSG);
        return;
    }
    match (s.connect)(&route) {
        Ok((client, literal)) => {
            install_client(s, client, literal);
            s.agent.model_id = route.model_id.clone();
            s.agent.thinking = effort_for(None, route.thinking_dialect);
            // Refresh the identity prompt for the NEW route — both the agent's stored
            // constitution and the live system message already in history (which
            // run_with_history seeds only once, on the first turn).
            let constitution = nh_tui::identity_constitution(&s.law_constitution, &route);
            if let Some(system) = s.history.first_mut() {
                if system.role == "system" {
                    system.content = Some(constitution.clone());
                }
            }
            s.agent.constitution = Some(constitution);
            s.agent.context_limit = route.context;
            s.route = route;
            let _ = writeln!(out, "switched to {}", s.route.id);
        }
        // Unknown key, delegate, whatever — keep the current route, say why.
        Err(e) => print_err(s, err, &e.to_string()),
    }
}

/// `/price` — the cost HUD quote for this instant, one aligned line.
fn print_price(s: &ChatSession, out: &mut dyn Write) {
    let now = (s.now)();
    let Some(quote) = s.route.price_at(now) else {
        let line = format!(
            "no price data for {id} — add a [routes.{id}.price] table to catalog.toml",
            id = s.route.id
        );
        let _ = writeln!(out, "{}", scrub_line(&s.scrubber, &line));
        return;
    };
    let line = format!(
        "{} | {} | in {:.4} hit / {:.4} miss | out {:.4} | {}/M tokens | confidence {} | session {}",
        s.route.id,
        s.route.peak_status(now, s.local_offset),
        quote.cache_hit,
        quote.cache_miss,
        quote.output,
        quote.currency,
        quote.confidence,
        session_money(s, now)
    );
    let _ = writeln!(out, "{}", scrub_line(&s.scrubber, &line));
    if quote.stale {
        // Honest-cost rule: stale data is flagged, never silently trusted.
        let warning = "warning: price data past valid_until — verify before trusting these numbers";
        let _ = writeln!(out, "{}", scrub_line(&s.scrubber, warning));
    }
}

/// `/tools` — builtin tools first, then MCP tools, one line each; MCP warnings
/// go to stderr after the list.
fn print_tools(s: &ChatSession, out: &mut dyn Write, err: &mut dyn Write) {
    for tool in &s.agent.tools {
        let spec = tool.spec();
        let first = spec.description.lines().next().unwrap_or("");
        let _ = writeln!(out, "{}", scrub_line(&s.scrubber, &format!("{} — {first}", spec.name)));
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
        "{} | {} | session {} | tokens {} in / {} out / {} cached",
        s.route.id,
        s.route.peak_status(now, s.local_offset),
        session_money(s, now),
        s.session_in,
        s.session_out,
        s.session_cached
    );
    if let Some(pct) = cache_hit_pct(s.session_in, s.session_cached) {
        line.push_str(&format!(" | cache {pct:.0}%"));
    }
    line
}

fn add_session_cost(s: &mut ChatSession, usage: &nh_core::wire::Usage, at: DateTime<Utc>) {
    let Some(quote) = s.route.price_at(at) else {
        return;
    };
    let amount = cost_of(
        &quote,
        usage.prompt_tokens,
        usage.cached_tokens.unwrap_or(0),
        usage.completion_tokens,
    );
    let uncertain = quote.stale || quote.confidence == PriceConfidence::VerifyLive;
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

fn session_money(s: &ChatSession, at: DateTime<Utc>) -> String {
    if s.session_cost.is_empty() {
        return s.route.price_at(at).map_or_else(
            || "—".into(),
            |quote| {
                let mut display = money_with_gloss(0.0, quote.currency, s.resolver.fx(), at);
                if quote.stale || quote.confidence == PriceConfidence::VerifyLive {
                    display.push('*');
                }
                display
            },
        );
    }
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
}

/// Load MCP tools from `.nosis/mcp.toml` when it exists. Any failure — unreadable
/// file, bad TOML, unreachable server — becomes a warning line, never an error.
fn load_mcp(
    root: &Path,
    policy: &nh_law::Policy,
    warnings: &mut Vec<String>,
) -> Vec<Box<dyn Tool>> {
    let path = root.join(".nosis").join("mcp.toml");
    if !path.is_file() {
        return Vec::new();
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            warnings.push(format!("could not read .nosis/mcp.toml ({e}) — continuing without MCP"));
            return Vec::new();
        }
    };
    match nh_tools::mcp::load_mcp_config(&text) {
        Ok(configs) => {
            let configs = cmd_run::filter_mcp_audiences(configs, policy, warnings);
            let set = nh_tools::mcp::mcp_tools(&configs);
            warnings.extend(set.warnings);
            set.tools
        }
        Err(e) => {
            warnings.push(format!(".nosis/mcp.toml: {e} — continuing without MCP"));
            Vec::new()
        }
    }
}

/// Scrub + control-char-escape one display line via the shared Scrubber.
fn scrub_line(scrubber: &SharedScrubber, text: &str) -> String {
    match scrubber.read() {
        Ok(guard) => cmd_run::safe_line(&guard, text),
        Err(poisoned) => cmd_run::safe_line(&poisoned.into_inner(), text),
    }
}

/// Scrub only (answers may keep their newlines).
fn scrub_text(scrubber: &SharedScrubber, text: &str) -> String {
    match scrubber.read() {
        Ok(guard) => guard.scrub(text),
        Err(poisoned) => poisoned.into_inner().scrub(text),
    }
}

fn print_err(s: &ChatSession, err: &mut dyn Write, msg: &str) {
    let _ = writeln!(err, "{}", scrub_line(&s.scrubber, msg));
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use nh_core::wire::{ChatRequest, ChatResponse, Usage};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Self-contained catalog: a peak-priced deepseek route, kimi, two free glm
    /// routes (alphabetical tie-break), a delegate route, and an unpriced route.
    const TEST_CATALOG: &str = r#"
        [fx]
        usd_per_cny = 0.139
        valid_until = "2026-07-24"
        price_confidence = "reported"

        [routes.deepseek-v4-flash]
        provider = "deepseek"
        model_id = "deepseek-v4-flash"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "deepseek"
        thinking_dialect = "deepseek-nhm"
        context = 1000

        [routes.deepseek-v4-flash.price]
        currency = "CNY"
        unit = "per_million_tokens"
        cache_hit = 0.02
        cache_miss = 1.00
        output = 2.00
        price_confidence = "confirmed"
        valid_until = "2026-07-24"

        [routes.deepseek-v4-flash.price.peak]
        multiplier = 2.0
        timezone = "Asia/Shanghai"
        windows = ["09:00-12:00", "14:00-18:00"]

        [routes."kimi-k2.6"]
        provider = "kimi"
        model_id = "kimi-k2.6"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "kimi"
        context = 2000

        [routes."kimi-k2.6".price]
        currency = "USD"
        unit = "per_million_tokens"
        cache_hit = 0.60
        cache_miss = 0.60
        output = 2.65
        price_confidence = "verify_live"

        [routes."glm-4.5-flash"]
        provider = "glm"
        model_id = "glm-4.5-flash"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "glm"

        [routes."glm-4.5-flash".price]
        currency = "USD"
        unit = "per_million_tokens"
        cache_hit = 0.0
        cache_miss = 0.0
        output = 0.0
        price_confidence = "reported"

        [routes."glm-4.7-flash"]
        provider = "glm"
        model_id = "glm-4.7-flash"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "glm"

        [routes."glm-4.7-flash".price]
        currency = "USD"
        unit = "per_million_tokens"
        cache_hit = 0.0
        cache_miss = 0.0
        output = 0.0
        price_confidence = "reported"

        [routes.opus-delegate]
        provider = "anthropic"
        model_id = "opus-delegate"
        base_url = ""
        wire = "openai"
        vault_entry = "anthropic"
        class = "delegate"

        [routes.unpriced]
        provider = "kimi"
        model_id = "unpriced"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "kimi"
    "#;

    /// ChatMessage literals live only in nh-core (CONTRACTS_M1.md §5.2) — build via serde.
    fn assistant_msg(text: &str) -> ChatMessage {
        serde_json::from_value(serde_json::json!({ "role": "assistant", "content": text }))
            .expect("valid assistant message")
    }

    struct MockClient {
        reply: String,
        calls: Arc<AtomicUsize>,
    }

    impl ChatClient for MockClient {
        fn complete(&self, _req: &ChatRequest) -> anyhow::Result<ChatResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ChatResponse {
                message: assistant_msg(&self.reply),
                finish_reason: "stop".into(),
                usage: Some(Usage {
                    prompt_tokens: 12,
                    completion_tokens: 7,
                    cached_tokens: Some(4),
                }),
            })
        }
    }

    /// Fixed instant: 2026-07-15 Beijing 08:00 (00:00 UTC) — off-peak, not stale.
    fn off_peak_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap()
    }

    fn beijing_offset() -> FixedOffset {
        FixedOffset::east_opt(8 * 3600).unwrap()
    }

    fn test_session(model: &str, tmp: &Path) -> (ChatSession, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let connect_calls = Arc::clone(&calls);
        let connect: ConnectFn = Box::new(move |route| {
            Ok((
                Box::new(MockClient { reply: "ok".into(), calls: Arc::clone(&connect_calls) })
                    as Box<dyn ChatClient>,
                format!("fake-key-{}", route.vault_entry),
            ))
        });
        let resolver = RouteResolver::from_toml(TEST_CATALOG).expect("test catalog parses");
        let route = resolver.resolve(model).expect("known test route");
        let (client, literal) = connect(&route).unwrap();
        let key_literals = vec![literal];
        let agent = AgentLoop {
            client,
            tools: builtin_tools(),
            ctx: ToolCtx::new(tmp.to_path_buf(), Box::new(|_| false)),
            receipts: ReceiptWriter {
                path: tmp.join("receipts.jsonl"),
                scrubber: Scrubber::new(key_literals.clone()),
            },
            model_id: route.model_id.clone(),
            max_turns: 20,
            thinking: effort_for(None, route.thinking_dialect),
            constitution: Some("test constitution\n".into()),
            context_limit: route.context,
            on_event: None,
        };
        let session = ChatSession {
            resolver,
            route,
            agent,
            law_constitution: "test constitution\n".into(),
            history: Vec::new(),
            session_in: 0,
            session_out: 0,
            session_cached: 0,
            session_cost: Vec::new(),
            scrubber: Arc::new(RwLock::new(Scrubber::new(key_literals.clone()))),
            key_literals,
            vault_entries: Vec::new(),
            connect,
            connected: true,
            now: Box::new(off_peak_now),
            local_offset: beijing_offset(),
            mcp_warnings: Vec::new(),
        };
        (session, calls)
    }

    /// Feed scripted lines, capture (stdout, stderr).
    fn drive(s: &mut ChatSession, script: &[&str]) -> (String, String) {
        let mut lines: VecDeque<String> = script.iter().map(|l| format!("{l}\n")).collect();
        let mut next = move || lines.pop_front();
        let mut out = Vec::new();
        let mut err = Vec::new();
        chat_loop(s, &mut next, &mut out, &mut err).expect("chat loop never errors");
        (String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    // ------------------------------------------------------------- switching

    #[test]
    fn model_switch_preserves_history_and_changes_route() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, calls) = test_session("deepseek-v4-flash", tmp.path());
        let (out, _err) = drive(&mut s, &["write a haiku", "/model kimi-k2.6", "another one"]);

        assert!(out.contains("switched to kimi-k2.6"), "got: {out}");
        assert_eq!(calls.load(Ordering::SeqCst), 2, "one wire call per task");
        // History survived the switch: system + (user, assistant) x 2.
        assert_eq!(s.history.len(), 5, "history: {:#?}", s.history);
        assert_eq!(s.history[0].role, "system");
        assert_eq!(s.history[1].content.as_deref(), Some("write a haiku"));
        assert_eq!(s.history[3].content.as_deref(), Some("another one"));
        // Active route changed.
        assert_eq!(s.route.id, "kimi-k2.6");
        assert_eq!(s.agent.model_id, "kimi-k2.6");
        assert_eq!(s.agent.context_limit, Some(2000));
        // Identity prompt refreshes to the new route, still appends the law text, and
        // the live system message in history is rewritten to match.
        let constitution = s.agent.constitution.clone().unwrap();
        assert!(
            constitution.contains("nosis on kimi-k2.6")
                && constitution.contains("never claim to be Claude"),
            "identity prompt for new route: {constitution}"
        );
        assert!(
            constitution.ends_with("test constitution\n"),
            "law text preserved: {constitution}"
        );
        assert_eq!(s.history[0].content.as_ref(), Some(&constitution));
    }

    #[test]
    fn provider_switch_resolves_the_provider_default() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        let (out, _err) = drive(&mut s, &["/provider glm"]);
        // Cheapest api route by output price; free glm routes tie -> alphabetical.
        assert!(out.contains("switched to glm-4.5-flash"), "got: {out}");
        assert_eq!(s.route.id, "glm-4.5-flash");
    }

    #[test]
    fn unknown_model_prints_resolver_error_and_keeps_route() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        let (out, err) = drive(&mut s, &["/model no-such-model"]);
        assert!(err.contains("unknown model id 'no-such-model'"), "got: {err}");
        assert!(err.contains("available:"), "must list options: {err}");
        assert!(!out.contains("switched"), "got: {out}");
        assert_eq!(s.route.id, "deepseek-v4-flash");
    }

    #[test]
    fn unknown_provider_prints_resolver_error_and_keeps_route() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        let (_out, err) = drive(&mut s, &["/provider acme"]);
        assert!(err.contains("unknown provider 'acme'"), "got: {err}");
        assert_eq!(s.route.id, "deepseek-v4-flash");
    }

    #[test]
    fn missing_key_on_switch_keeps_current_route() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        s.connect = Box::new(|route| {
            anyhow::bail!("no key found for \"{}\" — run `nh key add {}`", route.vault_entry, route.vault_entry)
        });
        let (out, err) = drive(&mut s, &["/model kimi-k2.6"]);
        assert!(err.contains("nh key add kimi"), "error says what to do next: {err}");
        assert!(!out.contains("switched"), "got: {out}");
        assert_eq!(s.route.id, "deepseek-v4-flash");
        assert_eq!(s.agent.model_id, "deepseek-v4-flash");
    }

    #[test]
    fn delegate_route_switch_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        let (_out, err) = drive(&mut s, &["/model opus-delegate"]);
        assert!(err.contains(DELEGATE_MSG), "got: {err}");
        assert_eq!(s.route.id, "deepseek-v4-flash");
    }

    // ------------------------------------------------------------- /price

    #[test]
    fn price_off_peak_line_is_scannable() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        let (out, _err) = drive(&mut s, &["/price"]);
        assert_eq!(
            out,
            "deepseek-v4-flash | off-peak | in 0.0200 hit / 1.0000 miss | out 2.0000 | CNY/M tokens | confidence confirmed | session ¥0.00 (≈$0.00)\n"
        );
    }

    #[test]
    fn price_peak_doubles_rates_and_shows_boundary() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        // Beijing 10:30 — inside the 09:00-12:00 window; local offset = Beijing.
        s.now = Box::new(|| Utc.with_ymd_and_hms(2026, 7, 15, 2, 30, 0).unwrap());
        let (out, _err) = drive(&mut s, &["/price"]);
        assert_eq!(
            out,
            "deepseek-v4-flash | peak 2x until 12:00 | in 0.0400 hit / 2.0000 miss | out 4.0000 | CNY/M tokens | confidence confirmed | session ¥0.00 (≈$0.00)\n"
        );
    }

    #[test]
    fn peak_boundary_is_shown_in_the_users_local_time() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        // Beijing 10:30 peak, but the user sits at UTC-6 (plan A.1: La Ceiba):
        // window end 12:00 Beijing = 04:00 UTC = 22:00 local.
        s.now = Box::new(|| Utc.with_ymd_and_hms(2026, 7, 15, 2, 30, 0).unwrap());
        s.local_offset = FixedOffset::west_opt(6 * 3600).unwrap();
        let (out, _err) = drive(&mut s, &["/price"]);
        assert!(out.contains("peak 2x until 22:00"), "got: {out}");
    }

    #[test]
    fn price_after_valid_until_adds_stale_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        s.now = Box::new(|| Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap());
        let (out, _err) = drive(&mut s, &["/price"]);
        assert!(out.contains("off-peak"), "got: {out}");
        assert!(
            out.contains("warning: price data past valid_until — verify before trusting these numbers"),
            "honest-cost rule: {out}"
        );
    }

    #[test]
    fn price_without_table_says_how_to_add_one() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("unpriced", tmp.path());
        let (out, _err) = drive(&mut s, &["/price"]);
        assert_eq!(
            out,
            "no price data for unpriced — add a [routes.unpriced.price] table to catalog.toml\n"
        );
    }

    // ------------------------------------------------------------- footer

    #[test]
    fn footer_after_each_answer_has_route_peak_and_session_tokens() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        let (out, err) = drive(&mut s, &["hello"]);
        assert!(out.contains("ok"), "answer on stdout: {out}");
        assert!(
            err.contains(
                "deepseek-v4-flash | off-peak | session <¥0.0001 (≈<$0.0001) | tokens 12 in / 7 out / 4 cached | cache 33%"
            ),
            "footer on stderr: {err}"
        );
        assert!(
            err.contains("cost <¥0.0001 (≈<$0.0001) — saved 15% vs no-cache"),
            "turn cost on stderr: {err}"
        );
    }

    #[test]
    fn session_usage_accumulates_across_turns_and_switches() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        let (_out, err) = drive(&mut s, &["one", "/model kimi-k2.6", "two"]);
        assert!(
            err.contains(
                "kimi-k2.6 | off-peak | session <¥0.0001 · <$0.0001* | tokens 24 in / 14 out / 8 cached | cache 33%"
            ),
            "cumulative after switch: {err}"
        );
        assert_eq!(s.session_cost.len(), 2, "native currencies stay separate");
    }

    #[test]
    fn footer_without_price_table_says_no_price_data() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("unpriced", tmp.path());
        let (_out, err) = drive(&mut s, &["hello"]);
        assert!(
            err.contains(
                "unpriced | no price data | session — | tokens 12 in / 7 out / 4 cached | cache 33%"
            ),
            "got: {err}"
        );
    }

    #[test]
    fn footer_omits_cache_chip_before_any_usage() {
        let tmp = tempfile::tempdir().unwrap();
        let (s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        let line = footer(&s);
        assert!(line.contains("session ¥0.00 (≈$0.00) | tokens 0 in / 0 out / 0 cached"));
        assert!(!line.contains("| cache"), "got: {line}");
    }

    // ------------------------------------------------------------- loop basics

    #[test]
    fn quit_exits_without_running_later_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, calls) = test_session("deepseek-v4-flash", tmp.path());
        let (_out, err) = drive(&mut s, &["/quit", "never runs"]);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(err.matches("nh> ").count(), 1, "no prompt after quit: {err}");
    }

    #[test]
    fn eof_exits_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, calls) = test_session("deepseek-v4-flash", tmp.path());
        let (out, err) = drive(&mut s, &[]);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(out.is_empty(), "stdout stays clean: {out}");
        assert_eq!(err, "nh> ");
    }

    #[test]
    fn blank_lines_reprompt_without_calling_the_model() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, calls) = test_session("deepseek-v4-flash", tmp.path());
        let (_out, err) = drive(&mut s, &["", "   ", "/quit"]);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(err.matches("nh> ").count(), 3, "got: {err}");
    }

    #[test]
    fn piped_input_with_bom_still_reads_commands() {
        // Windows PowerShell pipes prefix the stream with U+FEFF.
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, calls) = test_session("deepseek-v4-flash", tmp.path());
        let (out, _err) = drive(&mut s, &["\u{feff}/price", "/quit"]);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "command, not a task");
        assert!(out.contains("off-peak"), "got: {out}");
    }

    #[test]
    fn unknown_command_prints_one_line_help() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        let (out, err) = drive(&mut s, &["/frobnicate"]);
        assert!(out.is_empty(), "help goes to stderr: {out}");
        assert!(
            err.contains("unknown command — try /model <id>, /provider <name>, /price, /tools, /quit"),
            "got: {err}"
        );
    }

    #[test]
    fn model_without_arg_prints_usage() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        let (_out, err) = drive(&mut s, &["/model", "/provider"]);
        assert!(err.contains("usage: /model <id>"), "got: {err}");
        assert!(err.contains("usage: /provider <name>"), "got: {err}");
    }

    #[test]
    fn tools_lists_builtins_one_per_line() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        let (out, _err) = drive(&mut s, &["/tools"]);
        for name in ["read_file — ", "edit_file — ", "exec_shell — "] {
            assert!(out.contains(name), "missing {name}: {out}");
        }
        assert_eq!(out.lines().count(), 3, "one line per tool: {out}");
    }

    /// Puts a session into the keyless-start state: stand-in client installed,
    /// not connected, and every reconnect attempt failing with the vault error.
    fn make_keyless(s: &mut ChatSession) {
        s.agent.client = Box::new(NotConnected {
            msg: "no key found for \"deepseek\" — run `nh key add deepseek`".into(),
        });
        s.connected = false;
        s.connect = Box::new(|route| {
            anyhow::bail!("no key found for \"{}\" — run `nh key add {}`", route.vault_entry, route.vault_entry)
        });
    }

    #[test]
    fn keyless_session_runs_commands_and_task_says_how_to_add_the_key() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
        make_keyless(&mut s);
        let (out, err) = drive(&mut s, &["/price", "hello", "/quit"]);
        assert!(out.contains("off-peak"), "/price works keyless: {out}");
        assert!(!out.contains("hello"), "no answer on stdout: {out}");
        assert!(err.contains("nh key add deepseek"), "task error says what to do: {err}");
    }

    #[test]
    fn keyless_session_reconnects_once_the_key_arrives() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, calls) = test_session("deepseek-v4-flash", tmp.path());
        // Keyless start, but the key arrives before the next task (the user ran
        // `nh key add deepseek` in another terminal): test_session's connect
        // succeeds, so the retry must swap in the real client and answer.
        s.agent.client = Box::new(NotConnected {
            msg: "no key found for \"deepseek\" — run `nh key add deepseek`".into(),
        });
        s.connected = false;
        let (out, err) = drive(&mut s, &["hello", "again"]);
        assert_eq!(calls.load(Ordering::SeqCst), 2, "real client answered both tasks");
        assert!(out.contains("ok"), "answer on stdout: {out}");
        assert!(!err.contains("no key found"), "stale error must not resurface: {err}");
        assert!(s.connected, "session marked connected after the retry");
        // The reconnect registered the new key on the scrub path.
        assert!(s.key_literals.contains(&"fake-key-deepseek".to_string()), "got: {:?}", s.key_literals);
    }

    // ------------------------------------------------------------- scrubbing

    #[test]
    fn both_session_keys_are_scrubbed_after_a_switch() {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, calls) = test_session("deepseek-v4-flash", tmp.path());
        let (_out, _err) = drive(&mut s, &["/provider glm"]);
        // Now make the active client leak both keys and run a task.
        s.agent.client = Box::new(MockClient {
            reply: "leak fake-key-deepseek and fake-key-glm end".into(),
            calls,
        });
        let (out, _err) = drive(&mut s, &["task"]);
        assert!(!out.contains("fake-key-deepseek"), "old key leaked: {out}");
        assert!(!out.contains("fake-key-glm"), "new key leaked: {out}");
        assert_eq!(out.matches("[REDACTED]").count(), 2, "got: {out}");
    }

    // ------------------------------------------------------------- peak status

    #[test]
    fn peak_status_second_window_and_multiplier_trim() {
        let resolver = RouteResolver::from_toml(TEST_CATALOG).unwrap();
        let route = resolver.resolve("deepseek-v4-flash").unwrap();
        // Beijing 15:00 — inside 14:00-18:00.
        let now = Utc.with_ymd_and_hms(2026, 7, 15, 7, 0, 0).unwrap();
        assert_eq!(route.peak_status(now, beijing_offset()), "peak 2x until 18:00");
        // Boundary math: 18:00 itself is off-peak (end exclusive).
        let end = Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
        assert_eq!(route.peak_status(end, beijing_offset()), "off-peak");
        let fractional = RouteResolver::from_toml(&TEST_CATALOG.replace(
            "multiplier = 2.0",
            "multiplier = 1.5",
        ))
        .unwrap()
        .resolve("deepseek-v4-flash")
        .unwrap();
        assert_eq!(
            fractional.peak_status(now, beijing_offset()),
            "peak 1.5x until 18:00"
        );
    }

    // ------------------------------------------------------------- mcp loading

    #[test]
    fn missing_mcp_toml_means_no_tools_and_no_warnings() {
        let tmp = tempfile::tempdir().unwrap();
        let mut warnings = Vec::new();
        let law = nh_law::load(tmp.path(), &LoadOptions { cli_autonomy: None });
        let tools = load_mcp(tmp.path(), &law.policy, &mut warnings);
        assert!(tools.is_empty());
        assert!(warnings.is_empty(), "got: {warnings:?}");
    }

    #[test]
    fn broken_mcp_toml_is_one_warning_and_chat_continues() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".nosis")).unwrap();
        std::fs::write(tmp.path().join(".nosis").join("mcp.toml"), "not [ valid").unwrap();
        let mut warnings = Vec::new();
        let law = nh_law::load(tmp.path(), &LoadOptions { cli_autonomy: None });
        let tools = load_mcp(tmp.path(), &law.policy, &mut warnings);
        assert!(tools.is_empty());
        assert_eq!(warnings.len(), 1, "exactly one warning: {warnings:?}");
        assert!(warnings[0].contains("mcp.toml"), "names the file: {warnings:?}");
        assert!(warnings[0].contains("continuing without MCP"), "got: {warnings:?}");
    }
}
