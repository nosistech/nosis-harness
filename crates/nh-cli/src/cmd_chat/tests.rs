use super::*;
use chrono::TimeZone;
use nh_core::receipt::ReceiptWriter;
use nh_core::wire::{ChatRequest, ChatResponse, ThinkingEffort, Usage};
use nh_law::LoadOptions;
use nh_routes::{ThinkingDialect, ThinkingPosture, Wire};
use nh_tools::{builtin_tools, ToolCtx};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Self-contained catalog: a peak-priced deepseek route, kimi, two free glm
/// routes (alphabetical tie-break), a delegate route, and an unpriced route.
/// Its 2026-07-24 dates are deliberate: injected clocks exercise both sides
/// of the freshness boundary.
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
    max_out = 64000

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
    modality = ["text", "image"]
    context = 2000
    max_out = 64000

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
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "anthropic"
    class = "delegate"

    [routes.unpriced]
    provider = "kimi"
    model_id = "unpriced"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "kimi"

    [routes.local-test]
    provider = "ollama"
    model_id = "user-filled-model"
    base_url = "http://127.0.0.1:11434/v1"
    wire = "openai"
    vault_entry = "ollama-local"
    class = "local"
    context = 8192
    max_out = 2048
    [routes.local-test.price]
    currency = "USD"
    unit = "per_million_tokens"
    cache_hit = 0.0
    cache_miss = 0.0
    output = 0.0
    price_confidence = "confirmed"
"#;

/// ChatMessage literals live only in nh-core (CONTRACTS_M1.md §5.2) - build via serde.
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

/// Fixed instant: 2026-07-15 Beijing 08:00 (00:00 UTC) - off-peak, not stale.
fn off_peak_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap()
}

fn beijing_offset() -> FixedOffset {
    FixedOffset::east_opt(8 * 3600).unwrap()
}

fn test_session(model: &str, tmp: &Path) -> (ChatSession, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let connect_calls = Arc::clone(&calls);
    let connect: ConnectFn = Box::new(move |route, _| {
        Ok((
            Box::new(MockClient {
                reply: "ok".into(),
                calls: Arc::clone(&connect_calls),
            }) as Box<dyn ChatClient>,
            nh_vault::secret(format!("fake-key-{}", route.vault_entry())),
        ))
    });
    let resolver = RouteResolver::from_toml(TEST_CATALOG).expect("test catalog parses");
    let route = resolver.resolve(model).expect("known test route");
    let profiles = Profiles::bundled();
    let execution_policy = profiles.effective("balanced", &route);
    let law_constitution = "test constitution\n";
    let constitution = cmd_run::agent_constitution(law_constitution, &route);
    let (client, literal) = connect(&route, execution_policy.output_cap).unwrap();
    let mut key_literals = SecretRegistry::new();
    key_literals.insert(literal);
    let test_scrubber = key_literals.scrubber();
    let agent = AgentLoop {
        client,
        tools: builtin_tools(),
        ctx: ToolCtx::new(tmp.to_path_buf(), Box::new(|_| false)),
        receipts: ReceiptWriter::for_path(tmp, tmp.join("receipts.jsonl"), test_scrubber.clone()),
        model_id: route.model_id().to_owned(),
        max_turns: 20,
        thinking: effort_for(
            None,
            execution_policy.posture,
            route.thinking_dialect(),
            route.wire(),
        ),
        profile: Some(execution_policy.profile.clone()),
        constitution: Some(constitution),
        context_limit: route.context(),
        on_event: None,
    };
    let session = ChatSession {
        resolver,
        route,
        profiles,
        active_profile: execution_policy.profile,
        agent,
        law_constitution: law_constitution.into(),
        history: Vec::new(),
        session_in: 0,
        session_out: 0,
        session_cached: Some(0),
        session_cost: Vec::new(),
        unpriced_turns: 0,
        scrubber: Arc::new(RwLock::new(test_scrubber)),
        key_literals,
        connect,
        connected: true,
        now: Box::new(off_peak_now),
        local_offset: beijing_offset(),
        mcp_warnings: Vec::new(),
        pending_images: Vec::new(),
    };
    (session, calls)
}

#[test]
fn chat_constitution_contains_the_authoritative_tool_result_rule() {
    let tmp = tempfile::tempdir().unwrap();
    let (session, _) = test_session("deepseek-v4-flash", tmp.path());
    let constitution = session.agent.constitution.as_deref().unwrap_or("");

    assert!(
        constitution.contains(nh_core::agent::TOOL_RESULT_STATE_RULE),
        "got: {constitution}"
    );
}

