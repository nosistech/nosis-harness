//! M2 context-engine integration tests. No network and no provider tokenizer coupling.

use std::sync::{Arc, Mutex};

use nh_core::agent::{AgentLoop, PrefixSeal};
use nh_core::receipt::ReceiptWriter;
use nh_core::wire::{
    cache_hit_pct, ChatClient, ChatMessage, ChatRequest, ChatResponse, ThinkingEffort, ToolCallReq,
    Usage,
};
use nh_tools::ToolCtx;

struct FinalAnswerClient;

impl ChatClient for FinalAnswerClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<ChatResponse> {
        Ok(final_answer(Some(Usage::default())))
    }
}

#[derive(Default)]
struct CacheState {
    previous: Option<Vec<Vec<u8>>>,
    prompt_tokens: u64,
    cached_tokens: u64,
}

struct PrefixCachingClient {
    state: Arc<Mutex<CacheState>>,
}

impl ChatClient for PrefixCachingClient {
    fn complete(&self, req: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let messages: Vec<Vec<u8>> = req
            .messages
            .iter()
            .map(|message| serde_json::to_vec(message).expect("message serializes"))
            .collect();
        let mut state = self.state.lock().unwrap();
        let shared = state.previous.as_ref().map_or(0, |previous| {
            previous
                .iter()
                .zip(&messages)
                .take_while(|(left, right)| left == right)
                .count()
        });
        let prompt_tokens = token_proxy(&messages);
        let cached_tokens = token_proxy(&messages[..shared]);
        state.prompt_tokens += prompt_tokens;
        state.cached_tokens += cached_tokens;
        state.previous = Some(messages);
        drop(state);

        Ok(final_answer(Some(Usage {
            prompt_tokens,
            completion_tokens: 1,
            cached_tokens: Some(cached_tokens),
        })))
    }
}

fn token_proxy(messages: &[Vec<u8>]) -> u64 {
    messages
        .iter()
        .map(|message| message.len() as u64)
        .sum::<u64>()
        / 4
}

fn final_answer(usage: Option<Usage>) -> ChatResponse {
    ChatResponse {
        message: message("assistant", "ok"),
        finish_reason: "stop".into(),
        usage,
    }
}

fn message(role: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: role.into(),
        content: Some(content.into()),
        tool_calls: None,
        tool_call_id: None,
        reasoning_content: None,
    }
}

fn assistant_tool_call(id: &str, reasoning: &str) -> ChatMessage {
    ChatMessage {
        role: "assistant".into(),
        content: None,
        tool_calls: Some(vec![ToolCallReq {
            id: id.into(),
            name: "read_file".into(),
            arguments: r#"{"path":"src/lib.rs"}"#.into(),
        }]),
        tool_call_id: None,
        reasoning_content: Some(reasoning.into()),
    }
}

fn tool_result(id: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: "tool".into(),
        content: Some(content.into()),
        tool_calls: None,
        tool_call_id: Some(id.into()),
        reasoning_content: None,
    }
}

fn agent(
    dir: &std::path::Path,
    client: Box<dyn ChatClient>,
    constitution: Option<String>,
    context_limit: Option<u64>,
) -> AgentLoop {
    AgentLoop {
        client,
        tools: Vec::new(),
        ctx: ToolCtx::new(dir.to_path_buf(), Box::new(|_| false)),
        receipts: ReceiptWriter::for_path(
            dir,
            dir.join("receipts.jsonl"),
            nh_vault::Scrubber::new(Vec::new()),
        ),
        model_id: "mock-model".into(),
        max_turns: 2,
        thinking: ThinkingEffort::None,
        profile: None,
        constitution,
        context_limit,
        on_event: None,
    }
}

fn compaction_history() -> Vec<ChatMessage> {
    vec![
        message("system", "immutable constitution bytes"),
        message("user", &"oldest turn ".repeat(80)),
        assistant_tool_call("drop-call", "reasoning that may be dropped"),
        tool_result("drop-call", "old tool result"),
        message("assistant", "oldest turn complete"),
        message("user", &"middle turn ".repeat(80)),
        message("assistant", "middle turn complete"),
        message("user", "recent user turn must survive"),
        assistant_tool_call("keep-call", "retained reasoning bytes"),
        tool_result("keep-call", "retained tool result"),
        message("assistant", "recent turn complete"),
    ]
}

