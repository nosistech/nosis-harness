use super::*;
use std::sync::{Arc, Mutex};

fn message(role: &str, content: impl Into<String>) -> ChatMessage {
    plain_msg(role, content.into())
}

#[test]
fn compaction_keeps_retained_messages_byte_identical_and_prefix_sealed() {
    let mut history = vec![
        message("system", "sealed constitution"),
        message("user", "old turn ".repeat(200)),
        message("assistant", "old answer"),
        message("user", "middle turn ".repeat(200)),
        message("assistant", "middle answer"),
        message("user", "recent a"),
        message("assistant", "recent answer a"),
        message("user", "recent b"),
        message("assistant", "recent answer b"),
    ];
    let retained_before: Vec<Vec<u8>> = history[5..].iter().map(message_bytes).collect();
    let seal = PrefixSeal::new(&history[..1]);

    let compaction = compact_history(&mut history, 100).expect("compaction fires");
    assert!(compaction.prefix_held);
    assert!(
        seal.check(&history),
        "PrefixSeal is enforced in release too"
    );
    assert_eq!(history[1].role, "system");
    assert!(history[1]
        .content
        .as_deref()
        .is_some_and(|text| text.starts_with("[nosis] earlier context compacted:")));
    assert_eq!(
        history[2..].iter().map(message_bytes).collect::<Vec<_>>(),
        retained_before
    );

    let mut drifted = history.clone();
    drifted[0].content = Some("changed".into());
    assert!(
        !seal.check(&drifted),
        "release builds must detect prefix drift"
    );
}

#[test]
fn compaction_fires_for_realistic_uniform_turns_at_seventy_percent() {
    let mut history = vec![message("system", "sealed constitution")];
    for turn in 0..14 {
        history.push(message("user", format!("user-{turn:02} ").repeat(10)));
        history.push(message("assistant", format!("asst-{turn:02} ").repeat(10)));
    }
    let before_tokens = estimate_tokens(&history);
    assert!(
        (650..=750).contains(&before_tokens),
        "history should resemble the 70% trigger: {before_tokens}"
    );
    let original: Vec<Vec<u8>> = history.iter().map(message_bytes).collect();
    let seal = PrefixSeal::new(&history[..1]);

    let compaction = compact_history(&mut history, 1_000).expect("compaction fires");
    let retained_start = compaction.messages + 1;

    assert!(compaction.prefix_held);
    assert!(seal.check(&history));
    assert_eq!(history[1].role, "system");
    assert!(history[1]
        .content
        .as_deref()
        .is_some_and(|text| text.starts_with("[nosis] earlier context compacted:")));
    assert_eq!(
        history[2..].iter().map(message_bytes).collect::<Vec<_>>(),
        original[retained_start..]
    );
}

#[test]
fn compaction_input_count_sees_a_new_large_tool_result() {
    let mut history = vec![
        message("system", "sealed constitution"),
        message("user", "read the large file"),
        ChatMessage {
            tool_calls: Some(vec![ToolCallReq {
                id: "c1".into(),
                name: "read_file".into(),
                arguments: r#"{"path":"large.txt"}"#.into(),
            }]),
            ..message("assistant", "")
        },
    ];
    let stale_provider_count = estimate_request_tokens(&history, &[], true);
    history.push(ChatMessage {
        role: "tool".into(),
        content: Some("x".repeat(4_000)),
        tool_calls: None,
        tool_call_id: Some("c1".into()),
        reasoning_content: None,
    });
    let estimated = estimate_request_tokens(&history, &[], true);

    assert!(stale_provider_count < 700);
    assert!(estimated > 700);
    assert_eq!(
        compaction_input_tokens(Some(stale_provider_count), estimated),
        estimated
    );
    assert_eq!(
        compaction_input_tokens(Some(estimated + 50), estimated),
        estimated + 50
    );
}

struct FinalAnswerClient;

impl ChatClient for FinalAnswerClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<crate::wire::ChatResponse> {
        Ok(crate::wire::ChatResponse {
            message: message("assistant", "paid answer"),
            finish_reason: "stop".into(),
            usage: None,
        })
    }
}

struct ProviderErrorClient;

impl ChatClient for ProviderErrorClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<crate::wire::ChatResponse> {
        anyhow::bail!("provider returned HTTP 500: original failure")
    }
}

struct FinishReasonClient {
    finish_reason: String,
}

impl ChatClient for FinishReasonClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<crate::wire::ChatResponse> {
        Ok(crate::wire::ChatResponse {
            message: message("assistant", "usable answer"),
            finish_reason: self.finish_reason.clone(),
            usage: None,
        })
    }
}

struct OverflowUsageClient {
    calls: Mutex<u8>,
}

impl ChatClient for OverflowUsageClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<crate::wire::ChatResponse> {
        let mut calls = self.calls.lock().unwrap();
        let call = *calls;
        *calls += 1;
        drop(calls);

