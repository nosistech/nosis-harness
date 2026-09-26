use super::*;
use crate::keymap::{key_hint_line, visible_key_bindings};
use crate::palette::{resolve_color_mode, trust_dial_lines, ColorMode};
use crate::state::{
    search_match_lines, Overlay, PickerKind, RouteTimingHistory, UiInputs,
    MIN_TYPICAL_DURATION_SAMPLES, PROMPT_ESTIMATE_UNAVAILABLE, PROMPT_HISTORY_CAPACITY,
};
use crate::timeline::{measured_duration, timeline_detail_lines, timeline_row};
use crate::worker::WorkerCommand;
use chrono::{DateTime, Utc};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use nh_core::agent::{CompactionEvent, MAX_TASK_BYTES};
use nh_core::receipt::{CompactionStats, ReceiptKind};
use nh_core::session_ledger::{
    fold_session, list_sessions, read_session, SessionBudget, SessionEvent, Surface,
};
use nh_core::terminal_capability::TerminalCapability;
use nh_core::wire::{ChatRequest, ChatResponse};
use nh_routes::Currency;
use ratatui::{
    backend::TestBackend,
    layout::Rect,
    style::{Color, Modifier},
    text::Line,
    widgets::{Paragraph, Wrap},
    Terminal,
};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TEST_CATALOG: &str = r#"
    [routes.test-route]
    provider = "test"
    model_id = "test-route"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "test"
    context = 1000

    [routes.other-route]
    provider = "other"
    model_id = "other-route"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "other"
    context = 2000

    [routes.zero-context]
    provider = "test"
    model_id = "zero-context"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "test"
    context = 0
"#;

// Far-future FX metadata keeps CNY-to-USD fixture glosses deterministic.
const METER_CATALOG: &str = r#"
    [fx]
    usd_per_cny = 0.139
    valid_until = "2099-01-01"
    price_confidence = "reported"

    [routes.meter-route]
    provider = "test"
    model_id = "meter-route"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "test"
    class = "api"
    context = 1000000
    [routes.meter-route.price]
    currency = "CNY"
    unit = "per_million_tokens"
    cache_hit = 0.02
    cache_miss = 1.0
    output = 2.0
    price_confidence = "confirmed"
    [routes.meter-route.price.peak]
    multiplier = 2.0
    timezone = "Asia/Shanghai"
    windows = ["09:00-12:00"]

    [routes.cny-top]
    provider = "test"
    model_id = "cny-top"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "test"
    class = "api"
    context = 1000000
    [routes.cny-top.price]
    currency = "CNY"
    unit = "per_million_tokens"
    cache_hit = 0.1
    cache_miss = 4.0
    output = 8.0
    price_confidence = "confirmed"

    [routes.usd-top]
    provider = "usd-test"
    model_id = "usd-top"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "usd-test"
    class = "api"
    context = 1000000
    [routes.usd-top.price]
    currency = "USD"
    unit = "per_million_tokens"
    cache_hit = 99.0
    cache_miss = 99.0
    output = 99.0
    price_confidence = "confirmed"
"#;

const PICKER_CATALOG: &str = r#"
    [fx]
    usd_per_cny = 0.139
    valid_until = "2020-01-01"
    price_confidence = "reported"

    [routes.a-cheap]
    provider = "alpha"
    model_id = "a-cheap"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "alpha"
    context = 100000
    [routes.a-cheap.price]
    currency = "USD"
    unit = "per_million_tokens"
    cache_hit = 0.1
    cache_miss = 0.1
    output = 0.1
    price_confidence = "confirmed"

    [routes.b-expensive]
    provider = "beta"
    model_id = "b-expensive"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "beta"
    context = 100000
    [routes.b-expensive.price]
    currency = "USD"
    unit = "per_million_tokens"
    cache_hit = 0.2
    cache_miss = 0.2
    output = 0.2
    price_confidence = "confirmed"

    [routes.c-unknown-context]
    provider = "gamma"
    model_id = "c-unknown-context"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "gamma"
    [routes.c-unknown-context.price]
    currency = "USD"
    unit = "per_million_tokens"
    cache_hit = 0.0
    cache_miss = 0.0
    output = 0.0
    price_confidence = "confirmed"

    [routes.d-unknown-price]
    provider = "delta"
    model_id = "d-unknown-price"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "delta"
    context = 100000

    [routes.e-cny]
    provider = "epsilon"
    model_id = "e-cny"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "epsilon"
    context = 100000
    [routes.e-cny.price]
    currency = "CNY"
    unit = "per_million_tokens"
    cache_hit = 0.3
    cache_miss = 0.3
    output = 0.3
    price_confidence = "confirmed"

    [routes.f-local]
    provider = "ollama"
    model_id = "user-filled-model"
    base_url = "http://127.0.0.1:11434/v1"
    wire = "openai"
    vault_entry = "ollama-local"
    class = "local"
    context = 8192
    max_out = 2048
    [routes.f-local.price]
    currency = "USD"
    unit = "per_million_tokens"
    cache_hit = 0.0
    cache_miss = 0.0
    output = 0.0
    price_confidence = "confirmed"
"#;

fn test_resolver() -> RouteResolver {
    RouteResolver::from_toml(TEST_CATALOG).expect("test catalog parses")
}

fn test_route() -> ResolvedRoute {
    test_resolver().resolve("test-route").unwrap()
}

fn empty_timing_history() -> RouteTimingHistory {
    RouteTimingHistory::default()
}

fn unavailable_prompt_estimate() -> Arc<AtomicU64> {
    Arc::new(AtomicU64::new(PROMPT_ESTIMATE_UNAVAILABLE))
}

fn test_app(budget: Option<u64>) -> App {
    test_app_with_timing(budget, empty_timing_history())
}

fn test_app_with_timing(budget: Option<u64>, route_timing_history: RouteTimingHistory) -> App {
    App::new(
        test_resolver(),
        test_route(),
        budget,
        Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        PolicyView {
            autonomy: Autonomy::Ask,
            auto_paths: Vec::new(),
            ask_paths: Vec::new(),
            block_paths: Vec::new(),
            block_commands: Vec::new(),
        },
        UiInputs {
            terminal_capability: TerminalCapability::Unicode,
            palette_entries: Vec::new(),
            credentialed_providers: Vec::new(),
            color_mode: ColorMode::Color,
            route_timing_history,
            prompt_base_tokens: unavailable_prompt_estimate(),
        },
        (Profiles::bundled(), "balanced".into()),
    )
}

fn meter_app() -> App {
    meter_app_from(METER_CATALOG)
}

fn meter_app_from(catalog: &str) -> App {
    let resolver = RouteResolver::from_toml(catalog).unwrap();
    let route = resolver.resolve("meter-route").unwrap();
    App::new(
        resolver,
        route,
        None,
        Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        PolicyView {
            autonomy: Autonomy::Ask,
            auto_paths: Vec::new(),
            ask_paths: Vec::new(),
            block_paths: Vec::new(),
            block_commands: Vec::new(),
        },
        UiInputs {
            terminal_capability: TerminalCapability::Unicode,
            palette_entries: Vec::new(),
            credentialed_providers: Vec::new(),
            color_mode: ColorMode::Color,
            route_timing_history: empty_timing_history(),
            prompt_base_tokens: unavailable_prompt_estimate(),
        },
        (Profiles::bundled(), "balanced".into()),
    )
}

fn picker_app() -> App {
    let resolver = RouteResolver::from_toml(PICKER_CATALOG).unwrap();
    let route = resolver.resolve("a-cheap").unwrap();
    App::new(
        resolver,
        route,
        None,
        Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        PolicyView {
            autonomy: Autonomy::Ask,
            auto_paths: Vec::new(),
            ask_paths: Vec::new(),
            block_paths: Vec::new(),
            block_commands: Vec::new(),
        },
        UiInputs {
            terminal_capability: TerminalCapability::Unicode,
            palette_entries: Vec::new(),
            credentialed_providers: vec!["alpha".into(), "beta".into()],
            color_mode: ColorMode::Color,
            route_timing_history: empty_timing_history(),
            prompt_base_tokens: unavailable_prompt_estimate(),
        },
        (Profiles::bundled(), "balanced".into()),
    )
}

fn fixed_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-07-18T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn dispatch_test_prompt(app: &mut App, prompt: &str) {
    app.set_status(Status::Idle, Utc::now());
    app.input = prompt.to_owned();
    assert_eq!(app.dispatch().as_deref(), Some(prompt.trim()));
    app.set_status(Status::Idle, Utc::now());
}

fn render_buffer(app: &App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn rendered_help_pages(app: &mut App, width: u16, height: u16) -> String {
    let mut pages = Vec::new();
    for _ in 0..64 {
        pages.push(buffer_text(&render_buffer(app, width, height)));
        let before = app.help_scroll.get();
        if before == app.help_max_scroll.get() {
            break;
        }
        reduce_key(app, code_key(KeyCode::PageDown));
        assert!(app.help_scroll.get() > before);
    }
    pages.join("\n")
}

fn rendered_waiting_transcript_pages(app: &mut App, width: u16, height: u16) -> String {
    let mut pages = Vec::new();
    for _ in 0..256 {
        pages.push(buffer_text(&render_buffer(app, width, height)));
        let before = app.scroll_back;
        if before == app.max_scroll.get() {
            break;
        }
        reduce_key(app, code_key(KeyCode::PageUp));
        assert!(app.scroll_back > before);
        assert_eq!(app.status, Status::Waiting);
        assert!(app.pending_approval.is_some());
    }
    pages.join("\n")
}

fn buffer_rows(buffer: &ratatui::buffer::Buffer) -> Vec<String> {
    let width = usize::from(buffer.area.width);
    buffer
        .content
        .chunks(width)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect())
        .collect()
}

fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
    buffer_rows(buffer).join("\n")
}

fn assert_ascii_frame(buffer: &ratatui::buffer::Buffer, frame_name: &str) {
    for y in buffer.area.y..buffer.area.bottom() {
        for x in buffer.area.x..buffer.area.right() {
            let symbol = buffer[(x, y)].symbol();
            for character in symbol.chars() {
                assert!(
                    character.is_ascii() || character == '\u{2026}',
                    "{frame_name}: U+{:04X} {:?} at ({x}, {y}) in cell {symbol:?}",
                    u32::from(character),
                    character
                );
            }
        }
    }
}

fn find_ascii_text(buffer: &ratatui::buffer::Buffer, needle: &str) -> (u16, u16) {
    for (y, row) in buffer_rows(buffer).iter().enumerate() {
        if let Some(byte_x) = row.find(needle) {
            let cell_x = row[..byte_x].chars().count();
            return (u16::try_from(cell_x).unwrap(), u16::try_from(y).unwrap());
        }
    }
    panic!("could not find {needle:?} in {}", buffer_text(buffer));
}

fn assert_plain_modal_ring(buffer: &ratatui::buffer::Buffer, area: Rect) {
    let right = area.right().saturating_sub(1);
    let bottom = area.bottom().saturating_sub(1);
    assert_eq!(buffer[(area.x, area.y)].symbol(), "┌");
    assert_eq!(buffer[(right, area.y)].symbol(), "┐");
    assert_eq!(buffer[(area.x, bottom)].symbol(), "└");
    assert_eq!(buffer[(right, bottom)].symbol(), "┘");
    for y in area.y.saturating_add(1)..bottom {
        assert_eq!(buffer[(area.x, y)].symbol(), "│", "left edge at y={y}");
        assert_eq!(buffer[(right, y)].symbol(), "│", "right edge at y={y}");
    }
    for x in area.x.saturating_add(1)..right {
        assert_eq!(buffer[(x, bottom)].symbol(), "─", "bottom edge at x={x}");
    }
}

fn modal_text(buffer: &ratatui::buffer::Buffer, area: Rect) -> String {
    let mut text = String::new();
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

fn approval(prompt: &str) -> (AgentEvent, Receiver<bool>) {
    let (reply, answers) = mpsc::channel();
    (
        AgentEvent::Approval(ApprovalRequest {
            prompt: prompt.into(),
            repeat_key: Some(prompt.into()),
            reply,
        }),
        answers,
    )
}

fn approval_once(prompt: &str) -> (AgentEvent, Receiver<bool>) {
    let (reply, answers) = mpsc::channel();
    (
        AgentEvent::Approval(ApprovalRequest {
            prompt: prompt.into(),
            repeat_key: None,
            reply,
        }),
        answers,
    )
}

fn receipt(task: &str, outcome: Outcome, usage: Option<Usage>) -> Receipt {
    Receipt {
        kind: nh_core::receipt::ReceiptKind::Task,
        ts_utc: "2026-07-14T12:00:00Z".into(),
        model_id: "test-route".into(),
        task: task.into(),
        turns: 3,
        tool_calls: 2,
        duration_ms: None,
        outcome,
        failure_class: (outcome != Outcome::Pass).then_some(FailureClass::Constraint),
        usage,
        cache_hit_pct: None,
        repairs: Default::default(),
        retries: Default::default(),
        compaction: Default::default(),
        effective_profile: None,
    }
}

fn timed_receipt(
    model_id: &str,
    duration_ms: Option<u64>,
    outcome: Outcome,
    kind: ReceiptKind,
) -> Receipt {
    let mut receipt = receipt("timing sample", outcome, None);
    receipt.model_id = model_id.into();
    receipt.duration_ms = duration_ms;
    receipt.kind = kind;
    receipt
}

fn timeline_event(task: &str, answer: &str) -> AgentEvent {
    timeline_event_on("test-route", task, answer)
}

fn timeline_event_on(route_id: &str, task: &str, answer: &str) -> AgentEvent {
    AgentEvent::TaskReceipt(TimelineSummary {
        route_id: route_id.into(),
        receipt: receipt(
            task,
            Outcome::Pass,
            Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 2,
                cached_tokens: Some(4),
                evidence: UsageEvidence::Measured,
            }),
        ),
        answer: answer.into(),
    })
}

#[test]
fn no_color_resolver_follows_non_empty_presence_without_reading_the_environment() {
    assert_eq!(resolve_color_mode(None), ColorMode::Color);
    assert_eq!(resolve_color_mode(Some(OsStr::new(""))), ColorMode::Color);
    assert_eq!(
        resolve_color_mode(Some(OsStr::new("0"))),
        ColorMode::NoColor
    );
}

#[test]
fn absent_or_empty_no_color_preserves_rendered_colour() {
    for value in [None, Some(OsStr::new(""))] {
        let mut app = test_app(None);
        app.color_mode = resolve_color_mode(value);
        let buffer = render_buffer(&app, 90, 20);
        let (title_x, title_y) = find_ascii_text(&buffer, "nosis");
        let (hint_x, hint_y) = find_ascii_text(&buffer, "/ commands");

        assert_eq!(buffer[(title_x, title_y)].fg, Color::White);
        assert!(buffer[(title_x, title_y)].modifier.contains(Modifier::BOLD));
        assert_eq!(buffer[(hint_x, hint_y)].fg, Color::DarkGray);
        assert!(buffer[(hint_x, hint_y)].modifier.contains(Modifier::DIM));
    }
}

#[test]
fn no_color_render_suppresses_only_colour_and_keeps_modifiers() {
    let mut app = test_app(None);
    app.color_mode = ColorMode::NoColor;
    let base = render_buffer(&app, 90, 20);
    let (title_x, title_y) = find_ascii_text(&base, "nosis");
    let (hint_x, hint_y) = find_ascii_text(&base, "/ commands");

    assert!(base
        .content
        .iter()
        .all(|cell| cell.fg == Color::Reset && cell.bg == Color::Reset));
    assert!(base[(title_x, title_y)].modifier.contains(Modifier::BOLD));
    assert!(base[(hint_x, hint_y)].modifier.contains(Modifier::DIM));

    app.status = Status::Working;
    let (event, _answer) = approval("cargo test --workspace");
    apply_event(&mut app, event);
    let waiting = render_buffer(&app, 100, 20);
    let (approval_x, approval_y) = find_ascii_text(&waiting, "approve:");
    let approval_cell = &waiting[(approval_x, approval_y)];
    assert_eq!(approval_cell.fg, Color::Reset);
    assert_eq!(approval_cell.bg, Color::Reset);
    assert!(approval_cell.modifier.contains(Modifier::REVERSED));
}

#[test]
fn outer_frame_and_each_status_word_render() {
    let cases = [
        (Status::Idle, "○ IDLE"),
        (Status::Working, "● WORKING"),
        (Status::FinishingInterrupted, "● WORKING - interrupted turn"),
        (Status::Waiting, "● WAITING ON YOU"),
        (Status::Blocked("offline".into()), "● BLOCKED - offline"),
    ];
    for (status, label) in cases {
        let mut app = test_app(None);
        app.status = status;
        let buffer = render_buffer(&app, 90, 20);
        let text = buffer_text(&buffer);

        assert_eq!(buffer[(0, 0)].symbol(), "┌");
        assert_eq!(buffer[(89, 0)].symbol(), "┐");
        assert_eq!(buffer[(0, 19)].symbol(), "└");
        assert_eq!(buffer[(89, 19)].symbol(), "┘");
        assert!(text.contains("nosis"), "got: {text}");
        assert!(text.contains("test-route"), "got: {text}");
        assert!(text.contains(label), "got: {text}");
    }
}

#[test]
fn blocked_status_chip_renders_budget_reason() {
    let status = Status::Blocked(BUDGET_REASON.into());
    let (label, style) = status_chip(&status, None, fixed_at(), usize::MAX, None);
    assert_eq!(label, "● BLOCKED - budget reached");
    assert_eq!(style.fg, Some(Color::Red));
    assert!(style.add_modifier.contains(Modifier::BOLD));

    let mut app = test_app(None);
    app.status = status;
    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(
        rendered.contains("● BLOCKED - budget reached"),
        "got: {rendered}"
    );
}

#[test]
fn blocked_status_chip_uses_only_the_first_non_empty_reason_line() {
    let status = Status::Blocked("\r\n  first line  \rsecond line\nthird line".into());
    let (label, _) = status_chip(&status, None, fixed_at(), usize::MAX, None);
    assert_eq!(label, "● BLOCKED - first line");
    assert!(!label.contains('\n'), "got: {label:?}");
    assert!(!label.contains('\r'), "got: {label:?}");

    let mut app = test_app(None);
    app.status = status;
    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(
        rendered.contains("● BLOCKED - first line"),
        "got: {rendered}"
    );
    assert!(!rendered.contains("second line"), "got: {rendered}");
    assert!(!rendered.contains("third line"), "got: {rendered}");
}

#[test]
fn blocked_status_chip_caps_long_reasons_and_appends_ellipsis() {
    let status = Status::Blocked("x".repeat(40));
    let (label, _) = status_chip(&status, None, fixed_at(), usize::MAX, None);
    let expected = format!("● BLOCKED - {}…", "x".repeat(32));
    assert_eq!(label, expected);
    assert!(label.ends_with('…'));

    let mut app = test_app(None);
    app.status = status;
    let rendered = buffer_text(&render_buffer(&app, 120, 20));
    assert!(rendered.contains(&expected), "got: {rendered}");
    assert!(!rendered.contains(&"x".repeat(33)), "got: {rendered}");
}

#[test]
fn blocked_status_chip_omits_separator_for_blank_reasons() {
    for reason in ["", "   "] {
        let status = Status::Blocked(reason.into());
        let (label, _) = status_chip(&status, None, fixed_at(), usize::MAX, None);
        assert_eq!(label, "● BLOCKED");
        assert!(!label.ends_with('-'));

        let mut app = test_app(None);
        app.status = status;
        let rows = buffer_rows(&render_buffer(&app, 90, 20));
        assert!(rows[0].contains("● BLOCKED"), "got: {}", rows[0]);
        assert!(!rows[0].contains("BLOCKED -"), "got: {}", rows[0]);
    }
}

#[test]
fn budget_reached_hint_bar_omits_enter_send() {
    let app = test_app(Some(0));
    assert!(app.budget_reached());
    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(rendered.contains("F1 help"), "got: {rendered}");
    assert!(rendered.contains("/ commands"), "got: {rendered}");
    assert!(!rendered.contains("Enter send"), "got: {rendered}");
}

#[test]
fn idle_hint_bar_keeps_enter_send() {
    let app = test_app(None);
    assert!(!app.budget_reached());
    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(rendered.contains("F1 help"), "got: {rendered}");
    assert!(rendered.contains("Enter send"), "got: {rendered}");
}

#[test]
fn help_overlay_renders_binding_details_and_hint_bar_renders_state_actions() {
    let mut app = test_app(None);
    let base = buffer_text(&render_buffer(&app, 100, 24));
    let expected_hint = key_hint_line(false, false, false, false, 98);
    assert!(base.contains(&expected_hint), "got: {base}");
    assert!(!base.contains("Ctrl+P"), "got: {base}");
    assert!(!base.contains("Ctrl+N"), "got: {base}");

    let (event, _answer) = approval("exec cargo test --workspace");
    apply_event(&mut app, event);
    assert_eq!(app.status, Status::Waiting);
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::F(1))),
        UiAction::None
    );
    assert_eq!(app.overlay, Overlay::Help);
    let rendered_help = rendered_help_pages(&mut app, 100, 24);
    let help = rendered_help
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(help.contains("Help"), "got: {help}");
    assert!(
        help.contains("Esc stop + close · ↑/↓ scroll · PgUp/PgDn page"),
        "got: {help}"
    );
    assert!(help.contains("Ctrl+P"), "got: {help}");
    assert!(help.contains("Ctrl+N"), "got: {help}");
    for binding in visible_key_bindings(false, true) {
        assert!(
            help.contains(binding.keys),
            "missing key {:?}: {help}",
            binding.keys
        );
        let detail = format!(
            "{}  {}{}",
            binding.display_keys(TerminalCapability::Unicode),
            binding.action,
            binding.detail
        );
        let wrapped = wrap_help_line(&detail, 72);
        assert_eq!(wrapped.concat(), detail);
        for line in wrapped {
            assert!(
                rendered_help.contains(&line),
                "missing detail line {line:?}: {rendered_help}"
            );
        }
    }
}

#[test]
fn f1_help_pages_all_controls_at_normal_and_small_terminal_sizes() {
    for (width, height) in [(80, 24), (60, 15)] {
        let mut app = test_app(None);
        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::F(1))),
            UiAction::None
        );

        let first = buffer_text(&render_buffer(&app, width, height));

        assert_eq!(app.overlay, Overlay::Help);
        assert!(first.contains("Help"), "{width}x{height}: {first}");
        assert!(first.contains("Esc close"), "{width}x{height}: {first}");
        assert!(
            first.contains("PgUp/PgDn page"),
            "{width}x{height}: {first}"
        );
        let all = rendered_help_pages(&mut app, width, height);
        assert!(
            all.contains("Shift+Enter/Ctrl+J"),
            "{width}x{height}: {all}"
        );
        assert!(all.contains("F2 / F3 / F4"), "{width}x{height}: {all}");

        reduce_key(&mut app, code_key(KeyCode::End));
        let old_max = app.help_max_scroll.get();
        assert_eq!(app.help_scroll.get(), old_max);
        let _ = render_buffer(&app, 80, 24);
        assert!(app.help_scroll.get() <= app.help_max_scroll.get());
        assert_eq!(reduce_key(&mut app, code_key(KeyCode::Esc)), UiAction::None);
        assert_eq!(app.overlay, Overlay::None);
    }
}

#[test]
fn narrow_hint_bar_prioritizes_help_and_the_current_state_actions() {
    let width_31 = key_hint_line(false, true, true, true, 31);
    let width_43 = key_hint_line(false, true, true, true, 43);
    let width_60 = key_hint_line(false, true, true, true, 60);
    assert_eq!(width_31, "F1 help   Esc stop   F4 decline");
    assert_eq!(width_43, width_31);
    assert_eq!(
        width_60,
        "F1 help   Esc stop   F4 decline   F2 approve   F3 repeat"
    );
    for hints in [&width_31, &width_43, &width_60] {
        assert!(hints.contains("F4 decline"), "got: {hints}");
        assert!(
            !hints.contains("F3 repeat") || hints.contains("F2 approve"),
            "repeat must never outrank approve: {hints}"
        );
    }

    let idle = buffer_text(&render_buffer(&test_app(None), 60, 15));
    assert!(idle.contains("F1 help"), "got: {idle}");
    assert!(idle.contains("Enter send"), "got: {idle}");

    let mut working = test_app(None);
    working.status = Status::Working;
    let working = buffer_text(&render_buffer(&working, 60, 15));
    assert!(working.contains("F1 help"), "got: {working}");
    assert!(working.contains("Esc stop"), "got: {working}");
    assert!(working.contains("Enter queue"), "got: {working}");

    let mut repeat_app = test_app(None);
    repeat_app.status = Status::Working;
    let (event, _answer) = approval("exec cargo test --workspace");
    apply_event(&mut repeat_app, event);
    let approval = buffer_text(&render_buffer(&repeat_app, 60, 15));
    for hint in [
        "F1 help",
        "Esc stop",
        "F2 approve",
        "F3 repeat",
        "F4 decline",
    ] {
        assert!(approval.contains(hint), "missing {hint:?}: {approval}");
    }

    let mut once_app = test_app(None);
    once_app.status = Status::Working;
    let (event, _answer) = approval_once("edit src/lib.rs");
    apply_event(&mut once_app, event);
    let once = buffer_text(&render_buffer(&once_app, 60, 15));
    for hint in ["F1 help", "Esc stop", "F2 approve", "F4 decline"] {
        assert!(once.contains(hint), "missing {hint:?}: {once}");
    }
    assert!(!once.contains("F3 repeat"), "got: {once}");
}

#[test]
fn help_escape_preserves_stop_semantics_while_idle_escape_only_closes() {
    let mut idle = test_app(None);
    reduce_key(&mut idle, code_key(KeyCode::F(1)));
    let _ = render_buffer(&idle, 60, 15);
    assert_eq!(
        reduce_key(&mut idle, code_key(KeyCode::Esc)),
        UiAction::None
    );
    assert_eq!(idle.overlay, Overlay::None);
    assert_eq!(idle.status, Status::Idle);

    let mut working = test_app(None);
    working.status = Status::Working;
    reduce_key(&mut working, code_key(KeyCode::F(1)));
    let help = buffer_text(&render_buffer(&working, 60, 15));
    assert!(help.contains("Esc stop + close"), "got: {help}");
    assert_eq!(
        reduce_key(&mut working, code_key(KeyCode::Esc)),
        UiAction::Interrupt
    );
    assert_eq!(working.overlay, Overlay::None);
    assert_eq!(working.status, Status::FinishingInterrupted);
}

#[test]
fn f1_closes_help_without_interrupting_work_or_forwarding_approval_keys() {
    for status in [Status::Working, Status::Waiting] {
        let mut app = test_app(None);
        app.status = Status::Working;
        let waiting = status == Status::Waiting;
        if waiting {
            let (event, answer) = approval("cargo test");
            apply_event(&mut app, event);
            assert_eq!(app.status, Status::Waiting);
            assert_eq!(answer.try_recv(), Err(TryRecvError::Empty));
        }

        reduce_key(&mut app, code_key(KeyCode::F(1)));
        assert_eq!(app.overlay, Overlay::Help);
        let help = buffer_text(&render_buffer(&app, 60, 15));
        assert!(help.contains("F1 close"), "got: {help}");
        if waiting {
            assert_eq!(
                reduce_key(&mut app, code_key(KeyCode::F(2))),
                UiAction::None
            );
            assert_eq!(
                reduce_key(&mut app, code_key(KeyCode::F(3))),
                UiAction::None
            );
            assert!(app.pending_approval.is_some());
            assert!(app.session_allow.is_empty());
        }
        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::F(1))),
            UiAction::None
        );
        assert_eq!(app.overlay, Overlay::None);
        assert_eq!(app.status, status);
    }
}

