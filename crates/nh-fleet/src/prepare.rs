//! Task preparation, credential preflight, test-provider wiring, and display helpers.

use crate::engine::{PreparedTask, TestProvider};
use crate::model::{validate_task_specs, Backend, EventCallback, Ladder, TaskSpec};
use crate::{DEFAULT_MAX_WORKERS, TEST_LOG_LOCK, TEST_PROVIDER_ENV};
#[cfg(any(test, debug_assertions))]
use crate::{TEST_EXECUTION_LOG_ENV, TEST_OUTCOME_ENV, TEST_SLEEP_MS_ENV};
use anyhow::bail;
#[cfg(any(test, debug_assertions))]
use anyhow::Context as _;
use nh_core::credential;
#[cfg(any(test, debug_assertions))]
use nh_core::receipt::Outcome;
use nh_core::wire::{ChatClient, ChatMessage, ChatRequest, ChatResponse, ThinkingEffort, Usage};
use nh_law::Law;
use nh_routes::{RouteClass, RouteResolver, ThinkingDialect};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, SecretRegistry};
use std::collections::{BTreeSet, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::Path;
#[cfg(any(test, debug_assertions))]
use std::path::PathBuf;
use std::thread;
#[cfg(any(test, debug_assertions))]
use std::time::Duration;

pub(super) fn prepare_new_tasks(
    resolver: &RouteResolver,
    default_route: &str,
    specs: &[TaskSpec],
    defer_offpeak: bool,
    ladder: Option<&Ladder>,
) -> anyhow::Result<Vec<PreparedTask>> {
    validate_task_specs(specs)?;
    if let Some(ladder) = ladder {
        if specs.iter().any(|spec| spec.model.is_some()) {
            bail!(
                "escalation ladder owns route selection — remove per-task model, or drop --escalate"
            );
        }
        if ladder.tiers().is_empty() {
            bail!("escalation ladder has no worker tiers");
        }
        for tier in ladder.tiers() {
            let route = resolver.resolve(&tier.route_id)?;
            if route.class() != RouteClass::Api {
                bail!("escalation ladder worker tiers must use priced api routes");
            }
        }
    }
    let mut ids = HashSet::new();
    let mut tasks = Vec::with_capacity(specs.len());
    for (index, spec) in specs.iter().enumerate() {
        let task = spec.task.trim();
        let task_id = match spec.id.as_deref() {
            Some(id) => id.to_string(),
            None => format!("t{index:03}-{:08x}", stable_hash(task) as u32),
        };
        if !ids.insert(task_id.clone()) {
            bail!("task id collision — choose unique ids");
        }
        let (route_id, effort) = match ladder {
            Some(ladder) => {
                let tier = &ladder.tiers()[0];
                (tier.route_id.as_str(), Some(tier.effort))
            }
            None => (spec.model.as_deref().unwrap_or(default_route), None),
        };
        let route = resolver.resolve(route_id)?;
        if route.class() == RouteClass::Delegate {
            bail!(
                "delegate routes are not available to fleet workers — pick an api or local route"
            );
        }
        tasks.push(PreparedTask {
            task_id,
            task: task.to_string(),
            route_id: route.id().to_owned(),
            attempt: 1,
            tier_idx: 0,
            effort,
            defer_offpeak: spec.defer_offpeak.unwrap_or(defer_offpeak),
            backend: spec.backend.unwrap_or(Backend::Native),
        });
    }
    Ok(tasks)
}

pub(super) fn scrub_prepared_tasks(
    tasks: &mut [PreparedTask],
    scrubber: &Scrubber,
) -> anyhow::Result<()> {
    let mut ids = HashSet::new();
    for task in tasks {
        task.task_id = scrubber.scrub(&task.task_id);
        task.task = scrubber.scrub(&task.task);
        task.route_id = scrubber.scrub(&task.route_id);
        if !ids.insert(task.task_id.clone()) {
            bail!("task ids collide after secret redaction — choose different ids");
        }
    }
    Ok(())
}

pub(super) fn preflight_keys(
    resolver: &RouteResolver,
    tasks: &[PreparedTask],
    ladder: Option<&Ladder>,
    law: &Law,
    using_test_provider: bool,
) -> anyhow::Result<SecretRegistry> {
    if using_test_provider {
        return Ok(SecretRegistry::new());
    }
    let vault = EnvFallbackVault {
        inner: KeyringVault,
    };
    let mut route_ids = BTreeSet::new();
    let has_native = tasks.iter().any(|task| task.backend == Backend::Native);
    for task in tasks {
        if task.backend == Backend::Native {
            route_ids.insert(task.route_id.clone());
        }
    }
    if has_native {
        if let Some(ladder) = ladder {
            route_ids.extend(ladder.tiers().iter().map(|tier| tier.route_id.clone()));
        }
    }
    let mut literals = SecretRegistry::new();
    for route_id in route_ids {
        let route = resolver.resolve(&route_id)?;
        let (_, literal) = credential::connect(
            &vault,
            &route,
            &law.policy.approved_audiences(route.vault_entry()),
            None,
        )?;
        literals.insert(literal);
    }
    Ok(literals)
}

/// TEST-ONLY provider seam used by the kill/resume process test. It is inert
/// unless the exact `NH_FLEET_TEST_PROVIDER=echo` opt-in is present; ordinary
/// runs always take the vault-backed credential boundary.
#[cfg(any(test, debug_assertions))]
pub(super) fn test_provider_from_env() -> anyhow::Result<Option<TestProvider>> {
    match std::env::var(TEST_PROVIDER_ENV) {
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(anyhow::anyhow!(
            "could not read {TEST_PROVIDER_ENV}: {error}"
        )),
        Ok(value) if value == "echo" => {
            let sleep_ms = std::env::var(TEST_SLEEP_MS_ENV)
                .ok()
                .map(|value| value.parse::<u64>())
                .transpose()
                .context("NH_FLEET_TEST_SLEEP_MS must be a whole number")?
                .unwrap_or(150);
            let outcome = match std::env::var(TEST_OUTCOME_ENV) {
                Err(std::env::VarError::NotPresent) => Outcome::Pass,
                Ok(value) if value == "pass" => Outcome::Pass,
                Ok(value) if value == "fail" => Outcome::Fail,
                Ok(value) if value == "partial" => Outcome::Partial,
                Ok(value) if value == "skip" => Outcome::Skip,
                Ok(value) if value == "timeout" => Outcome::Timeout,
                Err(error) => {
                    return Err(anyhow::anyhow!(
                        "could not read {TEST_OUTCOME_ENV}: {error}"
                    ))
                }
                Ok(_) => {
                    bail!("NH_FLEET_TEST_OUTCOME accepts pass, fail, partial, skip, or timeout")
                }
            };
            Ok(Some(TestProvider {
                execution_log: std::env::var_os(TEST_EXECUTION_LOG_ENV).map(PathBuf::from),
                sleep: Duration::from_millis(sleep_ms),
                outcome,
            }))
        }
        Ok(_) => bail!("NH_FLEET_TEST_PROVIDER only accepts the test value 'echo'"),
    }
}

#[cfg(not(any(test, debug_assertions)))]
pub(super) fn test_provider_from_env() -> anyhow::Result<Option<TestProvider>> {
    if std::env::var_os(TEST_PROVIDER_ENV).is_some() {
        bail!("{TEST_PROVIDER_ENV} is unavailable in release builds");
    }
    Ok(None)
}

pub(super) struct EchoClient {
    pub(super) task_id: String,
    pub(super) config: TestProvider,
}

impl ChatClient for EchoClient {
    fn complete(&self, _req: &ChatRequest) -> anyhow::Result<ChatResponse> {
        thread::sleep(self.config.sleep);
        if let Some(path) = &self.config.execution_log {
            append_execution_log(path, &self.task_id)?;
        }
        Ok(ChatResponse {
            message: ChatMessage {
                role: "assistant".into(),
                content: Some(format!("echo completed {}", self.task_id)),
                parts: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
            finish_reason: "stop".into(),
            usage: Some(Usage {
                prompt_tokens: 1,
                completion_tokens: 1,
                cached_tokens: None,
            }),
        })
    }
}

pub(super) fn append_execution_log(path: &Path, task_id: &str) -> anyhow::Result<()> {
    let _guard = TEST_LOG_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("test execution log lock was poisoned"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{task_id}")?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

pub(super) fn stable_hash(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub(super) fn effort_for(dialect: ThinkingDialect) -> ThinkingEffort {
    match dialect {
        ThinkingDialect::AlwaysThinking
        | ThinkingDialect::AlwaysThinkingEffort
        | ThinkingDialect::GlmHm => ThinkingEffort::High,
        ThinkingDialect::DeepseekNhm | ThinkingDialect::KimiToggle | ThinkingDialect::None => {
            ThinkingEffort::None
        }
    }
}

pub(super) fn effort_name(effort: ThinkingEffort) -> &'static str {
    match effort {
        ThinkingEffort::None => "none",
        ThinkingEffort::Low => "low",
        ThinkingEffort::High => "high",
        ThinkingEffort::Max => "max",
    }
}

pub(super) fn effective_workers(
    requested: usize,
    original: Option<usize>,
) -> anyhow::Result<usize> {
    let workers = if requested == 0 {
        original.unwrap_or(DEFAULT_MAX_WORKERS)
    } else {
        requested
    };
    if workers == 0 {
        bail!("max_workers must be at least 1");
    }
    Ok(workers)
}

pub(super) fn emit(callback: &Option<EventCallback>, scrubber: &Scrubber, line: &str) {
    if let Some(callback) = callback {
        callback(&nh_vault::safe_line(scrubber, line));
    }
}
