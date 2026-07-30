use super::*;
use crate::palette::trust_dial_lines;
use crate::state::{Overlay, PickerKind, UiDiscovery};
use crate::worker::WorkerCommand;
use chrono::{DateTime, Utc};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use nh_core::agent::MAX_TASK_BYTES;
use nh_core::wire::{ChatRequest, ChatResponse};
use ratatui::{
    backend::TestBackend,
    layout::Rect,
    style::{Color, Modifier},
    text::Line,
    widgets::{Paragraph, Wrap},
    Terminal,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
"#;

// Far-future `valid_until` = "fresh" for fixture purposes, matching the
// 2099-01-01 sentinel in nh-mcp. Tests here that need a stale price inject
// their own clock; a dated fixture would age out and break the live-clock test.
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
    valid_until = "2099-01-01"
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
    valid_until = "2099-01-01"

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
    valid_until = "2099-01-01"

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
    valid_until = "2099-01-01"

    [routes.d-unknown-price]
    provider = "delta"
    model_id = "d-unknown-price"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "delta"
    context = 100000

    [routes.e-stale]
    provider = "epsilon"
    model_id = "e-stale"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "epsilon"
    context = 100000
    [routes.e-stale.price]
    currency = "USD"
    unit = "per_million_tokens"
    cache_hit = 0.3
    cache_miss = 0.3
    output = 0.3
    price_confidence = "confirmed"
    valid_until = "2020-01-01"

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

fn test_app(budget: Option<u64>) -> App {
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
        UiDiscovery {
            palette_entries: Vec::new(),
            credentialed_providers: Vec::new(),
        },
        (Profiles::bundled(), "balanced".into()),
    )
}

fn meter_app() -> App {
    let resolver = RouteResolver::from_toml(METER_CATALOG).unwrap();
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
        UiDiscovery {
            palette_entries: Vec::new(),
            credentialed_providers: Vec::new(),
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
        UiDiscovery {
            palette_entries: Vec::new(),
            credentialed_providers: vec!["alpha".into(), "beta".into()],
        },
        (Profiles::bundled(), "balanced".into()),
    )
}

fn fixed_at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-07-18T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn render_buffer(app: &App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render(frame, app)).unwrap();
    terminal.backend().buffer().clone()
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

fn find_ascii_text(buffer: &ratatui::buffer::Buffer, needle: &str) -> (u16, u16) {
    for (y, row) in buffer_rows(buffer).iter().enumerate() {
        if let Some(x) = row.find(needle) {
            return (u16::try_from(x).unwrap(), u16::try_from(y).unwrap());
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
            reply,
        }),
        answers,
    )
}

fn receipt(task: &str, outcome: Outcome, usage: Option<Usage>) -> Receipt {
    Receipt {
        ts_utc: "2026-07-14T12:00:00Z".into(),
        model_id: "test-route".into(),
        task: task.into(),
        turns: 3,
        tool_calls: 2,
        outcome,
        failure_class: (outcome != Outcome::Pass).then_some(FailureClass::Constraint),
        usage,
        effective_profile: None,
    }
}

fn timeline_event(task: &str, answer: &str) -> AgentEvent {
    AgentEvent::TaskReceipt(TimelineSummary {
        receipt: receipt(
            task,
            Outcome::Pass,
            Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 2,
                cached_tokens: Some(4),
            }),
        ),
        answer: answer.into(),
    })
}

#[test]
fn outer_frame_and_each_status_word_render() {
    let cases = [
        (Status::Idle, "○ IDLE"),
        (Status::Working, "● WORKING"),
        (Status::Waiting, "● WAITING ON YOU"),
        (Status::Blocked("offline".into()), "● BLOCKED"),
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
        fresh.contains("/ commands   ↑↓ scroll   Enter send   Ctrl+C quit"),
        "got: {fresh}"
    );

    app.input = "start".into();
    app.dispatch().unwrap();
    let active = buffer_text(&render_buffer(&app, 90, 20));
    assert!(!active.contains("Welcome to nosis."), "got: {active}");
    assert!(active.contains("❯ you"), "got: {active}");
    assert!(active.contains("   start"), "got: {active}");
    assert!(
        active.contains("/ commands   ↑↓ scroll   Enter send   Ctrl+C quit"),
        "got: {active}"
    );
}

