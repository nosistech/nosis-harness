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
use nh_core::receipt::{FailureClass, Outcome, Receipt, ReceiptWriter, RepairStats};
use nh_core::wire::{ChatClient, ChatMessage, ChatRequest, ChatResponse, ThinkingEffort, Usage};
use nh_law::{Law, Verdict};
use nh_routes::{Profiles, ResolvedRoute};
use nh_tools::{builtin_tools, Access, Guard, Tool, ToolArgs, ToolCtx, ToolExecution, ToolSpec};
#[cfg(test)]
use nh_vault::Scrubber;
use nh_vault::{SecretRegistry, SecretValue};

use crate::session::{
    effort_for, identity_constitution, install_literal, safe_line, scrub_full_line,
};
use crate::{AgentEvent, ConnectFn, SharedScrubber, TimelineSummary};

const APPROVAL_WAIT_POLL: Duration = Duration::from_millis(10);
const JOIN_POLL: Duration = Duration::from_millis(2);
pub(super) const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(250);

struct TrackedTool {
    inner: Box<dyn Tool>,
    events: Sender<AgentEvent>,
}

impl Tool for TrackedTool {
    fn spec(&self) -> ToolSpec {
        self.inner.spec()
    }

    fn execute(&self, args: ToolArgs, ctx: &ToolCtx) -> anyhow::Result<String> {
        let name = self.inner.spec().name;
        let _ = self.events.send(AgentEvent::ToolStarted {
            name: name.clone(),
            started_at: Utc::now(),
        });
        let _finished = ToolFinishedGuard {
            events: &self.events,
            name,
        };
        self.inner.execute(args, ctx)
    }

    fn execute_with_audit(&self, args: ToolArgs, ctx: &ToolCtx) -> anyhow::Result<ToolExecution> {
        let name = self.inner.spec().name;
        let _ = self.events.send(AgentEvent::ToolStarted {
            name: name.clone(),
            started_at: Utc::now(),
        });
        let _finished = ToolFinishedGuard {
            events: &self.events,
            name,
        };
        self.inner.execute_with_audit(args, ctx)
    }
}

struct ToolFinishedGuard<'a> {
    events: &'a Sender<AgentEvent>,
    name: String,
}

impl Drop for ToolFinishedGuard<'_> {
    fn drop(&mut self) {
        let _ = self.events.send(AgentEvent::ToolFinished {
            name: self.name.clone(),
        });
    }
}

fn tracked_tools(tools: Vec<Box<dyn Tool>>, events: &Sender<AgentEvent>) -> Vec<Box<dyn Tool>> {
    tools
        .into_iter()
        .map(|inner| {
            Box::new(TrackedTool {
                inner,
                events: events.clone(),
            }) as Box<dyn Tool>
        })
        .collect()
}

struct TrackedClient {
    inner: Box<dyn ChatClient>,
    route: String,
    events: Sender<AgentEvent>,
}

impl ChatClient for TrackedClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let _ = self.events.send(AgentEvent::ModelStarted {
            route: self.route.clone(),
            started_at: Utc::now(),
        });
        let _finished = ModelFinishedGuard {
            events: &self.events,
            route: &self.route,
        };
        self.inner.complete(request)
    }
}

struct ModelFinishedGuard<'a> {
    events: &'a Sender<AgentEvent>,
    route: &'a str,
}

impl Drop for ModelFinishedGuard<'_> {
    fn drop(&mut self) {
        let _ = self.events.send(AgentEvent::ModelFinished {
            route: self.route.to_owned(),
        });
    }
}

fn tracked_client(
    client: Box<dyn ChatClient>,
    route: &str,
    events: &Sender<AgentEvent>,
) -> Box<dyn ChatClient> {
    Box::new(TrackedClient {
        inner: client,
        route: route.to_owned(),
        events: events.clone(),
    })
}

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
                if let Some(join) = self.join.take() {
                    return if join.join().is_ok() {
                        WorkerShutdown::Clean
                    } else {
                        WorkerShutdown::Panicked
                    };
                }
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
    WorkerSession::new(config, events, shutdown).run(commands);
}

