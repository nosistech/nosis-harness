//! `nh run` - resolve the route, fetch the key, drive the agent loop.
//! Progress = one short line per tool call via `on_event`; exec_shell additionally
//! surfaces through its approval prompt. All errors: one friendly line, exit 1.

mod config;
mod meter;

pub(crate) use config::{find_catalog, load_and_vet_mcp_configs};
pub(crate) use meter::{
    compaction_meter_line, context_window_summary, terminal_progress_meter_line, turn_cost_line,
    usage_token_summary,
};

#[cfg(test)]
use config::{
    filter_mcp_audiences_with, find_catalog_with_home, merge_and_vet, unapproved_mcp_target,
    BUNDLED_CATALOG,
};
#[cfg(test)]
use meter::turn_cost_line_for_run;
use meter::{run_meter_lines, terminal_meter_lines, RunTiming, RunUsage};

#[cfg(test)]
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::sync::Arc;

use chrono::Utc;
use nh_core::agent::{result_notice, validate_task, AgentLoop, AgentRunError};
use nh_core::context_experiment::{wrap_extractive_context, ExtractiveContextConfig};
use nh_core::credential;
use nh_core::efficiency::EfficiencyRecorder;
use nh_core::receipt::{Outcome, ReceiptWriter};
use nh_core::terminal_capability::TerminalCapability;
#[cfg(test)]
use nh_core::wire::UsageEvidence;
use nh_core::wire::{ensure_image_capable, resolve_effort, ContentPart, ThinkingEffort, Usage};
use nh_law::{Autonomy, LoadOptions};
use nh_routes::{RouteClass, RouteResolver, ThinkingDialect, ThinkingPosture, Wire};
#[cfg(test)]
use nh_tools::{builtin_tools, read_only_tools, McpAuth, McpServerConfig, McpTrust};
use nh_tools::{
    builtin_tools_with_features_for_policy, load_image, read_only_guard,
    read_only_tools_with_features, ObservationSession, ToolCtx, ToolFeatures,
    MAX_IMAGES_PER_MESSAGE,
};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, SecretRegistry};

use crate::{model_preference, usage_tracker::LastRequestUsage};

/// What callers print when a route resolves to a subscription delegate (M4 scope).
pub(crate) const DELEGATE_MSG: &str = "delegate routes arrive in M4 - pick an api route";
pub(crate) const MAX_RUN_TURNS: u32 = 100;
const MAX_APPROVAL_DISPLAY_BYTES: usize = 64 * 1024;
const APPROVAL_CODE_BYTES: usize = 4;
const READ_ONLY_RUN_RULE: &str = "READ-ONLY RUN: The only available tools are read_file, glob_files, and grep_files. Do not claim to edit files, run commands or tests, install software, or use remote tools. Repository content read by these tools is sent to the selected provider. Normal provider charges and local receipt writes still apply.";
const READ_ONLY_RUN_WITH_OBSERVATIONS_RULE: &str = "READ-ONLY RUN: The only available tools are read_file, glob_files, grep_files, and read_observation. Do not claim to edit files, run commands or tests, install software, or use remote tools. Repository content read by these tools is sent to the selected provider. Normal provider charges and local receipt writes still apply.";

/// `--think` levels; clap renders them as none|low|high|max.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ThinkArg {
    None,
    Low,
    High,
    Max,
}