#[test]
fn centered_modal_frames_clear_transcript_for_every_overlay() {
    let terminal = Rect::new(0, 0, 100, 30);
    let cases = [
        (
            Overlay::CommandMenu { selected: 0 },
            modal_area(terminal, 14),
            "Commands",
        ),
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
        UiDiscovery {
            palette_entries: Vec::new(),
            credentialed_providers: Vec::new(),
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
        (KeyCode::Char('y'), true),
        (KeyCode::Char('Y'), true),
        (KeyCode::Char('n'), false),
        (KeyCode::Char('N'), false),
        (KeyCode::Esc, false),
    ] {
        let mut app = test_app(None);
        app.status = Status::Working;
        let (event, answer) = approval("cargo test --workspace");
        apply_event(&mut app, event);
        assert_eq!(reduce_key(&mut app, code_key(key)), UiAction::None);
        assert_eq!(answer.recv().unwrap(), expected);
        assert!(app.pending_approval.is_none());
        assert_eq!(app.status, Status::Working);
    }

    let mut app = test_app(None);
    app.status = Status::Working;
    let (event, answer) = approval("cargo test --workspace");
    apply_event(&mut app, event);
    assert_eq!(
        reduce_key(&mut app, code_key(KeyCode::Char('x'))),
        UiAction::None
    );
    assert_eq!(answer.try_recv(), Err(TryRecvError::Empty));
    assert!(app.pending_approval.is_some());
    assert_eq!(app.status, Status::Waiting);
}

#[test]
fn approval_reducer_ignores_non_shift_modifiers() {
    for character in ['a', 'y', 'n'] {
        let mut app = test_app(None);
        app.status = Status::Working;
        let (event, answer) = approval("cargo test --workspace");
        apply_event(&mut app, event);

        let key = KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL);
        assert_eq!(reduce_key(&mut app, key), UiAction::None);
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
    reduce_key(&mut app, char_key('y'));
    assert!(answer.recv().unwrap());
    assert_eq!(app.working_since, Some(started));

    apply_event(&mut app, AgentEvent::Answer("done".into()));
    assert_eq!(app.working_since, None);
}

#[test]
fn approval_always_records_and_auto_approves_identical_action() {
    let mut uppercase = test_app(None);
    uppercase.status = Status::Working;
    let (event, answer) = approval("cargo test");
    apply_event(&mut uppercase, event);
    reduce_key(&mut uppercase, char_key('A'));
    assert!(answer.recv().unwrap());
    assert_eq!(uppercase.session_allow, vec!["cargo test"]);

    let mut app = test_app(None);
    app.status = Status::Working;
    let action = "cargo test --workspace";
    let (first, first_answer) = approval(action);
    apply_event(&mut app, first);
    reduce_key(&mut app, char_key('a'));
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
fn approval_row_names_the_command_and_is_amber_reversed() {
    let mut app = test_app(None);
    app.status = Status::Working;
    let (event, _answer) = approval("cargo test --workspace");
    apply_event(&mut app, event);

    let buffer = render_buffer(&app, 100, 20);
    let text = buffer_text(&buffer);
    let (x, y) = find_ascii_text(&buffer, "approve:");
    let cell = &buffer[(x, y)];
    assert!(
        text.contains("approve: cargo test --workspace   [y] yes  [a] always  [n]/[Esc] no"),
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
fn failed_billed_turn_marks_the_session_meter_incomplete() {
    let mut app = meter_app();
    apply_event(&mut app, AgentEvent::MeterIncomplete);

    let money = app.session_money(fixed_at());
    assert!(
        money.contains("? incomplete - failed turn usage not reported"),
        "got: {money}"
    );
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
            }),
        ),
        "partial answer".into(),
        false,
    );

    assert_eq!(entry.turn, 7);
    assert_eq!(entry.outcome, Outcome::Partial);
    assert_eq!(entry.tokens(), (120, 30, 50));
    assert_eq!(entry.turns, 3);
    assert_eq!(entry.tool_calls, 2);
    assert_eq!(entry.answer, "partial answer");
    assert!(!entry.compacted);
}

