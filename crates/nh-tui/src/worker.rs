use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, RecvTimeoutError, Sender},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use chrono::Utc;
use nh_core::agent::AgentLoop;
use nh_core::receipt::{FailureClass, Outcome, Receipt, ReceiptWriter};
use nh_core::wire::{ChatClient, ChatMessage, ChatRequest, ChatResponse, ThinkingEffort, Usage};
use nh_law::{Law, Verdict};
use nh_routes::{Profiles, ResolvedRoute};
use nh_tools::{builtin_tools, Access, Guard, ToolCtx};
#[cfg(test)]
use nh_vault::Scrubber;
use nh_vault::{SecretRegistry, SecretValue};

use super::{
    effort_for, identity_constitution, install_literal, safe_line, scrub_full_line, AgentEvent,
    ConnectFn, SharedScrubber, TimelineSummary,
};

const APPROVAL_WAIT_POLL: Duration = Duration::from_millis(10);
const JOIN_POLL: Duration = Duration::from_millis(2);
pub(super) const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(250);

fn apply_new_credential(
    shared: &SharedScrubber,
    key_literals: &mut SecretRegistry,
    literal: SecretValue,
    agent: &mut AgentLoop,
) {
    // One place updates every scrubber snapshot so they can never diverge (audit H-03):
    // shared/UI egress, the tool boundary, and receipts all see the new credential.
    install_literal(shared, key_literals, literal);
    agent.ctx.scrubber = key_literals.scrubber();
    agent.receipts.replace_scrubber(key_literals.scrubber());
}

/// One approval decision waiting for the main-thread UI.
pub struct ApprovalRequest {
    pub prompt: String,
    pub reply: Sender<bool>,
}

pub(super) enum WorkerCommand {
    Task(String),
    SwitchRoute(Box<ResolvedRoute>),
    SetEffort(ThinkingEffort),
    SetProfile(String),
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkerShutdown {
    Clean,
    Panicked,
    Detached,
}

pub(super) struct Worker {
    pub(super) commands: Sender<WorkerCommand>,
    pub(super) events: Receiver<AgentEvent>,
    join: Option<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

impl Worker {
    fn close_events(&mut self) {
        let (_closed_tx, closed_rx) = mpsc::channel();
        drop(std::mem::replace(&mut self.events, closed_rx));
    }

    pub(super) fn shutdown(&mut self) -> WorkerShutdown {
        self.shutdown_with_timeout(SHUTDOWN_TIMEOUT)
    }

