//! Opt-in, local-only efficiency measurements for one `nh run` task.
//!
//! Records contain sizes and observed outcomes only. Task text, message text,
//! tool arguments/results, reasoning text, provider errors, headers, and
//! credentials are never serialized.

use crate::receipt::{FailureClass, Outcome, Receipt, ReceiptKind};
use crate::wire::{
    ChatClient, ChatRequest, ChatResponse, ContentPart, FinishReason, RetryExhausted, RetryStats,
    Usage, UsageEvidence,
};
use chrono::{DateTime, SecondsFormat, Utc};
use nh_routes::ResolvedRoute;
use nh_tools::{
    CommandOutcome, FileChangeKind, Tool, ToolArgs, ToolAudit, ToolCtx, ToolExecution, ToolSpec,
};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const EFFICIENCY_SCHEMA_VERSION: u32 = 1;
pub const EFFICIENCY_RELATIVE_PATH: &str = ".nosis/efficiency-v1.jsonl";
pub const MAX_EFFICIENCY_LOG_BYTES: u64 = 8 * 1024 * 1024;

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize)]
struct RouteMeasurement {
    route_id: String,
    model_id: String,
    price: PriceContext,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum PriceContext {
    Captured {
        quote_at_utc: String,
        unit: &'static str,
        currency: String,
        input_cache_hit: f64,
        input_cache_miss: f64,
        output: f64,
        confidence: String,
        peak: bool,
        limitation: &'static str,
    },
    Unavailable {
        evaluator_action: &'static str,
    },
}

#[derive(Debug, Clone, Serialize)]
struct RequestSizeEstimates {
    basis: &'static str,
    system_bytes: u64,
    user_bytes: u64,
    assistant_bytes: u64,
    tool_bytes: u64,
    tool_schema_bytes: Option<u64>,
    reasoning_bytes: u64,
    image_base64_bytes: u64,
    total_bucket_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
struct ToolSchemaMeasurement {
    basis: &'static str,
    tool_count: u64,
    canonical_json_bytes: Option<u64>,
    identity: Option<String>,
    same_as_previous_request: Option<bool>,
    limitation: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct UsageMeasurement {
    usage_object_reported: bool,
    evidence: &'static str,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    cached_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct RetryMeasurement {
    retries: Option<u32>,
    rate_limited: Option<u32>,
    attempts: Option<u32>,
    attempt_level_usage: &'static str,
}

#[derive(Debug, Serialize)]
struct TaskStartRecord {
    schema_version: u32,
    record_type: &'static str,
    task_id: String,
    recorded_at_utc: String,
    route: RouteMeasurement,
}

#[derive(Debug, Serialize)]
struct RequestRecord {
    schema_version: u32,
    record_type: &'static str,
    task_id: String,
    request_seq: u64,
    recorded_at_utc: String,
    route: RouteMeasurement,
    sizes: RequestSizeEstimates,
    tool_schema: ToolSchemaMeasurement,
    elapsed_ms: u64,
    result: &'static str,
    finish_reason: Option<&'static str>,
    usage: UsageMeasurement,
    retries: RetryMeasurement,
}

#[derive(Debug, Serialize)]
struct ToolRecord {
    schema_version: u32,
    record_type: &'static str,
    task_id: String,
    tool_seq: u64,
    recorded_at_utc: String,
    tool_name: String,
    elapsed_ms: u64,
    outcome: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ContextEconomicsMeasurement {
    pub(crate) quote_available: bool,
    pub(crate) currency: Option<String>,
    pub(crate) cache_hit_per_million: Option<f64>,
    pub(crate) cache_miss_per_million: Option<f64>,
    pub(crate) output_per_million: Option<f64>,
    pub(crate) preceding_cached_tokens: Option<u64>,
    pub(crate) assumed_future_requests: u32,
    pub(crate) retrieval_output_token_allowance: u64,
    pub(crate) estimated_savings: Option<f64>,
    pub(crate) estimated_penalty: Option<f64>,
    pub(crate) limitation: &'static str,
}

#[derive(Debug, Clone)]
pub(crate) struct ContextMeasurement {
    pub(crate) request_seq: u64,
    pub(crate) decision: &'static str,
    pub(crate) reason: &'static str,
    pub(crate) original_estimated_tokens: u64,
    pub(crate) sent_estimated_tokens: u64,
    pub(crate) removed_estimated_tokens: u64,
    pub(crate) effective_context_limit: u64,
    pub(crate) original_context_percent: u64,
    pub(crate) sent_context_percent: u64,
    pub(crate) archive_count: u32,
    pub(crate) messages_archived: u64,
    pub(crate) system_messages_preserved: u64,
    pub(crate) user_messages_preserved: u64,
    pub(crate) history_rewrite_detected: bool,
    pub(crate) economics: ContextEconomicsMeasurement,
}

#[derive(Debug, Serialize)]
struct ContextRecord {
    schema_version: u32,
    record_type: &'static str,
    task_id: String,
    recorded_at_utc: String,
    request_seq: u64,
    decision: &'static str,
    reason: &'static str,
    original_estimated_tokens: u64,
    sent_estimated_tokens: u64,
    removed_estimated_tokens: u64,
    effective_context_limit: u64,
    original_context_percent: u64,
    sent_context_percent: u64,
    archive_count: u32,
    messages_archived: u64,
    system_messages_preserved: u64,
    user_messages_preserved: u64,
    history_rewrite_detected: bool,
    economics: ContextEconomicsMeasurement,
}

#[derive(Debug, Serialize)]
struct TaskRecord {
    schema_version: u32,
    record_type: &'static str,
    task_id: String,
    recorded_at_utc: String,
    route: RouteMeasurement,
    receipt_kind: &'static str,
    receipt_outcome: &'static str,
    correctness_assessed: bool,
    failure_class: Option<&'static str>,
    turns: u32,
    tool_calls: u32,
    duration_ms: Option<u64>,
    usage: UsageMeasurement,
    retries: RetryMeasurement,
    compaction: crate::receipt::CompactionStats,
    recorder: RecorderMeasurement,
}

#[derive(Debug, Serialize)]
struct RecorderMeasurement {
    records_dropped_before_summary: u64,
    task_duration_basis: &'static str,
}

struct EfficiencyInner {
    root: PathBuf,
    path: PathBuf,
    scrubber: nh_vault::Scrubber,
    task_id: String,
    route: RouteMeasurement,
    max_bytes: u64,
    request_seq: AtomicU64,
    tool_seq: AtomicU64,
    dropped_records: AtomicU64,
    first_error: Mutex<Option<String>>,
    previous_tool_schema_identity: Mutex<Option<String>>,
}

/// Shared recorder used by request, tool, and task boundaries for one run.
#[derive(Clone)]
pub struct EfficiencyRecorder {
    inner: Arc<EfficiencyInner>,
}

impl EfficiencyRecorder {
    /// Construct a recorder only when the caller's explicit flag is enabled.
    /// The disabled path performs no filesystem operation.
    pub fn project_if_enabled(
        enabled: bool,
        root: impl Into<PathBuf>,
        route: &ResolvedRoute,
        quote_at: DateTime<Utc>,
        scrubber: nh_vault::Scrubber,
    ) -> Option<Self> {
        enabled.then(|| Self::project(root, route, quote_at, scrubber))
    }