#[test]
fn every_overlay_labels_escape_as_stop_when_the_turn_is_active() {
    let mut idle = test_app(None);
    idle.overlay = Overlay::Search {
        query: String::new(),
        selected: 0,
        original_scroll: 0,
    };
    let idle = buffer_text(&render_buffer(&idle, 80, 24));
    assert!(idle.contains("Esc cancel"), "got: {idle}");
    assert!(!idle.contains("Esc stop + close"), "got: {idle}");

    for status in [Status::Working, Status::Waiting] {
        let mut app = test_app(None);
        app.status = status;
        app.overlay = Overlay::Picker {
            kind: PickerKind::Model,
            selected: 0,
            rows: vec![crate::state::PickerRow {
                value: "test-route".into(),
                label: "test route".into(),
            }],
        };
        let picker = buffer_text(&render_buffer(&app, 80, 24));
        assert!(picker.contains("Esc stop + close"), "got: {picker}");
        assert!(!picker.contains("Esc cancel"), "got: {picker}");

        app.overlay = Overlay::Search {
            query: String::new(),
            selected: 0,
            original_scroll: 0,
        };
        let search = buffer_text(&render_buffer(&app, 80, 24));
        assert!(search.contains("Esc stop + close"), "got: {search}");
        assert!(!search.contains("Esc cancel"), "got: {search}");
    }
}

#[test]
fn project_identity_is_scrubbed_fitted_and_recoverable_from_help() {
    let mut app = test_app(None);
    app.project_root = Some(PathBuf::from(
        "/workspace/very long unicode Ω 项目 project name that exceeds the header",
    ));

    let idle_header = buffer_rows(&render_buffer(&app, 60, 15))[0].clone();
    assert!(idle_header.contains("○ IDLE"), "got: {idle_header}");
    assert!(idle_header.contains("very"), "got: {idle_header}");

    for (status, marker) in [
        (Status::Working, "● WORKING"),
        (Status::Waiting, "● WAITING ON YOU"),
        (Status::Blocked("policy".into()), "● BLOCKED"),
    ] {
        app.status = status;
        let header = buffer_rows(&render_buffer(&app, 60, 15))[0].clone();
        assert!(header.contains(marker), "missing {marker:?}: {header}");
        assert!(header.contains("test-route"), "route hidden: {header}");
    }

    app.status = Status::Idle;
    reduce_key(&mut app, code_key(KeyCode::F(1)));
    let help = rendered_help_pages(&mut app, 60, 15);
    for visible in [
        "Project:",
        "workspace",
        "unicode",
        "Ω",
        "项",
        "目",
        "exceeds",
        "header",
    ] {
        assert!(help.contains(visible), "missing {visible:?}: {help}");
    }
}

#[test]
fn help_path_wrapping_preserves_repeated_spaces_and_wide_characters() {
    let path = "Project: C:\\My  Project\\项目  notes";
    let wrapped = wrap_help_line(path, 12);

    assert_eq!(wrapped.concat(), path);
    assert!(wrapped
        .iter()
        .all(|line| Line::from(line.as_str()).width() <= 12));
}

#[test]
fn esc_binding_reaches_both_key_surfaces_only_when_it_can_interrupt() {
    for status in [Status::Idle, Status::FinishingInterrupted] {
        let mut app = test_app(None);
        app.status = status;
        let base = buffer_text(&render_buffer(&app, 120, 24));
        assert!(!base.contains("Esc stop"), "got: {base}");
        reduce_key(&mut app, code_key(KeyCode::F(1)));
        let help = rendered_help_pages(&mut app, 120, 24);
        assert!(!help.contains("Esc interrupt"), "got: {help}");
    }

    for status in [Status::Working, Status::Waiting] {
        let mut app = test_app(None);
        app.status = status;
        let base = buffer_text(&render_buffer(&app, 120, 24));
        assert!(base.contains("Esc stop"), "got: {base}");
        reduce_key(&mut app, code_key(KeyCode::F(1)));
        let help = rendered_help_pages(&mut app, 120, 24);
        assert!(
            help.contains("interrupt the turn; close overlays; decline approvals"),
            "got: {help}"
        );
    }
}

#[test]
fn esc_handler_uses_the_same_interrupt_predicate_as_the_key_surfaces() {
    for status in [
        Status::Idle,
        Status::Working,
        Status::Waiting,
        Status::FinishingInterrupted,
        Status::Blocked("done".into()),
    ] {
        let expected = status.esc_interrupts_turn();
        let mut app = test_app(None);
        app.status = status;
        let action = reduce_key(&mut app, code_key(KeyCode::Esc));
        assert_eq!(action == UiAction::Interrupt, expected);
    }
}

#[test]
fn budget_reached_hides_send_from_both_key_surfaces() {
    let mut app = test_app(Some(0));
    let base = buffer_text(&render_buffer(&app, 100, 24));
    assert!(!base.contains("Enter send"), "got: {base}");

    reduce_key(&mut app, code_key(KeyCode::F(1)));
    let help = buffer_text(&render_buffer(&app, 100, 24));
    assert!(!help.contains("Enter"), "got: {help}");
    assert!(!help.contains("send task"), "got: {help}");
}

#[test]
fn question_mark_opens_help_only_for_an_empty_input() {
    let mut empty = test_app(None);
    assert_eq!(reduce_key(&mut empty, char_key('?')), UiAction::None);
    assert_eq!(empty.overlay, Overlay::Help);
    assert!(empty.input.is_empty());
    assert_eq!(
        reduce_key(&mut empty, code_key(KeyCode::Esc)),
        UiAction::None
    );
    assert_eq!(empty.overlay, Overlay::None);

    let mut composing = test_app(None);
    type_text(&mut composing, "why");
    assert_eq!(reduce_key(&mut composing, char_key('?')), UiAction::None);
    assert_eq!(composing.input, "why?");
    assert_eq!(composing.overlay, Overlay::None);
}

#[test]
fn ctrl_w_deletes_trailing_space_then_the_previous_word() {
    let mut app = test_app(None);
    app.input = "alpha beta   ".into();
    app.pending_send = true;

    assert_eq!(reduce_key(&mut app, ctrl_key('w')), UiAction::None);
    assert_eq!(app.input, "alpha ");
    assert!(app.pending_send);
    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(rendered.contains("❯ [queued] alpha "), "got: {rendered}");
    assert!(!rendered.contains("beta"), "got: {rendered}");

    assert_eq!(reduce_key(&mut app, ctrl_key('w')), UiAction::None);
    assert!(app.input.is_empty());
    assert!(!app.pending_send);
}

#[test]
fn ctrl_word_delete_aliases_match_ctrl_w_but_plain_backspace_does_not() {
    for key in [
        KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
        ctrl_key('h'),
        KeyEvent::new(
            KeyCode::Char('H'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ),
    ] {
        let mut app = test_app(None);
        app.input = "one two".into();
        reduce_key(&mut app, key);
        assert_eq!(app.input, "one ");
    }

    let mut app = test_app(None);
    app.input = "one two".into();
    reduce_key(&mut app, code_key(KeyCode::Backspace));
    assert_eq!(app.input, "one tw");
}

#[test]
fn word_and_line_deletion_work_while_composing_a_queued_task() {
    let mut app = test_app(None);
    app.status = Status::Working;
    type_text(&mut app, "queued words");
    reduce_key(&mut app, code_key(KeyCode::Enter));
    assert!(app.pending_send);

    reduce_key(&mut app, ctrl_key('w'));
    assert_eq!(app.input, "queued ");
    assert!(app.pending_send);
    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(rendered.contains("[queued] queued "), "got: {rendered}");

    reduce_key(&mut app, ctrl_key('u'));
    assert!(app.input.is_empty());
    assert!(!app.pending_send);
    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(!rendered.contains("[queued]"), "got: {rendered}");
}

#[test]
fn ctrl_u_clears_the_idle_input_and_pending_marker() {
    let mut app = test_app(None);
    app.input = "whole line".into();
    app.pending_send = true;
    assert_eq!(reduce_key(&mut app, ctrl_key('u')), UiAction::None);
    assert!(app.input.is_empty());
    assert!(!app.pending_send);
}

#[test]
fn word_and_line_deletion_stay_live_in_the_slash_command_menu() {
    let mut app = test_app(None);
    type_text(&mut app, "/model other-route");
    assert!(matches!(app.overlay, Overlay::CommandMenu { .. }));

    reduce_key(&mut app, ctrl_key('w'));
    assert_eq!(app.input, "/model ");
    assert!(matches!(app.overlay, Overlay::CommandMenu { selected: 0 }));

    reduce_key(&mut app, ctrl_key('u'));
    assert!(app.input.is_empty());
    assert!(!app.pending_send);
    assert_eq!(app.overlay, Overlay::None);
}

#[test]
fn prompt_history_walks_both_directions_and_restores_the_draft() {
    let mut app = test_app(None);
    dispatch_test_prompt(&mut app, "first prompt");
    dispatch_test_prompt(&mut app, "second prompt");
    app.input = "draft in progress".into();

    reduce_key(&mut app, ctrl_key('p'));
    assert_eq!(app.input, "second prompt");
    reduce_key(&mut app, ctrl_key('p'));
    assert_eq!(app.input, "first prompt");
    reduce_key(&mut app, ctrl_key('p'));
    assert_eq!(app.input, "first prompt");
    reduce_key(&mut app, ctrl_key('n'));
    assert_eq!(app.input, "second prompt");
    reduce_key(&mut app, ctrl_key('n'));
    assert_eq!(app.input, "draft in progress");
    reduce_key(&mut app, ctrl_key('n'));
    assert_eq!(app.input, "draft in progress");

    app.status = Status::Working;
    app.input = "queued draft".into();
    reduce_key(&mut app, ctrl_key('p'));
    assert_eq!(app.input, "second prompt");
    assert_eq!(app.status, Status::Working);
    reduce_key(&mut app, ctrl_key('n'));
    assert_eq!(app.input, "queued draft");
}

#[test]
fn prompt_history_skips_empty_and_immediately_duplicate_prompts() {
    let mut app = test_app(None);
    app.input = "   ".into();
    assert_eq!(app.dispatch(), None);
    assert!(app.prompt_history.is_empty());

    dispatch_test_prompt(&mut app, "repeat this");
    dispatch_test_prompt(&mut app, "  repeat this  ");
    dispatch_test_prompt(&mut app, "different prompt");

    assert_eq!(
        app.prompt_history,
        ["repeat this".to_owned(), "different prompt".to_owned()]
    );
}

#[test]
fn prompt_history_drops_the_oldest_entry_at_its_session_cap() {
    let mut app = test_app(None);
    for index in 0..PROMPT_HISTORY_CAPACITY.saturating_add(3) {
        dispatch_test_prompt(&mut app, &format!("prompt {index}"));
    }

    assert_eq!(app.prompt_history.len(), PROMPT_HISTORY_CAPACITY);
    assert_eq!(
        app.prompt_history.first().map(String::as_str),
        Some("prompt 3")
    );
    let expected_last = format!("prompt {}", PROMPT_HISTORY_CAPACITY + 2);
    assert_eq!(
        app.prompt_history.last().map(String::as_str),
        Some(expected_last.as_str())
    );
}

#[test]
fn editing_a_recalled_prompt_restarts_the_next_walk_at_the_newest_entry() {
    let mut app = test_app(None);
    dispatch_test_prompt(&mut app, "first prompt");
    dispatch_test_prompt(&mut app, "second prompt");
    app.input = "draft".into();

    reduce_key(&mut app, ctrl_key('p'));
    assert_eq!(app.input, "second prompt");
    reduce_key(&mut app, char_key('x'));
    assert_eq!(app.input, "second promptx");
    assert!(app.prompt_history_index.is_none());
    assert!(app.prompt_history_draft.is_none());

    reduce_key(&mut app, ctrl_key('p'));
    assert_eq!(app.input, "second prompt");
    reduce_key(&mut app, ctrl_key('n'));
    assert_eq!(app.input, "second promptx");
}

#[test]
fn prompt_recall_is_inert_during_overlays_and_approvals() {
    let mut app = test_app(None);
    dispatch_test_prompt(&mut app, "remembered prompt");
    app.input = "draft".into();
    app.overlay = Overlay::Help;

    reduce_key(&mut app, ctrl_key('p'));
    assert_eq!(app.input, "draft");
    assert!(app.prompt_history_index.is_none());

    app.overlay = Overlay::None;
    app.status = Status::Working;
    let (event, answer) = approval("cargo test --workspace");
    apply_event(&mut app, event);
    reduce_key(&mut app, ctrl_key('p'));
    assert_eq!(app.input, "draft");
    assert_eq!(answer.try_recv(), Err(TryRecvError::Empty));
    assert!(app.prompt_history_index.is_none());
}

#[test]
fn f1_opens_read_only_help_without_changing_composed_input() {
    let mut app = test_app(None);
    type_text(&mut app, "draft");
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::F(1))),
        UiAction::None
    );
    assert_eq!(app.overlay, Overlay::Help);
    assert_eq!(app.input, "draft");

    assert_eq!(reduce_key(&mut app, char_key('x')), UiAction::None);
    assert_eq!(app.overlay, Overlay::Help);
    assert_eq!(app.input, "draft");
}

#[test]
fn failed_blocked_hint_bar_keeps_enter_send_when_budget_remains() {
    let mut app = test_app(None);
    app.status = Status::Working;
    reduce_agent_event(&mut app, AgentEvent::Failed("offline".into()));
    assert_eq!(app.status, Status::Blocked("offline".into()));
    assert!(!app.budget_reached());

    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(
        rendered.contains("F1 help") && rendered.contains("Enter send"),
        "got: {rendered}"
    );
}

#[test]
fn budget_reached_empty_input_renders_truthful_placeholder() {
    let app = test_app(Some(0));
    assert!(app.budget_reached());
    assert!(app.input.is_empty());
    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(
        rendered.contains("❯ budget reached - press Ctrl+C twice to exit"),
        "got: {rendered}"
    );
    assert!(
        !rendered.contains("type a task and press Enter…"),
        "got: {rendered}"
    );
}

#[test]
fn failed_blocked_empty_input_keeps_normal_placeholder() {
    let mut app = test_app(None);
    app.status = Status::Working;
    reduce_agent_event(&mut app, AgentEvent::Failed("offline".into()));
    assert_eq!(app.status, Status::Blocked("offline".into()));
    assert!(!app.budget_reached());
    assert!(app.input.is_empty());

    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(
        rendered.contains("❯ type a task and press Enter…"),
        "got: {rendered}"
    );
    assert!(
        !rendered.contains("budget reached - press Ctrl+C twice to exit"),
        "got: {rendered}"
    );
}

#[test]
fn blocked_reason_stays_on_the_header_at_realistic_width() {
    let app = test_app(Some(0));
    let rows = buffer_rows(&render_buffer(&app, 90, 20));
    assert!(
        rows[0].contains("● BLOCKED - budget reached"),
        "got: {}",
        rows[0]
    );
    assert!(
        rows[0].contains("test-route · effort: none"),
        "got: {}",
        rows[0]
    );
    assert!(
        rows.iter()
            .skip(1)
            .all(|row| !row.contains("● BLOCKED - budget reached")),
        "got: {}",
        rows.join("\n")
    );
}

#[test]
fn long_blocked_reason_keeps_both_titles_readable_at_width_eighty() {
    let mut app = test_app(None);
    app.status = Status::Blocked("x".repeat(100));
    let rows = buffer_rows(&render_buffer(&app, 80, 20));
    let header = &rows[0];

    assert!(header.contains("● BLOCKED - "), "got: {header}");
    assert!(
        header.contains("test-route · effort: none"),
        "got: {header}"
    );
    let expected_join = format!(
        "● BLOCKED - {}… ─ test-route · effort: none ",
        "x".repeat(27)
    );
    assert!(
        header.contains(&expected_join),
        "titles must remain separated by the border: {header}"
    );
}

#[test]
fn long_blocked_reason_keeps_both_titles_readable_at_width_seventy_nine() {
    let mut app = test_app(None);
    app.status = Status::Blocked("x".repeat(100));
    let rows = buffer_rows(&render_buffer(&app, 79, 20));
    let header = &rows[0];

    let expected_join = format!(
        "● BLOCKED - {}… ─ test-route · effort: none ",
        "x".repeat(26)
    );
    assert!(
        header.contains(&expected_join),
        "titles must remain separated by the border: {header}"
    );
}

#[test]
fn chat_roles_label_indented_turns_and_leave_a_visual_gap() {
    let mut app = test_app(None);
    app.input = "fix this test".into();
    assert_eq!(app.dispatch().as_deref(), Some("fix this test"));
    apply_event(&mut app, AgentEvent::Answer("done cleanly".into()));

    let rows = buffer_rows(&render_buffer(&app, 90, 20));
    let user_row = rows.iter().position(|row| row.contains("❯ you")).unwrap();
    let task_row = rows
        .iter()
        .position(|row| row.contains("   fix this test"))
        .unwrap();
    let nosis_row = rows.iter().position(|row| row.contains("◆ nosis")).unwrap();
    let answer_row = rows
        .iter()
        .position(|row| row.contains("   done cleanly"))
        .unwrap();

    assert_eq!(task_row, user_row + 1);
    assert_eq!(answer_row, nosis_row + 1);
    assert!(nosis_row > task_row + 1);
    assert!(rows[task_row + 1].trim_matches(['│', ' ']).is_empty());
}

#[test]
fn empty_state_and_key_strip_are_self_teaching_then_conversation_replaces_welcome() {
    let mut app = test_app(None);
    let fresh = buffer_text(&render_buffer(&app, 90, 20));
    assert!(fresh.contains("Welcome to nosis."), "got: {fresh}");
    assert!(
        fresh.contains("Type a task and press Enter."),
        "got: {fresh}"
    );
    assert!(
        fresh.contains("e.g. \"fix the failing test in this repo\""),
        "got: {fresh}"
    );
    assert!(
        fresh.contains("Type / to see everything nosis can do."),
        "got: {fresh}"
    );
    assert!(
        fresh.contains("F1 help") && fresh.contains("Enter send"),
        "got: {fresh}"
    );

    app.input = "start".into();
    app.dispatch().unwrap();
    let active = buffer_text(&render_buffer(&app, 90, 20));
    assert!(!active.contains("Welcome to nosis."), "got: {active}");
    assert!(active.contains("❯ you"), "got: {active}");
    assert!(active.contains("   start"), "got: {active}");
    assert!(
        active.contains("F1 help") && active.contains("Enter queue"),
        "got: {active}"
    );
}

#[test]
fn modal_frames_clear_transcript_for_every_overlay() {
    let terminal = Rect::new(0, 0, 100, 30);
    let cases = [
        (
            Overlay::Search {
                query: "safe".into(),
                selected: 0,
                original_scroll: 0,
            },
            search_modal_area(terminal),
            "Search transcript",
        ),
        (
            Overlay::CommandMenu { selected: 0 },
            modal_area(terminal, 14),
            "Commands",
        ),
        (Overlay::Help, modal_area(terminal, 22), "Help"),
        (
            Overlay::TrustDial,
            modal_area(terminal, 8),
            "Trust Dial · read-only",
        ),
        (
            Overlay::Palette {
                filter: String::new(),
                selected: 0,
                detail: None,
            },
            modal_area(terminal, 18),
            "Commands + Tools",
        ),
        (
            Overlay::Timeline {
                selected: 0,
                inspecting: false,
                note: None,
            },
            modal_area(terminal, 20),
            "Timeline",
        ),
    ];

    for (overlay, area, title) in cases {
        let mut app = test_app(None);
        for _ in 0..40 {
            app.push_line(
                "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ",
                TranscriptKind::Progress,
            );
        }
        apply_event(&mut app, timeline_event("safe task", "safe answer"));
        app.overlay = overlay;

        let buffer = render_buffer(&app, terminal.width, terminal.height);
        let modal = modal_text(&buffer, area);
        assert_plain_modal_ring(&buffer, area);
        assert!(modal.contains(title), "got: {modal}");
        assert!(!modal.contains('Z'), "transcript bled into modal: {modal}");
    }
}

#[test]
fn ascii_mode_sweeps_main_transcript_and_every_overlay_frame() {
    let mut app = test_app(None);
    app.terminal_capability = TerminalCapability::AsciiFallback;

    let empty = render_buffer(&app, 100, 30);
    assert_ascii_frame(&empty, "main screen");
    let empty_text = buffer_text(&empty);
    assert!(empty_text.contains("o IDLE"), "got: {empty_text}");
    assert!(empty_text.contains("^v scroll"), "got: {empty_text}");
    assert_eq!(empty[(0, 0)].symbol(), "+");
    assert_eq!(empty[(99, 29)].symbol(), "+");

    app.push_line("test task", TranscriptKind::Task);
    app.push_line("test answer", TranscriptKind::Answer);
    app.push_line("test progress", TranscriptKind::Progress);
    apply_event(&mut app, timeline_event("timeline task", "timeline answer"));
    app.status = Status::Blocked("approval required".into());

    let transcript = render_buffer(&app, 100, 30);
    assert_ascii_frame(&transcript, "transcript");
    let transcript_text = buffer_text(&transcript);
    assert!(transcript_text.contains("> you"), "got: {transcript_text}");
    assert!(
        transcript_text.contains("* nosis"),
        "got: {transcript_text}"
    );
    assert!(
        transcript_text.contains("- test progress"),
        "got: {transcript_text}"
    );
    assert!(
        transcript_text.contains("! BLOCKED"),
        "got: {transcript_text}"
    );

    for index in 0..40 {
        app.push_line(&format!("overflow line {index}"), TranscriptKind::Progress);
    }
    let newest = render_buffer(&app, 100, 30);
    assert_ascii_frame(&newest, "transcript newest overflow");
    let newest_text = buffer_text(&newest);
    assert!(newest_text.contains("^ more"), "got: {newest_text}");
    app.scroll_back = app.max_scroll.get();
    let oldest = render_buffer(&app, 100, 30);
    assert_ascii_frame(&oldest, "transcript oldest overflow");
    let oldest_text = buffer_text(&oldest);
    assert!(oldest_text.contains("v more"), "got: {oldest_text}");
    app.scroll_back = 0;

    let overlays = [
        (
            "search overlay",
            Overlay::Search {
                query: "test".into(),
                selected: 0,
                original_scroll: 0,
            },
        ),
        ("command menu overlay", Overlay::CommandMenu { selected: 0 }),
        ("help overlay", Overlay::Help),
        ("trust dial overlay", Overlay::TrustDial),
        (
            "timeline overlay",
            Overlay::Timeline {
                selected: 0,
                inspecting: true,
                note: None,
            },
        ),
        (
            "palette overlay",
            Overlay::Palette {
                filter: String::new(),
                selected: 0,
                detail: None,
            },
        ),
        (
            "picker overlay",
            Overlay::Picker {
                kind: PickerKind::Model,
                selected: 0,
                rows: Vec::new(),
            },
        ),
    ];
    for (frame_name, overlay) in overlays {
        app.overlay = overlay;
        let buffer = render_buffer(&app, 100, 30);
        assert_ascii_frame(&buffer, frame_name);
    }

    let mut picker = picker_app();
    picker.terminal_capability = TerminalCapability::AsciiFallback;
    type_text(&mut picker, "/provider");
    assert_eq!(
        reduce_key(&mut picker, code_key(KeyCode::Enter)),
        UiAction::None
    );
    let picker_frame = render_buffer(&picker, 100, 30);
    assert_ascii_frame(&picker_frame, "populated provider picker overlay");
    let picker_text = buffer_text(&picker_frame);
    assert!(
        picker_text.contains("alpha - a-cheap - credential available"),
        "got: {picker_text}"
    );
}

#[test]
fn ascii_mode_preserves_model_answer_content() {
    let mut app = test_app(None);
    app.terminal_capability = TerminalCapability::AsciiFallback;
    let answer = "price \u{00a5}100 \u{2248} USD 1";

    apply_event(&mut app, AgentEvent::Answer(answer.into()));

    let displayed = app
        .transcript
        .iter()
        .find(|line| line.kind == TranscriptKind::Answer)
        .expect("model answer remains a distinct transcript entry");
    assert!(displayed.kind == TranscriptKind::Answer);
    assert_eq!(displayed.text, answer);

    let notice = app.transcript.last().unwrap();
    assert!(notice.kind == TranscriptKind::Progress);
    assert_eq!(notice.text, nh_core::agent::result_notice(false));
}

#[test]
fn ascii_mode_preserves_approval_command_bytes() {
    let mut app = test_app(None);
    app.terminal_capability = TerminalCapability::AsciiFallback;
    app.status = Status::Working;
    let command = "compare \u{00a5}100 \u{2192} result \u{2248} USD 1";
    let (event, _answer) = approval(command);

    apply_event(&mut app, event);

    let line = &app.transcript.last().unwrap().text;
    let suffix = format!("   {APPROVAL_LEGEND}");
    let displayed_command = line
        .strip_prefix("approve: ")
        .and_then(|line| line.strip_suffix(&suffix))
        .unwrap();
    let pending_command = &app.pending_approval.as_ref().unwrap().prompt;
    assert_eq!(displayed_command.as_bytes(), pending_command.as_bytes());
}

#[test]
fn ascii_mode_preserves_typed_task_content() {
    let mut app = test_app(None);
    app.terminal_capability = TerminalCapability::AsciiFallback;
    let task = "compare \u{00a5}100 \u{2248} USD 1";
    app.input = task.into();

    assert_eq!(app.dispatch().as_deref(), Some(task));

    let displayed = app.transcript.last().unwrap();
    assert!(displayed.kind == TranscriptKind::Task);
    assert_eq!(displayed.text, task);
}

#[test]
fn ascii_mode_still_maps_nosis_chrome() {
    let mut app = test_app(None);
    app.terminal_capability = TerminalCapability::AsciiFallback;

    app.push_line(
        "chrome \u{00b7} \u{2192} \u{00a5} \u{2248}",
        TranscriptKind::Progress,
    );

    assert_eq!(app.transcript.last().unwrap().text, "chrome - > Y ~");
}

#[test]
fn ascii_mode_preserves_auto_approved_command_content() {
    let mut app = test_app(None);
    app.terminal_capability = TerminalCapability::AsciiFallback;
    app.status = Status::Working;
    let command = "compare \u{00a5}100 \u{2192} result \u{2248} USD 1";
    app.session_allow.push(command.into());
    let (event, answer) = approval(command);

    apply_event(&mut app, event);

    assert!(answer.recv().unwrap());
    assert_eq!(
        app.transcript.last().unwrap().text,
        format!("auto-approved (session rule): {command}")
    );
}

#[test]
fn unicode_mode_preserves_existing_main_transcript_and_overlay_glyphs() {
    let mut app = test_app(None);
    let main = render_buffer(&app, 100, 30);
    let main_text = buffer_text(&main);
    assert_eq!(main[(0, 0)].symbol(), "\u{250c}");
    assert_eq!(main[(99, 29)].symbol(), "\u{2518}");
    assert!(main_text.contains("\u{25cb} IDLE"), "got: {main_text}");
    assert!(
        main_text.contains("\u{2191}\u{2193} scroll"),
        "got: {main_text}"
    );
    assert!(main_text.contains('\u{2500}'), "got: {main_text}");

    app.push_line("test task", TranscriptKind::Task);
    app.push_line("test answer", TranscriptKind::Answer);
    app.push_line("test progress", TranscriptKind::Progress);
    app.status = Status::Blocked("approval required".into());
    let transcript_text = buffer_text(&render_buffer(&app, 100, 30));
    assert!(
        transcript_text.contains("\u{276f} you"),
        "got: {transcript_text}"
    );
    assert!(
        transcript_text.contains("\u{25c6} nosis"),
        "got: {transcript_text}"
    );
    assert!(
        transcript_text.contains("\u{b7} test progress"),
        "got: {transcript_text}"
    );
    assert!(
        transcript_text.contains("\u{25cf} BLOCKED"),
        "got: {transcript_text}"
    );

    for index in 0..40 {
        app.push_line(&format!("overflow line {index}"), TranscriptKind::Progress);
    }
    let newest = render_buffer(&app, 100, 30);
    let newest_text = buffer_text(&newest);
    assert!(newest_text.contains("\u{2191} more"), "got: {newest_text}");
    app.scroll_back = app.max_scroll.get();
    let oldest_text = buffer_text(&render_buffer(&app, 100, 30));
    assert!(oldest_text.contains("\u{2193} more"), "got: {oldest_text}");

    app.overlay = Overlay::Help;
    let overlay = render_buffer(&app, 100, 30);
    assert_plain_modal_ring(&overlay, modal_area(Rect::new(0, 0, 100, 30), 22));
    let overlay_text = buffer_text(&overlay);
    assert!(overlay_text.contains("Help"), "got: {overlay_text}");
}