        if call == 0 {
            return Ok(crate::wire::ChatResponse {
                message: ChatMessage {
                    role: "assistant".into(),
                    content: Some("working".into()),
                    tool_calls: Some(vec![ToolCallReq {
                        id: "overflow-probe".into(),
                        name: "missing_tool".into(),
                        arguments: "{}".into(),
                    }]),
                    tool_call_id: None,
                    reasoning_content: None,
                },
                finish_reason: "tool_calls".into(),
                usage: Some(Usage {
                    prompt_tokens: u64::MAX,
                    completion_tokens: u64::MAX,
                    cached_tokens: Some(u64::MAX),
                }),
            });
        }

        Ok(crate::wire::ChatResponse {
            message: message("assistant", "answer after overflow"),
            finish_reason: "stop".into(),
            usage: Some(Usage {
                prompt_tokens: 1,
                completion_tokens: 1,
                cached_tokens: Some(1),
            }),
        })
    }
}

fn agent_with_receipt_path(
    dir: &std::path::Path,
    receipt_path: std::path::PathBuf,
    client: Box<dyn ChatClient>,
    events: Arc<Mutex<Vec<String>>>,
) -> AgentLoop {
    AgentLoop {
        client,
        tools: Vec::new(),
        ctx: ToolCtx::new(dir.to_path_buf(), Box::new(|_| false)),
        receipts: ReceiptWriter::for_path(dir, receipt_path, nh_vault::Scrubber::new(Vec::new())),
        model_id: "mock-model".into(),
        max_turns: 1,
        thinking: ThinkingEffort::None,
        profile: None,
        constitution: None,
        context_limit: None,
        on_event: Some(Box::new(move |line| {
            events.lock().unwrap().push(line.to_owned())
        })),
    }
}

fn run_finish_reason(reason: &str) -> (String, Receipt, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut agent = agent_with_receipt_path(
        dir.path(),
        dir.path().join("receipts.jsonl"),
        Box::new(FinishReasonClient {
            finish_reason: reason.to_owned(),
        }),
        Arc::clone(&events),
    );
    let (answer, receipt) = agent.run("finish").unwrap();
    let emitted = events.lock().unwrap().clone();
    (answer, receipt, emitted)
}

#[test]
fn normal_finish_reasons_remain_passes() {
    for reason in ["", "stop", "end_turn", "stop_sequence"] {
        let (answer, receipt, emitted) = run_finish_reason(reason);
        assert_eq!(answer, "usable answer", "{reason}");
        assert_eq!(receipt.outcome, Outcome::Pass, "{reason}");
        assert_eq!(receipt.failure_class, None, "{reason}");
        assert!(emitted.is_empty(), "{reason}: {emitted:?}");
    }
}

#[test]
fn truncated_finish_reasons_return_the_answer_as_partial() {
    for reason in ["length", "max_tokens", "model_length"] {
        let (answer, receipt, emitted) = run_finish_reason(reason);
        assert_eq!(answer, "usable answer", "{reason}");
        assert_eq!(receipt.outcome, Outcome::Partial, "{reason}");
        assert_eq!(
            receipt.failure_class,
            Some(FailureClass::Constraint),
            "{reason}"
        );
        assert!(emitted.is_empty(), "{reason}: {emitted:?}");
    }
}

#[test]
fn content_filter_returns_the_answer_as_a_filtered_failure() {
    let (answer, receipt, emitted) = run_finish_reason("content_filter");

    assert_eq!(answer, "usable answer");
    assert_eq!(receipt.outcome, Outcome::Fail);
    assert_eq!(receipt.failure_class, Some(FailureClass::Filtered));
    assert!(emitted.is_empty());
}

#[test]
fn unknown_finish_reason_returns_the_answer_as_partial_and_emits() {
    let (answer, receipt, emitted) = run_finish_reason(" future_reason ");

    assert_eq!(answer, "usable answer");
    assert_eq!(receipt.outcome, Outcome::Partial);
    assert_eq!(receipt.failure_class, Some(FailureClass::Constraint));
    assert_eq!(
        emitted,
        vec!["unrecognized finish reason 'future_reason' - treated as partial"]
    );
}

#[test]
fn usage_overflow_omits_receipt_usage_but_keeps_the_answer() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut agent = agent_with_receipt_path(
        dir.path(),
        dir.path().join("receipts.jsonl"),
        Box::new(OverflowUsageClient {
            calls: Mutex::new(0),
        }),
        events,
    );
    agent.max_turns = 2;

    let (answer, receipt) = agent.run("overflow").unwrap();

    assert_eq!(answer, "answer after overflow");
    assert_eq!(receipt.outcome, Outcome::Pass);
    assert!(receipt.usage.is_none());
}