    fn project(
        root: impl Into<PathBuf>,
        route: &ResolvedRoute,
        quote_at: DateTime<Utc>,
        scrubber: nh_vault::Scrubber,
    ) -> Self {
        Self::project_with_limit(root, route, quote_at, scrubber, MAX_EFFICIENCY_LOG_BYTES)
    }

    fn project_with_limit(
        root: impl Into<PathBuf>,
        route: &ResolvedRoute,
        quote_at: DateTime<Utc>,
        scrubber: nh_vault::Scrubber,
        max_bytes: u64,
    ) -> Self {
        let root = root.into();
        let safe_value = |value: &str| nh_vault::sanitize_untrusted_text(&scrubber.scrub(value));
        let price = route.price_at(quote_at).map_or(
            PriceContext::Unavailable {
                evaluator_action: "supply the matching catalog snapshot; no zero price is implied",
            },
            |quote| PriceContext::Captured {
                quote_at_utc: timestamp(quote_at),
                unit: "per_million_tokens",
                currency: quote.currency.as_str().to_owned(),
                input_cache_hit: quote.cache_hit,
                input_cache_miss: quote.cache_miss,
                output: quote.output,
                confidence: quote.confidence.as_str().to_owned(),
                peak: quote.peak,
                limitation:
                    "task-start quote; a task crossing a pricing window may use different rates",
            },
        );
        let route = RouteMeasurement {
            route_id: safe_value(route.id()),
            model_id: safe_value(route.model_id()),
            price,
        };
        let path = root.join(EFFICIENCY_RELATIVE_PATH);
        let recorder = Self {
            inner: Arc::new(EfficiencyInner {
                root,
                path,
                scrubber,
                task_id: task_id(),
                route,
                max_bytes,
                request_seq: AtomicU64::new(0),
                tool_seq: AtomicU64::new(0),
                dropped_records: AtomicU64::new(0),
                first_error: Mutex::new(None),
                previous_tool_schema_identity: Mutex::new(None),
            }),
        };
        recorder.record_task_start();
        recorder
    }

    /// Stable identifier shared by every measurement record for this task.
    pub fn task_id(&self) -> &str {
        &self.inner.task_id
    }

    pub fn wrap_client(&self, inner: Box<dyn ChatClient>) -> Box<dyn ChatClient> {
        Box::new(MeasuredClient {
            inner,
            recorder: self.clone(),
        })
    }

    pub fn wrap_tools(&self, tools: Vec<Box<dyn Tool>>) -> Vec<Box<dyn Tool>> {
        tools
            .into_iter()
            .map(|inner| {
                Box::new(MeasuredTool {
                    inner,
                    recorder: self.clone(),
                }) as Box<dyn Tool>
            })
            .collect()
    }

    pub(crate) fn record_context(&self, measurement: ContextMeasurement) {
        let record = ContextRecord {
            schema_version: EFFICIENCY_SCHEMA_VERSION,
            record_type: "context",
            task_id: self.inner.task_id.clone(),
            recorded_at_utc: timestamp(Utc::now()),
            request_seq: measurement.request_seq,
            decision: measurement.decision,
            reason: measurement.reason,
            original_estimated_tokens: measurement.original_estimated_tokens,
            sent_estimated_tokens: measurement.sent_estimated_tokens,
            removed_estimated_tokens: measurement.removed_estimated_tokens,
            effective_context_limit: measurement.effective_context_limit,
            original_context_percent: measurement.original_context_percent,
            sent_context_percent: measurement.sent_context_percent,
            archive_count: measurement.archive_count,
            messages_archived: measurement.messages_archived,
            system_messages_preserved: measurement.system_messages_preserved,
            user_messages_preserved: measurement.user_messages_preserved,
            history_rewrite_detected: measurement.history_rewrite_detected,
            economics: measurement.economics,
        };
        self.record(&record);
    }