enum CommandAction {
    Run(String),
    Continue,
    Stop,
}

struct WorkerSession {
    route: ResolvedRoute,
    profiles: Profiles,
    active_profile: String,
    law_constitution: String,
    agent: AgentLoop,
    history: Vec<ChatMessage>,
    session_usage: Usage,
    scrubber: SharedScrubber,
    connect: ConnectFn,
    key_literals: SecretRegistry,
    connected: bool,
    events: Sender<AgentEvent>,
    shutdown: Arc<AtomicBool>,
}

impl WorkerSession {
    fn new(config: WorkerConfig, events: Sender<AgentEvent>, shutdown: Arc<AtomicBool>) -> Self {
        let WorkerConfig {
            route,
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
        let (client, key_literals, connected) = match connection {
            Ok((client, literal)) => {
                let mut literals = SecretRegistry::new();
                install_literal(&scrubber, &mut literals, literal);
                (
                    tracked_client(shutdown_aware(client, &shutdown), route.id(), &events),
                    literals,
                    true,
                )
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
        let agent = AgentLoop {
            client,
            tools: tracked_tools(builtin_tools(), &events),
            ctx,
            receipts: ReceiptWriter::project(repo_root, key_literals.scrubber()),
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
                let _ =
                    progress_events.send(AgentEvent::Progress(safe_line(&event_scrubber, line)));
            })),
        };

        Self {
            route,
            profiles,
            active_profile,
            law_constitution,
            agent,
            history: Vec::new(),
            session_usage: Usage {
                cached_tokens: Some(0),
                ..Usage::default()
            },
            scrubber,
            connect,
            key_literals,
            connected,
            events,
            shutdown,
        }
    }

    fn run(mut self, commands: Receiver<WorkerCommand>) {
        while !self.stopped() {
            let Ok(command) = commands.recv() else {
                break;
            };
            if self.stopped() {
                break;
            }
            match self.handle_command(command) {
                CommandAction::Run(task) => {
                    if !self.run_task(task) {
                        break;
                    }
                }
                CommandAction::Continue => {}
                CommandAction::Stop => break,
            }
        }
    }

    fn stopped(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }

    fn handle_command(&mut self, command: WorkerCommand) -> CommandAction {
        match command {
            WorkerCommand::Task(task) => CommandAction::Run(task),
            WorkerCommand::SwitchRoute(next_route) => {
                let previous_route = self.route.id().to_owned();
                let next_route_id = next_route.id().to_owned();
                let execution_policy = self.profiles.effective(&self.active_profile, &next_route);
                let connection = (self.connect)(&next_route, execution_policy.output_cap);
                self.replace_connection(connection, &next_route_id);
                self.agent.model_id = next_route.model_id().to_owned();
                self.agent.thinking = effort_for(
                    execution_policy.posture,
                    next_route.thinking_dialect(),
                    next_route.wire(),
                );
                self.agent.profile = Some(execution_policy.profile.clone());
                self.active_profile = execution_policy.profile;
                let constitution = identity_constitution(&self.law_constitution, &next_route);
                self.agent.constitution = Some(constitution.clone());
                replace_system_message(&mut self.history, constitution);
                if !self.history.is_empty() && previous_route != next_route_id {
                    self.history.push(ChatMessage {
                        role: "system".into(),
                        content: Some(format!(
                            "Route changed: {previous_route} → {next_route_id}."
                        )),
                        tool_calls: None,
                        tool_call_id: None,
                        reasoning_content: None,
                    });
                }
                self.agent.context_limit = next_route.context();
                self.route = *next_route;
                CommandAction::Continue
            }
            WorkerCommand::SetEffort(effort) => {
                self.agent.thinking = effort;
                CommandAction::Continue
            }
            WorkerCommand::SetProfile(name) => {
                let execution_policy = self.profiles.effective(&name, &self.route);
                let connection = (self.connect)(&self.route, execution_policy.output_cap);
                let route_id = self.route.id().to_owned();
                self.replace_connection(connection, &route_id);
                self.agent.thinking = effort_for(
                    execution_policy.posture,
                    self.route.thinking_dialect(),
                    self.route.wire(),
                );
                self.agent.profile = Some(execution_policy.profile.clone());
                self.active_profile = execution_policy.profile;
                CommandAction::Continue
            }
            WorkerCommand::Stop => CommandAction::Stop,
        }
    }