#[test]
fn every_new_surface_scrubs_literals_and_control_characters() {
    let transcript_secret = "fake-key-transcript";
    let modal_secret = "fake-key-modal";
    let empty_literal = "Type a task and press Enter.";
    let hud_literal = "no price data";
    let scrubber = Arc::new(RwLock::new(Scrubber::new(vec![
        transcript_secret.into(),
        modal_secret.into(),
        empty_literal.into(),
        hud_literal.into(),
    ])));
    let mut app = App::new(
        test_resolver(),
        test_route(),
        None,
        scrubber,
        PolicyView {
            autonomy: Autonomy::Ask,
            auto_paths: Vec::new(),
            ask_paths: Vec::new(),
            block_paths: vec![format!("{modal_secret}\r\x1b[2K")],
            block_commands: Vec::new(),
        },
        UiInputs {
            terminal_capability: TerminalCapability::Unicode,
            palette_entries: Vec::new(),
            credentialed_providers: Vec::new(),
            color_mode: ColorMode::Color,
            route_timing_history: empty_timing_history(),
            prompt_base_tokens: unavailable_prompt_estimate(),
        },
        (Profiles::bundled(), "balanced".into()),
    );

    let empty = buffer_text(&render_buffer(&app, 100, 24));
    assert!(empty.matches("[REDACTED]").count() >= 2, "got: {empty}");
    assert!(!empty.contains(empty_literal), "got: {empty}");
    assert!(!empty.contains(hud_literal), "got: {empty}");

    apply_event(
        &mut app,
        AgentEvent::Progress(format!("value={transcript_secret}\r\x1b[2K")),
    );
    let transcript = buffer_text(&render_buffer(&app, 100, 24));
    assert!(transcript.contains("[REDACTED]"), "got: {transcript}");
    assert!(!transcript.contains(transcript_secret), "got: {transcript}");
    assert!(!transcript.contains('\r'), "got: {transcript}");
    assert!(!transcript.contains('\x1b'), "got: {transcript}");

    app.overlay = Overlay::TrustDial;
    let modal = buffer_text(&render_buffer(&app, 100, 24));
    assert!(modal.contains("[REDACTED]"), "got: {modal}");
    assert!(!modal.contains(modal_secret), "got: {modal}");
    assert!(!modal.contains('\r'), "got: {modal}");
    assert!(!modal.contains('\x1b'), "got: {modal}");
}

#[test]
fn reducer_drives_every_semaforo_transition() {
    let mut app = test_app(None);
    assert_eq!(app.status, Status::Idle);
    app.input = "first task".into();
    assert_eq!(app.dispatch().as_deref(), Some("first task"));
    assert_eq!(app.status, Status::Working);

    let (event, answer) = approval("cargo test");
    assert_eq!(apply_event(&mut app, event), &Status::Waiting);
    app.answer_approval(true);
    assert!(answer.recv().unwrap());
    assert_eq!(app.status, Status::Working);
    assert_eq!(
        apply_event(&mut app, AgentEvent::Answer("done".into())),
        &Status::Idle
    );
    assert!(app.transcript.iter().any(|line| {
        line.kind == TranscriptKind::Progress
            && line.text.contains("Result not independently verified")
    }));

    app.input = "second task".into();
    app.dispatch().unwrap();
    assert_eq!(
        apply_event(&mut app, AgentEvent::Failed("offline".into())),
        &Status::Blocked("offline".into())
    );
    app.input = "retry".into();
    app.dispatch().unwrap();
    assert_eq!(app.status, Status::Working);
}

#[test]
fn working_enter_queues_an_editable_task_and_idle_dispatches_it_exactly_once() {
    let mut app = test_app(None);
    app.status = Status::Working;
    type_text(&mut app, "next task");

    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );
    assert_eq!(app.status, Status::Working);
    assert_eq!(app.input, "next task");
    assert!(app.pending_send);
    assert!(app
        .transcript
        .iter()
        .all(|line| line.kind != TranscriptKind::Task));

    type_text(&mut app, "!");
    reduce_key(&mut app, code_key(KeyCode::Backspace));
    assert_eq!(app.input, "next task");
    assert!(app.pending_send);

    let queued = render_buffer(&app, 90, 20);
    let (queued_x, queued_y) = find_ascii_text(&queued, "[queued]");
    assert_eq!(queued[(queued_x, queued_y)].fg, Color::Yellow);
    assert!(queued[(queued_x, queued_y)]
        .modifier
        .contains(Modifier::BOLD));

    let (previous, action) =
        reduce_agent_event(&mut app, AgentEvent::Answer("current done".into()));
    assert_eq!(previous, Status::Working);
    assert_eq!(action, UiAction::Dispatch("next task".into()));
    assert_eq!(app.status, Status::Working);
    assert!(app.input.is_empty());
    assert!(!app.pending_send);
    assert_eq!(
        app.transcript
            .iter()
            .filter(|line| line.kind == TranscriptKind::Task)
            .count(),
        1
    );

    let (_, repeated) = reduce_agent_event(&mut app, AgentEvent::Progress("late event".into()));
    assert_eq!(repeated, UiAction::None);
    assert_eq!(
        app.transcript
            .iter()
            .filter(|line| line.kind == TranscriptKind::Task)
            .count(),
        1
    );
}

#[test]
fn cancelled_turn_records_measured_cost_before_dispatching_the_queue() {
    let mut app = meter_app();
    app.status = Status::FinishingInterrupted;
    app.input = "next task".into();
    app.pending_send = true;
    let usage = Usage {
        prompt_tokens: 100,
        completion_tokens: 20,
        cached_tokens: Some(40),
        evidence: UsageEvidence::Measured,
    };
    let (_, usage_action) = reduce_agent_event(&mut app, AgentEvent::Usage(usage.clone()));
    assert_eq!(usage_action, UiAction::None);
    assert_eq!(app.status, Status::FinishingInterrupted);

    let mut cancelled_receipt = receipt("interrupted", Outcome::Pass, Some(usage));
    cancelled_receipt.kind = ReceiptKind::CancelledTurn;
    let (previous, action) = reduce_agent_event(
        &mut app,
        AgentEvent::CancelledTurn(TimelineSummary {
            route_id: "meter-route".into(),
            receipt: cancelled_receipt,
            answer: "cancelled before the answer was shown".into(),
        }),
    );

    assert_eq!(previous, Status::FinishingInterrupted);
    assert_eq!(action, UiAction::Dispatch("next task".into()));
    assert_eq!(app.status, Status::Working);
    assert_eq!(app.timeline.len(), 1);
    assert_eq!(app.timeline[0].kind, ReceiptKind::CancelledTurn);
    assert_eq!(app.timeline[0].tokens(), Some((100, 20, Some(40))));
    assert!(!app.session_cost.is_empty());
    assert!(app.transcript.iter().any(|line| line
        .text
        .contains("provider may still bill the completed request")));
}

#[test]
fn pending_send_while_working_renders_plain_queued_marker() {
    let mut app = test_app(None);
    app.status = Status::Working;
    type_text(&mut app, "next task");
    reduce_key(&mut app, code_key(KeyCode::Enter));

    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(rendered.contains("[queued] next task"), "got: {rendered}");
    assert!(!rendered.contains("[queued -"), "got: {rendered}");
}

#[test]
fn failed_turn_queue_prompts_for_enter_and_enter_dispatches() {
    let mut app = test_app(None);
    app.status = Status::Working;
    type_text(&mut app, "retry");
    reduce_key(&mut app, code_key(KeyCode::Enter));

    let (previous, event_action) =
        reduce_agent_event(&mut app, AgentEvent::Failed("offline".into()));
    assert_eq!(previous, Status::Working);
    assert_eq!(event_action, UiAction::None);
    assert_eq!(app.status, Status::Blocked("offline".into()));
    assert_eq!(app.input, "retry");
    assert!(app.pending_send);
    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(
        rendered.contains("[queued - press Enter] retry"),
        "got: {rendered}"
    );

    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::Dispatch("retry".into())
    );
    assert_eq!(app.status, Status::Working);
    assert!(app.input.is_empty());
    assert!(!app.pending_send);
    assert_eq!(
        app.transcript
            .iter()
            .filter(|line| line.kind == TranscriptKind::Task)
            .count(),
        1
    );
}

#[test]
fn longer_blocked_queue_marker_places_cursor_after_input() {
    let mut app = test_app(None);
    app.status = Status::Working;
    type_text(&mut app, "retry");
    reduce_key(&mut app, code_key(KeyCode::Enter));
    reduce_agent_event(&mut app, AgentEvent::Failed("offline".into()));
    let backend = TestBackend::new(90, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();

    let rendered_input = "[queued - press Enter] retry";
    let (input_x, input_y) = find_ascii_text(terminal.backend().buffer(), rendered_input);
    let cursor = terminal.backend().cursor_position();
    assert_eq!(
        (cursor.x, cursor.y),
        (
            input_x + u16::try_from(rendered_input.len()).unwrap(),
            input_y
        )
    );
}

#[test]
fn budget_blocked_queue_reports_budget_and_enter_does_not_dispatch() {
    let mut app = test_app(Some(100));
    app.status = Status::Working;
    type_text(&mut app, "must not run");
    reduce_key(&mut app, code_key(KeyCode::Enter));

    let (_, usage_action) = reduce_agent_event(
        &mut app,
        AgentEvent::Usage(Usage {
            prompt_tokens: 80,
            completion_tokens: 20,
            cached_tokens: None,
            evidence: UsageEvidence::Measured,
        }),
    );
    assert_eq!(usage_action, UiAction::None);
    assert_eq!(app.status, Status::Blocked(BUDGET_REASON.into()));
    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(
        rendered.contains("[queued - budget reached] must not run"),
        "got: {rendered}"
    );

    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );
    assert_eq!(app.input, "must not run");
    assert!(app.pending_send);
    assert!(app
        .transcript
        .iter()
        .all(|line| line.kind != TranscriptKind::Task));
}

#[test]
fn clearing_or_blank_queued_input_never_dispatches() {
    let mut app = test_app(None);
    app.status = Status::Working;
    type_text(&mut app, "x");
    reduce_key(&mut app, code_key(KeyCode::Enter));
    assert!(app.pending_send);

    reduce_key(&mut app, code_key(KeyCode::Backspace));
    assert!(app.input.is_empty());
    assert!(!app.pending_send);
    assert!(!buffer_text(&render_buffer(&app, 90, 20)).contains("[queued"));
    let (_, action) = reduce_agent_event(&mut app, AgentEvent::Answer("done".into()));
    assert_eq!(action, UiAction::None);
    assert!(app
        .transcript
        .iter()
        .all(|line| line.kind != TranscriptKind::Task));

    let mut whitespace = test_app(None);
    whitespace.status = Status::Working;
    type_text(&mut whitespace, "   ");
    reduce_key(&mut whitespace, code_key(KeyCode::Enter));
    assert!(!whitespace.pending_send);
    let (_, action) = reduce_agent_event(&mut whitespace, AgentEvent::Answer("done".into()));
    assert_eq!(action, UiAction::None);
    assert_eq!(whitespace.input, "   ");
    assert!(whitespace
        .transcript
        .iter()
        .all(|line| line.kind != TranscriptKind::Task));
}

#[test]
fn queued_slash_input_uses_the_command_path_instead_of_becoming_a_task() {
    let mut app = test_app(None);
    app.status = Status::Working;
    type_text(&mut app, "/typo");
    assert_eq!(app.overlay, Overlay::None);
    reduce_key(&mut app, code_key(KeyCode::Enter));

    let (_, action) = reduce_agent_event(&mut app, AgentEvent::Answer("done".into()));

    assert_eq!(action, UiAction::None);
    assert!(app.input.is_empty());
    assert!(!app.pending_send);
    assert!(app
        .transcript
        .iter()
        .any(|line| line.text.contains("unknown command")));
    assert!(app
        .transcript
        .iter()
        .all(|line| line.kind != TranscriptKind::Task));
}

#[test]
fn approval_forwards_yes_and_no_then_returns_to_working() {
    for approved in [true, false] {
        let mut app = test_app(None);
        app.status = Status::Working;
        let (event, answer) = approval("edit src/lib.rs");
        apply_event(&mut app, event);
        assert_eq!(app.status, Status::Waiting);
        app.answer_approval(approved);
        assert_eq!(answer.recv().unwrap(), approved);
        assert_eq!(app.status, Status::Working);
    }
}

#[test]
fn approval_reducer_accepts_only_explicit_choices() {
    for (key, expected) in [
        (KeyCode::F(2), true),
        (KeyCode::F(3), true),
        (KeyCode::F(4), false),
    ] {
        let mut app = test_app(None);
        app.status = Status::Working;
        type_text(&mut app, "queued draft");
        reduce_key(&mut app, code_key(KeyCode::Enter));
        let (event, answer) = approval("cargo test --workspace");
        apply_event(&mut app, event);
        assert_eq!(reduce_key(&mut app, code_key(key)), UiAction::None);
        assert_eq!(answer.recv().unwrap(), expected);
        assert!(app.pending_approval.is_none());
        assert_eq!(app.status, Status::Working);
        assert_eq!(app.input, "queued draft");
        assert!(app.pending_send);
    }
}

#[test]
fn typing_paste_and_enter_across_approval_arrival_preserve_the_queued_draft() {
    let mut app = test_app(None);
    app.status = Status::Working;
    type_text(&mut app, "next ");
    let (event, answer) = approval("exec cargo test --workspace");
    apply_event(&mut app, event);

    type_text(&mut app, "yan");
    assert_eq!(
        reduce_input_event(&mut app, Event::Paste(" pasted".into())),
        UiAction::None
    );
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );

    assert_eq!(answer.try_recv(), Err(TryRecvError::Empty));
    assert!(app.pending_approval.is_some());
    assert_eq!(app.status, Status::Waiting);
    assert_eq!(app.input, "next yan pasted");
    assert!(app.pending_send);

    reduce_key(&mut app, code_key(KeyCode::F(4)));
    assert!(!answer.recv().unwrap());
    assert_eq!(app.status, Status::Working);
    assert_eq!(app.input, "next yan pasted");
    assert!(app.pending_send);

    let (_, first) = reduce_agent_event(&mut app, AgentEvent::Answer("finished".into()));
    assert_eq!(first, UiAction::Dispatch("next yan pasted".into()));
    assert!(app.input.is_empty());
    assert!(!app.pending_send);
    let (_, second) = reduce_agent_event(&mut app, AgentEvent::Answer("finished again".into()));
    assert_eq!(second, UiAction::None);
}

#[test]
fn escape_declines_approval_and_interrupts_the_turn_without_firing_the_queue() {
    let mut app = test_app(None);
    app.status = Status::Working;
    type_text(&mut app, "queued draft");
    reduce_key(&mut app, code_key(KeyCode::Enter));
    let (event, answer) = approval("cargo test --workspace");
    apply_event(&mut app, event);

    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Esc)),
        UiAction::Interrupt
    );
    assert!(!answer.recv().unwrap());
    assert_eq!(app.status, Status::FinishingInterrupted);
    assert_eq!(app.input, "queued draft");
    assert!(app.pending_send);
    assert!(app.pending_approval.is_none());
    assert!(app
        .transcript
        .iter()
        .all(|line| line.kind != TranscriptKind::Task));
}

#[test]
fn ctrl_c_uses_interrupt_clear_and_double_press_steps() {
    let mut working = test_app(None);
    working.status = Status::Working;
    assert_eq!(reduce_key(&mut working, ctrl_key('c')), UiAction::Interrupt);
    assert_eq!(working.status, Status::FinishingInterrupted);
    assert!(working.last_ctrl_c.is_none());

    working.input = "draft".into();
    working.pending_send = true;
    assert_eq!(reduce_key(&mut working, ctrl_key('c')), UiAction::None);
    assert!(working.input.is_empty());
    assert!(!working.pending_send);
    assert!(working.last_ctrl_c.is_none());

    assert_eq!(reduce_key(&mut working, ctrl_key('c')), UiAction::None);
    assert!(working.last_ctrl_c.is_some());
    assert_eq!(reduce_key(&mut working, ctrl_key('c')), UiAction::Quit);
}

#[test]
fn ctrl_c_exit_arm_expires_and_other_keys_disarm_it() {
    let mut app = test_app(None);
    app.last_ctrl_c = Some(Instant::now() - CTRL_C_EXIT_WINDOW - Duration::from_millis(1));
    assert_eq!(reduce_key(&mut app, ctrl_key('c')), UiAction::None);
    assert!(app.last_ctrl_c.is_some());

    assert_eq!(reduce_key(&mut app, char_key('x')), UiAction::None);
    assert!(app.last_ctrl_c.is_none());
    app.input.clear();
    assert_eq!(reduce_key(&mut app, ctrl_key('c')), UiAction::None);
}

#[test]
fn ctrl_c_declines_a_pending_approval_before_interrupting() {
    let mut app = test_app(None);
    app.status = Status::Working;
    let (event, answer) = approval("cargo test --workspace");
    apply_event(&mut app, event);

    assert_eq!(reduce_key(&mut app, ctrl_key('c')), UiAction::Interrupt);
    assert!(!answer.recv().unwrap());
    assert!(app.pending_approval.is_none());
    assert_eq!(app.status, Status::FinishingInterrupted);
}

#[test]
fn approval_reducer_ignores_modified_function_keys() {
    for (code, modifiers) in [
        (KeyCode::F(2), KeyModifiers::SHIFT),
        (KeyCode::F(3), KeyModifiers::CONTROL),
        (KeyCode::F(4), KeyModifiers::ALT),
    ] {
        let mut app = test_app(None);
        app.status = Status::Working;
        let (event, answer) = approval("exec cargo test --workspace");
        apply_event(&mut app, event);

        let key = KeyEvent::new(code, modifiers);
        assert_eq!(reduce_key(&mut app, key), UiAction::None);
        assert_eq!(answer.try_recv(), Err(TryRecvError::Empty));
        assert!(app.pending_approval.is_some());
        assert_eq!(app.status, Status::Waiting);
    }

    for character in ['a', 'y'] {
        let mut app = test_app(None);
        app.status = Status::Working;
        let (event, answer) = approval("exec cargo test --workspace");
        apply_event(&mut app, event);

        assert_eq!(reduce_key(&mut app, ctrl_key(character)), UiAction::None);
        assert_eq!(answer.try_recv(), Err(TryRecvError::Empty));
        assert!(app.pending_approval.is_some());
        assert_eq!(app.status, Status::Waiting);
    }
}

#[test]
fn non_press_approval_key_events_are_ignored() {
    for (code, kind) in [
        (KeyCode::F(2), crossterm::event::KeyEventKind::Repeat),
        (KeyCode::F(4), crossterm::event::KeyEventKind::Release),
    ] {
        let mut app = test_app(None);
        app.status = Status::Working;
        let (event, answer) = approval("exec cargo test --workspace");
        apply_event(&mut app, event);

        let key = KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind);
        assert_eq!(
            reduce_input_event(&mut app, Event::Key(key)),
            UiAction::None
        );
        assert_eq!(answer.try_recv(), Err(TryRecvError::Empty));
        assert!(app.pending_approval.is_some());
        assert_eq!(app.status, Status::Waiting);
    }
}

#[test]
fn approval_interlude_preserves_the_task_heartbeat_origin() {
    let mut app = test_app(None);
    let started = fixed_at();
    app.set_status(Status::Working, started);
    let (event, answer) = approval("cargo test --workspace");
    apply_event(&mut app, event);

    assert_eq!(app.working_since, Some(started));
    reduce_key(&mut app, code_key(KeyCode::F(2)));
    assert!(answer.recv().unwrap());
    assert_eq!(app.working_since, Some(started));

    apply_event(&mut app, AgentEvent::Answer("done".into()));
    assert_eq!(app.working_since, None);
}

#[test]
fn approval_always_records_and_auto_approves_identical_action() {
    let mut app = test_app(None);
    app.status = Status::Working;
    let action = "exec cargo test --workspace";
    let (first, first_answer) = approval(action);
    apply_event(&mut app, first);
    reduce_key(&mut app, code_key(KeyCode::F(3)));
    assert!(first_answer.recv().unwrap());
    assert_eq!(app.session_allow, vec![action]);

    let (second, second_answer) = approval(action);
    apply_event(&mut app, second);
    assert!(second_answer.recv().unwrap());
    assert!(app.pending_approval.is_none());
    assert_eq!(app.status, Status::Working);
    assert!(app
        .transcript
        .iter()
        .any(|line| line.text.contains("auto-approved (session rule)")));
}

#[test]
fn ineligible_approval_cannot_save_or_match_a_session_repeat_rule() {
    let mut app = test_app(None);
    app.status = Status::Working;
    let display = "exec deploy [REDACTED]";
    let (first, first_answer) = approval_once(display);
    apply_event(&mut app, first);

    let rendered = buffer_text(&render_buffer(&app, 80, 24));
    assert!(rendered.contains("[F2] approve once"), "got: {rendered}");
    assert!(!rendered.contains("[F3]"), "got: {rendered}");
    assert!(!rendered.contains("F3 repeat"), "got: {rendered}");
    reduce_key(&mut app, code_key(KeyCode::F(3)));
    assert_eq!(first_answer.try_recv(), Err(TryRecvError::Empty));
    assert!(app.pending_approval.is_some());
    assert!(app.session_allow.is_empty());
    assert_eq!(app.status, Status::Waiting);
    assert_eq!(
        app.transcript.last().unwrap().text,
        "repeat unavailable; choose F2 to approve once or F4 to decline"
    );
    reduce_key(&mut app, code_key(KeyCode::F(2)));
    assert!(first_answer.recv().unwrap());

    app.session_allow.push(display.into());
    let (second, second_answer) = approval_once(display);
    apply_event(&mut app, second);
    assert_eq!(second_answer.try_recv(), Err(TryRecvError::Empty));
    assert!(app.pending_approval.is_some());
    assert_eq!(app.status, Status::Waiting);

    reduce_key(&mut app, code_key(KeyCode::F(1)));
    let help = rendered_help_pages(&mut app, 80, 24);
    assert!(
        help.contains("F2 / F4") && help.contains("repeat unavailable"),
        "got: {help}"
    );
    assert!(!help.contains("F2 / F3 / F4"), "got: {help}");
}

#[test]
fn approval_row_names_the_command_and_is_amber_reversed() {
    let mut app = test_app(None);
    app.status = Status::Working;
    let (event, _answer) = approval("cargo test --workspace");
    apply_event(&mut app, event);

    let buffer = render_buffer(&app, 100, 20);
    let text = buffer_text(&buffer);
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let (x, y) = find_ascii_text(&buffer, "approve:");
    let cell = &buffer[(x, y)];
    assert!(
        normalized.contains("approve: cargo test --workspace"),
        "got: {text}"
    );
    assert!(normalized.contains("[F2] approve once"), "got: {text}");
    assert!(
        normalized.contains("[F3] repeat identical shell command this"),
        "got: {text}"
    );
    assert!(normalized.contains("[F4] decline"), "got: {text}");
    assert!(
        normalized.contains("[Esc] decline + stop turn"),
        "got: {text}"
    );
    assert!(!APPROVAL_LEGEND.contains("interrupt"));
    assert_eq!(cell.fg, Color::Yellow);
    assert!(cell.modifier.contains(Modifier::REVERSED));
}

#[test]
fn approval_row_keeps_the_full_action_and_visible_legend() {
    let mut app = test_app(None);
    app.status = Status::Working;
    let action = format!("exec {}", "x".repeat(700));
    let (event, _answer) = approval(&action);
    apply_event(&mut app, event);

    let line = app.transcript.last().unwrap();
    assert!(line.text.contains(&action));
    assert!(line.text.contains(APPROVAL_LEGEND));
    assert!(!line.text.contains("more chars"));
}

#[test]
fn long_approval_wraps_without_loss_and_waiting_scroll_keeps_the_choice_pending() {
    let markers = (0..80)
        .map(|index| format!("approval-segment-{index:03}"))
        .collect::<Vec<_>>();
    let action = format!("exec {}", markers.join(" "));

    for (width, height) in [(80, 24), (40, 20)] {
        let mut app = test_app(None);
        app.status = Status::Working;
        let (event, answer) = approval(&action);
        apply_event(&mut app, event);

        let rendered = rendered_waiting_transcript_pages(&mut app, width, height);
        let normalized = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
        for marker in &markers {
            assert!(
                rendered.contains(marker),
                "{width}x{height} missing {marker:?}: {rendered}"
            );
        }
        for legend in [
            "[F2]",
            "approve",
            "once",
            "[F3]",
            "repeat",
            "identical",
            "shell",
            "command",
            "this",
            "session",
            "[F4]",
            "decline",
            "[Esc]",
            "stop",
            "turn",
        ] {
            assert!(
                normalized.contains(legend),
                "{width}x{height} missing {legend:?}: {rendered}"
            );
        }
        let stored = app
            .transcript
            .iter()
            .find(|line| line.kind == TranscriptKind::Approval)
            .unwrap();
        assert!(stored.text.contains(&action));
        assert!(stored.text.contains(APPROVAL_LEGEND));
        assert!(!stored.text.contains('…'));
        assert_eq!(answer.try_recv(), Err(TryRecvError::Empty));

        let before = app.scroll_back;
        reduce_key(&mut app, code_key(KeyCode::PageDown));
        assert!(app.scroll_back < before);
        assert_eq!(answer.try_recv(), Err(TryRecvError::Empty));
        reduce_key(&mut app, code_key(KeyCode::F(2)));
        assert!(answer.recv().unwrap());
    }
}

#[test]
fn hud_at_width_79_uses_the_shared_ellipsis_fitter_and_wide_hud_is_unchanged() {
    let mut app = test_app(Some(1_000));
    let usage = Usage {
        prompt_tokens: 600,
        completion_tokens: 200,
        cached_tokens: Some(300),
        evidence: UsageEvidence::Measured,
    };
    app.usage = Some(usage.clone());
    app.last_request_usage = Some(usage);
    let full = app.hud_line(Utc::now());
    assert!(Line::from(full.as_str()).width() > 77);

    let marker = fit_with_ellipsis("long", 1);
    let narrow = render_buffer(&app, 79, 20);
    let narrow_rows = buffer_rows(&narrow);
    let narrow_y = narrow_rows
        .iter()
        .position(|row| row.contains("session"))
        .unwrap();
    assert_eq!(
        narrow[(77, u16::try_from(narrow_y).unwrap())].symbol(),
        marker
    );

    let wide = render_buffer(&app, 240, 20);
    let wide_rows = buffer_rows(&wide);
    let wide_y = wide_rows
        .iter()
        .position(|row| row.contains("session"))
        .unwrap();
    let wide_inner = (1..wide.area.width.saturating_sub(1))
        .map(|x| wide[(x, u16::try_from(wide_y).unwrap())].symbol())
        .collect::<String>();
    assert_eq!(wide_inner.trim_end(), full);
}

#[test]
fn scrub_full_line_escapes_bidi_strips_zero_width_and_does_not_truncate() {
    let scrubber = Arc::new(RwLock::new(Scrubber::new(Vec::new())));
    assert_eq!(
        scrub_full_line(&scrubber, "allow\u{202e}den\u{200b}y"),
        "allow\\u{202e}deny"
    );

    let long = "x".repeat(1_100);
    assert_eq!(scrub_full_line(&scrubber, &long), long);
}

#[test]
fn failed_event_renders_one_friendly_line_without_a_trace() {
    let mut app = test_app(None);
    app.status = Status::Working;
    apply_event(
        &mut app,
        AgentEvent::Failed("network unavailable\nstack backtrace:\n0: internal frame".into()),
    );

    assert!(matches!(
        &app.status,
        Status::Blocked(reason)
            if reason.starts_with("network unavailable") && reason.contains("backtrace")
    ));
    assert_eq!(app.transcript.len(), 1);
    assert_eq!(
        app.transcript[0].text,
        "! network unavailable - retry the task or type /help"
    );
    let text = buffer_text(&render_buffer(&app, 100, 20));
    assert!(text.contains("! network unavailable - retry the task"));
    assert!(!text.contains("backtrace"), "got: {text}");
    assert!(!text.contains("internal frame"), "got: {text}");
}

