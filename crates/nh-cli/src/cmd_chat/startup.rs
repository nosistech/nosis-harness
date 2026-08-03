//! Construction of one interactive chat session and all of its boundaries.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use chrono::Utc;
use nh_core::agent::AgentLoop;
use nh_core::credential;
use nh_core::receipt::ReceiptWriter;
use nh_core::session_ledger::{
    new_session_id, RestoredSession, SessionEvent, SessionLedger, Surface,
};
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
    fn load(model: &str, profile: &str, resuming: bool) -> anyhow::Result<Self> {
        let cwd = std::env::current_dir()?;
        let (root, catalog) = cmd_run::find_catalog(&cwd)?;
        let law = nh_law::load(&root, &LoadOptions { cli_autonomy: None });
        let warning_scrubber = Scrubber::new(Vec::new());
        print_warnings(&law.warnings, &warning_scrubber);

        let resolver = RouteResolver::from_toml(&catalog)?;
        let route = match resolver.resolve(model) {
            Ok(route) => route,
            Err(_) if resuming => anyhow::bail!(
                "session route {model} is no longer available - restore it in catalog.toml, then retry"
            ),
            Err(error) => return Err(error),
        };
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
    open_session(model, profile, None)
}

pub(super) fn reopen(restored: RestoredSession) -> anyhow::Result<ChatSession> {
    validate_surface(&restored)?;
    let route_id = restored.route_id.clone();
    let profile = restored.profile.clone();
    open_session(&route_id, &profile, Some(restored))
}

fn validate_surface(restored: &RestoredSession) -> anyhow::Result<()> {
    if restored.surface != Surface::Chat {
        anyhow::bail!(
            "session belongs to the tui - run `nh resume {}`",
            restored.session_id
        );
    }
    Ok(())
}

fn open_session(
    model: &str,
    profile: &str,
    restored: Option<RestoredSession>,
) -> anyhow::Result<ChatSession> {
    let startup = Startup::load(model, profile, restored.is_some())?;
    let connect = connector(
        &startup.law.policy,
        startup.resolver.routes_with_modality("image"),
    );
    open_prepared(startup, restored, connect, chat_tools)
}