#[test]
fn compaction_progress_marks_only_the_current_timeline_turn() {
    let mut app = test_app(None);
    app.status = Status::Working;
    apply_event(
        &mut app,
        AgentEvent::Progress("context 73% - compacted 8 earlier messages".into()),
    );
    apply_event(&mut app, timeline_event("first", "one"));
    assert!(app.timeline[0].compacted);

    apply_event(&mut app, AgentEvent::Answer("one".into()));
    app.input = "second".into();
    assert_eq!(app.dispatch().as_deref(), Some("second"));
    apply_event(&mut app, timeline_event("second", "two"));
    assert!(!app.timeline[1].compacted);
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
    for word in ["list", "trust", "quit", "?", "R"] {
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
fn paste_appends_to_input_without_dispatching() {
    let mut app = test_app(None);

    let action = reduce_input_event(&mut app, Event::Paste("foo bar".into()));

    assert_eq!(action, UiAction::None);
    assert_eq!(app.input, "foo bar");
    assert!(app.transcript.is_empty());
    assert_eq!(app.status, Status::Idle);
}

#[test]
fn multiline_paste_becomes_one_input_line_without_dispatching() {
    let mut app = test_app(None);

    let action = reduce_input_event(&mut app, Event::Paste("line1\nline2".into()));

    assert_eq!(action, UiAction::None);
    assert_eq!(app.input, "line1 line2");
    assert!(app.transcript.is_empty());
    assert_eq!(app.status, Status::Idle);
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
    assert!(labels[0].contains("a-cheap · cheapest capable · USD"));
    assert!(labels[1].contains("b-expensive · 2.0x price · USD"));
    assert!(
        labels[2].contains("relative unavailable: context unknown · USD"),
        "got: {}",
        labels[2]
    );
    assert!(
        labels[3].contains("price unknown · currency unknown"),
        "got: {}",
        labels[3]
    );
    assert!(labels[4].contains("price stale"), "got: {}", labels[4]);
    assert!(
        labels[5].contains("local · explicit selection only · no billed tokens"),
        "got: {}",
        labels[5]
    );
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
fn working_state_keeps_scroll_live_and_blocks_text_input() {
    let mut app = test_app(None);
    app.status = Status::Working;
    app.max_scroll.set(20);
    app.input = "unchanged".into();

    reduce_key(&mut app, code_key(KeyCode::PageUp));
    assert_eq!(app.scroll_back, 5);
    reduce_key(&mut app, char_key('x'));
    assert_eq!(app.input, "unchanged");
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
                    .scroll((scroll, 0)),
                frame.area(),
            );
        })
        .unwrap();
    assert!(buffer_text(terminal.backend().buffer()).contains("newest"));
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

    app.scroll_back = u16::MAX;
    let oldest = buffer_text(&render_buffer(&app, 80, 16));
    assert!(!oldest.contains("↑ more"), "got: {oldest}");
    assert!(oldest.contains("↓ more"), "got: {oldest}");
}