#[test]
fn unknown_billed_turn_refuses_session_money_and_token_numbers() {
    let mut app = meter_app();
    app.budget = Some(100);
    let usage = Usage {
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_tokens: None,
        evidence: UsageEvidence::Unknown,
    };
    apply_event(&mut app, AgentEvent::Usage(usage.clone()));
    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "meter-route".into(),
            receipt: receipt("unmetered", Outcome::Fail, Some(usage)),
            answer: "error".into(),
        }),
    );

    let money = app.session_money(fixed_at());
    assert_eq!(money, "unavailable - meter incomplete");
    let hud = app.hud_line(fixed_at());
    assert!(hud.contains("tokens unavailable - usage unknown"));
    assert!(hud.contains("budget usage unavailable/100"));
    assert!(!hud.contains("in 0"), "got: {hud}");
    assert_eq!(timeline_row(&app.timeline[0]), "#1  fail  usage unknown");
    assert_eq!(
        timeline_detail_lines(&app.timeline[0])[10],
        "tokens: unavailable - usage unknown"
    );
}

#[test]
fn partial_timeline_usage_is_a_lower_bound_without_a_cache_claim() {
    let entry = TimelineEntry::from_receipt(
        1,
        receipt(
            "partial meter",
            Outcome::Pass,
            Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 2,
                cached_tokens: Some(4),
                evidence: UsageEvidence::Partial,
            }),
        ),
        "done".into(),
        Default::default(),
    );

    assert_eq!(timeline_row(&entry), "#1  completed  ~10/~2 lower bound");
    let detail = &timeline_detail_lines(&entry)[10];
    assert_eq!(detail, "tokens: ~10 in / ~2 out - lower bound");
    assert!(!detail.contains("cache"));
}

#[test]
fn timeline_entry_projects_receipt_outcome_and_tokens() {
    let entry = TimelineEntry::from_receipt(
        7,
        receipt(
            "summarize the workspace",
            Outcome::Partial,
            Some(Usage {
                prompt_tokens: 120,
                completion_tokens: 30,
                cached_tokens: Some(50),
                evidence: UsageEvidence::Measured,
            }),
        ),
        "partial answer".into(),
        Default::default(),
    );

    assert_eq!(entry.turn, 7);
    assert_eq!(entry.outcome, Outcome::Partial);
    assert_eq!(entry.tokens(), Some((120, 30, Some(50))));
    assert_eq!(entry.turns, 3);
    assert_eq!(entry.tool_calls, 2);
    assert_eq!(entry.answer, "partial answer");
    assert!(!entry.compacted);
}

#[test]
fn completed_timeline_row_renders_measured_duration_without_an_estimate_marker() {
    let mut app = test_app(None);
    let mut completed = receipt("timed", Outcome::Pass, None);
    completed.duration_ms = Some(1_234);
    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "test-route".into(),
            receipt: completed,
            answer: "done".into(),
        }),
    );
    app.overlay = Overlay::Timeline {
        selected: 0,
        inspecting: true,
        note: None,
    };

    let rendered = buffer_text(&render_buffer(&app, 180, 20));

    assert!(
        rendered.contains("#1  completed  1.234s"),
        "got: {rendered}"
    );
    assert!(rendered.contains("duration: 1.234s"), "got: {rendered}");
    assert!(!rendered.contains("~1.234s"), "got: {rendered}");
}

#[test]
fn completed_timeline_row_without_duration_omits_it_instead_of_rendering_zero() {
    let mut app = test_app(None);
    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "test-route".into(),
            receipt: receipt("legacy", Outcome::Pass, None),
            answer: "done".into(),
        }),
    );
    app.overlay = Overlay::Timeline {
        selected: 0,
        inspecting: true,
        note: None,
    };

    let rendered = buffer_text(&render_buffer(&app, 180, 20));

    assert_eq!(
        timeline_row(&app.timeline[0]),
        "#1  completed  usage unreported"
    );
    assert!(!rendered.contains("duration:"), "got: {rendered}");
    assert!(!rendered.contains("  0s"), "got: {rendered}");
}

#[test]
fn typed_compaction_event_marks_only_the_current_timeline_turn() {
    let mut app = meter_app();
    app.status = Status::Working;
    apply_event(
        &mut app,
        AgentEvent::Compaction(CompactionEvent::new_at(
            73,
            8,
            40_000,
            None,
            fixed_at().timestamp(),
        )),
    );
    assert_eq!(app.current_task_compaction.events, 1);
    assert_eq!(app.current_task_compaction.messages_elided, 8);
    assert_eq!(app.current_task_compaction.estimated_tokens_elided, 40_000);
    assert!(app
        .transcript
        .last()
        .is_some_and(|line| line.text.contains("~40000 tokens elided")));
    apply_event(&mut app, AgentEvent::Answer("one".into()));
    apply_event(&mut app, timeline_event_on("meter-route", "first", "one"));
    assert!(app.timeline[0].compacted);
    assert_eq!(app.timeline[0].compaction.events, 1);
    assert!(app.last_compaction_hud.is_some());

    app.input = "second".into();
    assert_eq!(app.dispatch().as_deref(), Some("second"));
    assert!(app.last_compaction_hud.is_none());
    apply_event(
        &mut app,
        AgentEvent::Progress("context 73% - compacted 8 earlier messages".into()),
    );
    apply_event(&mut app, timeline_event("second", "two"));
    assert!(!app.timeline[1].compacted);
    assert!(app.last_compaction_hud.is_none());
}

#[test]
fn timeline_reducer_scrubs_and_enter_inspects_the_selected_turn() {
    let mut app = test_app(None);
    apply_event(&mut app, timeline_event("first", "answer one"));
    apply_event(&mut app, timeline_event("second", "answer two"));

    type_text(&mut app, "/timeline");
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );
    match &app.overlay {
        Overlay::Timeline { selected, .. } => assert_eq!(*selected, 1),
        _ => panic!("timeline must open"),
    }
    reduce_key(&mut app, code_key(KeyCode::Up));
    reduce_key(&mut app, code_key(KeyCode::Enter));
    match &app.overlay {
        Overlay::Timeline {
            selected,
            inspecting,
            ..
        } => {
            assert_eq!(*selected, 0);
            assert!(*inspecting);
            assert_eq!(app.timeline[*selected].answer, "answer one");
        }
        _ => panic!("timeline must stay open"),
    }
    assert_eq!(app.timeline.len(), 2);
    assert!(app.input.is_empty());
}

#[test]
fn printable_words_type_freely_without_opening_overlays() {
    for word in ["list", "trust", "quit", "R"] {
        let mut app = test_app(None);
        type_text(&mut app, word);

        assert_eq!(app.input, word);
        assert_eq!(app.overlay, Overlay::None);
        assert!(app.transcript.is_empty());
    }
}

#[test]
fn slash_opens_live_command_menu_and_mod_filter_surfaces_model() {
    let mut app = test_app(None);
    type_text(&mut app, "/mod");

    assert!(matches!(app.overlay, Overlay::CommandMenu { .. }));
    let matches = command_matches(&app);
    assert!(matches.iter().any(|entry| entry.name == "/model <id>"));
    let rendered = buffer_text(&render_buffer(&app, 90, 22));
    assert!(rendered.contains("Commands"), "got: {rendered}");
    assert!(rendered.contains("/model <id>"), "got: {rendered}");

    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );
    assert_eq!(app.input, "/model ");
    assert!(matches!(app.overlay, Overlay::CommandMenu { .. }));

    assert_eq!(reduce_key(&mut app, code_key(KeyCode::Esc)), UiAction::None);
    assert_eq!(app.overlay, Overlay::None);
    assert!(app.input.is_empty());
}

#[test]
fn command_menu_newline_shortcuts_never_execute_the_selected_command() {
    let mut app = test_app(None);
    type_text(&mut app, "/mod");

    for key in [
        KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
        ctrl_key('j'),
    ] {
        assert_eq!(reduce_key(&mut app, key), UiAction::None);
        assert_eq!(app.input, "/mod");
        assert!(matches!(app.overlay, Overlay::CommandMenu { .. }));
        assert!(app.transcript.is_empty());
    }

    assert_eq!(
        reduce_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)
        ),
        UiAction::None
    );
    assert_eq!(app.input, "/model ");
}

#[test]
fn paste_appends_to_input_without_dispatching() {
    let mut app = test_app(None);

    let action = reduce_input_event(&mut app, Event::Paste("foo bar".into()));

    assert_eq!(action, UiAction::None);
    assert_eq!(app.input, "foo bar");
    assert!(app.transcript.is_empty());
    assert_eq!(app.status, Status::Idle);
}

#[test]
fn paste_while_working_appends_to_the_queued_input_buffer() {
    let mut app = test_app(None);
    app.status = Status::Working;
    type_text(&mut app, "next ");

    let action = reduce_input_event(&mut app, Event::Paste("line\nfrom paste".into()));

    assert_eq!(action, UiAction::None);
    assert_eq!(app.input, "next line\nfrom paste");
    assert_eq!(app.overlay, Overlay::None);
    assert_eq!(app.status, Status::Working);
    assert!(app.transcript.is_empty());
    assert!(!app.pending_send);
    assert!(!buffer_text(&render_buffer(&app, 90, 20)).contains("[queued"));

    reduce_key(&mut app, code_key(KeyCode::Enter));
    reduce_input_event(&mut app, Event::Paste(" edited".into()));
    assert_eq!(app.input, "next line\nfrom paste edited");
    assert!(app.pending_send);
}

#[test]
fn multiline_paste_preserves_newlines_without_dispatching() {
    let mut app = test_app(None);

    let action = reduce_input_event(&mut app, Event::Paste("line1\nline2".into()));

    assert_eq!(action, UiAction::None);
    assert_eq!(app.input, "line1\nline2");
    assert!(app.transcript.is_empty());
    assert_eq!(app.status, Status::Idle);
}

#[test]
fn dispatched_multiline_prompt_renders_every_line_in_the_transcript() {
    let mut app = test_app(None);
    let task = "first line\nsecond line\nthird line";
    reduce_input_event(&mut app, Event::Paste(task.into()));

    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::Dispatch(task.to_owned())
    );
    let task_lines = app
        .transcript
        .iter()
        .filter(|line| line.kind == TranscriptKind::Task)
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(task_lines, ["first line", "second line", "third line"]);

    let rendered = buffer_text(&render_buffer(&app, 80, 20));
    for line in task.lines() {
        assert!(rendered.contains(line), "missing {line:?}: {rendered}");
    }
}

#[test]
fn shift_enter_and_ctrl_j_insert_newlines_while_plain_enter_submits() {
    let mut app = test_app(None);
    type_text(&mut app, "first");

    assert_eq!(
        reduce_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
        UiAction::None
    );
    type_text(&mut app, "second");
    assert_eq!(reduce_key(&mut app, ctrl_key('j')), UiAction::None);
    type_text(&mut app, "third");
    assert_eq!(app.input, "first\nsecond\nthird");
    assert!(app.transcript.is_empty());

    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::Dispatch("first\nsecond\nthird".to_owned())
    );
    assert!(app.input.is_empty());

    let mut control_enter = test_app(None);
    type_text(&mut control_enter, "send with control enter");
    assert_eq!(
        reduce_key(
            &mut control_enter,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)
        ),
        UiAction::Dispatch("send with control enter".to_owned())
    );
}

#[test]
fn composer_cursor_editing_is_utf8_safe_and_line_navigation_preserves_column() {
    let mut app = test_app(None);
    reduce_input_event(&mut app, Event::Paste("aé\nxy".into()));

    reduce_key(&mut app, code_key(KeyCode::Home));
    reduce_key(&mut app, code_key(KeyCode::Up));
    reduce_key(&mut app, code_key(KeyCode::Right));
    reduce_key(&mut app, code_key(KeyCode::Delete));
    reduce_key(&mut app, char_key('中'));
    assert_eq!(app.input, "a中\nxy");

    reduce_key(&mut app, code_key(KeyCode::Down));
    reduce_key(&mut app, code_key(KeyCode::Backspace));
    assert_eq!(app.input, "a中\nx");
}

#[test]
fn paste_normalizes_crlf_and_bare_cr_to_one_newline_each() {
    let mut app = test_app(None);

    reduce_input_event(&mut app, Event::Paste("one\r\ntwo\rthree".into()));

    assert_eq!(app.input, "one\ntwo\nthree");
    assert!(app.transcript.is_empty());
}

#[test]
fn multiline_composer_shows_at_most_five_rows_and_keeps_latest_visible_when_narrow() {
    let mut app = test_app(None);
    reduce_input_event(
        &mut app,
        Event::Paste("row1\nrow2\nrow3\nrow4\nrow5\nrow6\nrow7".into()),
    );

    let rendered = buffer_text(&render_buffer(&app, 24, 13));

    assert!(rendered.contains("row7"), "got: {rendered}");
    assert!(rendered.contains("row3"), "got: {rendered}");
    assert!(!rendered.contains("row2"), "got: {rendered}");
}

#[test]
fn typing_and_paste_never_grow_input_past_the_shared_task_limit() {
    let mut app = test_app(None);
    app.input = "x".repeat(MAX_TASK_BYTES - 1);

    reduce_input_event(&mut app, Event::Paste("yz".into()));
    assert_eq!(app.input.len(), MAX_TASK_BYTES);
    assert!(app.input.ends_with('y'));

    reduce_key(&mut app, code_key(KeyCode::Char('z')));
    assert_eq!(app.input.len(), MAX_TASK_BYTES);
    assert!(app.input.ends_with('y'));
}

#[test]
fn paste_updates_the_open_slash_menu_without_dispatching() {
    let mut app = test_app(None);
    type_text(&mut app, "/");
    assert!(matches!(app.overlay, Overlay::CommandMenu { .. }));

    let action = reduce_input_event(&mut app, Event::Paste("mod".into()));

    assert_eq!(action, UiAction::None);
    assert_eq!(app.input, "/mod");
    assert!(matches!(app.overlay, Overlay::CommandMenu { selected: 0 }));
    assert!(command_matches(&app)
        .iter()
        .any(|entry| entry.name == "/model <id>"));
    assert!(app.transcript.is_empty());
}

#[test]
fn bad_model_command_is_one_friendly_line_and_keeps_route() {
    let mut app = test_app(None);
    let original = app.route.id().to_owned();
    type_text(&mut app, "/model missing-route");

    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );

    assert_eq!(app.route.id(), original);
    assert_eq!(app.transcript.len(), 1);
    assert!(
        app.transcript[0]
            .text
            .contains("unknown model id 'missing-route'"),
        "got: {}",
        app.transcript[0].text
    );
    assert_eq!(app.overlay, Overlay::None);
    assert!(app.input.is_empty());
}

#[test]
fn bare_model_opens_an_honest_catalog_picker_and_uses_the_typed_switch_action() {
    let mut app = picker_app();
    app.push_line("conversation stays", TranscriptKind::Answer);
    let transcript = app
        .transcript
        .iter()
        .map(|line| line.text.clone())
        .collect::<Vec<_>>();

    type_text(&mut app, "/model");
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );
    let labels = match &app.overlay {
        Overlay::Picker {
            kind: PickerKind::Model,
            selected,
            rows,
        } => {
            assert_eq!(*selected, 0);
            rows.iter().map(|row| row.label.clone()).collect::<Vec<_>>()
        }
        other => panic!("expected model picker, got {other:?}"),
    };
    assert_eq!(labels.len(), 6);
    assert_eq!(labels[0], "a-cheap · capable · est $0.0001");
    assert_eq!(labels[1], "b-expensive · capable · est $0.0002");
    assert!(
        labels[2].contains("c-unknown-context · context unknown · free"),
        "got: {}",
        labels[2]
    );
    assert!(
        labels[3].contains("d-unknown-price · capable · price unknown"),
        "got: {}",
        labels[3]
    );
    assert!(
        labels[4].contains("est ¥0.0003 · fx stale · comparison refused"),
        "got: {}",
        labels[4]
    );
    assert!(
        labels[5].contains("local endpoint · explicit selection only · billing unknown"),
        "got: {}",
        labels[5]
    );
    assert!(labels
        .iter()
        .all(|label| !label.contains("cheapest") && !label.contains("x price")));
    let rendered = buffer_text(&render_buffer(&app, 100, 24));
    assert!(rendered.contains("Select model"), "got: {rendered}");

    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Down)),
        UiAction::None
    );
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::SwitchRoute("b-expensive".into())
    );
    assert_eq!(
        app.transcript
            .iter()
            .map(|line| line.text.clone())
            .collect::<Vec<_>>(),
        transcript
    );

    let mut typed = picker_app();
    type_text(&mut typed, "/model b-expensive");
    assert_eq!(
        reduce_key(&mut typed, code_key(KeyCode::Enter)),
        UiAction::SwitchRoute("b-expensive".into())
    );

    let mut local = picker_app();
    type_text(&mut local, "/model f-local");
    assert_eq!(
        reduce_key(&mut local, code_key(KeyCode::Enter)),
        UiAction::SwitchRoute("f-local".into())
    );
}

#[test]
fn bare_provider_lists_only_credentialed_providers_and_selects_their_default() {
    let mut app = picker_app();
    type_text(&mut app, "/provider");
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );

    let labels = match &app.overlay {
        Overlay::Picker {
            kind: PickerKind::Provider,
            rows,
            ..
        } => rows.iter().map(|row| row.label.clone()).collect::<Vec<_>>(),
        other => panic!("expected provider picker, got {other:?}"),
    };
    assert_eq!(labels.len(), 2);
    assert!(labels[0].contains("alpha · a-cheap · credential available"));
    assert!(labels[1].contains("beta · b-expensive · credential available"));
    assert!(labels.iter().all(|line| !line.contains("gamma")));

    reduce_key(&mut app, code_key(KeyCode::Down));
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::SwitchRoute("b-expensive".into())
    );
}

#[test]
fn bare_profile_lists_three_profiles_and_escape_is_a_no_op() {
    let mut app = picker_app();
    type_text(&mut app, "/profile");
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );
    match &app.overlay {
        Overlay::Picker {
            kind: PickerKind::Profile,
            selected,
            rows,
        } => {
            assert_eq!(*selected, 1);
            assert_eq!(
                rows.iter()
                    .map(|row| row.value.as_str())
                    .collect::<Vec<_>>(),
                ["frugal", "balanced", "max-quality"]
            );
        }
        other => panic!("expected profile picker, got {other:?}"),
    }
    let before = app.active_profile.clone();
    assert_eq!(reduce_key(&mut app, code_key(KeyCode::Esc)), UiAction::None);
    assert_eq!(app.overlay, Overlay::None);
    assert_eq!(app.active_profile, before);

    type_text(&mut app, "/profile");
    reduce_key(&mut app, code_key(KeyCode::Enter));
    reduce_key(&mut app, code_key(KeyCode::Down));
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::SetProfile("max-quality".into())
    );
    assert_eq!(app.active_profile, "max-quality");
}

#[test]
fn effort_command_sets_header_and_invalid_value_shows_usage() {
    let mut app = test_app(None);
    type_text(&mut app, "/effort High");
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::SetEffort(ThinkingEffort::High)
    );
    app.set_effort(ThinkingEffort::High);
    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(rendered.contains("effort: high"), "got: {rendered}");

    type_text(&mut app, "/effort MAX");
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::SetEffort(ThinkingEffort::Max)
    );
    app.set_effort(ThinkingEffort::Max);

    let before = app.transcript.len();
    type_text(&mut app, "/effort extreme");
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );
    assert_eq!(app.transcript.len(), before + 1);
    assert_eq!(
        app.transcript.last().map(|line| line.text.as_str()),
        Some("unknown reasoning effort - use /effort <none|low|high|max>")
    );
    assert_eq!(app.effort, ThinkingEffort::Max);
}

#[test]
fn profile_command_updates_active_profile_and_hud_chip() {
    let mut app = test_app(None);
    assert!(app.hud_line(fixed_at()).contains("profile balanced"));

    type_text(&mut app, "/profile frugal");
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::SetProfile("frugal".into())
    );
    assert_eq!(app.active_profile, "frugal");
    assert!(app.hud_line(fixed_at()).contains("profile frugal"));
    assert_eq!(
        app.transcript.last().map(|line| line.text.as_str()),
        Some("profile frugal - next turn: thinking none · max output 16384")
    );
}

#[test]
fn unknown_profile_is_a_calm_no_op() {
    let mut app = test_app(None);
    let before = app.active_profile.clone();

    type_text(&mut app, "/profile extravagant");
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );

    assert_eq!(app.active_profile, before);
    assert_eq!(
        app.transcript.last().map(|line| line.text.as_str()),
        Some("unknown profile 'extravagant' - use /profile <frugal|balanced|max-quality>")
    );
}

#[test]
fn slash_lines_are_commands_never_dispatched_as_tasks() {
    let mut app = test_app(None);
    type_text(&mut app, "/typo");

    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );
    assert_eq!(app.transcript.len(), 1);
    assert_eq!(
        app.transcript[0].text,
        "unknown command - type / to see all"
    );
    assert!(app
        .transcript
        .iter()
        .all(|line| line.kind != TranscriptKind::Task));
}

#[test]
fn keyboard_arrows_pages_and_end_control_transcript_scroll() {
    let mut app = test_app(None);
    for index in 0..20 {
        app.push_line(&format!("line {index}"), TranscriptKind::Progress);
    }
    let _ = render_buffer(&app, 80, 16);
    assert!(app.max_scroll.get() > 0);
    assert_eq!(app.scroll_back, 0);

    reduce_key(&mut app, code_key(KeyCode::Up));
    assert_eq!(app.scroll_back, 1);
    reduce_key(&mut app, code_key(KeyCode::Down));
    assert_eq!(app.scroll_back, 0);

    reduce_key(&mut app, code_key(KeyCode::PageUp));
    assert_eq!(app.scroll_back, 5);
    reduce_key(&mut app, code_key(KeyCode::PageDown));
    assert_eq!(app.scroll_back, 0);

    app.scroll_back = 9;
    reduce_key(&mut app, code_key(KeyCode::End));
    assert_eq!(app.scroll_back, 0);
}

#[test]
fn working_state_keeps_all_scroll_keys_live_while_text_remains_editable() {
    let mut app = test_app(None);
    app.status = Status::Working;
    app.max_scroll.set(20);
    app.input = "unchanged".into();
    app.prompt_history.push("remembered prompt".into());

    reduce_key(&mut app, code_key(KeyCode::PageUp));
    assert_eq!(app.scroll_back, 5);
    reduce_key(&mut app, code_key(KeyCode::Up));
    assert_eq!(app.scroll_back, 6);
    reduce_key(&mut app, code_key(KeyCode::Down));
    assert_eq!(app.scroll_back, 5);
    reduce_key(&mut app, code_key(KeyCode::PageDown));
    assert_eq!(app.scroll_back, 0);
    assert_eq!(app.input, "unchanged");
    assert!(app.prompt_history_index.is_none());

    reduce_key(&mut app, char_key('x'));
    assert_eq!(app.input, "unchangedx");
    reduce_key(&mut app, code_key(KeyCode::Backspace));
    assert_eq!(app.input, "unchanged");
}

#[test]
fn waiting_approval_keeps_multiline_navigation_and_transcript_paging_distinct() {
    let mut app = test_app(None);
    app.status = Status::Working;
    app.max_scroll.set(20);
    app.input = "top\nbottom".into();
    let (event, answer) = approval("exec cargo test --workspace");
    apply_event(&mut app, event);

    reduce_key(&mut app, code_key(KeyCode::Up));
    assert_eq!(app.input_cursor_index(), 3);
    assert_eq!(app.scroll_back, 0);
    reduce_key(&mut app, code_key(KeyCode::Down));
    assert_eq!(app.input_cursor_index(), 7);
    reduce_key(&mut app, code_key(KeyCode::PageUp));
    assert_eq!(app.scroll_back, 5);
    reduce_key(&mut app, code_key(KeyCode::PageDown));
    assert_eq!(app.scroll_back, 0);
    reduce_key(&mut app, code_key(KeyCode::End));
    assert_eq!(app.input_cursor_index(), app.input.len());

    app.input.clear();
    app.input_cursor = None;
    app.scroll_back = 5;
    reduce_key(&mut app, code_key(KeyCode::End));
    assert_eq!(app.scroll_back, 0);
    assert_eq!(answer.try_recv(), Err(TryRecvError::Empty));
    assert!(app.pending_approval.is_some());
}

#[test]
fn wrapped_rows_matches_word_wrap_and_keeps_the_newest_line_reachable() {
    let lines = vec![Line::from("123456 123456 123456"), Line::from("newest")];
    assert_eq!(wrapped_rows(&lines, 10), 4);

    let (scroll, max_scroll, overflow) = transcript_scroll_state(&lines, Rect::new(0, 0, 10, 2), 0);
    assert_eq!(scroll, 2);
    assert_eq!(max_scroll, 2);
    assert!(overflow.above);
    assert!(!overflow.below);

    let backend = TestBackend::new(10, 2);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(
                Paragraph::new(lines.clone())
                    .wrap(Wrap { trim: false })
                    .scroll((u16::try_from(scroll).unwrap(), 0)),
                frame.area(),
            );
        })
        .unwrap();
    assert!(buffer_text(terminal.backend().buffer()).contains("newest"));
}

#[test]
fn task_and_answer_wrap_with_a_hanging_indent_at_79_and_a_wider_width() {
    for width in [79, 111] {
        for (kind, word) in [
            (TranscriptKind::Task, "taskword"),
            (TranscriptKind::Answer, "answerword"),
        ] {
            let mut app = test_app(None);
            app.push_line(&format!("{word} ").repeat(40), kind);
            let buffer = render_buffer(&app, width, 30);
            let positions = buffer_rows(&buffer)
                .into_iter()
                .filter_map(|row| row.find(word).map(|byte_x| row[..byte_x].chars().count()))
                .collect::<Vec<_>>();

            assert!(positions.len() >= 3, "width {width}: {positions:?}");
            assert!(
                positions.iter().all(|position| *position == 4),
                "width {width}: {positions:?}"
            );
        }
    }
}

#[test]
fn speaker_group_separator_renders_blank_after_wrapped_task() {
    let mut app = test_app(None);
    app.push_line(
        &format!("{}wraptail", "taskword ".repeat(10)),
        TranscriptKind::Task,
    );
    app.push_line("answer", TranscriptKind::Answer);

    let width = 32;
    let buffer = render_buffer(&app, width, 20);
    let rows = buffer_rows(&buffer);
    let task_positions = rows
        .iter()
        .filter_map(|row| {
            row.find("taskword")
                .map(|byte_x| row[..byte_x].chars().count())
        })
        .collect::<Vec<_>>();
    let (_, tail_y) = find_ascii_text(&buffer, "wraptail");
    let (_, answer_header_y) = find_ascii_text(&buffer, "◆ nosis");

    assert!(task_positions.len() >= 2, "got: {rows:?}");
    assert!(
        task_positions.iter().all(|position| *position == 4),
        "got: {task_positions:?}"
    );
    assert_eq!(answer_header_y, tail_y.saturating_add(2));
    let separator_y = tail_y.saturating_add(1);
    for x in 1..width.saturating_sub(1) {
        assert_eq!(
            buffer[(x, separator_y)].symbol(),
            " ",
            "separator cell at x={x}, y={separator_y}"
        );
    }
}