    fn replace_connection(
        &mut self,
        connection: anyhow::Result<(Box<dyn ChatClient>, SecretValue)>,
        route: &str,
    ) {
        match connection {
            Ok((client, literal)) => self.install_connection(client, literal, route),
            Err(error) => {
                self.agent.client = Box::new(NotConnected {
                    message: error.to_string(),
                });
                self.connected = false;
            }
        }
    }

    fn install_connection(
        &mut self,
        client: Box<dyn ChatClient>,
        literal: SecretValue,
        route: &str,
    ) {
        apply_new_credential(
            &self.scrubber,
            &mut self.key_literals,
            literal,
            &mut self.agent,
        );
        self.agent.client =
            tracked_client(shutdown_aware(client, &self.shutdown), route, &self.events);
        self.connected = true;
    }

    fn ensure_connected(&mut self) -> anyhow::Result<()> {
        if self.connected {
            return Ok(());
        }
        let execution_policy = self.profiles.effective(&self.active_profile, &self.route);
        let (client, literal) = (self.connect)(&self.route, execution_policy.output_cap)?;
        let route_id = self.route.id().to_owned();
        self.install_connection(client, literal, &route_id);
        Ok(())
    }

    /// Returns false only when shutdown should stop the command loop.
    fn run_task(&mut self, task: String) -> bool {
        if let Err(error) = self.ensure_connected() {
            if self.stopped() {
                return false;
            }
            self.send_failure(&task, &error.to_string(), false);
            return true;
        }

        match self.agent.run_with_history(&mut self.history, &task) {
            Ok((answer, receipt)) => {
                if self.stopped() {
                    return false;
                }
                if let Some(usage) = &receipt.usage {
                    if add_usage(&mut self.session_usage, usage) {
                        let _ = self.events.send(AgentEvent::MeterIncomplete);
                    } else {
                        let _ = self
                            .events
                            .send(AgentEvent::Usage(self.session_usage.clone()));
                    }
                }
                let _ = self.events.send(AgentEvent::Answer(answer.clone()));
                let _ = self
                    .events
                    .send(AgentEvent::TaskReceipt(TimelineSummary { receipt, answer }));
            }
            Err(error) => {
                if self.stopped() {
                    return false;
                }
                self.send_failure(&task, &error.to_string(), true);
            }
        }
        true
    }

    fn send_failure(&self, task: &str, error: &str, meter_incomplete: bool) {
        let reason = safe_line(&self.scrubber, error);
        if meter_incomplete {
            let _ = self.events.send(AgentEvent::MeterIncomplete);
        }
        let _ = self.events.send(AgentEvent::Failed(reason.clone()));
        let _ = self
            .events
            .send(AgentEvent::TaskReceipt(failed_timeline_summary(
                self.route.model_id(),
                task,
                &reason,
            )));
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
    let cached_tokens = match (total.cached_tokens, usage.cached_tokens) {
        (Some(total_cached), Some(cached)) => {
            let Some(total_cached) = total_cached.checked_add(cached) else {
                return true;
            };
            Some(total_cached)
        }
        _ => None,
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
            cache_hit_pct: None,
            repairs: RepairStats::default(),
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
mod tests;