    /// Record the core receipt as an execution summary, never as correctness.
    pub fn record_task(&self, receipt: &Receipt) {
        let record = TaskRecord {
            schema_version: EFFICIENCY_SCHEMA_VERSION,
            record_type: "task",
            task_id: self.inner.task_id.clone(),
            recorded_at_utc: timestamp(Utc::now()),
            route: self.inner.route.clone(),
            receipt_kind: receipt_kind(receipt.kind),
            receipt_outcome: receipt_outcome(receipt.outcome),
            correctness_assessed: false,
            failure_class: receipt.failure_class.map(failure_class),
            turns: receipt.turns,
            tool_calls: receipt.tool_calls,
            duration_ms: receipt.duration_ms,
            usage: usage_measurement(receipt.usage.as_ref()),
            retries: task_retry_measurement(receipt.turns, receipt.retries),
            compaction: *receipt.compaction,
            recorder: RecorderMeasurement {
                records_dropped_before_summary: self
                    .inner
                    .dropped_records
                    .load(Ordering::Relaxed),
                task_duration_basis:
                    "receipt wall time includes local efficiency recorder append, flush, and sync overhead",
            },
        };
        self.record(&record);
    }

    /// First local write failure, scrubbed for safe stderr presentation.
    pub fn warning(&self) -> Option<String> {
        match self.inner.first_error.lock() {
            Ok(error) => error.clone(),
            Err(_) => Some("efficiency recorder error state is unavailable".to_owned()),
        }
    }

    pub const fn relative_path() -> &'static str {
        EFFICIENCY_RELATIVE_PATH
    }

    fn record_task_start(&self) {
        let record = TaskStartRecord {
            schema_version: EFFICIENCY_SCHEMA_VERSION,
            record_type: "task_start",
            task_id: self.inner.task_id.clone(),
            recorded_at_utc: timestamp(Utc::now()),
            route: self.inner.route.clone(),
        };
        self.record(&record);
    }

    fn record_request(
        &self,
        request: &ChatRequest,
        elapsed_ms: u64,
        result: &anyhow::Result<ChatResponse>,
    ) {
        let request_seq = self
            .inner
            .request_seq
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let (status, finish_reason, usage, retries) = match result {
            Ok(response) => (
                "response",
                Some(finish_reason(&response.finish_reason)),
                usage_measurement(response.usage.as_ref()),
                retry_measurement(Some(response.retries), None),
            ),
            Err(error) => {
                let exhausted = error.downcast_ref::<RetryExhausted>();
                (
                    "error",
                    None,
                    usage_measurement(exhausted.and_then(|failure| failure.usage.as_ref())),
                    exhausted.map_or_else(
                        || retry_measurement(None, None),
                        |failure| retry_measurement(Some(failure.stats), Some(failure.attempts)),
                    ),
                )
            }
        };
        let record = RequestRecord {
            schema_version: EFFICIENCY_SCHEMA_VERSION,
            record_type: "request",
            task_id: self.inner.task_id.clone(),
            request_seq,
            recorded_at_utc: timestamp(Utc::now()),
            route: self.inner.route.clone(),
            sizes: request_sizes(request),
            tool_schema: self.tool_schema_measurement(&request.tools),
            elapsed_ms,
            result: status,
            finish_reason,
            usage,
            retries,
        };
        self.record(&record);
    }

    fn tool_schema_measurement(&self, tools: &[ToolSpec]) -> ToolSchemaMeasurement {
        let (canonical_json_bytes, identity) = tool_schema_identity(tools)
            .map(|(bytes, identity)| (Some(bytes), Some(identity)))
            .unwrap_or((None, None));
        let same_as_previous_request = identity.as_ref().and_then(|identity| {
            let mut previous = self.inner.previous_tool_schema_identity.lock().ok()?;
            let same = previous.as_ref().map(|prior| prior == identity);
            *previous = Some(identity.clone());
            same
        });
        ToolSchemaMeasurement {
            basis: "ordered ToolSpec array as canonical serde_json UTF-8 before provider encoding",
            tool_count: u64::try_from(tools.len()).unwrap_or(u64::MAX),
            canonical_json_bytes,
            identity,
            same_as_previous_request,
            limitation: "noncryptographic schema-only drift indicator; does not prove provider token order or cache reuse",
        }
    }

    fn record_tool(&self, name: &str, elapsed_ms: u64, outcome: &'static str) {
        let tool_seq = self
            .inner
            .tool_seq
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let name = nh_vault::sanitize_untrusted_text(&self.inner.scrubber.scrub(name));
        let record = ToolRecord {
            schema_version: EFFICIENCY_SCHEMA_VERSION,
            record_type: "tool",
            task_id: self.inner.task_id.clone(),
            tool_seq,
            recorded_at_utc: timestamp(Utc::now()),
            tool_name: name,
            elapsed_ms,
            outcome,
        };
        self.record(&record);
    }