#[test]
fn speaker_group_separator_survives_as_first_visible_scrolled_row() {
    let mut app = test_app(None);
    app.push_line(
        &format!("{}wraptail", "taskword ".repeat(10)),
        TranscriptKind::Task,
    );
    for answer in ["answer one", "answer two", "answer three"] {
        app.push_line(answer, TranscriptKind::Answer);
    }
    app.scroll_back = 2;

    let width = 32;
    let buffer = render_buffer(&app, width, 9);
    let rows = buffer_rows(&buffer);
    let first_transcript_y = 1_u16;
    let overflow_marker_x = width.saturating_sub(1).saturating_sub(6);

    assert!(app.max_scroll.get() > app.scroll_back);
    assert!(rows[usize::from(first_transcript_y)].contains("↑ more"));
    assert!(rows[usize::from(first_transcript_y.saturating_add(1))].contains("◆ nosis"));
    for x in 1..overflow_marker_x {
        assert_eq!(
            buffer[(x, first_transcript_y)].symbol(),
            " ",
            "first visible separator cell at x={x}"
        );
    }
}

#[test]
fn progress_wraps_under_its_marker() {
    let mut app = test_app(None);
    app.push_line(&"progressword ".repeat(40), TranscriptKind::Progress);
    let buffer = render_buffer(&app, 79, 30);
    let positions = buffer_rows(&buffer)
        .into_iter()
        .filter_map(|row| {
            row.find("progressword")
                .map(|byte_x| row[..byte_x].chars().count())
        })
        .collect::<Vec<_>>();

    assert!(positions.len() >= 3, "got: {positions:?}");
    assert!(
        positions.iter().all(|position| *position == 3),
        "got: {positions:?}"
    );
}

#[test]
fn selected_search_highlight_keeps_its_byte_range_on_a_wrapped_answer() {
    let mut app = test_app(None);
    let wrapped = format!("{}needle {}", "alpha ".repeat(14), "omega ".repeat(24));
    app.push_line(&wrapped, TranscriptKind::Answer);
    for index in 0..35 {
        app.push_line(&format!("newer filler {index}"), TranscriptKind::Progress);
    }
    reduce_key(&mut app, ctrl_key('f'));
    type_text(&mut app, "needle");

    let rendered = render_buffer(&app, 79, 30);
    let (x, y) = find_ascii_text(&rendered, "needle");
    assert!(x >= 4);
    assert!(y < search_modal_area(Rect::new(0, 0, 79, 30)).y);
    assert!(app.search_match_scroll.get() > 0);
    for offset in 0..6 {
        let cell = &rendered[(x.saturating_add(offset), y)];
        assert_eq!(cell.fg, Color::Cyan);
        assert!(cell.modifier.contains(Modifier::BOLD | Modifier::REVERSED));
    }

    reduce_key(&mut app, code_key(KeyCode::Enter));
    let focused = buffer_text(&render_buffer(&app, 79, 30));
    assert!(focused.contains("needle"), "got: {focused}");
}

#[test]
fn more_markers_render_only_for_overflow_and_track_both_directions() {
    let mut app = test_app(None);
    app.push_line("short", TranscriptKind::Progress);
    let short = buffer_text(&render_buffer(&app, 80, 16));
    assert!(!short.contains("↑ more"), "got: {short}");
    assert!(!short.contains("↓ more"), "got: {short}");

    for index in 0..30 {
        app.push_line(&format!("output line {index}"), TranscriptKind::Progress);
    }
    let newest = buffer_text(&render_buffer(&app, 80, 16));
    assert!(newest.contains("↑ more"), "got: {newest}");
    assert!(!newest.contains("↓ more"), "got: {newest}");

    app.scroll_back = 3;
    let middle = buffer_text(&render_buffer(&app, 80, 16));
    assert!(middle.contains("↑ more"), "got: {middle}");
    assert!(middle.contains("↓ more"), "got: {middle}");

    app.scroll_back = usize::from(u16::MAX);
    let oldest = buffer_text(&render_buffer(&app, 80, 16));
    assert!(!oldest.contains("↑ more"), "got: {oldest}");
    assert!(oldest.contains("↓ more"), "got: {oldest}");
}

#[test]
fn search_matches_case_insensitively_navigates_in_order_and_wraps() {
    let mut app = test_app(None);
    for line in ["Alpha first", "not this", "middle ALPHA", "alpha last"] {
        app.push_line(line, TranscriptKind::Progress);
    }

    assert_eq!(reduce_key(&mut app, ctrl_key('f')), UiAction::None);
    type_text(&mut app, "aLpHa");
    let selected = |app: &App| match &app.overlay {
        Overlay::Search {
            query, selected, ..
        } => {
            assert_eq!(query, "aLpHa");
            *selected
        }
        other => panic!("expected search overlay, got {other:?}"),
    };
    assert_eq!(selected(&app), 0);
    reduce_key(&mut app, code_key(KeyCode::Down));
    assert_eq!(selected(&app), 1);
    reduce_key(&mut app, code_key(KeyCode::Down));
    assert_eq!(selected(&app), 2);
    reduce_key(&mut app, code_key(KeyCode::Down));
    assert_eq!(selected(&app), 0);
    reduce_key(&mut app, code_key(KeyCode::Up));
    assert_eq!(selected(&app), 2);

    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(rendered.contains("match 3/3"), "got: {rendered}");
}

#[test]
fn search_match_ranges_are_exact_for_mixed_case_ascii_occurrences() {
    let mut app = test_app(None);
    app.push_line("start AlPhA and ALPHA end", TranscriptKind::Answer);

    let matches = search_match_lines(&app.transcript, "aLpHa");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].line_index, 0);
    assert_eq!(matches[0].ranges, vec![6..11, 16..21]);
    assert_eq!(
        &app.transcript[0].text[matches[0].ranges[0].clone()],
        "AlPhA"
    );
    assert_eq!(
        &app.transcript[0].text[matches[0].ranges[1].clone()],
        "ALPHA"
    );
}

#[test]
fn search_ranges_stay_utf8_safe_and_non_ascii_case_is_literal() {
    let mut app = test_app(None);
    app.push_line("préfix ALPHA café", TranscriptKind::Answer);

    let ascii = search_match_lines(&app.transcript, "alpha");
    let expected_start = "préfix ".len();
    assert_eq!(ascii[0].ranges, vec![expected_start..expected_start + 5]);
    assert_eq!(&app.transcript[0].text[ascii[0].ranges[0].clone()], "ALPHA");
    assert!(app.transcript[0]
        .text
        .is_char_boundary(ascii[0].ranges[0].start));
    assert!(app.transcript[0]
        .text
        .is_char_boundary(ascii[0].ranges[0].end));

    reduce_key(&mut app, ctrl_key('f'));
    type_text(&mut app, "alpha");
    let rendered = render_buffer(&app, 100, 30);
    let (match_x, match_y) = find_ascii_text(&rendered, "ALPHA café");
    for offset in 0..5 {
        let cell = &rendered[(match_x.saturating_add(offset), match_y)];
        assert_eq!(cell.fg, Color::Cyan);
        assert!(cell.modifier.contains(Modifier::BOLD | Modifier::REVERSED));
    }
    assert_eq!(
        rendered[(match_x.saturating_sub(1), match_y)].fg,
        Color::White
    );
    assert_eq!(
        rendered[(match_x.saturating_add(5), match_y)].fg,
        Color::White
    );
    reduce_key(&mut app, code_key(KeyCode::Esc));

    assert!(search_match_lines(&app.transcript, "CAFÉ").is_empty());
    let literal = search_match_lines(&app.transcript, "CAFé");
    assert_eq!(literal.len(), 1);
    assert_eq!(
        &app.transcript[0].text[literal[0].ranges[0].clone()],
        "café"
    );
}

#[test]
fn search_highlights_every_visible_hit_marks_the_selection_and_closes_cleanly() {
    let mut app = test_app(None);
    for index in 0..8 {
        app.push_line(&format!("older filler {index}"), TranscriptKind::Answer);
    }
    app.push_line("first needle result", TranscriptKind::Answer);
    app.push_line("between results", TranscriptKind::Answer);
    app.push_line("second NeEdLe and NEEDLE result", TranscriptKind::Answer);
    for index in 0..40 {
        app.push_line(&format!("newer filler {index}"), TranscriptKind::Answer);
    }

    reduce_key(&mut app, ctrl_key('f'));
    type_text(&mut app, "nEeDlE");
    reduce_key(&mut app, code_key(KeyCode::Down));
    let highlighted = render_buffer(&app, 100, 30);
    let highlighted_text = buffer_text(&highlighted);
    assert!(
        highlighted_text.contains("match 2/3"),
        "got: {highlighted_text}"
    );

    let (ordinary_x, ordinary_y) = find_ascii_text(&highlighted, "needle result");
    for offset in 0..6 {
        let ordinary = &highlighted[(ordinary_x.saturating_add(offset), ordinary_y)];
        assert_eq!(ordinary.fg, Color::Yellow);
        assert!(ordinary.modifier.contains(Modifier::BOLD));
        assert!(!ordinary.modifier.contains(Modifier::REVERSED));
    }

    let (selected_x, selected_y) = find_ascii_text(&highlighted, "NeEdLe and");
    for offset in 0..6 {
        let selected = &highlighted[(selected_x.saturating_add(offset), selected_y)];
        assert_eq!(selected.fg, Color::Cyan);
        assert!(selected
            .modifier
            .contains(Modifier::BOLD | Modifier::REVERSED));
    }
    assert!(selected_y < search_modal_area(Rect::new(0, 0, 100, 30)).y);
    assert_eq!(
        highlighted[(selected_x.saturating_sub(1), selected_y)].fg,
        Color::White
    );
    assert_eq!(
        highlighted[(selected_x.saturating_add(6), selected_y)].fg,
        Color::White
    );

    let (additional_x, additional_y) = find_ascii_text(&highlighted, "NEEDLE result");
    for offset in 0..6 {
        let additional = &highlighted[(additional_x.saturating_add(offset), additional_y)];
        assert_eq!(additional.fg, Color::Yellow);
        assert!(additional.modifier.contains(Modifier::BOLD));
        assert!(!additional.modifier.contains(Modifier::REVERSED));
    }
    assert_eq!(
        highlighted[(additional_x.saturating_add(6), additional_y)].fg,
        Color::White
    );

    reduce_key(&mut app, code_key(KeyCode::Down));
    let third = render_buffer(&app, 100, 30);
    assert!(buffer_text(&third).contains("match 3/3"));
    let (former_x, former_y) = find_ascii_text(&third, "NeEdLe and");
    let (third_x, third_y) = find_ascii_text(&third, "NEEDLE result");
    assert_eq!(third[(former_x, former_y)].fg, Color::Yellow);
    assert_eq!(third[(third_x, third_y)].fg, Color::Cyan);
    assert!(third[(third_x, third_y)]
        .modifier
        .contains(Modifier::BOLD | Modifier::REVERSED));

    reduce_key(&mut app, code_key(KeyCode::Enter));
    assert_eq!(app.overlay, Overlay::None);
    let closed = render_buffer(&app, 100, 30);
    for needle in ["NeEdLe and", "NEEDLE result"] {
        let (closed_x, closed_y) = find_ascii_text(&closed, needle);
        for offset in 0..6 {
            let closed_match = &closed[(closed_x.saturating_add(offset), closed_y)];
            assert_eq!(closed_match.fg, Color::White);
            assert!(!closed_match
                .modifier
                .intersects(Modifier::BOLD | Modifier::REVERSED));
        }
    }
}

#[test]
fn selected_search_highlight_stays_visible_without_transcript_overflow() {
    let mut app = test_app(None);
    for index in 0..10 {
        app.push_line(&format!("short filler {index}"), TranscriptKind::Answer);
    }
    app.push_line("short selected needle", TranscriptKind::Answer);
    let panel = search_modal_area(Rect::new(0, 0, 90, 20));
    let natural = render_buffer(&app, 90, 20);
    let (_, natural_y) = find_ascii_text(&natural, "needle");
    assert!(natural_y >= panel.y);

    reduce_key(&mut app, ctrl_key('f'));
    type_text(&mut app, "needle");

    let rendered = render_buffer(&app, 90, 20);
    let (x, y) = find_ascii_text(&rendered, "needle");
    assert!(y < panel.y);
    for offset in 0..6 {
        let cell = &rendered[(x.saturating_add(offset), y)];
        assert_eq!(cell.fg, Color::Cyan);
        assert!(cell.modifier.contains(Modifier::BOLD | Modifier::REVERSED));
    }
}

#[test]
fn search_zero_matches_is_explicit_and_enter_keeps_the_overlay_open() {
    let mut app = test_app(None);
    app.push_line("visible transcript", TranscriptKind::Answer);
    reduce_key(&mut app, ctrl_key('f'));
    type_text(&mut app, "missing literal");

    let rendered = buffer_text(&render_buffer(&app, 90, 20));
    assert!(rendered.contains("0 matches"), "got: {rendered}");
    reduce_key(&mut app, code_key(KeyCode::Enter));
    assert!(matches!(app.overlay, Overlay::Search { .. }));
}

#[test]
fn search_escape_restores_the_exact_prior_scroll_position() {
    let mut app = test_app(None);
    app.push_line("old target", TranscriptKind::Progress);
    for _ in 0..40 {
        app.push_line("newer filler", TranscriptKind::Progress);
    }
    let _ = render_buffer(&app, 80, 16);
    app.scroll_back = 7;

    reduce_key(&mut app, ctrl_key('f'));
    type_text(&mut app, "old target");
    let _ = render_buffer(&app, 80, 16);
    assert_ne!(app.search_match_scroll.get(), 7);
    reduce_key(&mut app, code_key(KeyCode::Esc));

    assert_eq!(app.overlay, Overlay::None);
    assert_eq!(app.scroll_back, 7);
}

#[test]
fn search_commits_a_match_that_was_scrolled_off_screen() {
    let mut app = test_app(None);
    app.push_line("unique oldest target", TranscriptKind::Progress);
    for _ in 0..80 {
        app.push_line("newer filler", TranscriptKind::Progress);
    }
    let bottom = buffer_text(&render_buffer(&app, 80, 16));
    assert!(!bottom.contains("unique oldest target"), "got: {bottom}");

    reduce_key(&mut app, ctrl_key('f'));
    type_text(&mut app, "oldest target");
    let _ = render_buffer(&app, 80, 16);
    reduce_key(&mut app, code_key(KeyCode::Enter));

    assert_eq!(app.overlay, Overlay::None);
    assert!(app.scroll_back > 0);
    let focused = buffer_text(&render_buffer(&app, 80, 16));
    assert!(focused.contains("unique oldest target"), "got: {focused}");
}

#[test]
fn search_reaches_a_match_beyond_the_old_u16_scrollback_cap() {
    let mut app = test_app(None);
    app.push_line("needle beyond cap", TranscriptKind::Progress);
    for _ in 0..usize::from(u16::MAX).saturating_add(32) {
        app.push_line("filler", TranscriptKind::Progress);
    }
    app.push_line("newest beyond paragraph cap", TranscriptKind::Progress);
    let bottom = buffer_text(&render_buffer(&app, 40, 12));
    assert!(
        bottom.contains("newest beyond paragraph cap"),
        "got: {bottom}"
    );
    assert!(app.max_scroll.get() > usize::from(u16::MAX));

    reduce_key(&mut app, ctrl_key('f'));
    type_text(&mut app, "needle");
    let _ = render_buffer(&app, 40, 12);
    reduce_key(&mut app, code_key(KeyCode::Enter));

    assert!(app.scroll_back > usize::from(u16::MAX));
    let focused = buffer_text(&render_buffer(&app, 40, 12));
    assert!(focused.contains("needle beyond cap"), "got: {focused}");
}

#[test]
fn search_reads_only_the_scrubbed_display_transcript() {
    let secret = "fake-search-secret";
    let mut app = test_app(None);
    app.scrubber = Arc::new(RwLock::new(Scrubber::new(vec![secret.into()])));
    app.push_line(&format!("credential={secret}"), TranscriptKind::Progress);
    assert!(app.transcript[0].text.contains("[REDACTED]"));
    assert!(!app.transcript[0].text.contains(secret));

    reduce_key(&mut app, ctrl_key('f'));
    type_text(&mut app, secret);
    let raw_query = buffer_text(&render_buffer(&app, 90, 20));
    assert!(raw_query.contains("0 matches"), "got: {raw_query}");
    reduce_key(&mut app, code_key(KeyCode::Esc));

    reduce_key(&mut app, ctrl_key('f'));
    type_text(&mut app, "[REDACTED]");
    let marker_query = buffer_text(&render_buffer(&app, 90, 20));
    assert!(marker_query.contains("match 1/1"), "got: {marker_query}");
}

#[test]
fn ctrl_f_and_search_command_share_the_same_overlay_path() {
    let mut via_key = test_app(None);
    via_key.input = "unfinished task".into();
    reduce_key(&mut via_key, ctrl_key('F'));
    assert!(matches!(
        via_key.overlay,
        Overlay::Search {
            ref query,
            selected: 0,
            original_scroll: 0,
        } if query.is_empty()
    ));
    assert_eq!(via_key.input, "unfinished task");

    let mut via_command = test_app(None);
    type_text(&mut via_command, "/search");
    reduce_key(&mut via_command, code_key(KeyCode::Enter));
    assert!(matches!(
        via_command.overlay,
        Overlay::Search {
            ref query,
            selected: 0,
            original_scroll: 0,
        } if query.is_empty()
    ));
    assert!(via_command.input.is_empty());
    assert!(builtin_palette_entries(false)
        .iter()
        .any(|entry| entry.name == "/search"));
}

#[test]
fn palette_filter_is_pure_and_finds_exec_shell() {
    let entries = builtin_palette_entries(false);
    let before = entries.clone();
    let filtered = filter_palette(&entries, "ex");

    assert!(
        filtered.iter().any(|entry| entry.name == "exec_shell"),
        "got: {:?}",
        filtered
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(entries, before);
    assert!(filter_palette(&entries, "timeline")
        .iter()
        .any(|entry| entry.name == "/timeline"));

    let denied = builtin_palette_entries(true);
    assert!(!denied.iter().any(|entry| entry.name == "exec_shell"));
    assert!(denied.iter().any(|entry| entry.name == "read_file"));
}

#[test]
fn mcp_palette_state_uses_auth_trust_and_warnings() {
    let configs = vec![
        mcp_config("plain", McpAuth::None, McpTrust::Ask),
        mcp_config(
            "keyed",
            McpAuth::ApiKey {
                vault_entry: "keyed".into(),
            },
            McpTrust::Auto,
        ),
        mcp_config("down", McpAuth::None, McpTrust::Ask),
        mcp_config("blocked", McpAuth::None, McpTrust::Block),
    ];
    let toolset = McpToolset {
        tools: Vec::new(),
        warnings: vec!["mcp server \"down\": connection refused".into()],
    };

    let entries = mcp_palette_entries(&configs, &toolset);
    let state = |name: &str| {
        entries
            .iter()
            .find(|entry| entry.name == name)
            .and_then(|entry| entry.state)
    };
    assert_eq!(state("plain"), Some(McpState::Enabled));
    assert_eq!(state("keyed"), Some(McpState::AuthOk));
    assert_eq!(state("down"), Some(McpState::Stale));
    assert_eq!(state("blocked"), Some(McpState::DiscoverOnly));
}

#[test]
fn empty_and_broken_mcp_config_project_without_panicking() {
    let empty = McpToolset {
        tools: Vec::new(),
        warnings: Vec::new(),
    };
    assert!(mcp_palette_entries(&[], &empty).is_empty());

    let broken = McpToolset {
        tools: Vec::new(),
        warnings: vec!["could not parse .nosis/mcp.toml".into()],
    };
    let entries = mcp_palette_entries(&[], &broken);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].state, Some(McpState::Stale));
}

#[test]
fn trust_dial_uses_plain_none_lines_for_empty_classes() {
    let lines = trust_dial_lines(&PolicyView {
        autonomy: Autonomy::Ask,
        auto_paths: Vec::new(),
        ask_paths: Vec::new(),
        block_paths: Vec::new(),
        block_commands: Vec::new(),
    });

    assert_eq!(
        lines,
        [
            "session autonomy: ask",
            "auto-approve: none",
            "always-ask: none",
            "hard-block/protected: none",
            "blocked command: none",
        ]
    );
    assert!(lines.iter().all(|line| !line.trim().is_empty()));
}

#[test]
fn overlays_suppress_task_dispatch_and_escape_restores_base_view() {
    for command in ["/trust", "/help", "/timeline", "/search"] {
        let mut app = test_app(None);
        type_text(&mut app, command);
        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::None
        );
        assert_ne!(app.overlay, Overlay::None);

        for character in "work".chars() {
            assert_eq!(reduce_key(&mut app, char_key(character)), UiAction::None);
        }
        assert_eq!(
            reduce_key(&mut app, code_key(KeyCode::Enter)),
            UiAction::None
        );
        assert!(app.input.is_empty());
        assert!(app.transcript.is_empty());
        assert_eq!(app.status, Status::Idle);

        assert_eq!(reduce_key(&mut app, code_key(KeyCode::Esc)), UiAction::None);
        assert_eq!(app.overlay, Overlay::None);
    }
}

#[test]
fn palette_enter_runs_commands_and_describes_tools() {
    let mut app = test_app(None);
    type_text(&mut app, "/help");
    reduce_key(&mut app, code_key(KeyCode::Enter));
    type_text(&mut app, "trust");
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );
    assert_eq!(app.overlay, Overlay::TrustDial);

    let mut app = test_app(None);
    type_text(&mut app, "/help");
    reduce_key(&mut app, code_key(KeyCode::Enter));
    type_text(&mut app, "quit");
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::Quit
    );

    let mut app = test_app(None);
    type_text(&mut app, "/help");
    reduce_key(&mut app, code_key(KeyCode::Enter));
    type_text(&mut app, "exec_shell");
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );
    match &app.overlay {
        Overlay::Palette { detail, .. } => {
            assert!(detail.as_deref().is_some_and(|line| !line.is_empty()));
        }
        _ => panic!("tool selection must keep the palette open"),
    }
}

fn mcp_config(name: &str, auth: McpAuth, trust: McpTrust) -> McpServerConfig {
    McpServerConfig {
        name: name.into(),
        url: "https://example.invalid/mcp".into(),
        spec: "2026-07-28".into(),
        auth,
        scopes: Vec::new(),
        default_mode: None,
        trust,
    }
}

fn char_key(character: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)
}

fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        assert_eq!(reduce_key(app, char_key(character)), UiAction::None);
    }
}

fn code_key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl_key(character: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL)
}

#[test]
fn prompt_cost_preview_is_uncached_input_only_and_never_changes_measured_state() {
    let mut app = meter_app();
    app.prompt_base_tokens.store(999_995, Ordering::Release);
    app.input = "task".into();
    let preview = app.prompt_cost_preview(fixed_at()).unwrap();
    assert_eq!(
        preview,
        "prompt ~¥1.00 (≈$0.14) estimated if uncached; output unknown"
    );
    let rendered = buffer_text(&render_buffer(&app, 240, 20));
    assert!(
        rendered.contains("estimated if uncached; output unknown"),
        "got: {rendered}"
    );
    assert!(app.session_cost.is_empty());
    assert!(app.usage.is_none());
    assert_eq!(app.session_money(fixed_at()), "¥0.00 (≈$0.00)");

    let measured_usage = Usage {
        prompt_tokens: 100,
        completion_tokens: 0,
        cached_tokens: Some(0),
        evidence: UsageEvidence::Measured,
    };
    let mut measured_receipt = receipt("measured", Outcome::Pass, Some(measured_usage));
    measured_receipt.model_id = "meter-route".into();
    let root = temp_dir();
    nh_core::receipt::ReceiptWriter::project(root.clone(), Scrubber::new(Vec::new()))
        .append(&measured_receipt)
        .unwrap();
    let durable = std::fs::read_to_string(root.join(".nosis").join("receipts.jsonl")).unwrap();
    assert!(!durable.contains("estimate"));
    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "meter-route".into(),
            receipt: measured_receipt,
            answer: "done".into(),
        }),
    );

    assert_eq!(app.session_cost.len(), 1);
    assert!((app.session_cost[0].amount - 0.0001).abs() < f64::EPSILON);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unpriced_route_renders_no_prompt_cost_preview_or_zero() {
    let mut app = test_app(None);
    app.prompt_base_tokens.store(10_000, Ordering::Release);
    app.input = "task".into();

    assert_eq!(app.prompt_cost_preview(fixed_at()), None);
    let rendered = buffer_text(&render_buffer(&app, 180, 20));
    assert!(
        !rendered.contains("estimated if uncached"),
        "got: {rendered}"
    );
    assert!(!rendered.contains("output unknown"), "got: {rendered}");
    assert!(!rendered.contains("~$0.00"), "got: {rendered}");
}

#[test]
fn cost_hud_omits_cache_before_usage_and_shows_it_after() {
    let mut app = test_app(None);
    let before = app.hud_line(Utc::now());
    assert!(before.contains("session - · no usage yet"), "got: {before}");
    assert!(before.contains("no price data"), "got: {before}");
    assert!(!before.contains("| cache "), "got: {before}");
    assert!(!before.contains("· cache "), "got: {before}");
    apply_event(
        &mut app,
        AgentEvent::Usage(Usage {
            prompt_tokens: 100,
            completion_tokens: 20,
            cached_tokens: Some(25),
            evidence: UsageEvidence::Measured,
        }),
    );
    let after = app.hud_line(Utc::now());
    assert!(after.contains("in 100 · out 20"), "got: {after}");
    assert!(after.contains("cache 25%"), "got: {after}");
}

#[test]
fn context_hud_uses_only_last_request_measured_or_partial_evidence() {
    let mut app = test_app(None);
    app.usage = Some(Usage {
        prompt_tokens: 900,
        completion_tokens: 40,
        cached_tokens: Some(100),
        evidence: UsageEvidence::Measured,
    });
    assert!(!app.hud_line(fixed_at()).contains("ctx"));

    apply_event(
        &mut app,
        AgentEvent::ModelFinished {
            route: "test-route".into(),
            usage: Some(Usage {
                prompt_tokens: 250,
                completion_tokens: 10,
                cached_tokens: None,
                evidence: UsageEvidence::Measured,
            }),
        },
    );
    let measured = app.hud_line(fixed_at());
    assert!(measured.contains("· ctx 25%"), "got: {measured}");
    assert!(!measured.contains("ctx 90%"), "got: {measured}");

    apply_event(
        &mut app,
        AgentEvent::ModelFinished {
            route: "test-route".into(),
            usage: Some(Usage {
                prompt_tokens: 250,
                completion_tokens: 10,
                cached_tokens: None,
                evidence: UsageEvidence::Partial,
            }),
        },
    );
    let partial = app.hud_line(fixed_at());
    assert!(partial.contains("· ctx ~25%"), "got: {partial}");
}

#[test]
fn context_hud_marks_tiny_measured_context_occupancy() {
    let mut app = meter_app();
    apply_event(
        &mut app,
        AgentEvent::ModelFinished {
            route: "meter-route".into(),
            usage: Some(Usage {
                prompt_tokens: 4_000,
                completion_tokens: 20,
                cached_tokens: None,
                evidence: UsageEvidence::Measured,
            }),
        },
    );

    let hud = app.hud_line(fixed_at());
    assert!(hud.contains("· ctx <1%"), "got: {hud}");
    assert!(!hud.contains("· ctx 0%"), "got: {hud}");
}

#[test]
fn context_hud_preserves_true_zero_context_occupancy() {
    let mut app = meter_app();
    apply_event(
        &mut app,
        AgentEvent::ModelFinished {
            route: "meter-route".into(),
            usage: Some(Usage {
                prompt_tokens: 0,
                completion_tokens: 20,
                cached_tokens: None,
                evidence: UsageEvidence::Measured,
            }),
        },
    );

    let hud = app.hud_line(fixed_at());
    assert!(hud.contains("· ctx 0%"), "got: {hud}");
    assert!(!hud.contains("· ctx <1%"), "got: {hud}");
}

#[test]
fn context_hud_composes_partial_and_tiny_markers() {
    let mut app = meter_app();
    apply_event(
        &mut app,
        AgentEvent::ModelFinished {
            route: "meter-route".into(),
            usage: Some(Usage {
                prompt_tokens: 4_000,
                completion_tokens: 20,
                cached_tokens: None,
                evidence: UsageEvidence::Partial,
            }),
        },
    );

    let hud = app.hud_line(fixed_at());
    assert!(hud.contains("· ctx ~<1%"), "got: {hud}");
    assert!(!hud.contains("· ctx ~0%"), "got: {hud}");
}