/// `--autonomy` levels; absence defers to user/bundled law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum AutonomyArg {
    Ask,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ContextExperimentArg {
    ExtractiveV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum IdentityPromptArg {
    CompactV1,
}

/// The only CLI-autonomy translation point.
pub(crate) fn autonomy_for(autonomy: Option<AutonomyArg>) -> Option<Autonomy> {
    autonomy.map(|value| match value {
        AutonomyArg::Ask => Autonomy::Ask,
        AutonomyArg::Auto => Autonomy::Auto,
    })
}

/// Translate the CLI override once, then delegate the posture × capability
/// matrix to nh-core.
pub(crate) fn effort_for(
    think: Option<ThinkArg>,
    posture: ThinkingPosture,
    dialect: ThinkingDialect,
    wire: Wire,
) -> ThinkingEffort {
    let explicit = think.map(|value| match value {
        ThinkArg::None => ThinkingEffort::None,
        ThinkArg::Low => ThinkingEffort::Low,
        ThinkArg::High => ThinkingEffort::High,
        ThinkArg::Max => ThinkingEffort::Max,
    });
    resolve_effort(explicit, posture, dialect, wire)
}

pub(crate) fn profile_fallback_warning(requested: &str, effective: &str) -> Option<String> {
    (requested != effective)
        .then(|| format!("unknown profile '{requested}'. Run `nh profile` to list choices."))
}

fn validate_context_experiment(
    context_experiment: Option<ContextExperimentArg>,
    retain_observations: bool,
) -> anyhow::Result<()> {
    if context_experiment.is_some() && !retain_observations {
        anyhow::bail!(
            "--context-experiment extractive-v1 requires --retain-observations so removed context stays recoverable"
        );
    }
    Ok(())
}

pub(crate) fn agent_constitution(
    law_constitution: &str,
    route: &nh_routes::ResolvedRoute,
    shell_unavailable: bool,
) -> String {
    nh_tui::identity_constitution(law_constitution, route, shell_unavailable)
}

fn run_constitution(
    law_constitution: &str,
    route: &nh_routes::ResolvedRoute,
    read_only: bool,
    retain_observations: bool,
    identity_prompt: Option<IdentityPromptArg>,
    shell_unavailable: bool,
) -> String {
    let session_law = nh_core::agent::session_law_constitution(law_constitution, shell_unavailable);
    let mut constitution = match identity_prompt {
        Some(IdentityPromptArg::CompactV1) => nh_core::agent::compact_identity_constitution_v1(
            &session_law,
            route.id(),
            route.provider(),
        ),
        None => nh_core::agent::identity_constitution(&session_law, route.id(), route.provider()),
    };
    if read_only {
        constitution.push_str("\n\n");
        constitution.push_str(if retain_observations {
            READ_ONLY_RUN_WITH_OBSERVATIONS_RULE
        } else {
            READ_ONLY_RUN_RULE
        });
    }
    constitution
}

pub(crate) struct RunOptions<'a> {
    pub(crate) max_turns: u32,
    pub(crate) think: Option<ThinkArg>,
    pub(crate) autonomy: Option<AutonomyArg>,
    pub(crate) profile: &'a str,
    pub(crate) images: &'a [String],
    pub(crate) read_only: bool,
    pub(crate) measure_efficiency: bool,
    pub(crate) enable_ranged_reads: bool,
    pub(crate) retain_observations: bool,
    pub(crate) context_experiment: Option<ContextExperimentArg>,
    pub(crate) identity_prompt: Option<IdentityPromptArg>,
    pub(crate) terminal_capability: TerminalCapability,
}