#[test]
fn chat_effort_is_wire_aware() {
    assert_eq!(
        effort_for(
            Some(crate::cmd_run::ThinkArg::High),
            ThinkingPosture::Ceiling,
            ThinkingDialect::DeepseekNhm,
            Wire::AnthropicMessages,
        ),
        ThinkingEffort::None
    );
    assert_eq!(
        effort_for(
            Some(crate::cmd_run::ThinkArg::High),
            ThinkingPosture::Ceiling,
            ThinkingDialect::DeepseekNhm,
            Wire::OpenAi,
        ),
        ThinkingEffort::High
    );
}

/// Feed scripted lines, capture (stdout, stderr).
fn drive(s: &mut ChatSession, script: &[&str]) -> (String, String) {
    let mut lines: VecDeque<String> = script.iter().map(|l| format!("{l}\n")).collect();
    let mut next = move || Ok(lines.pop_front().map(ChatInput::Line));
    let mut out = Vec::new();
    let mut err = Vec::new();
    chat_loop(s, &mut next, &mut out, &mut err).expect("chat loop never errors");
    (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn capped_reader_drains_an_oversized_line_before_the_next_command() {
    let mut input = vec![b'x'; MAX_TASK_BYTES + 1];
    input.extend_from_slice(b"\n/quit\n");
    let mut reader = std::io::Cursor::new(input);

    assert_eq!(
        read_chat_input(&mut reader).unwrap(),
        Some(ChatInput::TooLong)
    );
    assert_eq!(
        read_chat_input(&mut reader).unwrap(),
        Some(ChatInput::Line("/quit".into()))
    );
    assert_eq!(read_chat_input(&mut reader).unwrap(), None);
}

// ------------------------------------------------------------- switching

#[test]
fn model_switch_preserves_history_and_changes_route() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, calls) = test_session("deepseek-v4-flash", tmp.path());
    let (out, _err) = drive(
        &mut s,
        &["write a haiku", "/model kimi-k2.6", "another one"],
    );

    assert!(out.contains("switched to kimi-k2.6"), "got: {out}");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "one wire call per task");
    // History survived the switch: system + (user, assistant) x 2.
    assert_eq!(s.history.len(), 5, "history: {:#?}", s.history);
    assert_eq!(s.history[0].role, "system");
    assert_eq!(s.history[1].content.as_deref(), Some("write a haiku"));
    assert_eq!(s.history[3].content.as_deref(), Some("another one"));
    // Active route changed.
    assert_eq!(s.route.id(), "kimi-k2.6");
    assert_eq!(s.agent.model_id, "kimi-k2.6");
    assert_eq!(s.agent.context_limit, Some(2000));
    // Identity prompt refreshes to the new route, still appends the law text, and
    // the live system message in history is rewritten to match.
    let constitution = s.agent.constitution.clone().unwrap();
    assert!(
        constitution.contains("nosis on kimi-k2.6")
            && constitution.contains("never claim to be Claude")
            && constitution.contains(nh_core::agent::TOOL_RESULT_STATE_RULE),
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
    assert_eq!(s.route.id(), "glm-4.5-flash");
}

#[test]
fn route_switch_reapplies_active_profile_clamp() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, calls) = test_session("deepseek-v4-flash", tmp.path());
    s.active_profile = "frugal".into();
    s.agent.profile = Some("frugal".into());
    let seen_caps = Arc::new(std::sync::Mutex::new(Vec::new()));
    let caps_for_connect = Arc::clone(&seen_caps);
    s.connect = Box::new(move |route, output_cap| {
        caps_for_connect.lock().unwrap().push(output_cap);
        Ok((
            Box::new(MockClient {
                reply: "ok".into(),
                calls: Arc::clone(&calls),
            }),
            nh_vault::secret(format!("fake-key-{}", route.vault_entry())),
        ))
    });

    let (out, err) = drive(&mut s, &["/model kimi-k2.6"]);

    assert!(!err.contains("unknown"), "got: {err}");
    assert!(out.contains("switched to kimi-k2.6"), "got: {out}");
    assert_eq!(*seen_caps.lock().unwrap(), vec![Some(16_384)]);
    assert_eq!(s.agent.profile.as_deref(), Some("frugal"));
}

#[test]
fn unknown_model_prints_resolver_error_and_keeps_route() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
    let (out, err) = drive(&mut s, &["/model no-such-model"]);
    assert!(
        err.contains("unknown model id 'no-such-model'"),
        "got: {err}"
    );
    assert!(err.contains("available:"), "must list options: {err}");
    assert!(!out.contains("switched"), "got: {out}");
    assert_eq!(s.route.id(), "deepseek-v4-flash");
}