#[test]
fn context_hud_omits_unknown_absent_and_undeclared_evidence_without_zero_claims() {
    let mut app = test_app(None);
    for usage in [
        None,
        Some(Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            cached_tokens: None,
            evidence: UsageEvidence::Unknown,
        }),
    ] {
        apply_event(
            &mut app,
            AgentEvent::ModelFinished {
                route: "test-route".into(),
                usage,
            },
        );
        let hud = app.hud_line(fixed_at());
        assert!(!hud.contains("ctx"), "got: {hud}");
        assert!(!hud.contains("ctx 0"), "got: {hud}");
    }

    let mut no_context = picker_app();
    let route = no_context.resolver.resolve("c-unknown-context").unwrap();
    no_context.switch_route(route);
    apply_event(
        &mut no_context,
        AgentEvent::ModelFinished {
            route: "c-unknown-context".into(),
            usage: Some(Usage {
                prompt_tokens: 100,
                completion_tokens: 1,
                cached_tokens: None,
                evidence: UsageEvidence::Measured,
            }),
        },
    );
    let hud = no_context.hud_line(fixed_at());
    assert!(!hud.contains("ctx"), "got: {hud}");
}

#[test]
fn context_hud_reports_over_window_and_non_finite_ratios_without_clamping() {
    let mut app = test_app(None);
    apply_event(
        &mut app,
        AgentEvent::ModelFinished {
            route: "test-route".into(),
            usage: Some(Usage {
                prompt_tokens: 1_250,
                completion_tokens: 1,
                cached_tokens: None,
                evidence: UsageEvidence::Measured,
            }),
        },
    );
    let over = app.hud_line(fixed_at());
    assert!(over.contains("· ctx 125%"), "got: {over}");
    assert!(!over.contains("· ctx 100%"), "got: {over}");

    let zero_context = app.resolver.resolve("zero-context").unwrap();
    app.switch_route(zero_context);
    apply_event(
        &mut app,
        AgentEvent::ModelFinished {
            route: "zero-context".into(),
            usage: Some(Usage {
                prompt_tokens: 1,
                completion_tokens: 1,
                cached_tokens: None,
                evidence: UsageEvidence::Measured,
            }),
        },
    );
    let non_finite = app.hud_line(fixed_at());
    assert!(non_finite.contains("· ctx inf%"), "got: {non_finite}");
}

#[test]
fn context_hud_drops_after_compaction_and_clears_on_route_switch() {
    let mut app = test_app(None);
    apply_event(
        &mut app,
        AgentEvent::ModelFinished {
            route: "test-route".into(),
            usage: Some(Usage {
                prompt_tokens: 900,
                completion_tokens: 1,
                cached_tokens: None,
                evidence: UsageEvidence::Measured,
            }),
        },
    );
    let before = app.hud_line(fixed_at());
    assert!(before.contains("ctx 90%"), "got: {before}");

    apply_event(
        &mut app,
        AgentEvent::Compaction(CompactionEvent::new_at(
            90,
            4,
            700,
            None,
            fixed_at().timestamp(),
        )),
    );
    apply_event(
        &mut app,
        AgentEvent::ModelFinished {
            route: "test-route".into(),
            usage: Some(Usage {
                prompt_tokens: 200,
                completion_tokens: 1,
                cached_tokens: None,
                evidence: UsageEvidence::Measured,
            }),
        },
    );
    let after = app.hud_line(fixed_at());
    assert!(after.contains("ctx 20%"), "got: {after}");
    assert!(!after.contains("ctx 90%"), "got: {after}");

    let other = app.resolver.resolve("other-route").unwrap();
    app.switch_route(other);
    let switched = app.hud_line(fixed_at());
    assert!(!switched.contains("ctx"), "got: {switched}");
}

#[test]
fn cost_hud_and_timeline_distinguish_absent_cache_from_measured_zero() {
    let mut app = test_app(None);
    app.usage = Some(Usage {
        prompt_tokens: 20,
        completion_tokens: 2,
        cached_tokens: None,
        evidence: UsageEvidence::Measured,
    });
    let absent_hud = app.hud_line(Utc::now());
    assert!(!absent_hud.contains("cache 0%"), "got: {absent_hud}");

    let absent_entry = TimelineEntry::from_receipt(
        1,
        receipt("absent", Outcome::Pass, app.usage.clone()),
        "done".into(),
        Default::default(),
    );
    assert_eq!(timeline_row(&absent_entry), "#1  completed  20/2");
    assert_eq!(
        timeline_detail_lines(&absent_entry)[10],
        "tokens: 20 in / 2 out"
    );

    app.usage.as_mut().unwrap().cached_tokens = Some(0);
    let measured_hud = app.hud_line(Utc::now());
    assert!(measured_hud.contains("cache 0%"), "got: {measured_hud}");
    let measured_entry = TimelineEntry::from_receipt(
        1,
        receipt("zero", Outcome::Pass, app.usage.clone()),
        "done".into(),
        Default::default(),
    );
    assert_eq!(
        timeline_row(&measured_entry),
        "#1  completed  20/2/0 cache 0%"
    );
    assert_eq!(
        timeline_detail_lines(&measured_entry)[10],
        "tokens: 20 in / 2 out / 0 cached | cache 0%"
    );
}

#[test]
fn measured_usage_without_cached_counter_bounds_both_tui_cost_sites_and_session() {
    let mut app = meter_app();
    let usage = Usage {
        prompt_tokens: 100_000,
        completion_tokens: 50_000,
        cached_tokens: None,
        evidence: UsageEvidence::Measured,
    };

    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "meter-route".into(),
            receipt: receipt("cold cache", Outcome::Pass, Some(usage.clone())),
            answer: "done".into(),
        }),
    );
    apply_event(&mut app, AgentEvent::Usage(usage.clone()));

    assert_eq!(app.session_cost.len(), 1);
    assert!(app.session_cost[0].upper_bound);
    assert!(!app.session_cost[0].uncertain);
    assert!(!app.session_cost_incomplete);
    assert_eq!(app.session_money(fixed_at()), "at most ¥0.20 (≈$0.03)");
    assert_eq!(
        savings_lines(&app.resolver, &app.route, &usage, fixed_at()),
        vec!["cost at most ¥0.20 (≈$0.03) - cache split not reported by provider"]
    );
    assert_eq!(
        app.transcript.first().map(|line| line.text.as_str()),
        Some("cost at most ¥0.20 (≈$0.03) - cache split not reported by provider")
    );
    assert!(!app
        .transcript
        .iter()
        .any(|line| line.text.contains("saved")));
    assert!(!app
        .transcript
        .iter()
        .any(|line| line.text.contains("naive")));

    let verify_live_catalog = METER_CATALOG.replacen(
        "price_confidence = \"confirmed\"",
        "price_confidence = \"verify_live\"",
        1,
    );
    let mut verify_live = meter_app_from(&verify_live_catalog);
    record_turn_cost(&mut verify_live, &usage, fixed_at());
    assert!(verify_live.session_cost[0].upper_bound);
    assert!(verify_live.session_cost[0].uncertain);
    assert_eq!(
        verify_live.session_money(fixed_at()),
        "at most ¥0.20 (≈$0.03)*"
    );
    assert_eq!(
        savings_lines(
            &verify_live.resolver,
            &verify_live.route,
            &usage,
            fixed_at()
        ),
        vec![
            "cost at most ¥0.20 (≈$0.03)* - cache split not reported by provider",
            "*price verify_live",
        ]
    );
}

#[test]
fn money_hud_uses_accumulated_turn_cost_in_native_currency() {
    let mut app = meter_app();
    let usage = Usage {
        prompt_tokens: 100_000,
        completion_tokens: 50_000,
        cached_tokens: Some(90_000),
        evidence: UsageEvidence::Measured,
    };
    app.usage = Some(usage.clone());
    record_turn_cost(&mut app, &usage, fixed_at());

    let hud = app.hud_line(fixed_at());
    assert!(
        hud.contains("session ¥0.11 (≈$0.02) · in 100000 · out 50000 · cache 90%"),
        "got: {hud}"
    );
    assert!(hud.contains("off-peak"), "got: {hud}");
}

#[test]
fn savings_line_renders_counterfactuals_and_keeps_measured_zero_exact() {
    let mut app = meter_app();
    let usage = Usage {
        prompt_tokens: 100_000,
        completion_tokens: 50_000,
        cached_tokens: Some(90_000),
        evidence: UsageEvidence::Measured,
    };
    assert_eq!(
        savings_lines(&app.resolver, &app.route, &usage, fixed_at()),
        vec![
            "cost ¥0.11 (≈$0.02) - saved 44% vs no-cache",
            "naive: peak ¥0.22 · no-cache ¥0.20 · top-tier ¥0.45",
        ]
    );

    let cold = Usage {
        cached_tokens: Some(0),
        ..usage
    };
    let cold_lines = savings_lines(&app.resolver, &app.route, &cold, fixed_at());
    assert_eq!(cold_lines[0], "cost ¥0.20 (≈$0.03)");
    assert!(!cold_lines[0].contains("saved"));
    assert!(!cold_lines[0].contains("at most"));
    record_turn_cost(&mut app, &cold, fixed_at());
    assert!(!app.session_cost[0].upper_bound);
    assert!(!app.session_money(fixed_at()).contains("at most"));
}

#[test]
fn tui_counterfactuals_stay_separate_from_the_inline_meter_shape() {
    let app = meter_app();
    let usage = Usage {
        prompt_tokens: 100_000,
        completion_tokens: 50_000,
        cached_tokens: Some(90_000),
        evidence: UsageEvidence::Measured,
    };

    let lines = savings_lines(&app.resolver, &app.route, &usage, fixed_at());
    let counterfactuals = lines[1]
        .strip_prefix("naive: ")
        .expect("the TUI keeps counterfactuals on a naive line");
    let inline_shape = format!("{}   ({counterfactuals})", lines[0]);

    assert_ne!(lines, vec![inline_shape.clone()]);
    assert!(!lines[0].contains("   ("));
    assert!(lines[1].starts_with("naive: peak "));
    assert!(inline_shape.contains("   (peak "));
}

#[test]
fn no_peak_route_drops_hud_and_naive_segments_cleanly_at_narrow_width() {
    let app = picker_app();
    let usage = Usage {
        prompt_tokens: 100_000,
        completion_tokens: 50_000,
        cached_tokens: Some(90_000),
        evidence: UsageEvidence::Measured,
    };

    let lines = savings_lines(&app.resolver, &app.route, &usage, fixed_at());
    assert_eq!(lines.len(), 2, "got: {lines:?}");
    assert!(
        lines[1].starts_with("naive: no-cache "),
        "got: {}",
        lines[1]
    );
    assert!(lines[1].contains(" · top-tier "), "got: {}", lines[1]);
    assert!(!lines[1].contains("peak"), "got: {}", lines[1]);
    assert!(!lines[1].contains("· ·"), "got: {}", lines[1]);
    assert!(!lines[1].ends_with('·'), "got: {}", lines[1]);

    let hud = app.hud_line(fixed_at());
    assert_eq!(hud, "session $0.00 · no usage yet · profile balanced");
    assert!(!hud.contains("· ·"), "got: {hud}");
    let narrow = buffer_text(&render_buffer(&app, 50, 8));
    assert!(narrow.contains("profile balanced"), "got: {narrow}");
    assert!(!narrow.contains("off-peak"), "got: {narrow}");
}

#[test]
fn local_hud_and_turn_meter_do_not_present_hardware_cost_as_zero() {
    let mut app = picker_app();
    app.route = app.resolver.resolve("f-local").unwrap();
    let usage = Usage {
        prompt_tokens: 100,
        completion_tokens: 20,
        cached_tokens: None,
        evidence: UsageEvidence::Measured,
    };

    assert!(
        app.hud_line(fixed_at()).contains("session billing unknown"),
        "got: {}",
        app.hud_line(fixed_at())
    );
    assert_eq!(
        savings_lines(&app.resolver, &app.route, &usage, fixed_at()),
        vec![nh_routes::LOCAL_METER_COPY.to_owned()]
    );
    record_turn_cost(&mut app, &usage, fixed_at());
    assert_eq!(
        app.transcript.last().map(|line| line.text.as_str()),
        Some(nh_routes::LOCAL_METER_COPY)
    );
    assert!(app.session_cost.is_empty());
    assert!(app.session_cost_incomplete);
    assert!(app
        .hud_line(fixed_at())
        .contains("session unavailable - meter incomplete"));
}

#[test]
fn compaction_without_exact_preceding_cache_refuses_money() {
    let mut app = meter_app();
    let usage = Usage {
        prompt_tokens: 100_000,
        completion_tokens: 10_000,
        cached_tokens: Some(90_000),
        evidence: UsageEvidence::Measured,
    };
    let mut stats = CompactionStats::default();
    stats.record_at(8, 40_000, None, fixed_at().timestamp());
    let mut compacted = receipt("compact", Outcome::Pass, Some(usage));
    compacted.compaction = Box::new(stats);

    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "meter-route".into(),
            receipt: compacted,
            answer: "done".into(),
        }),
    );

    let detail = app.timeline[0].compaction_detail.as_deref().unwrap();
    assert_eq!(
        detail,
        "compaction 1 event · 8 messages elided · ~40000 tokens elided · next-call money not stated - exact preceding-call cached tokens unavailable"
    );
    assert!(!detail.contains('¥'), "got: {detail}");
    assert!(!detail.contains('$'), "got: {detail}");
    assert_eq!(
        app.last_compaction_hud.as_deref(),
        Some("compact ~40000t · net not stated")
    );
}

#[test]
fn compaction_without_exact_event_time_refuses_money() {
    let mut app = meter_app();
    let mut stats = CompactionStats::default();
    stats.record(8, 40_000, Some(160_000));
    let mut compacted = receipt("compact", Outcome::Pass, None);
    compacted.compaction = Box::new(stats);

    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "meter-route".into(),
            receipt: compacted,
            answer: "done".into(),
        }),
    );

    let detail = app.timeline[0].compaction_detail.as_deref().unwrap();
    assert!(
        detail.contains("exact compaction time unavailable"),
        "got: {detail}"
    );
    assert!(!detail.contains('¥'), "got: {detail}");
    assert!(!detail.contains('$'), "got: {detail}");
}

#[test]
fn local_compaction_never_presents_zero_billed_saving() {
    let mut app = picker_app();
    app.route = app.resolver.resolve("f-local").unwrap();
    let mut stats = CompactionStats::default();
    stats.record_at(8, 40_000, None, fixed_at().timestamp());
    let mut compacted = receipt("compact", Outcome::Pass, None);
    compacted.compaction = Box::new(stats);

    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "f-local".into(),
            receipt: compacted,
            answer: "done".into(),
        }),
    );

    let detail = app.timeline[0].compaction_detail.as_deref().unwrap();
    assert!(
        detail.contains(nh_routes::LOCAL_METER_COPY),
        "got: {detail}"
    );
    assert!(
        detail.contains("next-call money not stated"),
        "got: {detail}"
    );
    assert!(!detail.contains("$0.00"), "got: {detail}");
}

#[test]
fn compaction_prices_cache_hit_saving_and_reports_negative_net() {
    let mut app = meter_app();
    let mut stats = CompactionStats::default();
    stats.record_at(8, 40_000, Some(160_000), fixed_at().timestamp());
    let mut compacted = receipt(
        "compact",
        Outcome::Pass,
        Some(Usage {
            prompt_tokens: 100_000,
            completion_tokens: 0,
            cached_tokens: Some(0),
            evidence: UsageEvidence::Measured,
        }),
    );
    compacted.ts_utc = "2026-07-14T02:00:00Z".into();
    compacted.compaction = Box::new(stats);

    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "meter-route".into(),
            receipt: compacted,
            answer: "done".into(),
        }),
    );

    let detail_before_switch = app.timeline[0].compaction_detail.clone().unwrap();
    assert_eq!(
        detail_before_switch,
        "compaction 1 event · 8 messages elided · ~40000 tokens elided · next-call estimate: cache-hit saving ~¥0.0008 · cache-reset surcharge ~¥0.12 · net loss ~¥0.12 (≈$0.02)"
    );
    assert_eq!(
        app.last_compaction_hud.as_deref(),
        Some("compact ~40000t · next-call net loss ~¥0.12 (≈$0.02)")
    );

    let next_route = app.resolver.resolve("cny-top").unwrap();
    app.switch_route(next_route);
    assert_eq!(
        app.timeline[0].compaction_detail.as_deref(),
        Some(detail_before_switch.as_str()),
        "stored receipt display must not be repriced after /model"
    );
    assert!(app.last_compaction_hud.is_none());
    assert_eq!(app.session_cost.len(), 1);
    assert!((app.session_cost[0].amount - 0.2).abs() < f64::EPSILON);
}

#[test]
fn delayed_receipt_uses_its_origin_route_without_restoring_a_stale_hud() {
    let mut app = meter_app();
    let mut stats = CompactionStats::default();
    stats.record_at(8, 40_000, Some(160_000), fixed_at().timestamp());
    let mut compacted = receipt(
        "compact",
        Outcome::Pass,
        Some(Usage {
            prompt_tokens: 100_000,
            completion_tokens: 0,
            cached_tokens: Some(0),
            evidence: UsageEvidence::Measured,
        }),
    );
    compacted.compaction = Box::new(stats);
    let next_route = app.resolver.resolve("cny-top").unwrap();
    app.switch_route(next_route);

    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "meter-route".into(),
            receipt: compacted,
            answer: "done".into(),
        }),
    );

    assert_eq!(app.route.id(), "cny-top");
    assert_eq!(
        app.timeline[0].compaction_detail.as_deref(),
        Some(
            "compaction 1 event · 8 messages elided · ~40000 tokens elided · next-call estimate: cache-hit saving ~¥0.0008 · cache-reset surcharge ~¥0.12 · net loss ~¥0.12 (≈$0.02)"
        )
    );
    assert!(app.last_compaction_hud.is_none());
    assert_eq!(app.session_cost.len(), 1);
    assert!((app.session_cost[0].amount - 0.1).abs() < f64::EPSILON);
}

#[test]
fn compaction_zero_net_says_break_even() {
    let mut app = meter_app();
    let mut stats = CompactionStats::default();
    stats.record_at(2, 49, Some(50), fixed_at().timestamp());
    let mut compacted = receipt("compact", Outcome::Pass, None);
    compacted.compaction = Box::new(stats);

    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "meter-route".into(),
            receipt: compacted,
            answer: "done".into(),
        }),
    );

    let detail = app.timeline[0].compaction_detail.as_deref().unwrap();
    assert!(
        detail.contains("net break-even ~¥0.00 (≈$0.00)"),
        "got: {detail}"
    );
}

#[test]
fn compaction_estimates_do_not_change_usage_or_session_cost() {
    let usage = Usage {
        prompt_tokens: 100_000,
        completion_tokens: 50_000,
        cached_tokens: Some(90_000),
        evidence: UsageEvidence::Measured,
    };
    let mut plain = meter_app();
    let mut compacted = meter_app();
    apply_event(&mut plain, AgentEvent::Usage(usage.clone()));
    apply_event(&mut compacted, AgentEvent::Usage(usage.clone()));

    let plain_receipt = receipt("plain", Outcome::Pass, Some(usage.clone()));
    apply_event(
        &mut plain,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "meter-route".into(),
            receipt: plain_receipt,
            answer: "done".into(),
        }),
    );

    let mut stats = CompactionStats::default();
    stats.record_at(8, 40_000, Some(160_000), fixed_at().timestamp());
    let mut compacted_receipt = receipt("compact", Outcome::Pass, Some(usage.clone()));
    compacted_receipt.compaction = Box::new(stats);
    apply_event(
        &mut compacted,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "meter-route".into(),
            receipt: compacted_receipt,
            answer: "done".into(),
        }),
    );

    assert_eq!(compacted.usage.as_ref().unwrap(), &usage);
    assert_eq!(
        compacted.session_money(fixed_at()),
        plain.session_money(fixed_at())
    );
    assert_eq!(compacted.session_cost.len(), plain.session_cost.len());
    assert_eq!(
        compacted.session_cost[0].amount,
        plain.session_cost[0].amount
    );
}

#[test]
fn default_compaction_keeps_timeline_and_hud_copy_exact() {
    let mut app = meter_app();
    let usage = Usage {
        prompt_tokens: 100_000,
        completion_tokens: 50_000,
        cached_tokens: Some(90_000),
        evidence: UsageEvidence::Measured,
    };
    apply_event(&mut app, AgentEvent::Usage(usage.clone()));
    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "meter-route".into(),
            receipt: receipt("plain", Outcome::Pass, Some(usage)),
            answer: "done".into(),
        }),
    );

    assert_eq!(
        timeline_row(&app.timeline[0]),
        "#1  completed  100000/50000/90000 cache 90%"
    );
    assert_eq!(
        timeline_detail_lines(&app.timeline[0]),
        vec![
            "TURN #1",
            "timestamp: 2026-07-14T12:00:00Z",
            "model: test-route",
            "task: plain",
            "kind: task",
            "execution outcome: completed",
            "verification: not recorded",
            "agent turns: 3",
            "tool calls: 2",
            "failure class: none",
            "tokens: 100000 in / 50000 out / 90000 cached | cache 90%",
            "compacted: no",
            "",
            "answer: done",
        ]
    );
    assert_eq!(
        app.hud_line(fixed_at()),
        "session ¥0.11 (≈$0.02) · in 100000 · out 50000 · cache 90% · off-peak · profile balanced"
    );
}

#[test]
fn multiple_compactions_refuse_one_aggregate_money_claim() {
    let mut app = meter_app();
    let mut stats = CompactionStats::default();
    stats.record_at(4, 20_000, Some(150_000), fixed_at().timestamp());
    stats.record_at(
        3,
        10_000,
        Some(120_000),
        fixed_at().timestamp().saturating_add(1),
    );
    let mut compacted = receipt("twice", Outcome::Pass, None);
    compacted.compaction = Box::new(stats);

    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "meter-route".into(),
            receipt: compacted,
            answer: "done".into(),
        }),
    );

    let detail = app.timeline[0].compaction_detail.as_deref().unwrap();
    assert_eq!(
        detail,
        "compaction 2 events · 7 messages elided · ~30000 tokens elided · aggregate money not stated - compactions affect separate next calls"
    );
    assert!(!detail.contains('¥'), "got: {detail}");
}

#[test]
fn why_command_uses_live_resolver_trace() {
    let mut app = meter_app();
    assert_eq!(explain_why(&mut app), UiAction::None);
    assert!(app
        .transcript
        .iter()
        .any(|line| line.text.starts_with("route: meter-route")));
    assert!(app
        .transcript
        .iter()
        .any(|line| line.text.starts_with("skipped ")));
}

#[test]
fn why_and_model_picker_refuse_to_price_partial_prior_usage() {
    let mut app = picker_app();
    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "a-cheap".into(),
            receipt: receipt(
                "partial",
                Outcome::Pass,
                Some(Usage {
                    prompt_tokens: 100,
                    completion_tokens: 20,
                    cached_tokens: Some(25),
                    evidence: UsageEvidence::Partial,
                }),
            ),
            answer: "done".into(),
        }),
    );

    assert_eq!(explain_why(&mut app), UiAction::None);
    assert!(app
        .transcript
        .iter()
        .any(|line| line.text.contains("prior usage is a lower bound")));
    app.input = "/model".into();
    assert_eq!(execute_command_menu(&mut app), UiAction::None);
    let Overlay::Picker { rows, .. } = &app.overlay else {
        panic!("/model must open the picker");
    };
    assert!(rows
        .iter()
        .filter(|row| row.value != "f-local")
        .all(|row| row
            .label
            .contains("est unavailable: prior usage is a lower bound")));
}

#[test]
fn typical_duration_is_absent_below_minimum_without_a_separator() {
    let mut receipts = (0..MIN_TYPICAL_DURATION_SAMPLES - 1)
        .map(|index| {
            timed_receipt(
                "test-route",
                Some(10_000 + index as u64 * 1_000),
                Outcome::Pass,
                ReceiptKind::Task,
            )
        })
        .collect::<Vec<_>>();
    receipts.extend(
        (0..10).map(|_| timed_receipt("other-route", Some(1), Outcome::Pass, ReceiptKind::Task)),
    );
    receipts.extend(
        (0..10).map(|_| timed_receipt("test-route", None, Outcome::Pass, ReceiptKind::Task)),
    );
    let mut app = test_app_with_timing(
        None,
        RouteTimingHistory::from_receipts(&test_resolver(), receipts),
    );
    app.set_status(Status::Working, Utc::now());

    let rendered = buffer_text(&render_buffer(&app, 150, 20));

    assert!(rendered.contains("WORKING"), "got: {rendered}");
    assert!(!rendered.contains("typically"), "got: {rendered}");
    assert!(!rendered.contains("s,"), "got: {rendered}");
}

#[test]
fn route_typical_uses_passing_timed_samples_and_a_median_on_every_live_surface() {
    let mut receipts = [10_000, 11_000, 12_000, 13_000, 600_000]
        .into_iter()
        .map(|duration_ms| {
            timed_receipt(
                "test-route",
                Some(duration_ms),
                Outcome::Pass,
                ReceiptKind::Task,
            )
        })
        .collect::<Vec<_>>();
    receipts.extend(
        (0..10).map(|_| timed_receipt("other-route", Some(1), Outcome::Pass, ReceiptKind::Task)),
    );
    receipts.extend(
        (0..10).map(|_| timed_receipt("test-route", None, Outcome::Pass, ReceiptKind::Task)),
    );
    receipts.extend(
        (0..10).map(|_| timed_receipt("test-route", Some(1), Outcome::Fail, ReceiptKind::Task)),
    );
    receipts.extend((0..10).map(|_| {
        timed_receipt(
            "test-route",
            Some(1),
            Outcome::Pass,
            ReceiptKind::CancelledTurn,
        )
    }));
    let mut app = test_app_with_timing(
        None,
        RouteTimingHistory::from_receipts(&test_resolver(), receipts),
    );
    assert_eq!(app.typical_duration_ms, Some(12_000));
    let started_at = Utc::now() - chrono::Duration::seconds(20);
    app.set_status(Status::Working, started_at);

    let working = buffer_text(&render_buffer(&app, 180, 20));
    assert!(working.contains(", typically ~12s"), "got: {working}");

    apply_event(
        &mut app,
        AgentEvent::ModelStarted {
            route: "test-route".into(),
            started_at,
        },
    );
    let model_call = buffer_text(&render_buffer(&app, 180, 20));
    assert!(
        model_call.contains("WAITING test-route") && model_call.contains(", typically ~12s"),
        "got: {model_call}"
    );

    app.interrupt_turn();
    let interrupted = buffer_text(&render_buffer(&app, 180, 20));
    assert!(
        interrupted.contains("WORKING - interrupted turn")
            && interrupted.contains(", typically ~12s"),
        "got: {interrupted}"
    );
}

#[test]
fn route_typical_is_omitted_when_receipts_cannot_distinguish_route_aliases() {
    let resolver = RouteResolver::from_toml(
        r#"
        [routes.alias-a]
        provider = "test"
        model_id = "shared-model"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"

        [routes.alias-b]
        provider = "test"
        model_id = "shared-model"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
        "#,
    )
    .unwrap();
    let route = resolver.resolve("alias-a").unwrap();
    let receipts = (0..MIN_TYPICAL_DURATION_SAMPLES).map(|index| {
        timed_receipt(
            "shared-model",
            Some(10_000 + index as u64 * 1_000),
            Outcome::Pass,
            ReceiptKind::Task,
        )
    });
    let mut history = RouteTimingHistory::from_receipts(&resolver, receipts);

    assert_eq!(history.typical_for(route.id()), None);
    for duration_ms in [10_000, 11_000, 12_000, 13_000, 14_000] {
        history.record(
            route.id(),
            &timed_receipt(
                "shared-model",
                Some(duration_ms),
                Outcome::Pass,
                ReceiptKind::Task,
            ),
        );
    }
    assert_eq!(history.typical_for(route.id()), Some(12_000));
    assert_eq!(history.typical_for("alias-b"), None);
}

