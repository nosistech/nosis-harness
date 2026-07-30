//! Integration tests for the agent loop — no network, scripted ChatClient.

use std::sync::{Arc, Mutex};

use nh_core::agent::{AgentLoop, MAX_TASK_BYTES};
use nh_core::receipt::{FailureClass, Outcome, Receipt, ReceiptWriter};
use nh_core::wire::{ChatClient, ChatMessage, ChatRequest, ChatResponse, ToolCallReq, Usage};
use nh_tools::{Tool, ToolCtx, ToolSpec};

/// Obviously fake test secret (never a real key shape in use).
const FAKE_SECRET: &str = "sk-test-00000000";

/// Pops scripted responses in order; errors if the script runs out.
struct ScriptedClient {
    responses: Mutex<Vec<ChatResponse>>,
}

impl ChatClient for ScriptedClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            anyhow::bail!("scripted client exhausted");
        }
        Ok(responses.remove(0))
    }
}

/// Always answers with the same tool call — the loop can never finish.
struct AlwaysToolCallClient;

impl ChatClient for AlwaysToolCallClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<ChatResponse> {
        Ok(tool_call_resp(
            "mystery_tool",
            r#"{"path":"x"}"#,
            "call_loop",
            None,
        ))
    }
}

/// Always fails, like a provider outage.
struct FailingClient;

impl ChatClient for FailingClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<ChatResponse> {
        anyhow::bail!("provider returned HTTP 500: overloaded")
    }
}

struct NeverCalledClient;

impl ChatClient for NeverCalledClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<ChatResponse> {
        panic!("invalid input reached the provider")
    }
}

/// Minimal in-test edit_file tool: exact string replace inside ctx.workdir.
struct TestEditFile;

impl Tool for TestEditFile {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "edit_file".into(),
            description: "replace old_string with new_string in path".into(),
            parameters: serde_json::json!({"type": "object"}),
        }
    }
    fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        let path = ctx.workdir.join(args["path"].as_str().unwrap());
        let text = std::fs::read_to_string(&path)?;
        let new = text.replace(
            args["old_string"].as_str().unwrap(),
            args["new_string"].as_str().unwrap(),
        );
        std::fs::write(&path, new)?;
        Ok("edited".into())
    }
}

fn tool_call_resp(name: &str, arguments: &str, id: &str, usage: Option<Usage>) -> ChatResponse {
    ChatResponse {
        message: ChatMessage {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![ToolCallReq {
                id: id.into(),
                name: name.into(),
                arguments: arguments.into(),
            }]),
            tool_call_id: None,
            reasoning_content: None,
        },
        finish_reason: "tool_calls".into(),
        usage,
    }
}

fn text_resp(text: &str, usage: Option<Usage>) -> ChatResponse {
    ChatResponse {
        message: ChatMessage {
            role: "assistant".into(),
            content: Some(text.into()),
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: None,
        },
        finish_reason: "stop".into(),
        usage,
    }
}

fn agent_in(
    dir: &std::path::Path,
    client: Box<dyn ChatClient>,
    tools: Vec<Box<dyn Tool>>,
    max_turns: u32,
) -> AgentLoop {
    AgentLoop {
        client,
        tools,
        ctx: ToolCtx::new(dir.to_path_buf(), Box::new(|_| true)),
        receipts: ReceiptWriter::project(
            dir,
            nh_vault::Scrubber::new(vec![FAKE_SECRET.to_string()]),
        ),
        model_id: "mock-model".into(),
        max_turns,
        thinking: nh_core::wire::ThinkingEffort::None,
        profile: None,
        constitution: None,
        context_limit: None,
        on_event: None,
    }
}

