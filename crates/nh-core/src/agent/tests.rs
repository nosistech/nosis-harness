use super::context::{estimate_tokens, message_bytes, IMAGE_ESTIMATE_TOKENS};
use super::*;
use crate::wire::UsageEvidence;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    let expected_elided_tokens = estimate_tokens(&history[1..5]);
    let seal = PrefixSeal::new(&history[..1]);

    let compaction = compact_history(&mut history, 100).expect("compaction fires");
    assert!(compaction.prefix_held);
    assert_eq!(compaction.estimated_tokens_elided, expected_elided_tokens);
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
        parts: None,
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
            retries: Default::default(),
        })
    }
}

struct ProviderErrorClient;

impl ChatClient for ProviderErrorClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<crate::wire::ChatResponse> {
        anyhow::bail!("provider returned HTTP 500: original failure")
    }
}

struct RetryFailureClient;

impl ChatClient for RetryFailureClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<crate::wire::ChatResponse> {
        Err(anyhow::Error::new(RetryExhausted {
            stats: RetryStats {
                retries: 2,
                rate_limited: 2,
            },
            usage: Some(Usage {
                prompt_tokens: 18,
                completion_tokens: 3,
                cached_tokens: Some(4),
                evidence: UsageEvidence::Measured,
            }),
            last_failure: "provider returned HTTP 429 - rate limited".into(),
            attempts: 3,
            elapsed: Duration::from_secs(6),
        }))
    }
}

struct RetriedTurnsClient {
    calls: Mutex<u8>,
}

impl ChatClient for RetriedTurnsClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<crate::wire::ChatResponse> {
        let mut calls = self.calls.lock().unwrap();
        let call = *calls;
        *calls += 1;
        drop(calls);

        if call == 0 {
            return Ok(crate::wire::ChatResponse {
                message: ChatMessage {
                    role: "assistant".into(),
                    content: None,
                    parts: None,
                    tool_calls: Some(vec![ToolCallReq {
                        id: "retry-tool".into(),
                        name: "missing_tool".into(),
                        arguments: "{}".into(),
                    }]),
                    tool_call_id: None,
                    reasoning_content: None,
                },
                finish_reason: "tool_calls".into(),
                usage: None,
                retries: RetryStats {
                    retries: 1,
                    rate_limited: 1,
                },
            });
        }

        Ok(crate::wire::ChatResponse {
            message: message("assistant", "done after retries"),
            finish_reason: "stop".into(),
            usage: None,
            retries: RetryStats {
                retries: 2,
                rate_limited: 1,
            },
        })
    }
}

struct UsageFinishClient {
    cached_tokens: Option<u64>,
}

impl ChatClient for UsageFinishClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<crate::wire::ChatResponse> {
        Ok(crate::wire::ChatResponse {
            message: message("assistant", "metered"),
            finish_reason: "stop".into(),
            usage: Some(Usage {
                prompt_tokens: 20,
                completion_tokens: 2,
                cached_tokens: self.cached_tokens,
                evidence: UsageEvidence::Measured,
            }),
            retries: Default::default(),
        })
    }
}

struct FinishReasonClient {
    finish_reason: FinishReason,
}