pub fn run(task: &str, model: Option<&str>, options: RunOptions<'_>) -> anyhow::Result<()> {
    let RunOptions {
        max_turns,
        think,
        autonomy,
        profile,
        images,
        read_only,
        measure_efficiency,
        enable_ranged_reads,
        retain_observations,
        context_experiment,
        identity_prompt,
        terminal_capability,
    } = options;
    validate_task(task)?;
    validate_image_count(images.len())?;
    validate_context_experiment(context_experiment, retain_observations)?;
    let cwd = std::env::current_dir()?;
    let (root, catalog) = find_catalog(&cwd)?;
    let law = nh_law::load_checked(
        &root,
        &LoadOptions {
            cli_autonomy: autonomy_for(autonomy),
        },
    )?;
    let warning_scrubber = Scrubber::new(Vec::new());
    for warning in &law.warnings {
        eprintln!("warning: {}", safe_line(&warning_scrubber, warning));
    }
    let shell_unavailable = law.policy.blocks_all_shell_commands();
    let resolver = Arc::new(RouteResolver::from_toml(&catalog)?);
    let model = model_preference::selected_model(model, &resolver)?;
    let route = resolver.resolve(&model)?;
    let (profiles, profile_warnings) = nh_routes::Profiles::load(&root);
    for warning in &profile_warnings {
        eprintln!("warning: {}", safe_line(&warning_scrubber, warning));
    }
    let execution_policy = profiles.effective(profile, &route);
    if let Some(error) = profile_fallback_warning(profile, &execution_policy.profile) {
        anyhow::bail!("{error}");
    }
    if route.class() == RouteClass::Delegate {
        anyhow::bail!("{DELEGATE_MSG}");
    }
    if !images.is_empty() {
        ensure_image_capable(&route, &resolver)?;
    }

    let vault = EnvFallbackVault {
        inner: KeyringVault,
    };
    let approved = law.policy.approved_audiences(route.vault_entry());
    let (client, literal) = credential::connect_with_catalog(
        &vault,
        &route,
        &approved,
        execution_policy.output_cap,
        &resolver,
    )?;
    let last_request_usage = LastRequestUsage::default();
    let client = last_request_usage.wrap(client);
    let mut active_secrets = SecretRegistry::new();
    active_secrets.insert(literal);
    let session_scrubber = active_secrets.scrubber();
    let quote_at = Utc::now();
    let efficiency = EfficiencyRecorder::project_if_enabled(
        measure_efficiency,
        root.clone(),
        &route,
        quote_at,
        session_scrubber.clone(),
    );
    let client = match &efficiency {
        Some(recorder) => recorder.wrap_client(client),
        None => client,
    };
    // Only the active route credential is materialized. Receipts, stdout,
    // progress, tools, and approvals all derive from its zeroizing registry.
    let approve_scrubber = session_scrubber.clone();
    let event_scrubber = session_scrubber.clone();
    let event_resolver = Arc::clone(&resolver);
    let event_route = route.clone();
    let policy = law.policy.clone();
    let guard = nh_tools::policy_guard(policy);
    let guard = if read_only {
        read_only_guard(guard)
    } else {
        guard
    };
    let mut ctx = ToolCtx::new(
        cwd,
        // Model-supplied commands are scrubbed + control-char-escaped before display
        // so the approval gate always shows one faithful line.
        Box::new(move |action| approve_on_stdin(&approval_line(&approve_scrubber, action))),
        guard,
        session_scrubber.clone(),
    );
    let observation_session = if retain_observations {
        let runtime_parent = nh_core::runtime_path::ensure_contained_dir(
            &root,
            std::path::Path::new(".nosis/observations"),
        )?;
        let session = ObservationSession::create(&runtime_parent, &ctx)?;
        ctx = ctx.with_observation_session(session.clone())?;
        Some(session)
    } else {
        None
    };
    let client = match context_experiment {
        Some(ContextExperimentArg::ExtractiveV1) => {
            let archive = ctx.observation_retainer().ok_or_else(|| {
                anyhow::anyhow!("context experiment requires an active observation session")
            })?;
            wrap_extractive_context(
                client,
                ExtractiveContextConfig {
                    context_limit: route.context().ok_or_else(|| {
                        anyhow::anyhow!(
                            "context experiment requires a catalog context limit for the selected model"
                        )
                    })?,
                    max_turns,
                    quote: route.price_at(quote_at),
                    archive,
                    measurement: efficiency.clone(),
                    cancel: Arc::clone(&ctx.cancel),
                },
            )
        }
        None => client,
    };
    let image_parts = images
        .iter()
        .map(|path| image_part(path, &ctx))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let receipts = ReceiptWriter::project(root.clone(), session_scrubber.clone());
    let tool_features = ToolFeatures {
        ranged_reads: enable_ranged_reads,
        observation_session: observation_session.clone(),
    };
    let tools = if read_only {
        read_only_tools_with_features(tool_features)
    } else {
        builtin_tools_with_features_for_policy(tool_features, &law.policy)
    };
    let tools = match &efficiency {
        Some(recorder) => recorder.wrap_tools(tools),
        None => tools,
    };
    let mut agent = AgentLoop {
        client,
        tools,
        ctx,
        receipts,
        model_id: route.model_id().to_owned(),
        max_turns,
        thinking: effort_for(
            think,
            execution_policy.posture,
            route.thinking_dialect(),
            route.wire(),
        ),
        profile: Some(execution_policy.profile.clone()),
        // Honest identity: name the real route + forbid claiming to be Claude/GPT.
        constitution: Some(run_constitution(
            &law.constitution,
            &route,
            read_only,
            retain_observations,
            identity_prompt,
            shell_unavailable,
        )),
        context_limit: route.context(),
        on_event: Some(Box::new(move |line| {
            let line = terminal_progress_meter_line(
                terminal_capability,
                &event_resolver,
                &event_route,
                line,
            );
            eprintln!("  {}", safe_line(&event_scrubber, &line))
        })),
    };

    if read_only {
        eprintln!(
            "running {} (read-only tools; max {max_turns} turns)",
            safe_line(&session_scrubber, route.model_id())
        );
        eprintln!(
            "  read content goes to the provider; provider costs and local receipts still apply"
        );
    } else {
        eprintln!(
            "running {} (max {max_turns} turns)",
            safe_line(&session_scrubber, route.model_id())
        );
    }
    if shell_unavailable {
        eprintln!("  {}", nh_core::agent::SHELL_UNAVAILABLE_SESSION_NOTICE);
    }
    if let Some(recorder) = &efficiency {
        eprintln!(
            "  local efficiency metadata: {} (task {}; receipt outcome is not correctness)",
            EfficiencyRecorder::relative_path(),
            safe_line(&session_scrubber, recorder.task_id())
        );
    }
    if enable_ranged_reads {
        eprintln!("  bounded ranged read_file arguments enabled for this run");
    }
    if retain_observations {
        eprintln!(
            "  scrubbed large tool results retained temporarily for this run; source tool limits still apply"
        );
    }
    if context_experiment.is_some() {
        eprintln!(
            "  extractive context experiment enabled; archived history remains scrubbed and session-local"
        );
    }
    let scrubber = session_scrubber;
    let started = Utc::now();
    let result = if image_parts.is_empty() {
        agent.run(task)
    } else {
        let mut history = Vec::new();
        agent.run_with_history_and_parts(&mut history, task, image_parts)
    };
    let ended = Utc::now();
    let context_usage = last_request_usage.snapshot();
    if let Some(error) = observation_session
        .as_ref()
        .and_then(|session| session.cleanup().err())
    {
        eprintln!(
            "warning: retained-observation cleanup incomplete: {}",
            safe_line(&scrubber, &error.to_string())
        );
    }
    let (answer, receipt) = match result {
        Ok(completed) => {
            if let Some(recorder) = &efficiency {
                recorder.record_task(&completed.1);
            }
            completed
        }
        Err(error) => {
            if let (Some(recorder), Some(run_error)) =
                (&efficiency, error.downcast_ref::<AgentRunError>())
            {
                recorder.record_task(run_error.receipt());
            }
            report_efficiency_warning(efficiency.as_ref(), &scrubber);
            if let Some(meter_lines) = failed_run_meter_lines(
                &error,
                &resolver,
                &route,
                context_usage.as_ref(),
                RunTiming { started, ended },
            ) {
                let stderr = io::stderr();
                let mut stderr = stderr.lock();
                for line in terminal_meter_lines(terminal_capability, meter_lines) {
                    writeln!(stderr, "{}", safe_line(&scrubber, &line))?;
                }
            }
            return Err(anyhow::anyhow!(
                "{}",
                safe_line(&scrubber, &error.to_string())
            ));
        }
    };
    report_efficiency_warning(efficiency.as_ref(), &scrubber);

    let meter_lines = terminal_meter_lines(
        terminal_capability,
        run_meter_lines(
            &resolver,
            &route,
            RunUsage::new(receipt.usage.as_ref(), context_usage.as_ref()),
            &receipt.compaction,
            receipt.turns,
            receipt.tool_calls,
            RunTiming { started, ended },
        ),
    );
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let stderr = io::stderr();
    let mut stderr = stderr.lock();
    write_run_output(
        &mut stdout,
        &mut stderr,
        &scrubber,
        &answer,
        &meter_lines,
        result_notice(read_only),
    )?;
    if receipt.outcome == Outcome::Timeout {
        anyhow::bail!("{}", max_turns_timeout_message(max_turns));
    }
    Ok(())
}