fn receipt_lines(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(dir.join(".nosis").join("receipts.jsonl"))
        .expect("receipts file must exist")
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn receipt_records_effective_profile_and_omits_none() {
    let profiled_dir = tempfile::tempdir().unwrap();
    let mut profiled = agent_in(
        profiled_dir.path(),
        Box::new(ScriptedClient {
            responses: Mutex::new(vec![text_resp("done", None)]),
        }),
        vec![],
        1,
    );
    profiled.profile = Some("frugal".into());
    let (_, receipt) = profiled.run("profiled").unwrap();
    assert_eq!(receipt.effective_profile.as_deref(), Some("frugal"));
    assert!(receipt_lines(profiled_dir.path())[0].contains(r#""effective_profile":"frugal""#));

    let default_dir = tempfile::tempdir().unwrap();
    let mut default = agent_in(
        default_dir.path(),
        Box::new(ScriptedClient {
            responses: Mutex::new(vec![text_resp("done", None)]),
        }),
        vec![],
        1,
    );
    let (_, receipt) = default.run("default").unwrap();
    assert_eq!(receipt.effective_profile, None);
    assert!(
        !receipt_lines(default_dir.path())[0].contains("effective_profile"),
        "None preserves the pre-profile JSON shape"
    );
}

#[test]
fn pre_profile_receipt_json_still_parses() {
    let receipt: Receipt = serde_json::from_str(
        r#"{"ts_utc":"2026-07-18T12:00:00Z","model_id":"mock","task":"old","turns":1,"tool_calls":0,"outcome":"pass"}"#,
    )
    .unwrap();
    assert_eq!(receipt.effective_profile, None);
}

#[test]
fn edits_file_passes_and_scrubs_receipt() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.py"), "def f():\n    return 1\n").unwrap();

    let script = vec![
        tool_call_resp(
            "edit_file",
            r#"{"path":"main.py","old_string":"return 1","new_string":"return 2"}"#,
            "call_1",
            Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
                cached_tokens: Some(2),
            }),
        ),
        text_resp(
            "done",
            Some(Usage {
                prompt_tokens: 20,
                completion_tokens: 3,
                cached_tokens: None,
            }),
        ),
    ];
    let mut agent = agent_in(
        dir.path(),
        Box::new(ScriptedClient {
            responses: Mutex::new(script),
        }),
        vec![Box::new(TestEditFile)],
        8,
    );
    let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    agent.on_event = Some(Box::new(move |line| {
        sink.lock().unwrap().push(line.to_string())
    }));

    let task = format!("fix main.py; stray key in task: {FAKE_SECRET}");
    let (text, receipt) = agent.run(&task).unwrap();

    assert_eq!(text, "done");
    assert_eq!(receipt.outcome, Outcome::Pass);
    assert_eq!(receipt.turns, 2);
    assert_eq!(receipt.tool_calls, 1);
    let usage = receipt.usage.expect("usage accumulated");
    assert_eq!(usage.prompt_tokens, 30);
    assert_eq!(usage.completion_tokens, 8);
    assert_eq!(
        usage.cached_tokens, None,
        "one unreported turn makes the cumulative cache measurement absent"
    );
    assert_eq!(receipt.cache_hit_pct, None);

    let edited = std::fs::read_to_string(dir.path().join("main.py")).unwrap();
    assert!(edited.contains("return 2"));
    assert!(!edited.contains("return 1"));

    let lines = receipt_lines(dir.path());
    assert_eq!(lines.len(), 1, "exactly one receipt per run");
    assert!(lines[0].contains(r#""outcome":"pass""#));
    assert!(lines[0].contains("[REDACTED]"), "secret must be redacted");
    assert!(
        !lines[0].contains(FAKE_SECRET),
        "secret must not leak into receipts"
    );

    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["turn 1: edit_file main.py"]
    );
}

#[test]
fn endless_tool_calls_hit_max_turns_and_write_timeout_receipt() {
    let dir = tempfile::tempdir().unwrap();
    // No tools registered: also proves unknown tool names never crash the loop.
    let mut agent = agent_in(dir.path(), Box::new(AlwaysToolCallClient), vec![], 3);

    let (text, receipt) = agent.run("never finishes").unwrap();

    assert_eq!(receipt.outcome, Outcome::Timeout);
    assert_eq!(receipt.failure_class, Some(FailureClass::Constraint));
    assert_eq!(receipt.turns, 3);
    assert_eq!(receipt.tool_calls, 3);
    assert!(text.contains("3 turns"));

    let lines = receipt_lines(dir.path());
    assert_eq!(lines.len(), 1, "exactly one receipt per run");
    assert!(lines[0].contains(r#""outcome":"timeout""#));
    assert!(lines[0].contains(r#""failure_class":"constraint""#));
}

#[test]
fn run_with_history_carries_context_and_writes_one_receipt_per_task() {
    let dir = tempfile::tempdir().unwrap();
    let script = vec![text_resp("four", None), text_resp("five", None)];
    let mut agent = agent_in(
        dir.path(),
        Box::new(ScriptedClient {
            responses: Mutex::new(script),
        }),
        vec![],
        8,
    );

    let mut history: Vec<ChatMessage> = Vec::new();
    let (text, receipt) = agent
        .run_with_history(&mut history, "what is 2+2?")
        .unwrap();
    assert_eq!(text, "four");
    assert_eq!(receipt.outcome, Outcome::Pass);
    // system + user + assistant
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].role, "system");
    assert_eq!(history[1].role, "user");
    assert_eq!(history[2].content.as_deref(), Some("four"));

    let (text, _) = agent.run_with_history(&mut history, "add one").unwrap();
    assert_eq!(text, "five");
    // Same session: no second system message, prior turns still present.
    assert_eq!(history.len(), 5);
    assert_eq!(history[3].role, "user");
    assert_eq!(history[4].content.as_deref(), Some("five"));
    assert_eq!(history.iter().filter(|m| m.role == "system").count(), 1);

    assert_eq!(receipt_lines(dir.path()).len(), 2, "one receipt per task");
}

#[test]
fn oversized_task_is_rejected_before_provider_history_or_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = agent_in(dir.path(), Box::new(NeverCalledClient), vec![], 8);
    let mut history = vec![ChatMessage {
        role: "system".into(),
        content: Some("existing session".into()),
        tool_calls: None,
        tool_call_id: None,
        reasoning_content: None,
    }];
    let error = agent
        .run_with_history(&mut history, &"x".repeat(MAX_TASK_BYTES + 1))
        .unwrap_err();

    assert!(error.to_string().contains("maximum"));
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].role, "system");
    assert_eq!(history[0].content.as_deref(), Some("existing session"));
    assert!(!dir.path().join(".nosis").join("receipts.jsonl").exists());
}

#[test]
fn run_with_history_keeps_tool_turns_even_on_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = agent_in(dir.path(), Box::new(AlwaysToolCallClient), vec![], 2);

    let mut history: Vec<ChatMessage> = Vec::new();
    let (_, receipt) = agent
        .run_with_history(&mut history, "never finishes")
        .unwrap();

    assert_eq!(receipt.outcome, Outcome::Timeout);
    // system, user, then (assistant tool-call + tool result) per turn.
    assert_eq!(history.len(), 6);
    assert_eq!(history[2].role, "assistant");
    assert_eq!(history[3].role, "tool");
    assert_eq!(history[4].role, "assistant");
    assert_eq!(history[5].role, "tool");
}

#[test]
fn provider_error_writes_fail_receipt_and_returns_err() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = agent_in(dir.path(), Box::new(FailingClient), vec![], 5);

    let err = agent.run("anything").unwrap_err();
    assert!(err.to_string().contains("HTTP 500"));

    let lines = receipt_lines(dir.path());
    assert_eq!(lines.len(), 1, "exactly one receipt per run");
    assert!(lines[0].contains(r#""outcome":"fail""#));
    assert!(lines[0].contains(r#""failure_class":"verification""#));
}