    fn record<T: Serialize>(&self, record: &T) {
        let result = (|| {
            let path = crate::runtime_path::ensure_contained_file(
                &self.inner.root,
                &self.inner.path,
                "efficiency log",
            )?;
            let line = crate::jsonl::serialize_scrubbed(
                record,
                &self.inner.scrubber,
                "efficiency record",
            )?;
            crate::jsonl::append_locked_line_bounded(&path, &line, self.inner.max_bytes)
        })();
        if let Err(error) = result {
            self.inner.dropped_records.fetch_add(1, Ordering::Relaxed);
            let safe = self.inner.scrubber.scrub(&error.to_string());
            if let Ok(mut first) = self.inner.first_error.lock() {
                if first.is_none() {
                    *first = Some(safe);
                }
            }
        }
    }
}

struct MeasuredClient {
    inner: Box<dyn ChatClient>,
    recorder: EfficiencyRecorder,
}

impl ChatClient for MeasuredClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let started = Instant::now();
        let result = self.inner.complete(request);
        self.recorder
            .record_request(request, elapsed_ms(started), &result);
        result
    }
}

struct MeasuredTool {
    inner: Box<dyn Tool>,
    recorder: EfficiencyRecorder,
}

impl Tool for MeasuredTool {
    fn spec(&self) -> ToolSpec {
        self.inner.spec()
    }

    fn execute(&self, args: ToolArgs, ctx: &ToolCtx) -> anyhow::Result<String> {
        self.execute_with_audit(args, ctx)
            .map(|execution| execution.output)
    }

    fn execute_with_audit(&self, args: ToolArgs, ctx: &ToolCtx) -> anyhow::Result<ToolExecution> {
        let name = self.inner.spec().name;
        let started = Instant::now();
        let result = self.inner.execute_with_audit(args, ctx);
        let outcome = match &result {
            Ok(execution) => observed_tool_outcome(&execution.audit),
            Err(_) => "error",
        };
        self.recorder
            .record_tool(&name, elapsed_ms(started), outcome);
        result
    }
}

fn observed_tool_outcome(audit: &[ToolAudit]) -> &'static str {
    audit
        .iter()
        .rev()
        .find_map(|fact| match fact {
            ToolAudit::FilePublished(FileChangeKind::Created) => Some("file_created"),
            ToolAudit::FilePublished(FileChangeKind::Edited) => Some("file_edited"),
            ToolAudit::Command(CommandOutcome::Exited(Some(0))) => Some("command_exit_zero"),
            ToolAudit::Command(CommandOutcome::Exited(Some(_))) => Some("command_exit_nonzero"),
            ToolAudit::Command(CommandOutcome::Exited(None)) => Some("command_exit_unknown"),
            ToolAudit::Command(CommandOutcome::Blocked) => Some("command_blocked"),
            ToolAudit::Command(CommandOutcome::Denied) => Some("command_denied"),
            ToolAudit::Command(CommandOutcome::CancelledBeforeStart) => {
                Some("command_cancelled_before_start")
            }
            ToolAudit::Command(CommandOutcome::Cancelled {
                termination_complete: true,
            }) => Some("command_cancelled_reaped"),
            ToolAudit::Command(CommandOutcome::Cancelled {
                termination_complete: false,
            }) => Some("command_cancelled_cleanup_incomplete"),
            ToolAudit::Command(CommandOutcome::TimedOut {
                termination_complete: true,
            }) => Some("command_timed_out_reaped"),
            ToolAudit::Command(CommandOutcome::TimedOut {
                termination_complete: false,
            }) => Some("command_timed_out_cleanup_incomplete"),
            ToolAudit::EditMatch(_) => None,
        })
        .unwrap_or("returned_unclassified")
}

fn request_sizes(request: &ChatRequest) -> RequestSizeEstimates {
    let mut sizes = RequestSizeEstimates {
        basis: "utf8_bytes_before_provider_encoding",
        system_bytes: 0,
        user_bytes: 0,
        assistant_bytes: 0,
        tool_bytes: 0,
        tool_schema_bytes: json_size(&request.tools),
        reasoning_bytes: 0,
        image_base64_bytes: 0,
        total_bucket_bytes: 0,
    };
    for message in &request.messages {
        if let Some(parts) = &message.parts {
            for part in parts {
                match part {
                    ContentPart::Text { text } => {
                        add_role_bytes(&mut sizes, &message.role, byte_len(text));
                    }
                    ContentPart::ImageB64 { data, .. } => {
                        sizes.image_base64_bytes =
                            sizes.image_base64_bytes.saturating_add(byte_len(data));
                    }
                }
            }
        } else if let Some(content) = &message.content {
            add_role_bytes(&mut sizes, &message.role, byte_len(content));
        }
        if let Some(calls) = &message.tool_calls {
            for call in calls {
                sizes.tool_bytes = sizes
                    .tool_bytes
                    .saturating_add(byte_len(&call.id))
                    .saturating_add(byte_len(&call.name))
                    .saturating_add(byte_len(&call.arguments));
            }
        }
        if let Some(id) = &message.tool_call_id {
            sizes.tool_bytes = sizes.tool_bytes.saturating_add(byte_len(id));
        }
        if let Some(reasoning) = &message.reasoning_content {
            sizes.reasoning_bytes = sizes.reasoning_bytes.saturating_add(byte_len(reasoning));
        }
    }
    sizes.total_bucket_bytes = sizes
        .system_bytes
        .saturating_add(sizes.user_bytes)
        .saturating_add(sizes.assistant_bytes)
        .saturating_add(sizes.tool_bytes)
        .saturating_add(sizes.tool_schema_bytes.unwrap_or(0))
        .saturating_add(sizes.reasoning_bytes)
        .saturating_add(sizes.image_base64_bytes);
    sizes
}