#[test]
fn heartbeat_formats_two_elapsed_deltas() {
    let now = fixed_at();
    for (seconds, expected) in [(2, "● WORKING · 2s"), (34, "● WORKING · 34s")] {
        let since = now - chrono::Duration::seconds(seconds);
        let (label, _) = status_chip(&Status::Working, Some(since), now, usize::MAX, None);
        assert_eq!(label, expected);
    }
}

#[test]
fn in_flight_tool_event_shows_name_and_elapsed_seconds_until_finish() {
    let mut app = test_app(None);
    let started_at = fixed_at();
    app.set_status(Status::Working, started_at);
    apply_event(
        &mut app,
        AgentEvent::ToolStarted {
            name: "exec_shell".into(),
            started_at,
        },
    );

    let (label, _) = tool_status_chip(
        "exec_shell",
        started_at,
        started_at + chrono::Duration::seconds(34),
    );
    assert_eq!(label, "● TOOL exec_shell · 34s");
    assert_eq!(
        app.active_tool.as_ref().map(|tool| tool.name.as_str()),
        Some("exec_shell")
    );

    apply_event(
        &mut app,
        AgentEvent::ToolFinished {
            name: "exec_shell".into(),
        },
    );
    assert!(app.active_tool.is_none());
}

#[test]
fn in_flight_model_event_shows_route_and_factual_elapsed_time_until_finish() {
    let mut app = test_app(None);
    let started_at = fixed_at();
    app.set_status(Status::Working, started_at);
    apply_event(
        &mut app,
        AgentEvent::ModelStarted {
            route: "other-route".into(),
            started_at,
        },
    );

    let (label, _) = model_status_chip(
        "other-route",
        started_at,
        started_at + chrono::Duration::seconds(34),
        None,
    );
    assert_eq!(label, "● WAITING other-route · 34s");
    assert!(!label.contains('%'));
    assert!(!label.to_ascii_lowercase().contains("estimated"));
    assert_eq!(
        app.active_model
            .as_ref()
            .map(|request| request.route.as_str()),
        Some("other-route")
    );

    apply_event(
        &mut app,
        AgentEvent::ModelFinished {
            route: "other-route".into(),
            usage: None,
        },
    );
    assert!(app.active_model.is_none());
}

#[test]
fn teaching_error_contains_cause_and_next_action() {
    let error = teaching_error("unknown model 'x'", "run /model to list routes");
    assert!(error.contains("unknown model 'x'"));
    assert!(error.contains("run /model to list routes"));
    assert!(error.contains(" - "));
}

#[test]
fn taskbar_semaforo_writes_only_on_waiting_transitions() {
    let mut bytes = Vec::new();
    let now = fixed_at();
    emit_taskbar_transition(&mut bytes, &Status::Working, &Status::Waiting, None, now).unwrap();
    emit_taskbar_transition(&mut bytes, &Status::Waiting, &Status::Waiting, None, now).unwrap();
    emit_taskbar_transition(&mut bytes, &Status::Waiting, &Status::Working, None, now).unwrap();
    assert_eq!(&bytes[..TASKBAR_WAITING.len()], TASKBAR_WAITING);
    assert_eq!(&bytes[TASKBAR_WAITING.len()..], TASKBAR_CLEAR);
}

#[test]
fn turn_finish_sets_idle_title_without_bell_under_threshold() {
    let now = fixed_at();
    let mut bytes = Vec::new();
    emit_taskbar_transition(
        &mut bytes,
        &Status::Working,
        &Status::Idle,
        Some(now - chrono::Duration::seconds(9)),
        now,
    )
    .unwrap();

    let mut expected = TASKBAR_CLEAR.to_vec();
    expected.extend_from_slice(TITLE_IDLE);
    assert_eq!(bytes, expected);
}

#[test]
fn long_turn_finish_sets_blocked_title_and_rings_once() {
    let now = fixed_at();
    let mut bytes = Vec::new();
    emit_taskbar_transition(
        &mut bytes,
        &Status::Working,
        &Status::Blocked("offline".into()),
        Some(now - chrono::Duration::seconds(10)),
        now,
    )
    .unwrap();

    let mut expected = TASKBAR_CLEAR.to_vec();
    expected.extend_from_slice(TITLE_BLOCKED);
    expected.push(b'\x07');
    assert_eq!(bytes, expected);
    assert!(!String::from_utf8_lossy(&bytes).contains("offline"));
}

#[test]
fn non_finish_transition_emits_no_turn_signal() {
    let now = fixed_at();
    let mut bytes = Vec::new();
    emit_taskbar_transition(
        &mut bytes,
        &Status::Idle,
        &Status::Blocked("offline".into()),
        Some(now - chrono::Duration::seconds(20)),
        now,
    )
    .unwrap();

    assert!(bytes.is_empty());
}

#[test]
fn budget_reached_blocks_and_refuses_another_dispatch() {
    let mut app = test_app(Some(100));
    app.status = Status::Working;
    type_text(&mut app, "must not run");
    reduce_key(&mut app, code_key(KeyCode::Enter));
    assert!(app.pending_send);

    let (previous, usage_action) = reduce_agent_event(
        &mut app,
        AgentEvent::Usage(Usage {
            prompt_tokens: 80,
            completion_tokens: 20,
            cached_tokens: None,
            evidence: UsageEvidence::Measured,
        }),
    );
    assert_eq!(previous, Status::Working);
    assert_eq!(usage_action, UiAction::None);
    assert_eq!(app.status, Status::Blocked(BUDGET_REASON.into()));

    let (previous, answer_action) =
        reduce_agent_event(&mut app, AgentEvent::Answer("finished".into()));
    assert_eq!(previous, Status::Blocked(BUDGET_REASON.into()));
    assert_eq!(answer_action, UiAction::None);
    assert_eq!(app.status, Status::Blocked(BUDGET_REASON.into()));
    assert_eq!(app.input, "must not run");
    assert!(app.pending_send);
    assert!(app
        .transcript
        .iter()
        .all(|line| line.kind != TranscriptKind::Task));

    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Enter)),
        UiAction::None
    );
    assert_eq!(app.input, "must not run");
    let (_, repeated) = reduce_agent_event(&mut app, AgentEvent::Progress("meter settled".into()));
    assert_eq!(repeated, UiAction::None);
    assert!(app
        .transcript
        .iter()
        .all(|line| line.kind != TranscriptKind::Task));
    let hud = app.hud_line(Utc::now());
    assert!(hud.contains("[#######] 100%"), "got: {hud}");
    assert!(hud.contains("100/100"), "got: {hud}");
}

#[test]
fn measured_usage_warns_once_when_it_crosses_eighty_percent() {
    let mut app = test_app(Some(100));
    apply_event(
        &mut app,
        AgentEvent::Usage(Usage {
            prompt_tokens: 78,
            completion_tokens: 1,
            cached_tokens: Some(0),
            evidence: UsageEvidence::Measured,
        }),
    );
    assert!(!app.budget_warned);
    assert!(app
        .transcript
        .iter()
        .all(|line| !line.text.starts_with("budget warning:")));

    apply_event(
        &mut app,
        AgentEvent::Usage(Usage {
            prompt_tokens: 79,
            completion_tokens: 1,
            cached_tokens: Some(0),
            evidence: UsageEvidence::Measured,
        }),
    );
    assert!(app.budget_warned);
    let warning = "budget warning: 80 observed tokens of 100; new tasks stop when observed usage reaches the budget";
    assert_eq!(
        app.transcript
            .iter()
            .filter(|line| line.text == warning)
            .count(),
        1
    );
    let rendered = buffer_text(&render_buffer(&app, 100, 20));
    assert!(rendered.contains(warning), "got: {rendered}");

    apply_event(
        &mut app,
        AgentEvent::Usage(Usage {
            prompt_tokens: 88,
            completion_tokens: 2,
            cached_tokens: Some(0),
            evidence: UsageEvidence::Measured,
        }),
    );
    assert_eq!(
        app.transcript
            .iter()
            .filter(|line| line.text.starts_with("budget warning:"))
            .count(),
        1
    );
}

#[test]
fn measured_usage_that_immediately_blocks_does_not_add_an_approach_warning() {
    let mut app = test_app(Some(100));
    apply_event(
        &mut app,
        AgentEvent::Usage(Usage {
            prompt_tokens: 100,
            completion_tokens: 1,
            cached_tokens: Some(0),
            evidence: UsageEvidence::Measured,
        }),
    );

    assert_eq!(app.status, Status::Blocked(BUDGET_REASON.into()));
    assert!(!app.budget_warned);
    let rendered = buffer_text(&render_buffer(&app, 100, 20));
    assert!(!rendered.contains("budget warning:"), "got: {rendered}");
}

#[test]
fn unmeasured_usage_produces_no_budget_warning_line() {
    for evidence in [UsageEvidence::Partial, UsageEvidence::Unknown] {
        let mut app = test_app(Some(100));
        apply_event(
            &mut app,
            AgentEvent::Usage(Usage {
                prompt_tokens: 85,
                completion_tokens: 0,
                cached_tokens: None,
                evidence,
            }),
        );

        assert!(!app.budget_warned);
        assert!(app
            .transcript
            .iter()
            .all(|line| !line.text.starts_with("budget warning:")));
        assert_eq!(
            app.status,
            Status::Blocked(BUDGET_USAGE_UNAVAILABLE_REASON.into())
        );
        assert!(app.budget_blocks_dispatch());
        assert!(app.transcript.iter().any(|line| {
            line.text
                == "budget usage is incomplete or unavailable - no more tasks will be sent in this budgeted session"
        }));
        app.status = Status::Idle;
        app.input = "must not dispatch".into();
        assert_eq!(app.dispatch(), None);

        apply_event(
            &mut app,
            AgentEvent::Usage(Usage {
                prompt_tokens: 90,
                completion_tokens: 1,
                cached_tokens: Some(0),
                evidence: UsageEvidence::Measured,
            }),
        );
        assert!(!app.usage.as_ref().unwrap().evidence.is_measured());
        assert!(app.budget_blocks_dispatch());
        let rendered = buffer_text(&render_buffer(&app, 100, 20));
        assert!(!rendered.contains("budget warning:"), "got: {rendered}");
        assert!(
            rendered.contains("budget usage unavailable"),
            "got: {rendered}"
        );
    }
}

#[test]
fn unbudgeted_unknown_usage_does_not_block_dispatch() {
    let mut app = test_app(None);
    apply_event(
        &mut app,
        AgentEvent::Usage(Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            cached_tokens: None,
            evidence: UsageEvidence::Unknown,
        }),
    );
    app.input = "continue without a token budget".into();

    assert!(!app.budget_blocks_dispatch());
    assert_eq!(
        app.dispatch().as_deref(),
        Some("continue without a token budget")
    );
}

#[test]
fn budget_warning_threshold_rounds_up_for_non_multiple_budgets() {
    let mut app = test_app(Some(101));
    apply_event(
        &mut app,
        AgentEvent::Usage(Usage {
            prompt_tokens: 80,
            completion_tokens: 0,
            cached_tokens: Some(0),
            evidence: UsageEvidence::Measured,
        }),
    );
    assert!(!app.budget_warned);

    apply_event(
        &mut app,
        AgentEvent::Usage(Usage {
            prompt_tokens: 81,
            completion_tokens: 0,
            cached_tokens: Some(0),
            evidence: UsageEvidence::Measured,
        }),
    );
    assert!(app.budget_warned);
}

#[test]
fn rendered_line_is_redacted_and_has_no_control_characters() {
    let secret = "hunter2-fake-tui-secret";
    let scrubber = Arc::new(RwLock::new(Scrubber::new(vec![secret.into()])));
    let mut app = App::new(
        test_resolver(),
        test_route(),
        None,
        scrubber,
        PolicyView {
            autonomy: Autonomy::Ask,
            auto_paths: Vec::new(),
            ask_paths: Vec::new(),
            block_paths: Vec::new(),
            block_commands: Vec::new(),
        },
        UiInputs {
            terminal_capability: TerminalCapability::Unicode,
            palette_entries: Vec::new(),
            credentialed_providers: Vec::new(),
            color_mode: ColorMode::Color,
            route_timing_history: empty_timing_history(),
            prompt_base_tokens: unavailable_prompt_estimate(),
        },
        (Profiles::bundled(), "balanced".into()),
    );
    apply_event(
        &mut app,
        AgentEvent::Progress(format!("value={secret}\r\x1b[2K")),
    );
    let rendered = &app.transcript[0].text;
    assert!(rendered.contains("[REDACTED]"), "got: {rendered}");
    assert!(!rendered.contains(secret), "got: {rendered}");
    assert!(!rendered.chars().any(char::is_control), "got: {rendered}");
}

#[test]
fn rendered_overlay_scrubs_descriptions_and_control_characters() {
    let secret = "hunter2-fake-overlay-secret";
    let scrubber = Arc::new(RwLock::new(Scrubber::new(vec![secret.into()])));
    let mut app = App::new(
        test_resolver(),
        test_route(),
        None,
        scrubber,
        PolicyView {
            autonomy: Autonomy::Ask,
            auto_paths: Vec::new(),
            ask_paths: Vec::new(),
            block_paths: Vec::new(),
            block_commands: Vec::new(),
        },
        UiInputs {
            terminal_capability: TerminalCapability::Unicode,
            palette_entries: vec![PaletteEntry {
                kind: "tool",
                name: "secret-tool".into(),
                description: format!("value={secret}\r\x1b[2K"),
                state: Some(McpState::Enabled),
                action: PaletteAction::Describe,
            }],
            credentialed_providers: Vec::new(),
            color_mode: ColorMode::Color,
            route_timing_history: empty_timing_history(),
            prompt_base_tokens: unavailable_prompt_estimate(),
        },
        (Profiles::bundled(), "balanced".into()),
    );
    app.overlay = Overlay::Palette {
        filter: "secret-tool".into(),
        selected: 0,
        detail: None,
    };
    let backend = TestBackend::new(100, 12);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let rendered: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(rendered.contains("[REDACTED]"), "got: {rendered}");
    assert!(!rendered.contains(secret), "got: {rendered}");
    assert!(!rendered.contains('\r'), "got: {rendered}");
    assert!(!rendered.contains('\x1b'), "got: {rendered}");
}

#[test]
fn rendered_timeline_scrubs_every_receipt_and_answer_line() {
    let secret = "sk-timeline-00000000";
    let mut app = test_app(None);
    apply_event(
        &mut app,
        AgentEvent::TaskReceipt(TimelineSummary {
            route_id: "test-route".into(),
            receipt: receipt(
                &format!("task value={secret}\r\x1b[2K"),
                Outcome::Pass,
                None,
            ),
            answer: format!("answer value={secret}\r\x1b[2K"),
        }),
    );
    app.overlay = Overlay::Timeline {
        selected: 0,
        inspecting: true,
        note: None,
    };
    let backend = TestBackend::new(120, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|frame| render(frame, &app)).unwrap();
    let rendered: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    assert!(rendered.contains("[REDACTED]"), "got: {rendered}");
    assert!(!rendered.contains(secret), "got: {rendered}");
    assert!(!rendered.contains('\r'), "got: {rendered}");
    assert!(!rendered.contains('\x1b'), "got: {rendered}");
}

#[test]
fn identity_constitution_is_stable_and_names_the_route_honestly() {
    let route = test_route();
    let first = identity_constitution("law bytes", &route, false);
    let second = identity_constitution("law bytes", &route, false);

    assert_eq!(first, second);
    assert!(first.contains("test-route"), "got: {first}");
    assert!(first.contains("via test"), "got: {first}");
    assert!(first.contains("never claim to be Claude"), "got: {first}");
    assert!(
        first.contains(nh_core::agent::TOOL_RESULT_STATE_RULE),
        "got: {first}"
    );
    assert!(first.ends_with("law bytes"), "got: {first}");

    let denied = identity_constitution("law bytes", &route, true);
    assert!(denied.contains(nh_core::agent::SHELL_UNAVAILABLE_SESSION_NOTICE));
    assert!(denied.ends_with("law bytes"), "got: {denied}");
}

struct MockClient {
    request_lengths: Arc<Mutex<Vec<usize>>>,
}

#[derive(Debug)]
struct RecordedRequest {
    model: String,
    message_count: usize,
    system: String,
    history: Vec<(String, String)>,
    tools: Vec<String>,
    effort: ThinkingEffort,
}

struct RecordingClient {
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

struct MeasuredCacheClient;

struct MeterGapClient {
    calls: Arc<AtomicU64>,
}

type CapturedRequestHistory = Arc<Mutex<Vec<Vec<(String, String)>>>>;

struct BlockingMeasuredClient {
    calls: Arc<AtomicU64>,
    requests: CapturedRequestHistory,
    started: mpsc::Sender<()>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

struct FailingClient;

impl ChatClient for FailingClient {
    fn complete(&self, _request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        anyhow::bail!("provider failed after request")
    }
}

impl ChatClient for BlockingMeasuredClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push(
            request
                .messages
                .iter()
                .map(|message| {
                    (
                        message.role.clone(),
                        message.content.clone().unwrap_or_default(),
                    )
                })
                .collect(),
        );
        if call == 0 {
            self.started.send(()).unwrap();
            let (released, wake) = &*self.release;
            let mut released = released.lock().unwrap();
            while !*released {
                released = wake.wait(released).unwrap();
            }
        }
        let mut message = request.messages.last().cloned().expect("user message");
        message.role = "assistant".into();
        message.content = Some("ok".into());
        message.tool_calls = None;
        message.tool_call_id = None;
        message.reasoning_content = None;
        Ok(ChatResponse {
            message,
            finish_reason: "stop".into(),
            usage: Some(Usage {
                prompt_tokens: 31,
                completion_tokens: 7,
                cached_tokens: Some(11),
                evidence: UsageEvidence::Measured,
            }),
            retries: Default::default(),
        })
    }
}

impl ChatClient for MeterGapClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let mut message = request.messages.last().cloned().expect("user message");
        message.role = "assistant".into();
        message.content = Some("ok".into());
        message.tool_calls = None;
        message.tool_call_id = None;
        message.reasoning_content = None;
        Ok(ChatResponse {
            message,
            finish_reason: "stop".into(),
            usage: (call == 0).then_some(Usage {
                prompt_tokens: 10,
                completion_tokens: 2,
                cached_tokens: Some(4),
                evidence: UsageEvidence::Measured,
            }),
            retries: Default::default(),
        })
    }
}

impl ChatClient for RecordingClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        self.requests.lock().unwrap().push(RecordedRequest {
            model: request.model.clone(),
            message_count: request.messages.len(),
            system: request.messages[0].content.clone().unwrap_or_default(),
            history: request
                .messages
                .iter()
                .map(|message| {
                    (
                        message.role.clone(),
                        message.content.clone().unwrap_or_default(),
                    )
                })
                .collect(),
            tools: request.tools.iter().map(|tool| tool.name.clone()).collect(),
            effort: request.thinking,
        });
        let mut message = request.messages.last().cloned().expect("user message");
        message.role = "assistant".into();
        message.content = Some("ok".into());
        message.tool_calls = None;
        message.tool_call_id = None;
        message.reasoning_content = None;
        Ok(ChatResponse {
            message,
            finish_reason: "stop".into(),
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 2,
                cached_tokens: Some(4),
                evidence: UsageEvidence::Measured,
            }),
            retries: Default::default(),
        })
    }
}

impl ChatClient for MeasuredCacheClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let mut message = request.messages.last().cloned().expect("user message");
        message.role = "assistant".into();
        message.content = Some("ok".into());
        message.tool_calls = None;
        message.tool_call_id = None;
        message.reasoning_content = None;
        Ok(ChatResponse {
            message,
            finish_reason: "stop".into(),
            usage: Some(Usage {
                prompt_tokens: 900,
                completion_tokens: 2,
                cached_tokens: Some(600),
                evidence: UsageEvidence::Measured,
            }),
            retries: Default::default(),
        })
    }
}

impl ChatClient for MockClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        self.request_lengths
            .lock()
            .unwrap()
            .push(request.messages.len());
        let mut message = request.messages.last().cloned().expect("user message");
        message.role = "assistant".into();
        message.content = Some("ok".into());
        message.tool_calls = None;
        message.tool_call_id = None;
        message.reasoning_content = None;
        Ok(ChatResponse {
            message,
            finish_reason: "stop".into(),
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 2,
                cached_tokens: Some(4),
                evidence: UsageEvidence::Measured,
            }),
            retries: Default::default(),
        })
    }
}

fn temp_dir() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "nh-tui-test-{}-{epoch}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn receive_completed_task(worker: &Worker, app: &mut App) {
    let mut saw_answer = false;
    let mut saw_receipt = false;
    while !saw_answer || !saw_receipt {
        let event = worker
            .events
            .recv_timeout(Duration::from_secs(2))
            .expect("worker completes the task");
        saw_answer |= matches!(&event, AgentEvent::Answer(_));
        saw_receipt |= matches!(&event, AgentEvent::TaskReceipt(_));
        match event {
            AgentEvent::Approval(_) => panic!("mock never asks for approval"),
            AgentEvent::Failed(reason) => panic!("worker failed: {reason}"),
            event => {
                apply_event(app, event);
            }
        }
    }
}

#[test]
fn detached_worker_shutdown_is_reported_as_unclean() {
    let error = finish_worker_shutdown(Ok(()), WorkerShutdown::Detached).unwrap_err();

    assert!(error.to_string().contains("did not stop within 250 ms"));
    assert!(error.to_string().contains("detached"));
}