impl ChatClient for FinishReasonClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<crate::wire::ChatResponse> {
        Ok(crate::wire::ChatResponse {
            message: message("assistant", "usable answer"),
            finish_reason: self.finish_reason.clone(),
            usage: None,
            retries: Default::default(),
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
                    parts: None,
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
                    evidence: UsageEvidence::Measured,
                }),
                retries: Default::default(),
            });
        }

        Ok(crate::wire::ChatResponse {
            message: message("assistant", "answer after overflow"),
            finish_reason: "stop".into(),
            usage: Some(Usage {
                prompt_tokens: 1,
                completion_tokens: 1,
                cached_tokens: Some(1),
                evidence: UsageEvidence::Measured,
            }),
            retries: Default::default(),
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
    run_finish_reason_value(reason.into())
}

fn run_finish_reason_value(reason: FinishReason) -> (String, Receipt, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut agent = agent_with_receipt_path(
        dir.path(),
        dir.path().join("receipts.jsonl"),
        Box::new(FinishReasonClient {
            finish_reason: reason,
        }),
        Arc::clone(&events),
    );
    let (answer, receipt) = agent.run("finish").unwrap();
    let emitted = events.lock().unwrap().clone();
    (answer, receipt, emitted)
}

#[test]
fn normal_finish_reasons_remain_passes() {
    for reason in ["stop", "end_turn", "stop_sequence"] {
        let (answer, receipt, emitted) = run_finish_reason(reason);
        assert_eq!(answer, "usable answer", "{reason}");
        assert_eq!(receipt.outcome, Outcome::Pass, "{reason}");
        assert_eq!(receipt.failure_class, None, "{reason}");
        assert!(emitted.is_empty(), "{reason}: {emitted:?}");
    }
}

#[test]
fn missing_finish_reason_is_partial_and_never_normal() {
    let (answer, receipt, emitted) = run_finish_reason_value(FinishReason::Missing);

    assert_eq!(answer, "usable answer");
    assert_eq!(receipt.outcome, Outcome::Partial);
    assert_eq!(receipt.failure_class, Some(FailureClass::Constraint));
    assert_eq!(emitted, vec!["finish reason missing - treated as partial"]);
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
fn filter_finish_reasons_return_the_answer_as_a_filtered_failure() {
    for reason in ["content_filter", "sensitive"] {
        let (answer, receipt, emitted) = run_finish_reason(reason);
        assert_eq!(answer, "usable answer", "{reason}");
        assert_eq!(receipt.outcome, Outcome::Fail, "{reason}");
        assert_eq!(
            receipt.failure_class,
            Some(FailureClass::Filtered),
            "{reason}"
        );
        assert!(emitted.is_empty(), "{reason}: {emitted:?}");
    }
}

#[test]
fn context_window_finish_reason_is_a_context_partial() {
    let (answer, receipt, emitted) = run_finish_reason("model_context_window_exceeded");
    assert_eq!(answer, "usable answer");
    assert_eq!(receipt.outcome, Outcome::Partial);
    assert_eq!(receipt.failure_class, Some(FailureClass::Context));
    assert!(emitted.is_empty());
}

#[test]
fn provider_interrupt_finish_reasons_are_constraint_partials() {
    for reason in ["network_error", "insufficient_system_resource"] {
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
fn unknown_finish_reason_returns_the_answer_as_partial_and_emits() {
    let (answer, receipt, emitted) = run_finish_reason("future_reason\nforged progress");

    assert_eq!(answer, "usable answer");
    assert_eq!(receipt.outcome, Outcome::Partial);
    assert_eq!(receipt.failure_class, Some(FailureClass::Constraint));
    assert_eq!(
        emitted,
        vec!["unrecognized finish reason - treated as partial"]
    );
}

struct MeasuredThenUnmeteredClient {
    calls: Mutex<u8>,
}

impl ChatClient for MeasuredThenUnmeteredClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<crate::wire::ChatResponse> {
        let mut calls = self.calls.lock().unwrap();
        let first = *calls == 0;
        *calls += 1;
        drop(calls);
        if first {
            return Ok(crate::wire::ChatResponse {
                message: ChatMessage {
                    role: "assistant".into(),
                    content: None,
                    parts: None,
                    tool_calls: Some(vec![ToolCallReq {
                        id: "meter-probe".into(),
                        name: "missing_tool".into(),
                        arguments: "{}".into(),
                    }]),
                    tool_call_id: None,
                    reasoning_content: None,
                },
                finish_reason: FinishReason::ToolUse,
                usage: Some(Usage {
                    prompt_tokens: 12,
                    completion_tokens: 3,
                    cached_tokens: Some(4),
                    evidence: UsageEvidence::Measured,
                }),
                retries: Default::default(),
            });
        }
        Ok(crate::wire::ChatResponse {
            message: message("assistant", "done"),
            finish_reason: FinishReason::Stop,
            usage: None,
            retries: Default::default(),
        })
    }
}

#[test]
fn unmetered_final_call_degrades_prior_measurement_to_partial() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut agent = agent_with_receipt_path(
        dir.path(),
        dir.path().join("receipts.jsonl"),
        Box::new(MeasuredThenUnmeteredClient {
            calls: Mutex::new(0),
        }),
        events,
    );
    agent.max_turns = 2;

    let (_, receipt) = agent.run("meter this").unwrap();
    let usage = receipt.usage.unwrap();

    assert_eq!(usage.evidence, UsageEvidence::Partial);
    assert_eq!((usage.prompt_tokens, usage.completion_tokens), (12, 3));
}

struct UnsafeFinishToolClient {
    finish_reason: FinishReason,
}

impl ChatClient for UnsafeFinishToolClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<crate::wire::ChatResponse> {
        Ok(crate::wire::ChatResponse {
            message: ChatMessage {
                role: "assistant".into(),
                content: Some("unsafe completion".into()),
                parts: None,
                tool_calls: Some(vec![ToolCallReq {
                    id: "must-not-run".into(),
                    name: "count_tool".into(),
                    arguments: "{}".into(),
                }]),
                tool_call_id: None,
                reasoning_content: None,
            },
            finish_reason: self.finish_reason.clone(),
            usage: None,
            retries: Default::default(),
        })
    }
}

struct CountingTool(Arc<AtomicUsize>);

impl Tool for CountingTool {
    fn spec(&self) -> nh_tools::ToolSpec {
        nh_tools::ToolSpec {
            name: "count_tool".into(),
            description: "count executions".into(),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    fn execute(&self, _args: serde_json::Value, _ctx: &ToolCtx) -> anyhow::Result<String> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok("executed".into())
    }
}

#[test]
fn truncated_or_filtered_completion_never_executes_attached_tool_call() {
    for (finish_reason, expected_outcome) in [
        (FinishReason::Truncated, Outcome::Partial),
        (FinishReason::Filtered, Outcome::Fail),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let executions = Arc::new(AtomicUsize::new(0));
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut agent = agent_with_receipt_path(
            dir.path(),
            dir.path().join("receipts.jsonl"),
            Box::new(UnsafeFinishToolClient { finish_reason }),
            events,
        );
        agent.tools = vec![Box::new(CountingTool(Arc::clone(&executions)))];
        let mut history = Vec::new();

        let (_, receipt) = agent
            .run_with_history(&mut history, "do not execute")
            .unwrap();

        assert_eq!(executions.load(Ordering::SeqCst), 0);
        assert_eq!(receipt.tool_calls, 0);
        assert_eq!(receipt.outcome, expected_outcome);
        assert!(!history.iter().any(|message| message.role == "tool"));
    }
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
fn receipt_cache_percentage_distinguishes_absent_from_measured_zero() {
    for (cached_tokens, expected) in [(None, None), (Some(0), Some(0.0))] {
        let dir = tempfile::tempdir().unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut agent = agent_with_receipt_path(
            dir.path(),
            dir.path().join("receipts.jsonl"),
            Box::new(UsageFinishClient { cached_tokens }),
            events,
        );

        let (_, receipt) = agent.run("meter").unwrap();

        assert_eq!(receipt.cache_hit_pct, expected);
        assert_eq!(
            receipt.usage.as_ref().and_then(|usage| usage.cached_tokens),
            cached_tokens
        );
    }
}

#[test]
fn cumulative_cache_measurement_is_absent_if_any_turn_omits_it() {
    let mut total = Usage {
        prompt_tokens: 0,
        completion_tokens: 0,
        cached_tokens: Some(0),
        evidence: UsageEvidence::Measured,
    };
    assert!(add_usage_checked(
        &mut total,
        &Usage {
            prompt_tokens: 10,
            completion_tokens: 1,
            cached_tokens: None,
            evidence: UsageEvidence::Measured,
        }
    ));
    assert!(add_usage_checked(
        &mut total,
        &Usage {
            prompt_tokens: 10,
            completion_tokens: 1,
            cached_tokens: Some(5),
            evidence: UsageEvidence::Measured,
        }
    ));
    assert_eq!(total.prompt_tokens, 20);
    assert_eq!(total.cached_tokens, None);
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
fn retry_exhaustion_stats_and_salvaged_usage_reach_failed_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let receipt_path = dir.path().join("receipts.jsonl");
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut agent = agent_with_receipt_path(
        dir.path(),
        receipt_path.clone(),
        Box::new(RetryFailureClient),
        events,
    );

    let error = agent.run("retry failure").unwrap_err();
    assert!(error
        .to_string()
        .contains("3 attempts over 6s; last provider failure"));

    let line = std::fs::read_to_string(receipt_path).unwrap();
    let receipt: Receipt = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(receipt.outcome, Outcome::Fail);
    assert_eq!(
        receipt.retries,
        RetryStats {
            retries: 2,
            rate_limited: 2,
        }
    );
    let usage = receipt.usage.unwrap();
    assert_eq!(usage.prompt_tokens, 18);
    assert_eq!(usage.completion_tokens, 3);
    assert_eq!(usage.cached_tokens, Some(4));
}

#[test]
fn successful_retry_stats_accumulate_across_turns_and_emit_one_line_per_call() {
    let dir = tempfile::tempdir().unwrap();
    let receipt_path = dir.path().join("receipts.jsonl");
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut agent = agent_with_receipt_path(
        dir.path(),
        receipt_path,
        Box::new(RetriedTurnsClient {
            calls: Mutex::new(0),
        }),
        Arc::clone(&events),
    );
    agent.max_turns = 2;

    let (answer, receipt) = agent.run("retry success").unwrap();
    assert_eq!(answer, "done after retries");
    assert_eq!(
        receipt.retries,
        RetryStats {
            retries: 3,
            rate_limited: 2,
        }
    );
    let events = events.lock().unwrap();
    assert!(events
        .iter()
        .any(|line| line == "turn 1: 2 attempts, 1 rate-limited"));
    assert!(events
        .iter()
        .any(|line| line == "turn 2: 3 attempts, 1 rate-limited"));
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
fn request_estimate_adds_only_the_documented_per_image_allowance() {
    let plain = vec![message("user", "look")];
    let image = vec![ChatMessage {
        role: "user".into(),
        content: None,
        parts: Some(vec![
            ContentPart::Text {
                text: "look".into(),
            },
            ContentPart::ImageB64 {
                media_type: "image/png".into(),
                data: "base64 bytes are not token-counted locally".repeat(100),
            },
        ]),
        tool_calls: None,
        tool_call_id: None,
        reasoning_content: None,
    }];

    assert_eq!(
        estimate_tokens(&image),
        estimate_tokens(&plain) + IMAGE_ESTIMATE_TOKENS
    );
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

#[test]
fn compaction_event_round_trips_explicitly_unavailable_time() {
    let event = CompactionEvent::new(72, 8, 512, None);
    let line = event.to_string();

    assert!(line.starts_with("context ~72%"));
    assert!(line.ends_with("Unix time unavailable"));
    assert_eq!(CompactionEvent::parse(&line), Some(event));
}

struct RepairingToolCallClient {
    calls: Mutex<u8>,
    observed_tool_result: Arc<Mutex<Option<String>>>,
}

impl ChatClient for RepairingToolCallClient {
    fn complete(&self, req: &ChatRequest) -> anyhow::Result<crate::wire::ChatResponse> {
        let mut calls = self.calls.lock().unwrap();
        let call = *calls;
        *calls += 1;
        drop(calls);

        if call == 0 {
            return Ok(crate::wire::ChatResponse {
                message: ChatMessage {
                    role: "assistant".into(),
                    content: None,
                    parts: None,
                    tool_calls: Some(vec![ToolCallReq {
                        id: "repair-1".into(),
                        name: "view_file".into(),
                        arguments: "```json\n{path: 'note.txt',}\n```".into(),
                    }]),
                    tool_call_id: None,
                    reasoning_content: None,
                },
                finish_reason: "tool_calls".into(),
                usage: None,
                retries: Default::default(),
            });
        }

        *self.observed_tool_result.lock().unwrap() = req
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "tool")
            .and_then(|message| message.content.clone());
        Ok(crate::wire::ChatResponse {
            message: message("assistant", "done"),
            finish_reason: "stop".into(),
            usage: None,
            retries: Default::default(),
        })
    }
}

#[test]
fn repaired_tool_call_is_visible_in_history_events_and_receipt_counter() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "evidence").unwrap();
    let observed = Arc::new(Mutex::new(None));
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut agent = agent_with_receipt_path(
        dir.path(),
        dir.path().join("receipts.jsonl"),
        Box::new(RepairingToolCallClient {
            calls: Mutex::new(0),
            observed_tool_result: Arc::clone(&observed),
        }),
        Arc::clone(&events),
    );
    agent.max_turns = 2;
    agent.tools = vec![Box::new(nh_tools::ReadFile)];

    let (answer, receipt) = agent.run("read it").unwrap();

    assert_eq!(answer, "done");
    assert_eq!(receipt.repairs.tool_call_repair_attempts, 1);
    let transcript = observed.lock().unwrap().clone().unwrap();
    assert!(transcript.starts_with("[tool-call repair: stripped JSON markdown fence; normalized single-quoted JSON strings; quoted unquoted JSON object keys; removed trailing JSON commas; mapped tool name 'view_file' to 'read_file']\n"));
    assert!(transcript.ends_with("evidence"));
    let emitted = events.lock().unwrap();
    assert!(emitted
        .iter()
        .any(|line| line.contains("mapped tool name 'view_file' to 'read_file'")));
    assert!(emitted
        .iter()
        .any(|line| line.contains("removed trailing JSON commas")));
}

struct AuditedEditClient {
    calls: Mutex<u8>,
    observed_tool_result: Arc<Mutex<Option<String>>>,
}

impl ChatClient for AuditedEditClient {
    fn complete(&self, req: &ChatRequest) -> anyhow::Result<crate::wire::ChatResponse> {
        let mut calls = self.calls.lock().unwrap();
        let call = *calls;
        *calls += 1;
        drop(calls);
        if call == 0 {
            return Ok(crate::wire::ChatResponse {
                message: ChatMessage {
                    role: "assistant".into(),
                    content: None,
                    parts: None,
                    tool_calls: Some(vec![ToolCallReq {
                        id: "audit-edit".into(),
                        name: "edit_file".into(),
                        arguments: "{}".into(),
                    }]),
                    tool_call_id: None,
                    reasoning_content: None,
                },
                finish_reason: "tool_calls".into(),
                usage: None,
                retries: Default::default(),
            });
        }
        *self.observed_tool_result.lock().unwrap() = req
            .messages
            .iter()
            .rev()
            .find(|message| message.role == "tool")
            .and_then(|message| message.content.clone());
        Ok(crate::wire::ChatResponse {
            message: message("assistant", "done"),
            finish_reason: "stop".into(),
            usage: None,
            retries: Default::default(),
        })
    }
}

struct AuditedEditTool;

impl Tool for AuditedEditTool {
    fn spec(&self) -> nh_tools::ToolSpec {
        nh_tools::ToolSpec {
            name: "edit_file".into(),
            description: "test audited edit".into(),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    fn execute(&self, _args: serde_json::Value, _ctx: &ToolCtx) -> anyhow::Result<String> {
        Ok("edited using indentation-flexible match".into())
    }

    fn execute_with_audit(
        &self,
        _args: serde_json::Value,
        _ctx: &ToolCtx,
    ) -> anyhow::Result<nh_tools::ToolExecution> {
        Ok(nh_tools::ToolExecution {
            output: "edited using indentation-flexible match".into(),
            audit: vec![ToolAudit::EditMatch(EditMatchTier::IndentationFlexible)],
        })
    }
}

#[test]
fn tolerant_edit_tier_is_visible_in_transcript_events_and_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let observed = Arc::new(Mutex::new(None));
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut agent = agent_with_receipt_path(
        dir.path(),
        dir.path().join("receipts.jsonl"),
        Box::new(AuditedEditClient {
            calls: Mutex::new(0),
            observed_tool_result: Arc::clone(&observed),
        }),
        Arc::clone(&events),
    );
    agent.max_turns = 2;
    agent.tools = vec![Box::new(AuditedEditTool)];

    let (_, receipt) = agent.run("edit").unwrap();

    assert_eq!(receipt.repairs.edit_indentation_matches, 1);
    assert_eq!(receipt.repairs.edit_whitespace_matches, 0);
    assert_eq!(
        observed.lock().unwrap().as_deref(),
        Some("edited using indentation-flexible match")
    );
    assert!(events
        .lock()
        .unwrap()
        .iter()
        .any(|line| line.contains("edit_file used indentation-flexible match")));
}

struct ErrorEchoTool;

impl Tool for ErrorEchoTool {
    fn spec(&self) -> nh_tools::ToolSpec {
        nh_tools::ToolSpec {
            name: "error_echo".into(),
            description: "return a caller-derived test error".into(),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    fn execute(&self, _args: serde_json::Value, _ctx: &ToolCtx) -> anyhow::Result<String> {
        anyhow::bail!("could not open tool-error-fixture - choose a readable file")
    }
}

#[test]
fn tool_error_is_scrubbed_before_entering_the_conversation() {
    const LITERAL: &str = "tool-error-fixture";
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut agent = agent_with_receipt_path(
        dir.path(),
        dir.path().join("receipts.jsonl"),
        Box::new(FinalAnswerClient),
        events,
    );
    agent.tools = vec![Box::new(ErrorEchoTool)];
    agent.ctx = ToolCtx::new(dir.path().to_path_buf(), Box::new(|_| false))
        .with_scrubber(nh_vault::Scrubber::new(vec![LITERAL.to_string()]));

    let run = agent.run_tool(&ToolCallReq {
        id: "error-echo".into(),
        name: "error_echo".into(),
        arguments: "{}".into(),
    });

    assert_eq!(
        run.output,
        "tool 'error_echo' failed: could not open [REDACTED] - choose a readable file"
    );
    assert!(!run.output.contains(LITERAL));
}

#[test]
fn shell_alias_uses_the_real_exec_approval_gate() {
    let dir = tempfile::tempdir().unwrap();
    let approvals = Arc::new(AtomicUsize::new(0));
    let approvals_seen = Arc::clone(&approvals);
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut agent = agent_with_receipt_path(
        dir.path(),
        dir.path().join("receipts.jsonl"),
        Box::new(FinalAnswerClient),
        events,
    );
    agent.tools = vec![Box::new(nh_tools::ExecShell)];
    agent.ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |_| {
            approvals_seen.fetch_add(1, Ordering::SeqCst);
            false
        }),
    )
    .with_guard(Box::new(|_| nh_tools::Guard::Allow));

    let run = agent.run_tool(&ToolCallReq {
        id: "alias-exec".into(),
        name: "shell".into(),
        arguments: r#"{"command":"echo should-not-run"}"#.into(),
    });

    assert_eq!(approvals.load(Ordering::SeqCst), 1);
    assert!(run.repair_attempted);
    assert!(run
        .output
        .contains("mapped tool name 'shell' to 'exec_shell'"));
    assert!(run.output.ends_with("user denied: echo should-not-run"));
}

#[test]
fn malformed_call_gets_one_bounded_repair_annotation_then_fails() {
    let dir = tempfile::tempdir().unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut agent = agent_with_receipt_path(
        dir.path(),
        dir.path().join("receipts.jsonl"),
        Box::new(FinalAnswerClient),
        events,
    );
    agent.tools = vec![Box::new(nh_tools::ReadFile)];

    let run = agent.run_tool(&ToolCallReq {
        id: "bad-json".into(),
        name: "read_file".into(),
        arguments: "{path => nope".into(),
    });

    assert!(run.repair_attempted);
    assert_eq!(run.repair_notes.len(), 1);
    assert_eq!(
        run.output
            .matches("[tool-call repair:")
            .collect::<Vec<_>>()
            .len(),
        1
    );
    assert!(run
        .output
        .contains("invalid arguments JSON for 'read_file'"));
}
