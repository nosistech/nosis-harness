//! nh-core - agent turn loop, wire client, receipts.
//! Every turn writes a scrubbed JSONL receipt to .nosis/receipts.jsonl (append-only).

pub mod wire {
    //! OpenAI-compatible chat wire (M0). Anthropic Messages wire lands in M1.
    use zeroize::Zeroizing;

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct ChatMessage {
        pub role: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tool_calls: Option<Vec<ToolCallReq>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub tool_call_id: Option<String>,
    }

    /// OpenAI shape: `arguments` is a raw JSON string.
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct ToolCallReq {
        pub id: String,
        pub name: String,
        pub arguments: String,
    }

    #[derive(Debug, Clone)]
    pub struct ChatRequest {
        pub model: String,
        pub messages: Vec<ChatMessage>,
        pub tools: Vec<nh_tools::ToolSpec>,
    }

    #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
    pub struct Usage {
        pub prompt_tokens: u64,
        pub completion_tokens: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub cached_tokens: Option<u64>,
    }

    #[derive(Debug, Clone)]
    pub struct ChatResponse {
        pub message: ChatMessage,
        pub finish_reason: String,
        pub usage: Option<Usage>,
    }

    /// Provider abstraction - tests inject a mock, production uses OpenAiCompatClient.
    pub trait ChatClient: Send + Sync {
        fn complete(&self, req: &ChatRequest) -> anyhow::Result<ChatResponse>;
    }

    /// Blocking reqwest client against `{base_url}/chat/completions` (M0: no streaming).
    /// API key held zeroized, injected per-call, never logged.
    pub struct OpenAiCompatClient {
        pub base_url: String,
        api_key: Zeroizing<String>,
        http: reqwest::blocking::Client,
    }

    impl OpenAiCompatClient {
        pub fn new(base_url: String, api_key: Zeroizing<String>) -> Self {
            Self { base_url, api_key, http: reqwest::blocking::Client::new() }
        }
    }

    impl ChatClient for OpenAiCompatClient {
        fn complete(&self, _req: &ChatRequest) -> anyhow::Result<ChatResponse> {
            let _ = (&self.api_key, &self.http);
            todo!("build agent")
        }
    }
}

pub mod receipt {
    //! Typed receipts (plan §2): why runs fail, not just that they failed.

    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Outcome {
        Pass,
        Fail,
        Partial,
        Skip,
        Timeout,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum FailureClass {
        Context,
        Constraint,
        Verification,
        Planning,
    }

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    pub struct Receipt {
        pub ts_utc: String,
        pub model_id: String,
        pub task: String,
        pub turns: u32,
        pub tool_calls: u32,
        pub outcome: Outcome,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub failure_class: Option<FailureClass>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub usage: Option<super::wire::Usage>,
    }

    /// Appends scrubbed JSONL lines to .nosis/receipts.jsonl (creates dir if missing).
    pub struct ReceiptWriter {
        pub path: std::path::PathBuf,
        pub scrubber: nh_vault::Scrubber,
    }

    impl ReceiptWriter {
        pub fn append(&self, _receipt: &Receipt) -> anyhow::Result<()> {
            todo!("build agent: serialize, scrub the line, append")
        }
    }
}

pub mod agent {
    //! The turn loop: send task → model responds with tool calls → execute (gated) →
    //! feed results back → repeat until final answer or max_turns (then Outcome::Timeout).

    use crate::receipt::{Receipt, ReceiptWriter};
    use crate::wire::ChatClient;
    use nh_tools::{Tool, ToolCtx};

    pub struct AgentLoop {
        pub client: Box<dyn ChatClient>,
        pub tools: Vec<Box<dyn Tool>>,
        pub ctx: ToolCtx,
        pub receipts: ReceiptWriter,
        pub model_id: String,
        pub max_turns: u32,
    }

    impl AgentLoop {
        /// Runs one task to completion. Always writes exactly one receipt, even on error.
        /// Returns the final assistant text. UX: caller prints progress per turn -
        /// one short line per tool call (name + key arg), never a wall of JSON.
        pub fn run(&mut self, _task: &str) -> anyhow::Result<(String, Receipt)> {
            todo!("build agent")
        }
    }
}