#[test]
fn unknown_provider_prints_resolver_error_and_keeps_route() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
    let (_out, err) = drive(&mut s, &["/provider acme"]);
    assert!(err.contains("unknown provider 'acme'"), "got: {err}");
    assert_eq!(s.route.id(), "deepseek-v4-flash");
}

#[test]
fn missing_key_on_switch_keeps_current_route() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
    s.connect = Box::new(|route, _| {
        anyhow::bail!(
            "no key found for \"{}\" - run `nh key add {}`",
            route.vault_entry(),
            route.vault_entry()
        )
    });
    let (out, err) = drive(&mut s, &["/model kimi-k2.6"]);
    assert!(
        err.contains("nh key add kimi"),
        "error says what to do next: {err}"
    );
    assert!(!out.contains("switched"), "got: {out}");
    assert_eq!(s.route.id(), "deepseek-v4-flash");
    assert_eq!(s.agent.model_id, "deepseek-v4-flash");
}

#[test]
fn delegate_route_switch_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
    let (_out, err) = drive(&mut s, &["/model opus-delegate"]);
    assert!(err.contains(DELEGATE_MSG), "got: {err}");
    assert_eq!(s.route.id(), "deepseek-v4-flash");
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
    // Beijing 10:30 - inside the 09:00-12:00 window; local offset = Beijing.
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
        out.contains(
            "warning: price freshness missing or expired - verify before trusting these numbers"
        ),
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
        "no price data for unpriced - add a [routes.unpriced.price] table to catalog.toml\n"
    );
}

#[test]
fn local_turn_and_price_command_use_the_ratified_meter_copy() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut session, _calls) = test_session("local-test", tmp.path());
    let (out, err) = drive(&mut session, &["hello", "/price"]);

    assert!(out.contains(nh_routes::LOCAL_METER_COPY), "got: {out}");
    assert!(err.contains(nh_routes::LOCAL_METER_COPY), "got: {err}");
    assert!(err.contains("session no billed tokens"), "got: {err}");
    assert!(!out.contains("$0.00"), "got: {out}");
    assert!(!err.contains("$0.00"), "got: {err}");
}

#[test]
fn model_command_can_switch_explicitly_to_a_local_route() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut session, _calls) = test_session("deepseek-v4-flash", tmp.path());
    let (out, err) = drive(&mut session, &["/model local-test", "hello"]);

    assert!(out.contains("switched to local-test"), "got: {out}");
    assert_eq!(session.route.class(), RouteClass::Local);
    assert!(err.contains(nh_routes::LOCAL_METER_COPY), "got: {err}");
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
        err.contains("cost <¥0.0001 (≈<$0.0001) - saved 15% vs no-cache"),
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
            "unpriced | no price data | session - (incomplete - 1 unpriced turn) | tokens 12 in / 7 out / 4 cached | cache 33%"
        ),
        "got: {err}"
    );
    assert_eq!(s.unpriced_turns, 1);
}

#[test]
fn invalid_usage_marks_the_session_cost_incomplete() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
    let usage = Usage {
        prompt_tokens: 10,
        completion_tokens: 1,
        cached_tokens: Some(11),
    };

    add_session_cost(&mut s, &usage, off_peak_now());

    assert!(s.session_cost.is_empty());
    assert_eq!(s.unpriced_turns, 1);
    assert!(session_money(&s, off_peak_now()).contains("incomplete"));
}

#[test]
fn session_usage_overflow_is_atomic_and_marks_the_meter_incomplete() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
    s.session_in = u64::MAX;
    s.session_out = 7;
    s.session_cached = Some(3);
    let usage = Usage {
        prompt_tokens: 1,
        completion_tokens: 2,
        cached_tokens: Some(1),
    };

    assert!(!add_session_usage(&mut s, &usage));
    assert_eq!(s.session_in, u64::MAX);
    assert_eq!(s.session_out, 7);
    assert_eq!(s.session_cached, Some(3));
    assert_eq!(s.unpriced_turns, 1);
}

#[test]
fn footer_omits_cache_chip_before_any_usage() {
    let tmp = tempfile::tempdir().unwrap();
    let (s, _calls) = test_session("deepseek-v4-flash", tmp.path());
    let line = footer(&s);
    assert!(line.contains("session ¥0.00 (≈$0.00) | tokens 0 in / 0 out"));
    assert!(!line.contains("cached"), "got: {line}");
    assert!(!line.contains("| cache"), "got: {line}");
}

