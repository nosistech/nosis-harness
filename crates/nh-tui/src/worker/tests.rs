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

#[test]
fn tracked_tool_emits_exact_start_and_finish_events() {
    struct TestTool;

    impl Tool for TestTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "test_tool".into(),
                description: "test only".into(),
                parameters: ToolArgs::default(),
            }
        }

        fn execute(&self, _args: ToolArgs, _ctx: &ToolCtx) -> anyhow::Result<String> {
            Ok("done".into())
        }
    }

    let (events, received) = mpsc::channel();
    let mut tools = tracked_tools(vec![Box::new(TestTool)], &events);
    let tool = tools.remove(0);
    let ctx = ToolCtx::new(PathBuf::from("."), Box::new(|_| false));

    assert_eq!(tool.execute(ToolArgs::default(), &ctx).unwrap(), "done");
    match received.recv().unwrap() {
        AgentEvent::ToolStarted { name, .. } => assert_eq!(name, "test_tool"),
        _ => panic!("first event must start the tool"),
    }
    match received.recv().unwrap() {
        AgentEvent::ToolFinished { name } => assert_eq!(name, "test_tool"),
        _ => panic!("second event must finish the tool"),
    }
}