    fn shutdown_with_timeout(&mut self, timeout: Duration) -> WorkerShutdown {
        if self.join.is_none() {
            return WorkerShutdown::Clean;
        }
        let deadline = Instant::now() + timeout;

        // Closing the event receiver drops every queued ApprovalRequest sender.
        // The App drops its currently displayed sender before calling this method.
        self.close_events();
        self.shutdown.store(true, Ordering::Release);
        let _ = self.commands.send(WorkerCommand::Stop);

        loop {
            if self.join.as_ref().is_some_and(JoinHandle::is_finished) {
                let join = self.join.take().expect("join handle checked above");
                return if join.join().is_ok() {
                    WorkerShutdown::Clean
                } else {
                    WorkerShutdown::Panicked
                };
            }
            let now = Instant::now();
            if now >= deadline {
                drop(self.join.take());
                return WorkerShutdown::Detached;
            }
            thread::sleep(JOIN_POLL.min(deadline.saturating_duration_since(now)));
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        match self.shutdown() {
            WorkerShutdown::Clean => {}
            WorkerShutdown::Panicked => {
                eprintln!("nh: agent worker panicked during shutdown");
            }
            WorkerShutdown::Detached => {
                eprintln!(
                    "nh: agent worker did not stop within {} ms; detached",
                    SHUTDOWN_TIMEOUT.as_millis()
                );
            }
        }
    }
}

pub(super) struct WorkerConfig {
    pub(super) route: ResolvedRoute,
    pub(super) profiles: Profiles,
    pub(super) active_profile: String,
    pub(super) law: Law,
    pub(super) repo_root: PathBuf,
    pub(super) workdir: PathBuf,
    pub(super) scrubber: SharedScrubber,
    pub(super) connect: ConnectFn,
    pub(super) initial: Option<anyhow::Result<(Box<dyn ChatClient>, SecretValue)>>,
}

pub(super) fn spawn_worker(config: WorkerConfig) -> anyhow::Result<Worker> {
    let (command_tx, command_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let shutdown = Arc::new(AtomicBool::new(false));
    let worker_shutdown = Arc::clone(&shutdown);
    let join = thread::Builder::new()
        .name("nh-agent".into())
        .spawn(move || worker_loop(config, command_rx, event_tx, worker_shutdown))
        .context("could not start the agent worker")?;
    Ok(Worker {
        commands: command_tx,
        events: event_rx,
        join: Some(join),
        shutdown,
    })
}

fn worker_loop(
    config: WorkerConfig,
    commands: Receiver<WorkerCommand>,
    events: Sender<AgentEvent>,
    shutdown: Arc<AtomicBool>,
) {
    let WorkerConfig {
        mut route,
        profiles,
        mut active_profile,
        law,
        repo_root,
        workdir,
        scrubber,
        connect,
        initial,
    } = config;
    let initial_policy = profiles.effective(&active_profile, &route);
    active_profile = initial_policy.profile.clone();
    let connection = match initial {
        Some(connection) => connection,
        None => connect(&route, initial_policy.output_cap),
    };
    let (client, mut key_literals, mut connected) = match connection {
        Ok((client, literal)) => {
            let mut literals = SecretRegistry::new();
            install_literal(&scrubber, &mut literals, literal);
            (shutdown_aware(client, &shutdown), literals, true)
        }
        Err(error) => (
            Box::new(NotConnected {
                message: error.to_string(),
            }) as Box<dyn ChatClient>,
            SecretRegistry::new(),
            false,
        ),
    };

    let approval_events = events.clone();
    let approval_scrubber = Arc::clone(&scrubber);
    let approval_shutdown = Arc::clone(&shutdown);
    let approve = Box::new(move |prompt: &str| {
        if approval_shutdown.load(Ordering::Acquire) {
            return false;
        }
        let (reply, answers) = mpsc::channel();
        let request = ApprovalRequest {
            prompt: scrub_full_line(&approval_scrubber, prompt),
            reply,
        };
        if approval_events.send(AgentEvent::Approval(request)).is_err() {
            return false;
        }
        wait_for_approval(&answers, &approval_shutdown)
    });

    let policy = law.policy.clone();
    let event_scrubber = Arc::clone(&scrubber);
    let progress_events = events.clone();
    let ctx = ToolCtx::new(workdir, approve)
        .with_scrubber(key_literals.scrubber())
        .with_guard(Box::new(move |access| match access {
            Access::Read(path) => verdict_to_guard(policy.read_verdict(path)),
            Access::Write(path) => verdict_to_guard(policy.write_verdict(path)),
            Access::Exec(command) => verdict_to_guard(policy.exec_verdict(command)),
            Access::Send(target) => verdict_to_guard(policy.send_verdict(target)),
        }));
    let law_constitution = law.constitution;
    let mut agent = AgentLoop {
        client,
        tools: builtin_tools(),
        ctx,
        receipts: ReceiptWriter::project(repo_root.clone(), key_literals.scrubber()),
        model_id: route.model_id().to_owned(),
        max_turns: 20,
        thinking: effort_for(
            initial_policy.posture,
            route.thinking_dialect(),
            route.wire(),
        ),
        profile: Some(active_profile.clone()),
        constitution: Some(identity_constitution(&law_constitution, &route)),
        context_limit: route.context(),
        on_event: Some(Box::new(move |line| {
            let _ = progress_events.send(AgentEvent::Progress(safe_line(&event_scrubber, line)));
        })),
    };

    let mut history: Vec<ChatMessage> = Vec::new();
    let mut session_usage = Usage::default();
    loop {
        if shutdown.load(Ordering::Acquire) {
            break;
        }
        let Ok(command) = commands.recv() else {
            break;
        };
        if shutdown.load(Ordering::Acquire) {
            break;
        }
        let task = match command {
            WorkerCommand::Task(task) => task,
            WorkerCommand::SwitchRoute(next_route) => {
                let execution_policy = profiles.effective(&active_profile, &next_route);
                let connection = connect(&next_route, execution_policy.output_cap);
                match connection {
                    Ok((client, literal)) => {
                        apply_new_credential(&scrubber, &mut key_literals, literal, &mut agent);
                        agent.client = shutdown_aware(client, &shutdown);
                        connected = true;
                    }
                    Err(error) => {
                        agent.client = Box::new(NotConnected {
                            message: error.to_string(),
                        });
                        connected = false;
                    }
                }
                agent.model_id = next_route.model_id().to_owned();
                agent.thinking = effort_for(
                    execution_policy.posture,
                    next_route.thinking_dialect(),
                    next_route.wire(),
                );
                agent.profile = Some(execution_policy.profile.clone());
                active_profile = execution_policy.profile;
                let constitution = identity_constitution(&law_constitution, &next_route);
                agent.constitution = Some(constitution.clone());
                replace_system_message(&mut history, constitution);
                agent.context_limit = next_route.context();
                route = *next_route;
                continue;
            }
            WorkerCommand::SetEffort(effort) => {
                agent.thinking = effort;
                continue;
            }
            WorkerCommand::SetProfile(name) => {
                let execution_policy = profiles.effective(&name, &route);
                let connection = connect(&route, execution_policy.output_cap);
                match connection {
                    Ok((client, literal)) => {
                        apply_new_credential(&scrubber, &mut key_literals, literal, &mut agent);
                        agent.client = shutdown_aware(client, &shutdown);
                        connected = true;
                    }
                    Err(error) => {
                        agent.client = Box::new(NotConnected {
                            message: error.to_string(),
                        });
                        connected = false;
                    }
                }
                agent.thinking = effort_for(
                    execution_policy.posture,
                    route.thinking_dialect(),
                    route.wire(),
                );
                agent.profile = Some(execution_policy.profile.clone());
                active_profile = execution_policy.profile;
                continue;
            }
            WorkerCommand::Stop => break,
        };
        if !connected {
            let execution_policy = profiles.effective(&active_profile, &route);
            match connect(&route, execution_policy.output_cap) {
                Ok((client, literal)) => {
                    apply_new_credential(&scrubber, &mut key_literals, literal, &mut agent);
                    agent.client = shutdown_aware(client, &shutdown);
                    connected = true;
                }
                Err(error) => {
                    if shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    let reason = safe_line(&scrubber, &error.to_string());
                    let _ = events.send(AgentEvent::Failed(reason.clone()));
                    let _ = events.send(AgentEvent::TaskReceipt(failed_timeline_summary(
                        route.model_id(),
                        &task,
                        &reason,
                    )));
                    continue;
                }
            }
        }
        match agent.run_with_history(&mut history, &task) {
            Ok((answer, receipt)) => {
                if shutdown.load(Ordering::Acquire) {
                    break;
                }
                if let Some(usage) = &receipt.usage {
                    if add_usage(&mut session_usage, usage) {
                        let _ = events.send(AgentEvent::MeterIncomplete);
                    } else {
                        let _ = events.send(AgentEvent::Usage(session_usage.clone()));
                    }
                }
                let _ = events.send(AgentEvent::Answer(answer.clone()));
                let _ = events.send(AgentEvent::TaskReceipt(TimelineSummary { receipt, answer }));
            }
            Err(error) => {
                if shutdown.load(Ordering::Acquire) {
                    break;
                }
                let reason = safe_line(&scrubber, &error.to_string());
                let _ = events.send(AgentEvent::MeterIncomplete);
                let _ = events.send(AgentEvent::Failed(reason.clone()));
                let _ = events.send(AgentEvent::TaskReceipt(failed_timeline_summary(
                    route.model_id(),
                    &task,
                    &reason,
                )));
            }
        }
    }
}

fn wait_for_approval(answers: &Receiver<bool>, shutdown: &AtomicBool) -> bool {
    loop {
        if shutdown.load(Ordering::Acquire) {
            return false;
        }
        match answers.recv_timeout(APPROVAL_WAIT_POLL) {
            Ok(approved) => return approved && !shutdown.load(Ordering::Acquire),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
}

struct ShutdownAwareClient {
    inner: Box<dyn ChatClient>,
    shutdown: Arc<AtomicBool>,
}

impl ChatClient for ShutdownAwareClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        if self.shutdown.load(Ordering::Acquire) {
            anyhow::bail!("agent worker stopped");
        }
        self.inner.complete(request)
    }
}

fn shutdown_aware(client: Box<dyn ChatClient>, shutdown: &Arc<AtomicBool>) -> Box<dyn ChatClient> {
    Box::new(ShutdownAwareClient {
        inner: client,
        shutdown: Arc::clone(shutdown),
    })
}

fn replace_system_message(history: &mut [ChatMessage], constitution: String) {
    if let Some(system) = history
        .first_mut()
        .filter(|message| message.role == "system")
    {
        system.content = Some(constitution);
        system.tool_calls = None;
        system.tool_call_id = None;
        system.reasoning_content = None;
    }
}

struct NotConnected {
    message: String,
}

impl ChatClient for NotConnected {
    fn complete(&self, _request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        anyhow::bail!("{}", self.message)
    }
}

fn add_usage(total: &mut Usage, usage: &Usage) -> bool {
    let Some(prompt_tokens) = total.prompt_tokens.checked_add(usage.prompt_tokens) else {
        return true;
    };
    let Some(completion_tokens) = total.completion_tokens.checked_add(usage.completion_tokens)
    else {
        return true;
    };
    let cached_tokens = match usage.cached_tokens {
        Some(cached) => {
            let Some(total_cached) = total.cached_tokens.unwrap_or(0).checked_add(cached) else {
                return true;
            };
            Some(total_cached)
        }
        None => total.cached_tokens,
    };

    total.prompt_tokens = prompt_tokens;
    total.completion_tokens = completion_tokens;
    total.cached_tokens = cached_tokens;
    false
}

fn failed_timeline_summary(model_id: &str, task: &str, reason: &str) -> TimelineSummary {
    TimelineSummary {
        receipt: Receipt {
            ts_utc: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            model_id: model_id.to_owned(),
            task: task.to_owned(),
            turns: 0,
            tool_calls: 0,
            outcome: Outcome::Fail,
            failure_class: Some(FailureClass::Verification),
            usage: None,
            effective_profile: None,
        },
        answer: format!("error: {reason}"),
    }
}

fn verdict_to_guard(verdict: Verdict) -> Guard {
    match verdict {
        Verdict::Allow => Guard::Allow,
        Verdict::Ask => Guard::Ask,
        Verdict::Block(reason) => Guard::Block(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker_around(join: JoinHandle<()>, shutdown: Arc<AtomicBool>) -> Worker {
        let (commands, _command_rx) = mpsc::channel();
        let (_event_tx, events) = mpsc::channel();
        Worker {
            commands,
            events,
            join: Some(join),
            shutdown,
        }
    }

    #[test]
    fn apply_new_credential_refreshes_every_scrubber() {
        struct PanicClient;

        impl ChatClient for PanicClient {
            fn complete(&self, _request: &ChatRequest) -> anyhow::Result<ChatResponse> {
                panic!("credential test must not call the provider")
            }
        }

        let shared = Arc::new(std::sync::RwLock::new(Scrubber::new(Vec::new())));
        let mut key_literals = SecretRegistry::new();
        let mut agent = AgentLoop {
            client: Box::new(PanicClient),
            tools: Vec::new(),
            ctx: ToolCtx::new(PathBuf::from("."), Box::new(|_| false)),
            receipts: ReceiptWriter::for_path(
                PathBuf::from("."),
                PathBuf::from("unused-receipts.jsonl"),
                Scrubber::new(Vec::new()),
            ),
            model_id: "test".into(),
            max_turns: 1,
            thinking: ThinkingEffort::None,
            profile: None,
            constitution: None,
            context_limit: None,
            on_event: None,
        };
        let literal = "fake-route-credential";

        apply_new_credential(
            &shared,
            &mut key_literals,
            nh_vault::secret(literal),
            &mut agent,
        );

        assert_eq!(key_literals.len(), 1);
        assert!(key_literals.contains(literal));
        assert_eq!(agent.ctx.scrubber.scrub(literal), "[REDACTED]");
        assert_eq!(agent.receipts.scrubber().scrub(literal), "[REDACTED]");
        let shared_output = match shared.read() {
            Ok(guard) => guard.scrub(literal),
            Err(poisoned) => poisoned.into_inner().scrub(literal),
        };
        assert_eq!(shared_output, "[REDACTED]");
    }

    #[test]
    fn worker_drop_unblocks_a_parked_approval() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = Arc::clone(&shutdown);
        let (reply, answers) = mpsc::channel();
        let (parked_tx, parked_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            parked_tx.send(()).unwrap();
            assert!(!wait_for_approval(&answers, &worker_shutdown));
        });
        parked_rx.recv().unwrap();

        let (commands, _command_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        event_tx
            .send(AgentEvent::Approval(ApprovalRequest {
                prompt: "parked".into(),
                reply,
            }))
            .unwrap();
        drop(event_tx);
        let worker = Worker {
            commands,
            events,
            join: Some(join),
            shutdown,
        };

        let started = Instant::now();
        drop(worker);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "approval shutdown exceeded its bound: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn worker_drop_detaches_an_uninterruptible_operation_at_deadline() {
        let shutdown = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let worker_release = Arc::clone(&release);
        let (finished_tx, finished_rx) = mpsc::channel();
        let join = thread::spawn(move || {
            while !worker_release.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(1));
            }
            finished_tx.send(()).unwrap();
        });
        let worker = worker_around(join, shutdown);

        let started = Instant::now();
        drop(worker);
        let elapsed = started.elapsed();
        assert!(
            elapsed >= SHUTDOWN_TIMEOUT.saturating_sub(Duration::from_millis(10)),
            "detached before its deadline"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "uninterruptible operation exceeded its bound: {elapsed:?}"
        );

        release.store(true, Ordering::Release);
        finished_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn shutdown_aware_client_refuses_a_new_provider_call() {
        struct PanicClient;

        impl ChatClient for PanicClient {
            fn complete(&self, _request: &ChatRequest) -> anyhow::Result<ChatResponse> {
                panic!("provider must not be called after shutdown")
            }
        }

        let shutdown = Arc::new(AtomicBool::new(true));
        let client = shutdown_aware(Box::new(PanicClient), &shutdown);
        let request = ChatRequest {
            model: "test".into(),
            messages: Vec::new(),
            tools: Vec::new(),
            thinking: ThinkingEffort::None,
        };

        assert_eq!(
            client.complete(&request).unwrap_err().to_string(),
            "agent worker stopped"
        );
    }

    #[test]
    fn add_usage_reports_overflow_without_partially_mutating_totals() {
        let mut total = Usage {
            prompt_tokens: u64::MAX,
            completion_tokens: 7,
            cached_tokens: Some(3),
        };
        let usage = Usage {
            prompt_tokens: 1,
            completion_tokens: 2,
            cached_tokens: Some(1),
        };

        assert!(add_usage(&mut total, &usage));
        assert_eq!(total.prompt_tokens, u64::MAX);
        assert_eq!(total.completion_tokens, 7);
        assert_eq!(total.cached_tokens, Some(3));
    }
}