fn add_role_bytes(sizes: &mut RequestSizeEstimates, role: &str, bytes: u64) {
    let bucket = match role {
        "system" => &mut sizes.system_bytes,
        "user" => &mut sizes.user_bytes,
        "assistant" => &mut sizes.assistant_bytes,
        _ => &mut sizes.tool_bytes,
    };
    *bucket = bucket.saturating_add(bytes);
}

fn json_size(value: &impl Serialize) -> Option<u64> {
    struct Counter(u64);

    impl std::io::Write for Counter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(byte_len(buffer));
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(counter.0)
}

fn tool_schema_identity(tools: &[ToolSpec]) -> Option<(u64, String)> {
    struct FnvWriter {
        hash: u64,
        bytes: u64,
    }

    impl std::io::Write for FnvWriter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            for byte in buffer {
                self.hash ^= u64::from(*byte);
                self.hash = self.hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
            self.bytes = self
                .bytes
                .saturating_add(u64::try_from(buffer.len()).unwrap_or(u64::MAX));
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut writer = FnvWriter {
        hash: 0xcbf2_9ce4_8422_2325,
        bytes: 0,
    };
    serde_json::to_writer(&mut writer, tools).ok()?;
    Some((writer.bytes, format!("fnv1a64:{:016x}", writer.hash)))
}

fn usage_measurement(usage: Option<&Usage>) -> UsageMeasurement {
    let reported = usage.is_some();
    let evidence = usage.map_or(UsageEvidence::Unknown, |usage| usage.evidence);
    let counters = usage.filter(|usage| usage.evidence.has_reported_counters());
    UsageMeasurement {
        usage_object_reported: reported,
        evidence: usage_evidence(evidence),
        prompt_tokens: counters.map(|usage| usage.prompt_tokens),
        completion_tokens: counters.map(|usage| usage.completion_tokens),
        cached_tokens: counters.and_then(|usage| usage.cached_tokens),
    }
}

fn retry_measurement(stats: Option<RetryStats>, attempts: Option<u32>) -> RetryMeasurement {
    RetryMeasurement {
        retries: stats.map(|stats| stats.retries),
        rate_limited: stats.map(|stats| stats.rate_limited),
        attempts: attempts.or_else(|| stats.map(|stats| stats.retries.saturating_add(1))),
        attempt_level_usage: "unavailable; provider clients report aggregate retry evidence",
    }
}

fn task_retry_measurement(turns: u32, stats: RetryStats) -> RetryMeasurement {
    RetryMeasurement {
        retries: Some(stats.retries),
        rate_limited: Some(stats.rate_limited),
        attempts: Some(turns.saturating_add(stats.retries)),
        attempt_level_usage: "unavailable; provider clients report aggregate retry evidence",
    }
}

fn finish_reason(reason: &FinishReason) -> &'static str {
    match reason {
        FinishReason::Missing => "missing",
        FinishReason::Stop => "stop",
        FinishReason::ToolUse => "tool_use",
        FinishReason::Truncated => "truncated",
        FinishReason::ContextWindow => "context_window",
        FinishReason::Filtered => "filtered",
        FinishReason::Interrupted => "interrupted",
        FinishReason::Unknown(_) => "unknown",
    }
}

fn usage_evidence(evidence: UsageEvidence) -> &'static str {
    match evidence {
        UsageEvidence::Measured => "measured",
        UsageEvidence::Partial => "partial",
        UsageEvidence::Unknown => "unknown",
    }
}

fn receipt_kind(kind: ReceiptKind) -> &'static str {
    match kind {
        ReceiptKind::Task => "task",
        ReceiptKind::CancelledTurn => "cancelled_turn",
    }
}

fn receipt_outcome(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Pass => "pass",
        Outcome::Fail => "fail",
        Outcome::Partial => "partial",
        Outcome::Skip => "skip",
        Outcome::Timeout => "timeout",
    }
}

fn failure_class(class: FailureClass) -> &'static str {
    match class {
        FailureClass::Context => "context",
        FailureClass::Constraint => "constraint",
        FailureClass::Filtered => "filtered",
        FailureClass::Verification => "verification",
        FailureClass::Planning => "planning",
        FailureClass::Unreceipted => "unreceipted",
    }
}