fn open_prepared<F>(
    startup: Startup,
    restored: Option<RestoredSession>,
    connect: ConnectFn,
    load_tools: F,
) -> anyhow::Result<ChatSession>
where
    F: FnOnce(&std::path::Path, &Policy, &SharedScrubber) -> (Vec<Box<dyn Tool>>, Vec<String>),
{
    let Startup {
        cwd,
        root,
        law,
        resolver,
        route,
        profiles,
        execution_policy,
    } = startup;
    let resolver = Arc::new(resolver);
    if let Some(saved) = &restored {
        if saved.model_id != route.model_id() {
            anyhow::bail!(
                "session route {} now points to a different model - restore the recorded catalog entry, then retry",
                saved.route_id
            );
        }
    }
    let initial = initial_connection(&connect, &route, execution_policy.output_cap)?;
    let registry_scrubber = initial.key_literals.scrubber();
    let scrubber: SharedScrubber = Arc::new(RwLock::new(registry_scrubber.clone()));
    let (tools, mcp_warnings) = load_tools(&root, &law.policy, &scrubber);

    let approve_scrubber = Arc::clone(&scrubber);
    let event_scrubber = Arc::clone(&scrubber);
    let policy = law.policy.clone();
    let law_constitution = law.constitution;
    let current_constitution = cmd_run::agent_constitution(&law_constitution, &route);
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
        receipts: ReceiptWriter::project(root.clone(), registry_scrubber.clone()),
        model_id: route.model_id().to_owned(),
        max_turns: 20,
        thinking: effort_for(
            None,
            execution_policy.posture,
            route.thinking_dialect(),
            route.wire(),
        ),
        profile: Some(execution_policy.profile.clone()),
        constitution: Some(current_constitution.clone()),
        context_limit: route.context(),
        on_event: Some(Box::new(move |line| {
            eprintln!("  {}", scrub_line(&event_scrubber, line));
        })),
    };

    let session_id = restored
        .as_ref()
        .map_or_else(new_session_id, |saved| saved.session_id.clone());
    let ledger = SessionLedger::create(root, session_id.clone(), registry_scrubber);
    let history = restored
        .as_ref()
        .map_or_else(Vec::new, |saved| saved.history.clone());
    let constitution_changed = history
        .first()
        .filter(|message| message.role == "system")
        .and_then(|message| message.content.as_deref())
        .is_some_and(|recorded| !recorded.ends_with(&law_constitution));
    let pending_route_context = restored.as_ref().and_then(|saved| {
        saved
            .turns
            .last()
            .is_some_and(|turn| turn.route_id != saved.route_id)
            .then(|| super::route_context_message(current_constitution.clone()))
    });
    let mut session = ChatSession {
        resolver,
        route,
        profiles,
        active_profile: execution_policy.profile,
        agent,
        law_constitution,
        history,
        session_in: 0,
        session_out: 0,
        session_cached: Some(0),
        // Turn-ledger usage is task-cumulative, so it cannot prove the final
        // provider call's cache measurement after a resume.
        last_cached_tokens: None,
        session_cost: Vec::new(),
        unpriced_turns: 0,
        key_literals: initial.key_literals,
        scrubber,
        connect,
        connected: initial.connected,
        now: Box::new(Utc::now),
        local_offset: *chrono::Local::now().offset(),
        mcp_warnings,
        pending_images: Vec::new(),
        ledger,
        ledger_failed: false,
        ledger_notice_shown: false,
        resumed: restored.is_some(),
        restored_turns: restored.as_ref().map_or(0, |saved| saved.turns.len()),
        dropped_torn_tail: restored
            .as_ref()
            .is_some_and(|saved| saved.dropped_torn_tail),
        constitution_changed,
        pending_route_context,
    };
    let event = if let Some(saved) = &restored {
        super::restore_session_totals(&mut session, &saved.turns)?;
        SessionEvent::Resumed {
            ts_utc: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        }
    } else {
        SessionEvent::Started {
            session_id,
            surface: Surface::Chat,
            route_id: session.route.id().to_owned(),
            model_id: session.route.model_id().to_owned(),
            profile: session.active_profile.clone(),
            created_utc: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        }
    };
    session.ledger_failed = session.ledger.append(&event).is_err();
    Ok(session)
}

#[cfg(test)]
pub(super) fn reopen_with_test_dependencies(
    root: &std::path::Path,
    resolver: RouteResolver,
    restored: RestoredSession,
    connect: ConnectFn,
) -> anyhow::Result<ChatSession> {
    validate_surface(&restored)?;
    let route = resolver.resolve(&restored.route_id)?;
    let profiles = Profiles::bundled();
    let execution_policy = profiles.effective(&restored.profile, &route);
    let law = nh_law::load(root, &LoadOptions { cli_autonomy: None });
    let startup = Startup {
        cwd: root.to_path_buf(),
        root: root.to_path_buf(),
        law,
        resolver,
        route,
        profiles,
        execution_policy,
    };
    open_prepared(startup, Some(restored), connect, |_, _, _| {
        (builtin_tools(), Vec::new())
    })
}

fn print_warnings(warnings: &[String], scrubber: &Scrubber) {
    for warning in warnings {
        eprintln!("warning: {}", cmd_run::safe_line(scrubber, warning));
    }
}

fn connector(policy: &Policy, image_capable_routes: Vec<String>) -> ConnectFn {
    let policy = policy.clone();
    Box::new(move |route, output_cap| {
        let vault = EnvFallbackVault {
            inner: KeyringVault,
        };
        credential::connect_with_image_routes(
            &vault,
            route,
            &policy.approved_audiences(route.vault_entry()),
            output_cap,
            &image_capable_routes,
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