#[test]
fn footer_distinguishes_absent_cache_measurement_from_measured_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());

    assert!(add_session_usage(
        &mut s,
        &Usage {
            prompt_tokens: 20,
            completion_tokens: 2,
            cached_tokens: None,
        }
    ));
    let absent = footer(&s);
    assert!(!absent.contains("cached"), "got: {absent}");
    assert!(!absent.contains("| cache"), "got: {absent}");

    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
    assert!(add_session_usage(
        &mut s,
        &Usage {
            prompt_tokens: 20,
            completion_tokens: 2,
            cached_tokens: Some(0),
        }
    ));
    let measured_zero = footer(&s);
    assert!(
        measured_zero.contains("tokens 20 in / 2 out / 0 cached | cache 0%"),
        "got: {measured_zero}"
    );
}

// ------------------------------------------------------------- loop basics

#[test]
fn quit_exits_without_running_later_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, calls) = test_session("deepseek-v4-flash", tmp.path());
    let (_out, err) = drive(&mut s, &["/quit", "never runs"]);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        err.matches("nh> ").count(),
        1,
        "no prompt after quit: {err}"
    );
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
    assert!(err.contains("unknown command - type /help"), "got: {err}");
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
fn chat_help_discovers_image_formats_and_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
    let (out, _err) = drive(&mut s, &["/help"]);

    assert!(out.contains("/image <path>"), "got: {out}");
    assert!(out.contains("PNG or JPEG"), "got: {out}");
    assert!(out.contains("max 4"), "got: {out}");
}

#[test]
fn image_without_path_prints_usage() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _calls) = test_session("kimi-k2.6", tmp.path());
    let (_out, err) = drive(&mut s, &["/image"]);

    assert!(
        err.contains("usage: /image <path> (PNG or JPEG; max 4)"),
        "got: {err}"
    );
}

#[test]
fn image_attaches_to_next_message_and_accepts_spaces_in_path() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("screen shot.png"),
        b"\x89PNG\r\n\x1a\nfixture",
    )
    .unwrap();
    let (mut s, calls) = test_session("kimi-k2.6", tmp.path());

    let (out, err) = drive(
        &mut s,
        &["/image screen shot.png", "why is this misaligned?"],
    );

    assert!(
        out.contains("attached screen shot.png for next message (1/4)"),
        "got: {out}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1, "one provider call");
    assert!(s.pending_images.is_empty());
    assert!(!err.contains("cannot read images"), "got: {err}");
    let parts = s.history[1]
        .parts
        .as_ref()
        .expect("multimodal user message");
    assert!(matches!(
        &parts[0],
        ContentPart::Text { text } if text == "why is this misaligned?"
    ));
    assert!(matches!(
        &parts[1],
        ContentPart::ImageB64 { media_type, data }
            if media_type == "image/png" && !data.is_empty()
    ));
}

#[test]
fn image_on_text_only_route_fails_before_read_or_model_call_and_teaches_switch() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("screen.png"), b"\x89PNG\r\n\x1a\nfixture").unwrap();
    let (mut s, calls) = test_session("deepseek-v4-flash", tmp.path());

    let (out, err) = drive(&mut s, &["/image screen.png"]);

    assert!(out.is_empty(), "no attachment confirmation: {out}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(s.pending_images.is_empty());
    assert!(
        err.contains("route deepseek-v4-flash accepts text only - it cannot read images."),
        "got: {err}"
    );
    assert!(
        err.contains("Image-capable routes: kimi-k2.6."),
        "got: {err}"
    );
    assert!(
        err.contains("Switch with /model <id> or --model <id>."),
        "got: {err}"
    );
}

#[test]
fn fifth_pending_image_is_refused_clearly() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("screen.png"), b"\x89PNG\r\n\x1a\nfixture").unwrap();
    let (mut s, calls) = test_session("kimi-k2.6", tmp.path());

    let (_out, err) = drive(
        &mut s,
        &[
            "/image screen.png",
            "/image screen.png",
            "/image screen.png",
            "/image screen.png",
            "/image screen.png",
        ],
    );

    assert_eq!(s.pending_images.len(), 4);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(
        err.contains("a message can attach at most 4 images"),
        "got: {err}"
    );
}

#[test]
fn tools_lists_builtins_one_per_line() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
    let (out, _err) = drive(&mut s, &["/tools"]);
    for name in ["read_file - ", "edit_file - ", "exec_shell - "] {
        assert!(out.contains(name), "missing {name}: {out}");
    }
    assert_eq!(out.lines().count(), 3, "one line per tool: {out}");
}