fn byte_len(value: impl AsRef<[u8]>) -> u64 {
    u64::try_from(value.as_ref().len()).unwrap_or(u64::MAX)
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn timestamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn task_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:032x}-{:08x}-{sequence:016x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::receipt::{CompactionStats, RepairStats};
    use crate::wire::{effort_label, ChatMessage, ThinkingEffort};
    use nh_routes::RouteResolver;
    use nh_tools::{Access, Guard};
    use serde_json::{json, Value};
    use std::collections::VecDeque;

    fn route() -> ResolvedRoute {
        RouteResolver::from_toml(
            r#"
            [routes.test-route]
            provider = "test"
            model_id = "test-model"
            base_url = "https://example.invalid"
            wire = "openai"
            vault_entry = "test"
            [routes.test-route.price]
            currency = "USD"
            unit = "per_million_tokens"
            cache_hit = 0.1
            cache_miss = 0.5
            output = 1.5
            price_confidence = "confirmed"
            "#,
        )
        .unwrap()
        .resolve("test-route")
        .unwrap()
    }

    fn fixed_at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-23T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn request(secret: &str) -> ChatRequest {
        ChatRequest {
            model: "test-model".into(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content: Some("system rule".into()),
                    parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning_content: None,
                },
                ChatMessage {
                    role: "user".into(),
                    content: Some(format!("task {secret}")),
                    parts: None,
                    tool_calls: None,
                    tool_call_id: None,
                    reasoning_content: Some(format!("reasoning {secret}")),
                },
            ],
            tools: vec![ToolSpec {
                name: "read_file".into(),
                description: format!("description {secret}"),
                parameters: json!({"type": "object", "secret": secret}),
            }],
            thinking: ThinkingEffort::Low,
        }
    }

    fn response(usage: Option<Usage>) -> ChatResponse {
        ChatResponse {
            message: ChatMessage {
                role: "assistant".into(),
                content: Some("provider output must not be recorded".into()),
                parts: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: Some("provider reasoning must not be recorded".into()),
            },
            finish_reason: FinishReason::Stop,
            usage,
            retries: RetryStats::default(),
        }
    }

    fn fingerprint(request: &ChatRequest) -> String {
        serde_json::to_string(&json!({
            "model": request.model,
            "messages": request.messages,
            "tools": request.tools,
            "thinking": effort_label(request.thinking),
        }))
        .unwrap()
    }

    fn records(root: &std::path::Path) -> Vec<Value> {
        std::fs::read_to_string(root.join(EFFICIENCY_RELATIVE_PATH))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    struct FingerprintClient {
        seen: Arc<Mutex<Option<String>>>,
        response: ChatResponse,
    }

    impl ChatClient for FingerprintClient {
        fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
            *self.seen.lock().unwrap() = Some(fingerprint(request));
            Ok(self.response.clone())
        }
    }

    enum FakeResult {
        Response(Option<Usage>),
        Exhausted,
    }

    struct SequenceClient {
        results: Mutex<VecDeque<FakeResult>>,
    }

    impl ChatClient for SequenceClient {
        fn complete(&self, _request: &ChatRequest) -> anyhow::Result<ChatResponse> {
            match self.results.lock().unwrap().pop_front().unwrap() {
                FakeResult::Response(usage) => Ok(response(usage)),
                FakeResult::Exhausted => Err(anyhow::Error::new(RetryExhausted {
                    stats: RetryStats {
                        retries: 2,
                        rate_limited: 1,
                    },
                    usage: Some(Usage {
                        prompt_tokens: 12,
                        completion_tokens: 3,
                        cached_tokens: None,
                        evidence: UsageEvidence::Partial,
                    }),
                    last_failure: [
                        "provi", "der f", "ixtur", "e cre", "denti", "al sk", "-fixt", "ure-a",
                        "bc123",
                    ]
                    .concat(),
                    attempts: 3,
                    elapsed: std::time::Duration::from_millis(5),
                })),
            }
        }
    }

    struct AuditedTool;

    impl Tool for AuditedTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "tool-fixture-secret".into(),
                description: "fixture".into(),
                parameters: json!({"type": "object"}),
            }
        }

        fn execute(&self, _args: ToolArgs, _ctx: &ToolCtx) -> anyhow::Result<String> {
            Ok("raw tool output must not be recorded".into())
        }

        fn execute_with_audit(
            &self,
            _args: ToolArgs,
            _ctx: &ToolCtx,
        ) -> anyhow::Result<ToolExecution> {
            Ok(ToolExecution {
                output: "raw tool output must not be recorded".into(),
                audit: vec![ToolAudit::Command(CommandOutcome::Exited(Some(7)))],
            })
        }
    }

    fn ctx(root: &std::path::Path) -> ToolCtx {
        ToolCtx::new(
            root.to_path_buf(),
            Box::new(|_| false),
            Box::new(|access| match access {
                Access::Read(_) | Access::Write(_) | Access::Exec(_) | Access::Send(_) => {
                    Guard::Allow
                }
            }),
            nh_vault::Scrubber::new(Vec::new()),
        )
    }

    fn receipt(secret: &str) -> Receipt {
        let mut compaction = CompactionStats::default();
        compaction.record(2, 30, Some(4));
        Receipt {
            kind: ReceiptKind::Task,
            ts_utc: "2026-09-23T12:00:00Z".into(),
            model_id: "test-model".into(),
            task: format!("task text {secret}"),
            turns: 2,
            tool_calls: 1,
            duration_ms: Some(25),
            outcome: Outcome::Pass,
            failure_class: None,
            usage: Some(Usage {
                prompt_tokens: 20,
                completion_tokens: 5,
                cached_tokens: Some(7),
                evidence: UsageEvidence::Measured,
            }),
            cache_hit_pct: Some(35.0),
            repairs: RepairStats::default(),
            retries: RetryStats {
                retries: 1,
                rate_limited: 1,
            },
            compaction: Box::new(compaction),
            effective_profile: Some("balanced".into()),
        }
    }

    #[test]
    fn disabled_measurement_creates_no_runtime_directory() {
        let root = tempfile::tempdir().unwrap();
        let recorder = EfficiencyRecorder::project_if_enabled(
            false,
            root.path(),
            &route(),
            fixed_at(),
            nh_vault::Scrubber::new(Vec::new()),
        );

        assert!(recorder.is_none());
        assert!(!root.path().join(".nosis").exists());
    }

    #[test]
    fn task_start_identifies_a_zero_request_task() {
        let root = tempfile::tempdir().unwrap();
        let recorder = EfficiencyRecorder::project(
            root.path(),
            &route(),
            fixed_at(),
            nh_vault::Scrubber::new(Vec::new()),
        );

        let records = records(root.path());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["record_type"], "task_start");
        assert_eq!(records[0]["task_id"], recorder.task_id());
        assert_eq!(records[0]["route"]["route_id"], "test-route");
        assert!(records[0].get("task").is_none());
        assert!(records[0].get("messages").is_none());
    }

    #[test]
    fn request_wrapper_preserves_request_and_records_only_size_metadata() {
        const SECRET: &str = "fixture-literal-abc123";
        let root = tempfile::tempdir().unwrap();
        let recorder = EfficiencyRecorder::project(
            root.path(),
            &route(),
            fixed_at(),
            nh_vault::Scrubber::new(vec![SECRET.into()]),
        );
        let request = request(SECRET);
        let before = fingerprint(&request);
        let seen = Arc::new(Mutex::new(None));
        let client = recorder.wrap_client(Box::new(FingerprintClient {
            seen: Arc::clone(&seen),
            response: response(Some(Usage {
                prompt_tokens: 10,
                completion_tokens: 2,
                cached_tokens: Some(3),
                evidence: UsageEvidence::Partial,
            })),
        }));

        client.complete(&request).unwrap();

        assert_eq!(seen.lock().unwrap().as_deref(), Some(before.as_str()));
        let raw = std::fs::read_to_string(root.path().join(EFFICIENCY_RELATIVE_PATH)).unwrap();
        assert!(!raw.contains(SECRET));
        assert!(!raw.contains("provider output"));
        assert!(!raw.contains("provider reasoning"));
        let records = records(root.path());
        assert_eq!(records[0]["record_type"], "task_start");
        let record = &records[1];
        assert_eq!(record["schema_version"], EFFICIENCY_SCHEMA_VERSION);
        assert_eq!(record["record_type"], "request");
        assert_eq!(
            record["sizes"]["basis"],
            "utf8_bytes_before_provider_encoding"
        );
        assert!(record["sizes"]["user_bytes"].as_u64().unwrap() > 0);
        assert!(record["sizes"]["tool_schema_bytes"].as_u64().unwrap() > 0);
        assert_eq!(record["tool_schema"]["tool_count"], 1);
        assert_eq!(
            record["tool_schema"]["canonical_json_bytes"],
            serde_json::to_vec(&request.tools).unwrap().len()
        );
        assert!(record["tool_schema"]["identity"]
            .as_str()
            .unwrap()
            .starts_with("fnv1a64:"));
        assert!(record["tool_schema"]["same_as_previous_request"].is_null());
        assert_eq!(record["usage"]["evidence"], "partial");
        assert_eq!(record["usage"]["cached_tokens"], 3);
        assert_eq!(record["route"]["price"]["input_cache_miss"], 0.5);
    }

    #[test]
    fn schema_identity_tracks_ordered_tools_without_hashing_message_content() {
        let root = tempfile::tempdir().unwrap();
        let recorder = EfficiencyRecorder::project(
            root.path(),
            &route(),
            fixed_at(),
            nh_vault::Scrubber::new(Vec::new()),
        );
        let client = recorder.wrap_client(Box::new(SequenceClient {
            results: Mutex::new(VecDeque::from([
                FakeResult::Response(None),
                FakeResult::Response(None),
                FakeResult::Response(None),
            ])),
        }));
        let first = request("first private prompt");
        let mut second = request("different private prompt");
        second.messages.reverse();
        second.tools = first.tools.clone();
        let mut changed = second.clone();
        changed.tools[0].parameters["properties"] = json!({ "path": { "type": "string" } });

        client.complete(&first).unwrap();
        client.complete(&second).unwrap();
        client.complete(&changed).unwrap();

        let records = records(root.path());
        let first_schema = &records[1]["tool_schema"];
        let same_schema = &records[2]["tool_schema"];
        let changed_schema = &records[3]["tool_schema"];
        assert_eq!(first_schema["identity"], same_schema["identity"]);
        assert_eq!(same_schema["same_as_previous_request"], true);
        assert_ne!(same_schema["identity"], changed_schema["identity"]);
        assert_eq!(changed_schema["same_as_previous_request"], false);
        assert_eq!(
            first_schema["canonical_json_bytes"],
            serde_json::to_vec(&first.tools).unwrap().len()
        );
        let raw = std::fs::read_to_string(root.path().join(EFFICIENCY_RELATIVE_PATH)).unwrap();
        assert!(!raw.contains("first private prompt"));
        assert!(!raw.contains("different private prompt"));
    }

    #[test]
    fn request_sizes_count_parts_instead_of_inactive_content() {
        let mut request = request("ordinary fixture");
        request.messages[1].content = Some("inactive fallback must not be counted".into());
        request.messages[1].parts = Some(vec![
            ContentPart::Text {
                text: "active text".into(),
            },
            ContentPart::ImageB64 {
                media_type: "image/png".into(),
                data: "YWJjZA==".into(),
            },
        ]);

        let sizes = request_sizes(&request);

        assert_eq!(sizes.user_bytes, byte_len("active text"));
        assert_eq!(sizes.image_base64_bytes, byte_len("YWJjZA=="));
    }

    #[test]
    fn request_records_distinguish_absent_unknown_and_retry_failure_usage() {
        let root = tempfile::tempdir().unwrap();
        let recorder = EfficiencyRecorder::project(
            root.path(),
            &route(),
            fixed_at(),
            nh_vault::Scrubber::new(Vec::new()),
        );
        let client = recorder.wrap_client(Box::new(SequenceClient {
            results: Mutex::new(VecDeque::from([
                FakeResult::Response(None),
                FakeResult::Response(Some(Usage {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                    cached_tokens: None,
                    evidence: UsageEvidence::Unknown,
                })),
                FakeResult::Exhausted,
            ])),
        }));
        let request = request("ordinary fixture");

        client.complete(&request).unwrap();
        client.complete(&request).unwrap();
        assert!(client.complete(&request).is_err());

        let records = records(root.path());
        assert_eq!(records.len(), 4);
        assert_eq!(records[0]["record_type"], "task_start");
        assert_eq!(records[1]["usage"]["usage_object_reported"], false);
        assert_eq!(records[1]["usage"]["prompt_tokens"], Value::Null);
        assert_eq!(records[2]["usage"]["usage_object_reported"], true);
        assert_eq!(records[2]["usage"]["evidence"], "unknown");
        assert_eq!(records[2]["usage"]["prompt_tokens"], Value::Null);
        assert_eq!(records[3]["result"], "error");
        assert_eq!(records[3]["usage"]["evidence"], "partial");
        assert_eq!(records[3]["retries"]["attempts"], 3);
        assert_eq!(records[3]["retries"]["retries"], 2);
        let raw = std::fs::read_to_string(root.path().join(EFFICIENCY_RELATIVE_PATH)).unwrap();
        assert!(!raw.contains("sk-fixture"));
        assert!(!raw.contains("last_failure"));
    }

    #[test]
    fn tool_and_task_records_keep_observed_outcomes_separate_from_correctness() {
        const SECRET: &str = "tool-fixture-secret";
        let root = tempfile::tempdir().unwrap();
        let recorder = EfficiencyRecorder::project(
            root.path(),
            &route(),
            fixed_at(),
            nh_vault::Scrubber::new(vec![SECRET.into()]),
        );
        let mut tools = recorder.wrap_tools(vec![Box::new(AuditedTool)]);
        let tool = tools.pop().unwrap();

        tool.execute_with_audit(json!({"path": "private.txt"}), &ctx(root.path()))
            .unwrap();
        recorder.record_task(&receipt(SECRET));

        let raw = std::fs::read_to_string(root.path().join(EFFICIENCY_RELATIVE_PATH)).unwrap();
        assert!(!raw.contains(SECRET));
        assert!(!raw.contains("private.txt"));
        assert!(!raw.contains("raw tool output"));
        let records = records(root.path());
        assert_eq!(records[0]["record_type"], "task_start");
        assert_eq!(records[1]["record_type"], "tool");
        assert_eq!(records[1]["outcome"], "command_exit_nonzero");
        assert_eq!(records[1]["tool_name"], "[REDACTED]");
        assert_eq!(records[2]["record_type"], "task");
        assert_eq!(records[2]["receipt_outcome"], "pass");
        assert_eq!(records[2]["correctness_assessed"], false);
        assert_eq!(records[2]["retries"]["attempts"], 3);
        assert_eq!(records[2]["compaction"]["events"], 1);
        assert_eq!(records[2]["recorder"]["records_dropped_before_summary"], 0);
        assert_eq!(
            records[2]["recorder"]["task_duration_basis"],
            "receipt wall time includes local efficiency recorder append, flush, and sync overhead"
        );
    }

    #[test]
    fn bounded_log_failure_is_reported_without_returning_an_error() {
        let root = tempfile::tempdir().unwrap();
        let recorder = EfficiencyRecorder::project_with_limit(
            root.path(),
            &route(),
            fixed_at(),
            nh_vault::Scrubber::new(Vec::new()),
            1,
        );

        recorder.record_task(&receipt("not persisted"));

        assert!(recorder.warning().unwrap().contains("1-byte growth limit"));
    }

    #[test]
    fn busy_measurement_log_retries_later_records_and_reports_the_drop() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(EFFICIENCY_RELATIVE_PATH);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .unwrap();
        held.lock().unwrap();
        let recorder = EfficiencyRecorder::project(
            root.path(),
            &route(),
            fixed_at(),
            nh_vault::Scrubber::new(Vec::new()),
        );
        assert!(recorder.warning().unwrap().contains("without waiting"));
        drop(held);

        recorder.record_task(&receipt("later summary"));

        let records = records(root.path());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["record_type"], "task");
        assert_eq!(records[0]["task_id"], recorder.task_id());
        assert_eq!(records[0]["recorder"]["records_dropped_before_summary"], 1);
        assert!(recorder.warning().unwrap().contains("without waiting"));
    }
}
