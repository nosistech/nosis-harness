//! Construction of one interactive chat session and all of its boundaries.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use chrono::Utc;
use nh_core::agent::AgentLoop;
use nh_core::credential;
use nh_core::receipt::ReceiptWriter;
use nh_core::wire::ChatClient;
use nh_law::{Law, LoadOptions, Policy};
use nh_routes::{EffectiveExecutionPolicy, Profiles, ResolvedRoute, RouteClass, RouteResolver};
use nh_tools::{builtin_tools, Access, Tool, ToolCtx};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, SecretRegistry};

use super::{load_mcp, scrub_line, ChatSession, ConnectFn, NotConnected, SharedScrubber};
use crate::cmd_run::{self, effort_for, DELEGATE_MSG};
use crate::guard_from;

struct Startup {
    cwd: PathBuf,
    root: PathBuf,
    law: Law,
    resolver: RouteResolver,
    route: ResolvedRoute,
    profiles: Profiles,
    execution_policy: EffectiveExecutionPolicy,
}

impl Startup {
    fn load(model: &str, profile: &str) -> anyhow::Result<Self> {
        let cwd = std::env::current_dir()?;
        let (root, catalog) = cmd_run::find_catalog(&cwd)?;
        let law = nh_law::load(&root, &LoadOptions { cli_autonomy: None });
        let warning_scrubber = Scrubber::new(Vec::new());
        print_warnings(&law.warnings, &warning_scrubber);

        let resolver = RouteResolver::from_toml(&catalog)?;
        let route = resolver.resolve(model)?;
        let (profiles, profile_warnings) = Profiles::load(&root);
        print_warnings(&profile_warnings, &warning_scrubber);
        let execution_policy = profiles.effective(profile, &route);
        if let Some(warning) = cmd_run::profile_fallback_warning(profile, &execution_policy.profile)
        {
            print_warnings(&[warning], &warning_scrubber);
        }
        if route.class() == RouteClass::Delegate {
            anyhow::bail!("{DELEGATE_MSG}");
        }

        Ok(Self {
            cwd,
            root,
            law,
            resolver,
            route,
            profiles,
            execution_policy,
        })
    }
}

struct InitialConnection {
    client: Box<dyn ChatClient>,
    key_literals: SecretRegistry,
    connected: bool,
}

pub(super) fn open(model: &str, profile: &str) -> anyhow::Result<ChatSession> {
    let Startup {
        cwd,
        root,
        law,
        resolver,
        route,
        profiles,
        execution_policy,
    } = Startup::load(model, profile)?;
    let connect = connector(&law.policy);
    let initial = initial_connection(&connect, &route, execution_policy.output_cap)?;
    let registry_scrubber = initial.key_literals.scrubber();
    let scrubber: SharedScrubber = Arc::new(RwLock::new(registry_scrubber.clone()));
    let (tools, mcp_warnings) = chat_tools(&root, &law.policy, &scrubber);

    let approve_scrubber = Arc::clone(&scrubber);
    let event_scrubber = Arc::clone(&scrubber);
    let policy = law.policy.clone();
    let law_constitution = law.constitution;
    let agent = AgentLoop {
        client: initial.client,
        tools,
        ctx: ToolCtx::new(
            cwd,
            Box::new(move |action| {
                cmd_run::approve_on_stdin(&scrub_line(&approve_scrubber, action))
            }),
        )
        .with_scrubber(registry_scrubber.clone())
        .with_guard(Box::new(move |access| match access {
            Access::Read(path) => guard_from(policy.read_verdict(path)),
            Access::Write(path) => guard_from(policy.write_verdict(path)),
            Access::Exec(command) => guard_from(policy.exec_verdict(command)),
            Access::Send(target) => guard_from(policy.send_verdict(target)),
        })),
        receipts: ReceiptWriter::project(root, registry_scrubber),
        model_id: route.model_id().to_owned(),
        max_turns: 20,
        thinking: effort_for(
            None,
            execution_policy.posture,
            route.thinking_dialect(),
            route.wire(),
        ),
        profile: Some(execution_policy.profile.clone()),
        constitution: Some(cmd_run::agent_constitution(&law_constitution, &route)),
        context_limit: route.context(),
        on_event: Some(Box::new(move |line| {
            eprintln!("  {}", scrub_line(&event_scrubber, line));
        })),
    };

    Ok(ChatSession {
        resolver,
        route,
        profiles,
        active_profile: execution_policy.profile,
        agent,
        law_constitution,
        history: Vec::new(),
        session_in: 0,
        session_out: 0,
        session_cached: Some(0),
        session_cost: Vec::new(),
        unpriced_turns: 0,
        key_literals: initial.key_literals,
        scrubber,
        connect,
        connected: initial.connected,
        now: Box::new(Utc::now),
        local_offset: *chrono::Local::now().offset(),
        mcp_warnings,
    })
}

fn print_warnings(warnings: &[String], scrubber: &Scrubber) {
    for warning in warnings {
        eprintln!("warning: {}", cmd_run::safe_line(scrubber, warning));
    }
}

fn connector(policy: &Policy) -> ConnectFn {
    let policy = policy.clone();
    Box::new(move |route, output_cap| {
        let vault = EnvFallbackVault {
            inner: KeyringVault,
        };
        credential::connect(
            &vault,
            route,
            &policy.approved_audiences(route.vault_entry()),
            output_cap,
        )
    })
}

fn initial_connection(
    connect: &ConnectFn,
    route: &ResolvedRoute,
    output_cap: Option<u64>,
) -> anyhow::Result<InitialConnection> {
    match connect(route, output_cap) {
        Ok((client, literal)) => {
            let mut key_literals = SecretRegistry::new();
            key_literals.insert(literal);
            Ok(InitialConnection {
                client,
                key_literals,
                connected: true,
            })
        }
        Err(error) if error.downcast_ref::<nh_vault::AudienceRefused>().is_some() => Err(error),
        Err(error) => {
            let warning_scrubber = Scrubber::new(Vec::new());
            print_warnings(&[error.to_string()], &warning_scrubber);
            Ok(InitialConnection {
                client: Box::new(NotConnected {
                    msg: error.to_string(),
                }),
                key_literals: SecretRegistry::new(),
                connected: false,
            })
        }
    }
}

fn chat_tools(
    root: &std::path::Path,
    policy: &Policy,
    scrubber: &SharedScrubber,
) -> (Vec<Box<dyn Tool>>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut tools = builtin_tools();
    let home = nh_law::user_home_dir();
    tools.extend(load_mcp(root, home.as_deref(), policy, &mut warnings));
    for warning in &warnings {
        eprintln!("warning: {}", scrub_line(scrubber, warning));
    }
    (tools, warnings)
}