fn failed_run_meter_lines(
    error: &anyhow::Error,
    resolver: &RouteResolver,
    route: &nh_routes::ResolvedRoute,
    context_usage: Option<&Usage>,
    timing: RunTiming,
) -> Option<Vec<String>> {
    let receipt = error.downcast_ref::<AgentRunError>()?.receipt();
    Some(run_meter_lines(
        resolver,
        route,
        RunUsage::new(receipt.usage.as_ref(), context_usage),
        &receipt.compaction,
        receipt.turns,
        receipt.tool_calls,
        timing,
    ))
}

fn report_efficiency_warning(recorder: Option<&EfficiencyRecorder>, scrubber: &Scrubber) {
    if let Some(error) = recorder.and_then(EfficiencyRecorder::warning) {
        eprintln!(
            "warning: efficiency measurement incomplete: {}",
            safe_line(scrubber, &error)
        );
    }
}

pub(crate) fn validate_image_count(count: usize) -> anyhow::Result<()> {
    if count > MAX_IMAGES_PER_MESSAGE {
        anyhow::bail!(
            "a message can attach at most {MAX_IMAGES_PER_MESSAGE} images - remove the extra image paths"
        );
    }
    Ok(())
}

pub(crate) fn image_part(path: &str, ctx: &ToolCtx) -> anyhow::Result<ContentPart> {
    let image = load_image(path, ctx)?;
    Ok(ContentPart::ImageB64 {
        media_type: image.media_type,
        data: image.data,
    })
}