#[test]
fn worker_cancels_one_turn_after_measuring_it_then_runs_the_next_task() {
    let root = temp_dir();
    let calls = Arc::new(AtomicU64::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (started_tx, started_rx) = mpsc::channel();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let client_calls = Arc::clone(&calls);
    let client_requests = Arc::clone(&requests);
    let client_release = Arc::clone(&release);
    let connect: ConnectFn = Box::new(move |_, _| {
        Ok((
            Box::new(BlockingMeasuredClient {
                calls: Arc::clone(&client_calls),
                requests: Arc::clone(&client_requests),
                started: started_tx.clone(),
                release: Arc::clone(&client_release),
            }),
            nh_vault::secret("fake-key-cancelled-worker"),
        ))
    });
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
    let mut worker = spawn_worker(WorkerConfig {
        route: test_route(),
        profiles: Profiles::bundled(),
        active_profile: "balanced".into(),
        budget: None,
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: None,
        resume: None,
    })
    .unwrap();

    worker
        .commands
        .send(WorkerCommand::Task("interrupted".into()))
        .unwrap();
    started_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("provider call started");
    worker.cancel_turn();
    let (released, wake) = &*release;
    *released.lock().unwrap() = true;
    wake.notify_all();

    let cancelled = loop {
        match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
            AgentEvent::CancelledTurn(summary) => break summary,
            AgentEvent::Usage(_)
            | AgentEvent::Progress(_)
            | AgentEvent::Compaction(_)
            | AgentEvent::ModelStarted { .. }
            | AgentEvent::ModelFinished { .. }
            | AgentEvent::ToolStarted { .. }
            | AgentEvent::ToolFinished { .. } => {}
            AgentEvent::TaskReceipt(_) => panic!("cancelled turn emitted a task receipt"),
            AgentEvent::Answer(_) => panic!("cancelled turn emitted an answer"),
            AgentEvent::Approval(_) => panic!("mock never asks for approval"),
            AgentEvent::Failed(reason) => panic!("worker failed: {reason}"),
        }
    };
    assert_eq!(cancelled.receipt.kind, ReceiptKind::CancelledTurn);
    assert_eq!(
        cancelled.receipt.usage,
        Some(Usage {
            prompt_tokens: 31,
            completion_tokens: 7,
            cached_tokens: Some(11),
            evidence: UsageEvidence::Measured,
        })
    );

    worker
        .commands
        .send(WorkerCommand::Task("next".into()))
        .unwrap();
    let mut saw_answer = false;
    let mut saw_receipt = false;
    while !saw_answer || !saw_receipt {
        match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
            AgentEvent::Answer(answer) => {
                assert_eq!(answer, "ok");
                saw_answer = true;
            }
            AgentEvent::TaskReceipt(summary) => {
                assert_eq!(summary.receipt.kind, ReceiptKind::Task);
                assert_eq!(summary.receipt.task, "next");
                saw_receipt = true;
            }
            AgentEvent::Usage(_)
            | AgentEvent::Progress(_)
            | AgentEvent::Compaction(_)
            | AgentEvent::ModelStarted { .. }
            | AgentEvent::ModelFinished { .. }
            | AgentEvent::ToolStarted { .. }
            | AgentEvent::ToolFinished { .. } => {}
            AgentEvent::CancelledTurn(_) => panic!("next turn inherited cancellation"),
            AgentEvent::Approval(_) => panic!("mock never asks for approval"),
            AgentEvent::Failed(reason) => panic!("worker failed: {reason}"),
        }
    }

    let requests = requests.lock().unwrap();
    assert_eq!(
        requests.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![2, 4]
    );
    assert_eq!(requests[1][1], ("user".into(), "interrupted".into()));
    assert_eq!(requests[1][2], ("assistant".into(), "ok".into()));
    assert_eq!(requests[1][3], ("user".into(), "next".into()));
    drop(requests);
    let durable = std::fs::read(root.join(".nosis").join("receipts.jsonl")).unwrap();
    let durable = nh_core::receipt::parse_receipt_jsonl(&durable, usize::MAX).unwrap();
    assert_eq!(durable.len(), 2);
    let cancelled = &durable[0];
    assert_eq!(cancelled.kind, ReceiptKind::CancelledTurn);
    assert_eq!(
        cancelled.usage,
        Some(Usage {
            prompt_tokens: 31,
            completion_tokens: 7,
            cached_tokens: Some(11),
            evidence: UsageEvidence::Measured,
        })
    );
    let next = &durable[1];
    assert_eq!(next.kind, ReceiptKind::Task);
    assert_eq!(next.task, "next");
    assert_eq!(worker.shutdown(), WorkerShutdown::Clean);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn worker_error_projects_cores_real_receipt_and_unknown_meter() {
    let root = temp_dir();
    let connect: ConnectFn = Box::new(|_, _| {
        Ok((
            Box::new(FailingClient),
            nh_vault::secret("fake-key-failing-provider"),
        ))
    });
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
    let mut worker = spawn_worker(WorkerConfig {
        route: test_route(),
        profiles: Profiles::bundled(),
        active_profile: "balanced".into(),
        budget: None,
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: None,
        resume: None,
    })
    .unwrap();
    worker
        .commands
        .send(WorkerCommand::Task("failing task".into()))
        .unwrap();
    let mut app = meter_app();
    app.budget = Some(100);
    let mut projected_receipt = None;
    let mut saw_usage = false;
    loop {
        let event = worker
            .events
            .recv_timeout(Duration::from_secs(2))
            .expect("failed worker reports its receipt");
        let terminal = matches!(&event, AgentEvent::Failed(_));
        match &event {
            AgentEvent::TaskReceipt(summary) => {
                assert!(projected_receipt.is_none());
                projected_receipt = Some(summary.receipt.clone());
            }
            AgentEvent::Usage(_) => {
                assert!(projected_receipt.is_some(), "usage preceded its receipt");
                saw_usage = true;
            }
            AgentEvent::Failed(_) => {
                assert!(projected_receipt.is_some(), "failure preceded its receipt");
                assert!(saw_usage, "failure preceded cumulative usage");
            }
            _ => {}
        }
        apply_event(&mut app, event);
        if terminal {
            break;
        }
    }
    let projected_receipt = projected_receipt.unwrap();

    let durable = std::fs::read(root.join(".nosis").join("receipts.jsonl")).unwrap();
    let expected = format!(
        "{}\n",
        nh_core::serialize_scrubbed_json(
            &projected_receipt,
            &Scrubber::new(Vec::new()),
            "receipt",
        )
        .unwrap()
    );
    assert_eq!(
        durable,
        expected.into_bytes(),
        "timeline receipt must be the exact durable receipt bytes"
    );

    assert_eq!(
        app.session_money(fixed_at()),
        "unavailable - meter incomplete"
    );
    assert_eq!(app.usage.as_ref().unwrap().evidence, UsageEvidence::Unknown);
    assert_eq!(
        app.status,
        Status::Blocked(BUDGET_USAGE_UNAVAILABLE_REASON.into())
    );
    let failure_line = app
        .transcript
        .iter()
        .find(|line| line.kind == TranscriptKind::Error)
        .expect("failure remains visible");
    assert!(failure_line.text.contains("budget usage unavailable"));
    assert!(!failure_line.text.contains("retry the task"));
    assert_eq!(app.timeline.len(), 1);
    assert_eq!(app.timeline[0].ts_utc, projected_receipt.ts_utc);
    assert_eq!(app.timeline[0].model_id, projected_receipt.model_id);
    assert_eq!(app.timeline[0].task, projected_receipt.task);
    assert_eq!(app.timeline[0].turns, 1);
    assert_eq!(app.timeline[0].tool_calls, 0);
    assert_eq!(app.timeline[0].outcome, Outcome::Fail);
    assert_eq!(
        app.timeline[0].failure_class,
        Some(FailureClass::Verification)
    );
    assert!(app.timeline[0].usage.is_none());
    assert_eq!(worker.shutdown(), WorkerShutdown::Clean);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn real_worker_and_ledger_replay_mark_a_measured_plus_unmetered_session() {
    let root = temp_dir();
    let calls = Arc::new(AtomicU64::new(0));
    let client_calls = Arc::clone(&calls);
    let connect: ConnectFn = Box::new(move |_, _| {
        Ok((
            Box::new(MeterGapClient {
                calls: Arc::clone(&client_calls),
            }),
            nh_vault::secret("fake-key-meter-gap"),
        ))
    });
    let resolver = RouteResolver::from_toml(METER_CATALOG).unwrap();
    let route = resolver.resolve("meter-route").unwrap();
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
    let mut worker = spawn_worker(WorkerConfig {
        route,
        profiles: Profiles::bundled(),
        active_profile: "balanced".into(),
        budget: Some(100),
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: None,
        resume: None,
    })
    .unwrap();
    let mut app = meter_app();
    app.budget = Some(100);

    for task in ["metered", "unmetered"] {
        worker
            .commands
            .send(WorkerCommand::Task(task.into()))
            .unwrap();
        let mut saw_receipt = false;
        let mut saw_usage = false;
        loop {
            let event = worker.events.recv_timeout(Duration::from_secs(2)).unwrap();
            let done = matches!(&event, AgentEvent::Answer(_));
            match &event {
                AgentEvent::TaskReceipt(_) => saw_receipt = true,
                AgentEvent::Usage(_) => {
                    assert!(saw_receipt, "usage preceded its receipt");
                    saw_usage = true;
                }
                AgentEvent::Answer(_) => {
                    assert!(saw_receipt, "answer preceded its receipt");
                    assert!(saw_usage, "answer preceded cumulative usage");
                }
                _ => {}
            }
            apply_event(&mut app, event);
            if done {
                break;
            }
        }
    }

    let usage = app.usage.as_ref().unwrap();
    assert_eq!(usage.evidence, UsageEvidence::Partial);
    assert_eq!((usage.prompt_tokens, usage.completion_tokens), (10, 2));
    assert!(app.session_money(fixed_at()).starts_with('~'));
    assert!(app
        .session_money(fixed_at())
        .contains("subtotal; meter incomplete"));
    let hud = app.hud_line(fixed_at());
    assert!(
        hud.contains("in ~10 · out ~2 · token lower bound"),
        "got: {hud}"
    );
    assert!(hud.contains("~12% ~12/100 lower bound"), "got: {hud}");
    assert_eq!(
        app.status,
        Status::Blocked(BUDGET_USAGE_UNAVAILABLE_REASON.into())
    );
    let duration = measured_duration(
        app.timeline[1]
            .duration_ms
            .expect("the real turn should carry its measured duration"),
    );
    let row = timeline_row(&app.timeline[1]);
    assert_eq!(row, format!("#2  completed  {duration}  usage unreported"));
    assert!(!duration.contains('~'));
    let detail = timeline_detail_lines(&app.timeline[1]);
    assert!(detail.contains(&format!("duration: {duration}")));
    assert!(detail.contains(&"tokens: unavailable - usage unreported".into()));

    assert_eq!(worker.shutdown(), WorkerShutdown::Clean);
    let sessions = list_sessions(&root).unwrap();
    assert_eq!(sessions.sessions.len(), 1);
    let restored = read_session(&root, &sessions.sessions[0].session_id).unwrap();
    assert_eq!(restored.budget, Some(SessionBudget::Tokens { limit: 100 }));
    assert_eq!(restored.turns.len(), 2);
    assert_eq!(
        restored.turns[0].usage.as_ref().unwrap().evidence,
        UsageEvidence::Measured
    );
    assert!(restored.turns[1].usage.is_none());

    let mut replay = meter_app();
    replay.budget = Some(100);
    restore_app(&mut replay, &restored, "law bytes").unwrap();
    assert_eq!(
        replay.usage.as_ref().unwrap().evidence,
        UsageEvidence::Partial
    );
    assert!(replay.session_money(fixed_at()).starts_with('~'));
    let replay_hud = replay.hud_line(fixed_at());
    assert!(replay_hud.contains("~12% ~12/100 lower bound"));
    assert!(replay_hud.contains("resumed"));
    assert_eq!(
        replay.status,
        Status::Blocked(BUDGET_USAGE_UNAVAILABLE_REASON.into())
    );

    let free_catalog = METER_CATALOG
        .replace("cache_hit = 0.02", "cache_hit = 0.0")
        .replace("cache_miss = 1.0", "cache_miss = 0.0")
        .replace("output = 2.0", "output = 0.0");
    let mut free_replay = meter_app_from(&free_catalog);
    restore_app(&mut free_replay, &restored, "law bytes").unwrap();
    let free_money = free_replay.session_money(fixed_at());
    assert_eq!(free_money, "unavailable - meter incomplete");
    assert!(!free_money.contains("0.00"));
    free_replay.add_session_cost(Currency::Usd, 0.01, false, false);
    let mixed_free_money = free_replay.session_money(fixed_at());
    assert!(mixed_free_money.starts_with("~$0.01"));
    assert!(!mixed_free_money.contains("¥0.00"));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn torn_session_tail_makes_budget_usage_and_cost_incomplete_on_restore() {
    let mut restored = fold_session(&[
        SessionEvent::Started {
            session_id: "torn-budget".into(),
            surface: Surface::Tui,
            route_id: "meter-route".into(),
            model_id: "meter-route".into(),
            profile: "balanced".into(),
            created_utc: "2026-09-20T12:00:00Z".into(),
            budget: Some(SessionBudget::Tokens { limit: 100 }),
        },
        SessionEvent::Turn {
            ts_utc: "2026-09-20T12:01:00Z".into(),
            route_id: "meter-route".into(),
            messages: Vec::new(),
            usage: Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 2,
                cached_tokens: Some(4),
                evidence: UsageEvidence::Measured,
            }),
        },
    ])
    .unwrap();
    restored.dropped_torn_tail = true;
    let mut app = meter_app();
    app.budget = restored
        .budget
        .as_ref()
        .and_then(SessionBudget::token_limit);

    restore_app(&mut app, &restored, "law bytes").unwrap();

    let usage = app.usage.as_ref().unwrap();
    assert_eq!(usage.evidence, UsageEvidence::Partial);
    assert_eq!((usage.prompt_tokens, usage.completion_tokens), (10, 2));
    assert_eq!(
        app.status,
        Status::Blocked(BUDGET_USAGE_UNAVAILABLE_REASON.into())
    );
    assert!(app.session_money(fixed_at()).contains("meter incomplete"));
    assert!(app
        .transcript
        .iter()
        .any(|line| { line.text.contains("in-flight usage may be missing") }));
}

#[test]
fn queued_task_transition_is_forwarded_to_the_worker_exactly_once() {
    let root = temp_dir();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let requests_for_connect = Arc::clone(&requests);
    let connect: ConnectFn = Box::new(move |_, _| {
        Ok((
            Box::new(RecordingClient {
                requests: Arc::clone(&requests_for_connect),
            }),
            nh_vault::secret("fake-key-queued-worker"),
        ))
    });
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
    let mut worker = spawn_worker(WorkerConfig {
        route: test_route(),
        profiles: Profiles::bundled(),
        active_profile: "balanced".into(),
        budget: None,
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: None,
        resume: None,
    })
    .unwrap();
    let mut app = test_app(None);

    type_text(&mut app, "first task");
    assert!(!handle_key(&mut app, &mut worker, code_key(KeyCode::Enter)));
    type_text(&mut app, "queued task");
    reduce_key(&mut app, code_key(KeyCode::Enter));
    assert!(app.pending_send);

    let mut answers = 0;
    while answers < 2 {
        let event = worker
            .events
            .recv_timeout(Duration::from_secs(2))
            .expect("queued worker task completes");
        match &event {
            AgentEvent::Approval(_) => panic!("recording client never asks for approval"),
            AgentEvent::Failed(reason) => panic!("worker failed: {reason}"),
            AgentEvent::Answer(_) => answers += 1,
            _ => {}
        }
        let (_, should_quit) = handle_agent_event(&mut app, &mut worker, event);
        assert!(!should_quit);
    }

    assert_eq!(
        app.transcript
            .iter()
            .filter(|line| line.kind == TranscriptKind::Task)
            .count(),
        2
    );
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert_eq!(worker.shutdown(), WorkerShutdown::Clean);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn model_switch_keeps_worker_history_transcript_and_updates_route_identity() {
    let root = temp_dir();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let requests_for_connect = Arc::clone(&requests);
    let connect: ConnectFn = Box::new(move |route, _| {
        Ok((
            Box::new(RecordingClient {
                requests: Arc::clone(&requests_for_connect),
            }),
            nh_vault::secret(format!("fake-key-{}", route.vault_entry())),
        ))
    });
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
    let mut worker = spawn_worker(WorkerConfig {
        route: test_route(),
        profiles: Profiles::bundled(),
        active_profile: "balanced".into(),
        budget: None,
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: None,
        resume: None,
    })
    .unwrap();
    let mut app = test_app(None);

    app.input = "first task".into();
    assert!(!handle_key(&mut app, &mut worker, code_key(KeyCode::Enter)));
    receive_completed_task(&worker, &mut app);
    let retained: Vec<_> = app
        .transcript
        .iter()
        .map(|line| line.text.clone())
        .collect();

    type_text(&mut app, "/model other-route");
    assert!(!handle_key(&mut app, &mut worker, code_key(KeyCode::Enter)));
    assert_eq!(app.route.id(), "other-route");
    assert_eq!(app.timeline.len(), 1);
    assert_eq!(
        app.transcript
            .iter()
            .take(retained.len())
            .map(|line| line.text.clone())
            .collect::<Vec<_>>(),
        retained
    );
    assert_eq!(
        app.transcript.last().map(|line| line.text.as_str()),
        Some("switched to other-route - context kept, cache resets")
    );

    type_text(&mut app, "/effort high");
    assert!(!handle_key(&mut app, &mut worker, code_key(KeyCode::Enter)));
    assert_eq!(app.effort, ThinkingEffort::High);

    app.input = "second task".into();
    assert!(!handle_key(&mut app, &mut worker, code_key(KeyCode::Enter)));
    receive_completed_task(&worker, &mut app);
    assert_eq!(app.timeline.len(), 2);

    assert_eq!(worker.shutdown(), WorkerShutdown::Clean);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2, "requests: {requests:#?}");
    assert_eq!(requests[0].model, "test-route");
    assert_eq!(requests[0].message_count, 2);
    assert!(requests[0].system.contains("nosis on test-route"));
    assert_eq!(requests[0].effort, ThinkingEffort::None);
    assert_eq!(requests[1].model, "other-route");
    assert_eq!(requests[1].message_count, 5, "history was not kept");
    assert!(requests[1].system.contains("nosis on test-route"));
    assert!(requests[1].system.contains("never claim to be Claude"));
    assert_eq!(requests[1].history[0].0, "system");
    assert!(requests[1].history[0].1.contains("nosis on test-route"));
    assert_eq!(requests[1].history[3].0, "system");
    assert!(requests[1].history[3].1.contains("nosis on other-route"));
    assert_eq!(
        requests[1]
            .history
            .iter()
            .filter(|(role, _)| role == "system")
            .count(),
        2
    );
    assert_eq!(requests[1].effort, ThinkingEffort::High);
    drop(requests);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn keyless_switch_accepts_route_then_next_task_surfaces_add_key_line() {
    let root = temp_dir();
    let request_lengths = Arc::new(Mutex::new(Vec::new()));
    let lengths_for_connect = Arc::clone(&request_lengths);
    let connect: ConnectFn = Box::new(move |route, _| {
        if route.id() == "other-route" {
            anyhow::bail!("no key found for \"other\" - run `nh key add other`");
        }
        Ok((
            Box::new(MockClient {
                request_lengths: Arc::clone(&lengths_for_connect),
            }),
            nh_vault::secret("fake-worker-secret"),
        ))
    });
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
    let mut worker = spawn_worker(WorkerConfig {
        route: test_route(),
        profiles: Profiles::bundled(),
        active_profile: "balanced".into(),
        budget: None,
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: None,
        resume: None,
    })
    .unwrap();
    worker
        .commands
        .send(WorkerCommand::SwitchRoute(Box::new(
            test_resolver().resolve("other-route").unwrap(),
        )))
        .unwrap();
    worker
        .commands
        .send(WorkerCommand::Task("hello".into()))
        .unwrap();

    let usage = worker.events.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        &usage,
        AgentEvent::Usage(Usage {
            evidence: UsageEvidence::Unknown,
            ..
        })
    ));
    let mut budgeted = meter_app();
    budgeted.budget = Some(100);
    apply_event(&mut budgeted, usage);
    assert_eq!(
        budgeted.status,
        Status::Blocked(BUDGET_USAGE_UNAVAILABLE_REASON.into())
    );
    match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
        AgentEvent::Failed(reason) => {
            assert!(reason.contains("nh key add other"), "got: {reason}");
            assert!(reason.contains("receipt unavailable"), "got: {reason}");
        }
        _ => panic!("keyless switched task must fail with one friendly line"),
    }
    assert!(worker
        .events
        .recv_timeout(Duration::from_millis(100))
        .is_err());
    assert!(request_lengths.lock().unwrap().is_empty());
    assert_eq!(worker.shutdown(), WorkerShutdown::Clean);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn worker_uses_injected_client_and_keeps_one_history_across_tasks() {
    let root = temp_dir();
    let request_lengths = Arc::new(Mutex::new(Vec::new()));
    let lengths_for_connect = Arc::clone(&request_lengths);
    let connect: ConnectFn = Box::new(move |_, _| {
        Ok((
            Box::new(MockClient {
                request_lengths: Arc::clone(&lengths_for_connect),
            }),
            nh_vault::secret("fake-worker-secret"),
        ))
    });
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
    let scrubber = Arc::new(RwLock::new(Scrubber::new(Vec::new())));
    let mut worker = spawn_worker(WorkerConfig {
        route: test_route(),
        profiles: Profiles::bundled(),
        active_profile: "balanced".into(),
        budget: None,
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber,
        connect,
        initial: None,
        resume: None,
    })
    .unwrap();

    for task in ["one", "two"] {
        worker
            .commands
            .send(WorkerCommand::Task(task.into()))
            .unwrap();
        let mut saw_answer = false;
        let mut saw_receipt = false;
        while !saw_answer || !saw_receipt {
            match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
                AgentEvent::Answer(answer) => {
                    assert_eq!(answer, "ok");
                    saw_answer = true;
                }
                AgentEvent::TaskReceipt(summary) => {
                    assert_eq!(summary.receipt.task, task);
                    assert_eq!(summary.receipt.outcome, Outcome::Pass);
                    assert_eq!(summary.answer, "ok");
                    saw_receipt = true;
                }
                AgentEvent::Usage(_)
                | AgentEvent::Progress(_)
                | AgentEvent::Compaction(_)
                | AgentEvent::ModelStarted { .. }
                | AgentEvent::ModelFinished { .. }
                | AgentEvent::ToolStarted { .. }
                | AgentEvent::ToolFinished { .. } => {}
                AgentEvent::Approval(_) => panic!("mock never asks for approval"),
                AgentEvent::CancelledTurn(_) => panic!("turn was not cancelled"),
                AgentEvent::Failed(reason) => panic!("worker failed: {reason}"),
            }
        }
    }
    assert_eq!(worker.shutdown(), WorkerShutdown::Clean);
    assert_eq!(*request_lengths.lock().unwrap(), vec![2, 4]);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn worker_emits_typed_compaction_with_exact_preceding_call_cache() {
    let root = temp_dir();
    let connect: ConnectFn = Box::new(|_, _| {
        Ok((
            Box::new(MeasuredCacheClient),
            nh_vault::secret("fake-worker-secret"),
        ))
    });
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
    let mut worker = spawn_worker(WorkerConfig {
        route: test_route(),
        profiles: Profiles::bundled(),
        active_profile: "balanced".into(),
        budget: None,
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: None,
        resume: None,
    })
    .unwrap();
    let mut app = test_app(None);

    for fill in ['a', 'b'] {
        worker
            .commands
            .send(WorkerCommand::Task(fill.to_string().repeat(1_200)))
            .unwrap();
        receive_completed_task(&worker, &mut app);
    }
    worker
        .commands
        .send(WorkerCommand::Task("c".repeat(1_200)))
        .unwrap();

    let mut live = None;
    let receipt = loop {
        let event = worker
            .events
            .recv_timeout(Duration::from_secs(2))
            .expect("third task compacts and completes");
        if let AgentEvent::Compaction(compaction) = &event {
            live = Some(*compaction);
        }
        let receipt = match &event {
            AgentEvent::TaskReceipt(summary) => Some(summary.receipt.clone()),
            _ => None,
        };
        apply_event(&mut app, event);
        if let Some(receipt) = receipt {
            break receipt;
        }
    };

    let live = live.expect("compaction is a typed worker event");
    assert_eq!(live.preceding_cached_tokens, Some(600));
    assert_eq!(receipt.compaction.events, 1);
    assert_eq!(receipt.compaction.messages_elided, live.messages_elided);
    assert_eq!(
        receipt.compaction.estimated_tokens_elided,
        live.estimated_tokens_elided
    );
    assert_eq!(receipt.compaction.preceding_cached_tokens, Some(600));
    assert_eq!(worker.shutdown(), WorkerShutdown::Clean);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn restored_worker_sends_restored_history_on_first_request() {
    let root = temp_dir();
    let live_requests = Arc::new(Mutex::new(Vec::new()));
    let requests_for_connect = Arc::clone(&live_requests);
    let live_connect: ConnectFn = Box::new(move |_, _| {
        Ok((
            Box::new(RecordingClient {
                requests: Arc::clone(&requests_for_connect),
            }),
            nh_vault::secret("fake-worker-secret"),
        ))
    });
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
    let mut live_worker = spawn_worker(WorkerConfig {
        route: test_route(),
        profiles: Profiles::bundled(),
        active_profile: "balanced".into(),
        budget: None,
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect: live_connect,
        initial: None,
        resume: None,
    })
    .unwrap();
    live_worker
        .commands
        .send(WorkerCommand::Task("before interruption".into()))
        .unwrap();
    receive_completed_task(&live_worker, &mut test_app(None));
    assert_eq!(live_worker.shutdown(), WorkerShutdown::Clean);
    assert_eq!(live_requests.lock().unwrap().len(), 1);

    let index = list_sessions(&root).unwrap();
    assert_eq!(index.sessions.len(), 1);
    let session_id = index.sessions[0].session_id.clone();
    let restored = read_session(&root, &session_id).unwrap();
    assert_eq!(restored.turns.len(), 1);
    let restored_message_count = restored.history.len();
    let restored_system = restored.history[0].content.clone().unwrap();
    std::fs::write(root.join(".nosis/law.toml"), "[exec]\nblock = [\"*\"]\n").unwrap();

    let resumed_requests = Arc::new(Mutex::new(Vec::new()));
    let requests_for_connect = Arc::clone(&resumed_requests);
    let resumed_connect: ConnectFn = Box::new(move |_, _| {
        Ok((
            Box::new(RecordingClient {
                requests: Arc::clone(&requests_for_connect),
            }),
            nh_vault::secret("fake-worker-secret"),
        ))
    });
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
    let mut resumed_worker = spawn_worker(WorkerConfig {
        route: test_route(),
        profiles: Profiles::bundled(),
        active_profile: "balanced".into(),
        budget: None,
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect: resumed_connect,
        initial: None,
        resume: Some(restored),
    })
    .unwrap();
    resumed_worker
        .commands
        .send(WorkerCommand::SwitchRoute(Box::new(test_route())))
        .unwrap();
    resumed_worker
        .commands
        .send(WorkerCommand::Task("after interruption".into()))
        .unwrap();
    receive_completed_task(&resumed_worker, &mut test_app(None));

    assert_eq!(resumed_worker.shutdown(), WorkerShutdown::Clean);
    let requests = resumed_requests.lock().unwrap();
    assert_eq!(requests.len(), 1, "requests: {requests:#?}");
    assert_eq!(requests[0].message_count, restored_message_count + 2);
    assert_eq!(requests[0].system, restored_system);
    assert_eq!(requests[0].history[0].0, "system");
    assert_eq!(requests[0].history[0].1, restored_system);
    assert_eq!(requests[0].history[1].1, "before interruption");
    assert_eq!(requests[0].history[2].1, "ok");
    assert_eq!(requests[0].history[3].0, "system");
    assert_eq!(
        requests[0].history[3].1,
        nh_core::agent::SHELL_UNAVAILABLE_SESSION_NOTICE
    );
    assert_eq!(requests[0].history[4].1, "after interruption");
    assert_eq!(
        requests[0]
            .history
            .iter()
            .filter(|(_, content)| {
                content.contains(nh_core::agent::SHELL_UNAVAILABLE_SESSION_NOTICE)
            })
            .count(),
        1,
        "reselecting the current route must preserve one pending correction"
    );
    assert!(!requests[0].tools.iter().any(|tool| tool == "exec_shell"));
    assert!(requests[0].tools.iter().any(|tool| tool == "edit_file"));
    drop(requests);
    let denied_history = read_session(&root, &session_id).unwrap();
    assert_eq!(denied_history.turns.len(), 2);

    std::fs::remove_file(root.join(".nosis/law.toml")).unwrap();
    let restored_requests = Arc::new(Mutex::new(Vec::new()));
    let requests_for_connect = Arc::clone(&restored_requests);
    let restored_connect: ConnectFn = Box::new(move |_, _| {
        Ok((
            Box::new(RecordingClient {
                requests: Arc::clone(&requests_for_connect),
            }),
            nh_vault::secret("fake-worker-secret"),
        ))
    });
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
    let mut restored_worker = spawn_worker(WorkerConfig {
        route: test_route(),
        profiles: Profiles::bundled(),
        active_profile: "balanced".into(),
        budget: None,
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect: restored_connect,
        initial: None,
        resume: Some(denied_history),
    })
    .unwrap();
    restored_worker
        .commands
        .send(WorkerCommand::SwitchRoute(Box::new(
            test_resolver().resolve("other-route").unwrap(),
        )))
        .unwrap();
    restored_worker
        .commands
        .send(WorkerCommand::Task("after shell restoration".into()))
        .unwrap();
    receive_completed_task(&restored_worker, &mut test_app(None));
    assert_eq!(restored_worker.shutdown(), WorkerShutdown::Clean);

    let requests = restored_requests.lock().unwrap();
    assert_eq!(requests.len(), 1, "requests: {requests:#?}");
    assert_eq!(requests[0].model, "other-route");
    let route_context = requests[0]
        .history
        .iter()
        .find(|(role, content)| role == "system" && content.contains("nosis on other-route"))
        .expect("switched route context");
    assert_eq!(
        route_context
            .1
            .matches(nh_core::agent::SHELL_AVAILABLE_SESSION_NOTICE)
            .count(),
        1
    );
    assert_eq!(
        requests[0]
            .history
            .iter()
            .filter(|(_, content)| {
                content.contains(nh_core::agent::SHELL_AVAILABLE_SESSION_NOTICE)
            })
            .count(),
        1
    );
    assert!(requests[0].tools.iter().any(|tool| tool == "exec_shell"));
    drop(requests);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn worker_profile_change_reconnects_with_clamp_and_records_next_turn() {
    let root = temp_dir();
    let seen_caps = Arc::new(Mutex::new(Vec::new()));
    let caps_for_connect = Arc::clone(&seen_caps);
    let request_lengths = Arc::new(Mutex::new(Vec::new()));
    let lengths_for_connect = Arc::clone(&request_lengths);
    let connect: ConnectFn = Box::new(move |_, output_cap| {
        caps_for_connect.lock().unwrap().push(output_cap);
        Ok((
            Box::new(MockClient {
                request_lengths: Arc::clone(&lengths_for_connect),
            }),
            nh_vault::secret("fake-worker-secret"),
        ))
    });
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
    let mut worker = spawn_worker(WorkerConfig {
        route: test_route(),
        profiles: Profiles::bundled(),
        active_profile: "balanced".into(),
        budget: None,
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: None,
        resume: None,
    })
    .unwrap();

    worker
        .commands
        .send(WorkerCommand::SetProfile("frugal".into()))
        .unwrap();
    worker
        .commands
        .send(WorkerCommand::Task("profiled turn".into()))
        .unwrap();

    let receipt = loop {
        match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
            AgentEvent::TaskReceipt(summary) => break summary.receipt,
            AgentEvent::Usage(_)
            | AgentEvent::Answer(_)
            | AgentEvent::Progress(_)
            | AgentEvent::Compaction(_)
            | AgentEvent::ModelStarted { .. }
            | AgentEvent::ModelFinished { .. }
            | AgentEvent::ToolStarted { .. }
            | AgentEvent::ToolFinished { .. } => {}
            AgentEvent::Approval(_) => panic!("mock never asks for approval"),
            AgentEvent::CancelledTurn(_) => panic!("turn was not cancelled"),
            AgentEvent::Failed(reason) => panic!("worker failed: {reason}"),
        }
    };
    assert_eq!(
        *seen_caps.lock().unwrap(),
        vec![Some(16_384), Some(16_384)],
        "profile change rebuilds the client with the clamped output cap"
    );
    assert_eq!(receipt.effective_profile.as_deref(), Some("frugal"));

    assert_eq!(worker.shutdown(), WorkerShutdown::Clean);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn keyless_worker_starts_and_task_surfaces_the_add_key_line() {
    let root = temp_dir();
    let message = "no key found for \"test\" - run `nh key add test`";
    let connect: ConnectFn = Box::new(move |_, _| anyhow::bail!("{message}"));
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
    let mut worker = spawn_worker(WorkerConfig {
        route: test_route(),
        profiles: Profiles::bundled(),
        active_profile: "balanced".into(),
        budget: None,
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: Some(Err(anyhow::anyhow!("{message}"))),
        resume: None,
    })
    .unwrap();
    worker
        .commands
        .send(WorkerCommand::Task("hello".into()))
        .unwrap();
    assert!(matches!(
        worker.events.recv_timeout(Duration::from_secs(2)).unwrap(),
        AgentEvent::Usage(Usage {
            evidence: UsageEvidence::Unknown,
            ..
        })
    ));
    match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
        AgentEvent::Failed(reason) => {
            assert!(reason.contains("nh key add test"), "got: {reason}");
            assert!(reason.contains("receipt unavailable"), "got: {reason}");
            assert!(!reason.chars().any(char::is_control), "got: {reason}");
        }
        _ => panic!("keyless task must fail with one friendly line"),
    }
    assert!(worker
        .events
        .recv_timeout(Duration::from_millis(100))
        .is_err());
    assert_eq!(worker.shutdown(), WorkerShutdown::Clean);
    std::fs::remove_dir_all(root).unwrap();
}