#[test]
fn palette_filter_is_pure_and_finds_exec_shell() {
    let entries = builtin_palette_entries();
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
    for command in ["/trust", "/help", "/timeline"] {
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

#[test]
fn cost_hud_omits_cache_before_usage_and_shows_it_after() {
    let mut app = test_app(None);
    let before = app.hud_line(Utc::now());
    assert!(before.contains("session - · in 0 · out 0"), "got: {before}");
    assert!(before.contains("no price data"), "got: {before}");
    assert!(!before.contains("| cache "), "got: {before}");
    assert!(!before.contains("· cache "), "got: {before}");
    apply_event(
        &mut app,
        AgentEvent::Usage(Usage {
            prompt_tokens: 100,
            completion_tokens: 20,
            cached_tokens: Some(25),
        }),
    );
    let after = app.hud_line(Utc::now());
    assert!(after.contains("in 100 · out 20"), "got: {after}");
    assert!(after.contains("cache 25%"), "got: {after}");
}

#[test]
fn money_hud_uses_accumulated_turn_cost_in_native_currency() {
    let mut app = meter_app();
    let usage = Usage {
        prompt_tokens: 100_000,
        completion_tokens: 50_000,
        cached_tokens: Some(90_000),
    };
    app.usage = usage.clone();
    record_turn_cost(&mut app, &usage, fixed_at());

    let hud = app.hud_line(fixed_at());
    assert!(
        hud.contains("session ¥0.11 (≈$0.02) · in 100000 · out 50000 · cache 90%"),
        "got: {hud}"
    );
    assert!(hud.contains("off-peak"), "got: {hud}");
}

#[test]
fn savings_line_renders_counterfactuals_and_omits_cold_claim() {
    let app = meter_app();
    let usage = Usage {
        prompt_tokens: 100_000,
        completion_tokens: 50_000,
        cached_tokens: Some(90_000),
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
}

#[test]
fn local_hud_and_turn_meter_do_not_present_hardware_cost_as_zero() {
    let mut app = picker_app();
    app.route = app.resolver.resolve("f-local").unwrap();
    let usage = Usage {
        prompt_tokens: 100,
        completion_tokens: 20,
        cached_tokens: None,
    };

    assert!(
        app.hud_line(fixed_at())
            .contains("session no billed tokens"),
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
fn heartbeat_formats_two_elapsed_deltas() {
    let now = fixed_at();
    for (seconds, expected) in [(2, "● WORKING · 2s"), (34, "● WORKING · 34s")] {
        let since = now - chrono::Duration::seconds(seconds);
        let (label, _) = status_chip(&Status::Working, Some(since), now);
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
fn teaching_error_contains_cause_and_next_action() {
    let error = teaching_error("unknown model 'x'", "run /model to list routes");
    assert!(error.contains("unknown model 'x'"));
    assert!(error.contains("run /model to list routes"));
    assert!(error.contains(" - "));
}

#[test]
fn taskbar_semaforo_writes_only_on_waiting_transitions() {
    let mut bytes = Vec::new();
    emit_taskbar_transition(&mut bytes, &Status::Working, &Status::Waiting).unwrap();
    emit_taskbar_transition(&mut bytes, &Status::Waiting, &Status::Waiting).unwrap();
    emit_taskbar_transition(&mut bytes, &Status::Waiting, &Status::Working).unwrap();
    assert_eq!(&bytes[..TASKBAR_WAITING.len()], TASKBAR_WAITING);
    assert_eq!(&bytes[TASKBAR_WAITING.len()..], TASKBAR_CLEAR);
}

#[test]
fn budget_reached_blocks_and_refuses_another_dispatch() {
    let mut app = test_app(Some(100));
    app.status = Status::Working;
    assert_eq!(
        apply_event(
            &mut app,
            AgentEvent::Usage(Usage {
                prompt_tokens: 80,
                completion_tokens: 20,
                cached_tokens: None,
            }),
        ),
        &Status::Blocked(BUDGET_REASON.into())
    );
    app.input = "must not run".into();
    assert!(app.dispatch().is_none());
    let hud = app.hud_line(Utc::now());
    assert!(hud.contains("[#######] 100%"), "got: {hud}");
    assert!(hud.contains("100/100"), "got: {hud}");
    apply_event(&mut app, AgentEvent::Answer("finished".into()));
    assert_eq!(app.status, Status::Blocked(BUDGET_REASON.into()));
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
        UiDiscovery {
            palette_entries: Vec::new(),
            credentialed_providers: Vec::new(),
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
        UiDiscovery {
            palette_entries: vec![PaletteEntry {
                kind: "tool",
                name: "secret-tool".into(),
                description: format!("value={secret}\r\x1b[2K"),
                state: Some(McpState::Enabled),
                action: PaletteAction::Describe,
            }],
            credentialed_providers: Vec::new(),
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
    let first = identity_constitution("law bytes", &route);
    let second = identity_constitution("law bytes", &route);

    assert_eq!(first, second);
    assert!(first.contains("test-route"), "got: {first}");
    assert!(first.contains("via test"), "got: {first}");
    assert!(first.contains("never claim to be Claude"), "got: {first}");
    assert!(
        first.contains(nh_core::agent::TOOL_RESULT_STATE_RULE),
        "got: {first}"
    );
    assert!(first.ends_with("law bytes"), "got: {first}");
}

struct MockClient {
    request_lengths: Arc<Mutex<Vec<usize>>>,
}

#[derive(Debug)]
struct RecordedRequest {
    model: String,
    message_count: usize,
    system: String,
    effort: ThinkingEffort,
}

struct RecordingClient {
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
}

struct FailingClient;

impl ChatClient for FailingClient {
    fn complete(&self, _request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        anyhow::bail!("provider failed after request")
    }
}

impl ChatClient for RecordingClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        self.requests.lock().unwrap().push(RecordedRequest {
            model: request.model.clone(),
            message_count: request.messages.len(),
            system: request.messages[0].content.clone().unwrap_or_default(),
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
            }),
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
            }),
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
fn worker_error_marks_the_rendered_session_meter_incomplete() {
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
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: None,
    })
    .unwrap();
    worker
        .commands
        .send(WorkerCommand::Task("failing task".into()))
        .unwrap();
    let mut app = meter_app();
    loop {
        let event = worker
            .events
            .recv_timeout(Duration::from_secs(2))
            .expect("failed worker reports its receipt");
        let done = matches!(event, AgentEvent::TaskReceipt(_));
        apply_event(&mut app, event);
        if done {
            break;
        }
    }

    assert!(app.has_failed_turn);
    assert!(app
        .session_money(fixed_at())
        .contains("? incomplete - failed turn usage not reported"));
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
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: None,
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
    assert_eq!(requests[1].message_count, 4, "history was not kept");
    assert!(requests[1].system.contains("nosis on other-route"));
    assert!(requests[1].system.contains("never claim to be Claude"));
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
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: None,
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

    match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
        AgentEvent::Failed(reason) => {
            assert!(reason.contains("nh key add other"), "got: {reason}");
        }
        _ => panic!("keyless switched task must fail with one friendly line"),
    }
    match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
        AgentEvent::TaskReceipt(summary) => {
            assert_eq!(summary.receipt.model_id, "other-route");
            assert_eq!(summary.receipt.task, "hello");
        }
        _ => panic!("failed switched task must produce a timeline receipt"),
    }
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
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber,
        connect,
        initial: None,
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
                | AgentEvent::ToolStarted { .. }
                | AgentEvent::ToolFinished { .. } => {}
                AgentEvent::MeterIncomplete => panic!("successful worker lost meter data"),
                AgentEvent::Approval(_) => panic!("mock never asks for approval"),
                AgentEvent::Failed(reason) => panic!("worker failed: {reason}"),
            }
        }
    }
    assert_eq!(worker.shutdown(), WorkerShutdown::Clean);
    assert_eq!(*request_lengths.lock().unwrap(), vec![2, 4]);
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
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: None,
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
            | AgentEvent::ToolStarted { .. }
            | AgentEvent::ToolFinished { .. } => {}
            AgentEvent::MeterIncomplete => panic!("successful worker lost meter data"),
            AgentEvent::Approval(_) => panic!("mock never asks for approval"),
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
        law,
        repo_root: root.clone(),
        workdir: root.clone(),
        scrubber: Arc::new(RwLock::new(Scrubber::new(Vec::new()))),
        connect,
        initial: Some(Err(anyhow::anyhow!("{message}"))),
    })
    .unwrap();
    worker
        .commands
        .send(WorkerCommand::Task("hello".into()))
        .unwrap();
    match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
        AgentEvent::Failed(reason) => {
            assert!(reason.contains("nh key add test"), "got: {reason}");
            assert!(!reason.chars().any(char::is_control), "got: {reason}");
        }
        _ => panic!("keyless task must fail with one friendly line"),
    }
    match worker.events.recv_timeout(Duration::from_secs(2)).unwrap() {
        AgentEvent::TaskReceipt(summary) => {
            assert_eq!(summary.receipt.task, "hello");
            assert_eq!(summary.receipt.outcome, Outcome::Fail);
            assert!(summary.answer.starts_with("error: "));
        }
        _ => panic!("failed task must still produce one timeline receipt"),
    }
    assert_eq!(worker.shutdown(), WorkerShutdown::Clean);
    std::fs::remove_dir_all(root).unwrap();
}