#[test]
fn receipt_failure_marks_answer_unreceipted_without_discarding_it_or_shadowing_errors() {
    let dir = tempfile::tempdir().unwrap();

    let success_path = dir.path().join("success-receipt-path");
    std::fs::create_dir(&success_path).unwrap();
    let success_events = Arc::new(Mutex::new(Vec::new()));
    let mut success = agent_with_receipt_path(
        dir.path(),
        success_path,
        Box::new(FinalAnswerClient),
        Arc::clone(&success_events),
    );
    let (answer, receipt) = success.run("finish").expect("answer survives");
    assert_eq!(answer, "paid answer");
    assert_eq!(receipt.outcome, Outcome::Fail);
    assert_eq!(receipt.failure_class, Some(FailureClass::Unreceipted));
    assert!(success_events
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.starts_with("receipt not written - outcome marked unreceipted:")));

    let partial_path = dir.path().join("partial-receipt-path");
    std::fs::create_dir(&partial_path).unwrap();
    let partial_events = Arc::new(Mutex::new(Vec::new()));
    let mut partial = agent_with_receipt_path(
        dir.path(),
        partial_path,
        Box::new(FinishReasonClient {
            finish_reason: "length".into(),
        }),
        partial_events,
    );
    let (answer, receipt) = partial.run("partial").expect("answer survives");
    assert_eq!(answer, "usable answer");
    assert_eq!(receipt.outcome, Outcome::Partial);
    assert_eq!(receipt.failure_class, Some(FailureClass::Unreceipted));

    let error_path = dir.path().join("error-receipt-path");
    std::fs::create_dir(&error_path).unwrap();
    let error_events = Arc::new(Mutex::new(Vec::new()));
    let mut failing = agent_with_receipt_path(
        dir.path(),
        error_path,
        Box::new(ProviderErrorClient),
        Arc::clone(&error_events),
    );
    let error = failing.run("fail").unwrap_err().to_string();
    assert_eq!(error, "provider returned HTTP 500: original failure");
    assert!(error_events
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.starts_with("receipt not written - outcome marked unreceipted:")));
}

#[test]
fn writable_receipt_path_keeps_pass_and_persists_the_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let receipt_path = dir.path().join("receipts.jsonl");
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut agent = agent_with_receipt_path(
        dir.path(),
        receipt_path.clone(),
        Box::new(FinalAnswerClient),
        Arc::clone(&events),
    );

    let (answer, receipt) = agent.run("finish").expect("answer and receipt survive");

    assert_eq!(answer, "paid answer");
    assert_eq!(receipt.outcome, Outcome::Pass);
    assert_eq!(receipt.failure_class, None);
    assert!(events.lock().unwrap().is_empty());
    let lines = std::fs::read_to_string(receipt_path).unwrap();
    let persisted = lines
        .lines()
        .map(|line| serde_json::from_str::<Receipt>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].outcome, Outcome::Pass);
    assert_eq!(persisted[0].task, "finish");
}

#[test]
fn request_estimate_counts_preserved_reasoning_and_tool_specs() {
    let messages = vec![ChatMessage {
        reasoning_content: Some("r".repeat(40)),
        ..message("assistant", "12345678")
    }];
    let tools = vec![nh_tools::ToolSpec {
        name: "read_file".into(),
        description: "read one file".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string"}}
        }),
    }];
    let old_content_only = estimate_request_tokens(&messages, &[], false);
    let honest = estimate_request_tokens(&messages, &tools, true);
    let tool_bytes = serde_json::to_vec(&tools).unwrap().len() as u64;
    let expected_delta = 40_u64.div_ceil(4) + tool_bytes.div_ceil(4);

    assert!(honest > old_content_only);
    assert!(honest.abs_diff(old_content_only + expected_delta) <= 1);
}

#[test]
fn effective_context_clamps_large_windows_before_compaction_threshold() {
    let raw = 1_000_000;
    let working = effective_context(raw);
    assert_eq!(working, 256_000);
    assert!(working < raw);
    assert_eq!(effective_context(128_000), 128_000);

    let observed = 200_000_f64;
    assert!(observed >= COMPACT_AT * working as f64);
    assert!(observed < COMPACT_AT * raw as f64);
}

#[test]
fn progress_line_shows_key_arg() {
    let call = ToolCallReq {
        id: "c1".into(),
        name: "edit_file".into(),
        arguments: r#"{"path":"src/lib.rs","old_string":"a","new_string":"b"}"#.into(),
    };
    assert_eq!(progress_line(2, &call), "turn 2: edit_file src/lib.rs");
}

#[test]
fn progress_line_survives_bad_json() {
    let call = ToolCallReq {
        id: "c1".into(),
        name: "read_file".into(),
        arguments: "{oops".into(),
    };
    assert_eq!(progress_line(1, &call), "turn 1: read_file");
}