fn write_run_output<W: Write, E: Write>(
    stdout: &mut W,
    stderr: &mut E,
    scrubber: &Scrubber,
    answer: &str,
    meter_lines: &[String],
    notice: &str,
) -> io::Result<()> {
    writeln!(stdout, "{}", safe_text(scrubber, answer))?;
    for line in meter_lines {
        writeln!(stderr, "{}", safe_line(scrubber, line))?;
    }
    writeln!(stderr, "{}", safe_line(scrubber, notice))?;
    Ok(())
}

fn max_turns_timeout_message(max_turns: u32) -> String {
    if max_turns < MAX_RUN_TURNS {
        let next = max_turns.saturating_mul(2).clamp(1, MAX_RUN_TURNS);
        format!("stopped at max turns ({max_turns}) - rerun with --max-turns {next}")
    } else {
        format!(
            "stopped at max turns ({max_turns}) - split the task or make the request more focused"
        )
    }
}

/// Scrub secrets, then escape for display. Every stderr line built from
/// model-controlled text goes through this - one choke point.
pub(crate) fn safe_line(scrubber: &Scrubber, text: &str) -> String {
    nh_vault::safe_line(scrubber, text)
}

pub(crate) fn approval_line(scrubber: &Scrubber, text: &str) -> String {
    nh_vault::escape_untrusted(&scrubber.scrub(text))
}

/// Scrub and control-escape each answer line while preserving every newline.
pub(crate) fn safe_text(scrubber: &Scrubber, text: &str) -> String {
    let mut safe = String::new();
    for segment in text.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map_or((segment, false), |line| (line, true));
        safe.push_str(&nh_vault::escape_untrusted(&scrubber.scrub(line)));
        if newline {
            safe.push('\n');
        }
    }
    safe
}

/// Approval gate: bounded stderr prompt, default deny. `display` is the command
/// already scrubbed and control-char-escaped by the caller (see `approval_line`).
pub(crate) fn approve_on_stdin(display: &str) -> bool {
    let stdin = io::stdin();
    let terminal = stdin.is_terminal();
    let mut input = stdin.lock();
    let stderr = io::stderr();
    let mut stderr = stderr.lock();
    approve_with_io(
        display,
        terminal,
        &mut input,
        &mut stderr,
        fresh_approval_bytes,
    )
}

fn approve_with_io<R: BufRead, W: Write, F: FnOnce() -> Option<[u8; APPROVAL_CODE_BYTES]>>(
    display: &str,
    terminal: bool,
    input: &mut R,
    stderr: &mut W,
    fresh_bytes: F,
) -> bool {
    if display.len() > MAX_APPROVAL_DISPLAY_BYTES {
        let _ = writeln!(
            stderr,
            "  approval refused: command is too large to display in full (maximum {MAX_APPROVAL_DISPLAY_BYTES} bytes)"
        );
        return false;
    }
    if !terminal {
        let _ = writeln!(
            stderr,
            "  approval refused: stdin is not a terminal; piped input cannot approve shell commands"
        );
        return false;
    }
    let Some(bytes) = fresh_bytes() else {
        let _ = writeln!(
            stderr,
            "  approval refused: could not generate a fresh confirmation code"
        );
        return false;
    };
    let code = encode_approval_code(bytes);
    if write!(
        stderr,
        "  approve? {display}\n  To approve this request, type {code} and press Enter; anything else declines [default: no]: "
    )
    .is_err()
        || stderr.flush().is_err()
    {
        return false;
    }
    let mut line = String::new();
    input.read_line(&mut line).is_ok() && approval_line_matches(&line, &code)
}

fn fresh_approval_bytes() -> Option<[u8; APPROVAL_CODE_BYTES]> {
    let mut bytes = [0_u8; APPROVAL_CODE_BYTES];
    getrandom::getrandom(&mut bytes).ok()?;
    Some(bytes)
}

fn encode_approval_code(bytes: [u8; APPROVAL_CODE_BYTES]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut code = String::with_capacity(APPROVAL_CODE_BYTES * 2);
    for byte in bytes {
        code.push(char::from(HEX[usize::from(byte >> 4)]));
        code.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    code
}

fn approval_line_matches(line: &str, code: &str) -> bool {
    let Some(line) = line.strip_suffix('\n') else {
        return false;
    };
    line.strip_suffix('\r').unwrap_or(line) == code
}

#[cfg(test)]
mod tests;