/// Puts a session into the keyless-start state: stand-in client installed,
/// not connected, and every reconnect attempt failing with the vault error.
fn make_keyless(s: &mut ChatSession) {
    s.agent.client = Box::new(NotConnected {
        msg: "no key found for \"deepseek\" - run `nh key add deepseek`".into(),
    });
    s.connected = false;
    s.connect = Box::new(|route, _| {
        anyhow::bail!(
            "no key found for \"{}\" - run `nh key add {}`",
            route.vault_entry(),
            route.vault_entry()
        )
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
    assert!(
        err.contains("nh key add deepseek"),
        "task error says what to do: {err}"
    );
}

#[test]
fn keyless_session_reconnects_once_the_key_arrives() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, calls) = test_session("deepseek-v4-flash", tmp.path());
    // Keyless start, but the key arrives before the next task (the user ran
    // `nh key add deepseek` in another terminal): test_session's connect
    // succeeds, so the retry must swap in the real client and answer.
    s.agent.client = Box::new(NotConnected {
        msg: "no key found for \"deepseek\" - run `nh key add deepseek`".into(),
    });
    s.connected = false;
    let (out, err) = drive(&mut s, &["hello", "again"]);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "real client answered both tasks"
    );
    assert!(out.contains("ok"), "answer on stdout: {out}");
    assert!(
        !err.contains("no key found"),
        "stale error must not resurface: {err}"
    );
    assert!(s.connected, "session marked connected after the retry");
    // The reconnect registered the new key on the scrub path.
    assert!(
        s.key_literals.contains("fake-key-deepseek"),
        "active key was not registered"
    );
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

#[test]
fn registry_union_keeps_a_switched_away_literal_after_rotation() {
    let mut registry = SecretRegistry::new();
    registry.insert(nh_vault::secret("retired-credential-a"));
    registry.insert(nh_vault::secret("current-credential-b"));
    let scrubbed = registry
        .scrubber()
        .scrub("retired-credential-a / current-credential-b");

    assert_eq!(scrubbed, "[REDACTED] / [REDACTED]");
}

#[test]
fn answer_controls_are_neutralized_without_losing_newlines() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, calls) = test_session("deepseek-v4-flash", tmp.path());
    s.agent.client = Box::new(MockClient {
        reply: "ordinary\x1b]0;pwned\x07\nnext\x1b[31m".into(),
        calls,
    });

    let (out, _err) = drive(&mut s, &["task"]);
    assert!(!out.contains('\x1b'), "got: {out}");
    assert!(!out.contains('\x07'), "got: {out}");
    assert!(out.contains("ordinary"), "got: {out}");
    assert!(out.contains("\nnext"), "got: {out}");
}

#[test]
fn switch_message_escapes_route_id_controls() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _calls) = test_session("deepseek-v4-flash", tmp.path());
    let resolver = RouteResolver::from_toml(
        r#"
        [routes."kimi\u001b]0;pwned\u0007"]
        provider = "kimi"
        model_id = "test-model"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "kimi"
        "#,
    )
    .unwrap();
    let route = resolver.resolve("kimi\x1b]0;pwned\x07").unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();

    switch_to(&mut s, route, &mut out, &mut err);
    let out = String::from_utf8(out).unwrap();
    assert!(!out.contains('\x1b'), "got: {out}");
    assert!(!out.contains('\x07'), "got: {out}");
    assert!(out.contains("switched to kimi"), "got: {out}");
}

// ------------------------------------------------------------- peak status

#[test]
fn peak_status_second_window_and_multiplier_trim() {
    let resolver = RouteResolver::from_toml(TEST_CATALOG).unwrap();
    let route = resolver.resolve("deepseek-v4-flash").unwrap();
    // Beijing 15:00 - inside 14:00-18:00.
    let now = Utc.with_ymd_and_hms(2026, 7, 15, 7, 0, 0).unwrap();
    assert_eq!(
        route.peak_status(now, beijing_offset()),
        "peak 2x until 18:00"
    );
    // Boundary math: 18:00 itself is off-peak (end exclusive).
    let end = Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).unwrap();
    assert_eq!(route.peak_status(end, beijing_offset()), "off-peak");
    let fractional =
        RouteResolver::from_toml(&TEST_CATALOG.replace("multiplier = 2.0", "multiplier = 1.5"))
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
    let tools = load_mcp(tmp.path(), None, &law.policy, &mut warnings);
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
    let tools = load_mcp(tmp.path(), None, &law.policy, &mut warnings);
    assert!(tools.is_empty());
    assert_eq!(warnings.len(), 1, "exactly one warning: {warnings:?}");
    assert!(
        warnings[0].contains("mcp.toml"),
        "names the file: {warnings:?}"
    );
    assert!(
        warnings[0].contains("continuing without MCP"),
        "got: {warnings:?}"
    );
}