fn assert_complete_tool_pairs(history: &[ChatMessage]) {
    for (index, message) in history.iter().enumerate() {
        if message.role == "tool" {
            let id = message
                .tool_call_id
                .as_deref()
                .expect("tool result has an id");
            assert!(history[..index].iter().any(|earlier| {
                earlier
                    .tool_calls
                    .iter()
                    .flatten()
                    .any(|call| call.id == id)
            }));
        }
        for call in message.tool_calls.iter().flatten() {
            assert!(
                history[index + 1..]
                    .iter()
                    .take_while(|following| following.role == "tool")
                    .any(|following| following.tool_call_id.as_deref() == Some(call.id.as_str())),
                "assistant tool call {} must retain its following result",
                call.id
            );
        }
    }
}

#[test]
fn compaction_preserves_prefix_user_boundary_tool_pairs_and_recent_turns() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = compaction_history();
    let original: Vec<Vec<u8>> = history
        .iter()
        .map(|message| serde_json::to_vec(message).unwrap())
        .collect();
    let expected_retained = original[7..].to_vec();
    let prefix_seal = PrefixSeal::new(&history[..1]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let mut loop_ = agent(dir.path(), Box::new(FinalAnswerClient), None, Some(100));
    loop_.on_event = Some(Box::new(move |line| {
        sink.lock().unwrap().push(line.to_string())
    }));

    loop_
        .run_with_history(&mut history, "current task")
        .unwrap();

    assert!(
        prefix_seal.check(&history),
        "all-build PrefixSeal must hold"
    );
    assert_eq!(
        history[1].role, "system",
        "the elision note is its own message at the compaction boundary"
    );
    let note = history[1].content.as_deref().unwrap();
    assert!(note.starts_with("[nosis] earlier context compacted: "));
    assert!(!note.contains("recent user turn must survive"));
    let retained: Vec<Vec<u8>> = history[2..2 + expected_retained.len()]
        .iter()
        .map(|message| serde_json::to_vec(message).unwrap())
        .collect();
    assert_eq!(
        retained, expected_retained,
        "every retained real message must stay byte-identical"
    );
    assert_eq!(
        history
            .iter()
            .filter(|message| {
                message.content.as_deref().is_some_and(|content| {
                    content.starts_with("[nosis] earlier context compacted:")
                })
            })
            .count(),
        1,
        "the marker is one separate synthetic message"
    );
    assert!(history.iter().any(
        |message| message.role == "user" && message.content.as_deref() == Some("current task")
    ));
    assert!(history.iter().any(|message| {
        message.reasoning_content.as_deref() == Some("retained reasoning bytes")
    }));
    assert_complete_tool_pairs(&history);

    let events = events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0].starts_with("context "));
    assert!(events[0].contains("% — compacted "));
}

#[test]
fn no_context_limit_never_compacts() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = compaction_history();
    let original: Vec<Vec<u8>> = history
        .iter()
        .map(|message| serde_json::to_vec(message).unwrap())
        .collect();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let mut loop_ = agent(dir.path(), Box::new(FinalAnswerClient), None, None);
    loop_.on_event = Some(Box::new(move |line| {
        sink.lock().unwrap().push(line.to_string())
    }));

    loop_
        .run_with_history(&mut history, "current task")
        .unwrap();

    let retained: Vec<Vec<u8>> = history[..original.len()]
        .iter()
        .map(|message| serde_json::to_vec(message).unwrap())
        .collect();
    assert_eq!(retained, original);
    assert!(!history.iter().any(|message| {
        message
            .content
            .as_deref()
            .is_some_and(|content| content.starts_with("[nosis] earlier context compacted:"))
    }));
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn stable_constitution_exceeds_sixty_percent_cache_hits_over_fifty_turns() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(Mutex::new(CacheState::default()));
    let client = PrefixCachingClient {
        state: Arc::clone(&state),
    };
    let constitution = "Nosis byte-stable operating constitution.\n".repeat(256);
    let expected_prefix = constitution.as_bytes().to_vec();
    let mut loop_ = agent(
        dir.path(),
        Box::new(client),
        Some(constitution),
        Some(10_000_000),
    );
    let mut history = Vec::new();

    for turn in 0..50 {
        loop_
            .run_with_history(&mut history, &format!("sequential task {turn}"))
            .unwrap();
        assert_eq!(
            history[0].content.as_deref().unwrap().as_bytes(),
            expected_prefix,
            "system prefix changed after turn {turn}"
        );
    }

    let state = state.lock().unwrap();
    let pct = cache_hit_pct(state.prompt_tokens, Some(state.cached_tokens)).unwrap();
    eprintln!("observed cache-hit percentage: {pct:.2}%");
    assert!(pct > 60.0, "observed cache-hit percentage was {pct:.2}%");
    assert_eq!(
        history.len(),
        101,
        "each task appends one user and one assistant"
    );
}
