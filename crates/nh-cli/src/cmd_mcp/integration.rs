use std::collections::BTreeSet;
use std::io::{BufRead, IsTerminal as _};
use std::path::Path;

use nh_law::{Policy, Verdict};
use nh_tools::{McpAuth, McpReviewSnapshot, McpReviewState, McpServerConfig, McpTrust};

use crate::private_state::{PrivateStateFile, PrivateStateRead};

const MAX_GUIDED_INPUT_BYTES: usize = 4 * 1024;
const MAX_GUIDED_NAME_BYTES: usize = 64;
const MAX_GUIDED_URL_BYTES: usize = 2 * 1024;
const CONFIG_STEM: &str = "mcp-config";

pub(crate) fn connect() -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    require_interactive_mutation(
        "connection setup",
        stdin.is_terminal(),
        stdout.is_terminal(),
    )?;
    let cwd = std::env::current_dir()?;
    let (root, _) = crate::cmd_run::find_catalog(&cwd)?;
    let law = nh_law::load_checked(&root, &nh_law::LoadOptions { cli_autonomy: None })?;
    crate::cmd_fleet::print_law_warnings(&law.warnings);
    let home = nh_law::user_home_dir().ok_or_else(|| {
        anyhow::anyhow!("home directory is unavailable - MCP configuration was not changed")
    })?;
    let executable = std::env::current_exe()?;
    connect_with_io(
        &home,
        &law.policy,
        &executable,
        &mut stdin.lock(),
        &mut stdout.lock(),
    )
}

pub(crate) fn status(server: Option<&str>) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (root, _) = crate::cmd_run::find_catalog(&cwd)?;
    let law = nh_law::load_checked(&root, &nh_law::LoadOptions { cli_autonomy: None })?;
    crate::cmd_fleet::print_law_warnings(&law.warnings);
    let home = nh_law::user_home_dir().ok_or_else(|| {
        anyhow::anyhow!("home directory is unavailable - MCP status cannot be read")
    })?;
    let executable = std::env::current_exe()?;
    status_with_io(
        &root,
        &home,
        &law.policy,
        &executable,
        server,
        &mut std::io::stdout().lock(),
    )
}

pub(crate) fn check(server: &str) -> anyhow::Result<()> {
    super::validate_state_name(server)?;
    let cwd = std::env::current_dir()?;
    let (root, _) = crate::cmd_run::find_catalog(&cwd)?;
    let law = nh_law::load_checked(&root, &nh_law::LoadOptions { cli_autonomy: None })?;
    crate::cmd_fleet::print_law_warnings(&law.warnings);
    let home = nh_law::user_home_dir().ok_or_else(|| {
        anyhow::anyhow!("home directory is unavailable - MCP metadata cannot be checked")
    })?;
    let executable = std::env::current_exe()?;
    check_with_io(
        &root,
        &home,
        &law.policy,
        &executable,
        server,
        &mut std::io::stdout().lock(),
    )
}

pub(crate) fn disable(server: &str) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    require_interactive_mutation("disable", stdin.is_terminal(), stdout.is_terminal())?;
    super::validate_state_name(server)?;
    let home = nh_law::user_home_dir().ok_or_else(|| {
        anyhow::anyhow!("home directory is unavailable - MCP review state was not changed")
    })?;
    disable_with_io(&home, server, &mut stdin.lock(), &mut stdout.lock())
}

fn connect_with_io(
    home: &Path,
    policy: &Policy,
    executable: &Path,
    input: &mut dyn BufRead,
    output: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
    connect_with_io_before_publish(home, policy, executable, input, output, || Ok(()))
}

fn connect_with_io_before_publish(
    home: &Path,
    policy: &Policy,
    executable: &Path,
    input: &mut dyn BufRead,
    output: &mut dyn std::io::Write,
    before_review_recheck: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let scrubber = nh_vault::Scrubber::new(Vec::new());
    writeln!(
        output,
        "Connect a supported modern stateless HTTP MCP server."
    )?;
    writeln!(
        output,
        "This saves configuration only. It does not contact a server or model."
    )?;

    let Some(name) = prompt(input, output, "MCP server name: ")? else {
        return cancelled(
            output,
            "Connection setup cancelled; configuration unchanged.",
        );
    };
    if name.is_empty() {
        return cancelled(
            output,
            "Connection setup cancelled; configuration unchanged.",
        );
    }
    validate_guided_name(&name)?;
    reject_shaped_input("MCP server name", &name)?;

    let (prior_config, configs) = read_user_configs(home)?;
    if configs.iter().any(|config| config.name == name) {
        anyhow::bail!("an MCP server with that name already exists; configuration was not changed")
    }
    let (orphan_text, _) = super::read_prior_review(home, &name)?;
    if orphan_text.is_some() {
        anyhow::bail!(
            "saved review state already exists for that name; choose another name or deliberately remove the saved state before reconnecting; configuration was not changed"
        )
    }

    let Some(url) = prompt(
        input,
        output,
        "Server URL (credential-free modern HTTP endpoint): ",
    )?
    else {
        return cancelled(
            output,
            "Connection setup cancelled; configuration unchanged.",
        );
    };
    if url.is_empty() {
        return cancelled(
            output,
            "Connection setup cancelled; configuration unchanged.",
        );
    }
    let (url, origin) = validate_guided_url(&url)?;

    let auth_input = prompt(input, output, "Authentication (none or api-key) [none]: ")?;
    let auth = match auth_input
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "none" => McpAuth::None,
        "api-key" | "apikey" => {
            let Some(entry) = prompt(input, output, "Vault entry name: ")? else {
                return cancelled(
                    output,
                    "Connection setup cancelled; configuration unchanged.",
                );
            };
            if entry.is_empty() {
                return cancelled(
                    output,
                    "Connection setup cancelled; configuration unchanged.",
                );
            }
            validate_guided_vault_entry(&entry)?;
            reject_shaped_input("vault entry name", &entry)?;
            McpAuth::ApiKey { vault_entry: entry }
        }
        "cancel" | "q" | "quit" => {
            return cancelled(
                output,
                "Connection setup cancelled; configuration unchanged.",
            );
        }
        _ => anyhow::bail!(
            "unsupported authentication choice; use none or api-key; configuration was not changed"
        ),
    };
    let config = McpServerConfig {
        name: name.clone(),
        url: url.clone(),
        spec: nh_tools::MCP_SPEC_VERSION.into(),
        auth,
        scopes: Vec::new(),
        default_mode: None,
        trust: McpTrust::Ask,
    };
    let table = nh_tools::render_mcp_server_config(&config)?;
    let replacement = append_server_table(prior_config.as_deref(), &table);
    if replacement.len() > nh_tools::MAX_MCP_CONFIG_BYTES {
        anyhow::bail!(
            "MCP configuration would exceed the {} byte limit; existing configuration was not changed",
            nh_tools::MAX_MCP_CONFIG_BYTES
        )
    }
    let validated = nh_tools::load_mcp_config(&replacement)?;
    if validated.iter().filter(|item| item.name == name).count() != 1 {
        anyhow::bail!("could not validate the new MCP server; configuration was not changed")
    }

    writeln!(output)?;
    writeln!(output, "Server: {}", super::safe(&scrubber, &name))?;
    writeln!(output, "Destination: {}", super::safe(&scrubber, &url))?;
    writeln!(output, "Protocol: {}", nh_tools::MCP_SPEC_VERSION)?;
    match &config.auth {
        McpAuth::None => writeln!(output, "Authentication: none")?,
        McpAuth::ApiKey { vault_entry } => writeln!(
            output,
            "Authentication: API key from vault entry {}",
            super::safe(&scrubber, vault_entry)
        )?,
        McpAuth::OAuth2 { .. } => unreachable!("guided connect does not configure OAuth"),
    }
    writeln!(output, "Trust: ask for each remote action")?;
    writeln!(output, "Enabled tools: 0 until an explicit review")?;
    write!(
        output,
        "Save this connection with zero enabled tools? [y/N] "
    )?;
    output.flush()?;
    if !read_yes(input)? {
        return cancelled(
            output,
            "Connection setup cancelled; configuration unchanged.",
        );
    }

    before_review_recheck()?;
    let (late_review, _) = super::read_prior_review(home, &name)?;
    if late_review.is_some() {
        anyhow::bail!(
            "saved review state appeared while connection setup was open; configuration was not changed"
        )
    }

    let file = PrivateStateFile::open(
        home,
        None,
        "mcp.toml",
        CONFIG_STEM,
        nh_tools::MAX_MCP_CONFIG_BYTES,
        true,
    )?
    .expect("configuration directory was created");
    file.publish(prior_config.as_deref(), &replacement)?;
    writeln!(
        output,
        "Saved MCP configuration. No server or model request was sent."
    )?;
    print_connect_next_steps(
        output, &scrubber, home, executable, policy, &config, &origin,
    )
}

fn status_with_io(
    root: &Path,
    home: &Path,
    policy: &Policy,
    executable: &Path,
    requested: Option<&str>,
    output: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
    let scrubber = nh_vault::Scrubber::new(Vec::new());
    let (_, configs) = read_user_configs(home)?;
    let mut warnings = Vec::new();
    let effective =
        crate::cmd_run::load_and_vet_mcp_configs(root, Some(home), policy, &mut warnings);
    writeln!(
        output,
        "MCP integration status (offline; connectivity was not checked)"
    )?;

    if let Some(name) = requested {
        let Some(config) = configs.iter().find(|config| config.name == name) else {
            return print_unconfigured_status(home, executable, name, output, &scrubber);
        };
        print_server_status(
            home, policy, executable, config, &effective, output, &scrubber,
        )?;
        return Ok(());
    }
    if configs.is_empty() {
        writeln!(output, "No operator-owned MCP servers are configured.")?;
        writeln!(
            output,
            "Next: {}",
            super::safe(
                &scrubber,
                &crate::cmd_setup::command_line(executable, &["mcp", "connect"])
            )
        )?;
        return Ok(());
    }
    for (index, config) in configs.iter().enumerate() {
        if index > 0 {
            writeln!(output)?;
        }
        print_server_status(
            home, policy, executable, config, &effective, output, &scrubber,
        )?;
    }
    Ok(())
}

fn print_server_status(
    home: &Path,
    policy: &Policy,
    executable: &Path,
    config: &McpServerConfig,
    effective: &[McpServerConfig],
    output: &mut dyn std::io::Write,
    scrubber: &nh_vault::Scrubber,
) -> anyhow::Result<()> {
    print_server_status_with_presence(
        ServerStatusContext {
            home,
            policy,
            executable,
            effective,
            scrubber,
            presence: &credential_presence,
        },
        config,
        output,
    )
}

struct ServerStatusContext<'a> {
    home: &'a Path,
    policy: &'a Policy,
    executable: &'a Path,
    effective: &'a [McpServerConfig],
    scrubber: &'a nh_vault::Scrubber,
    presence: &'a dyn Fn(&McpServerConfig) -> CredentialPresence,
}

fn print_server_status_with_presence(
    context: ServerStatusContext<'_>,
    config: &McpServerConfig,
    output: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
    let ServerStatusContext {
        home,
        policy,
        executable,
        effective,
        scrubber,
        presence,
    } = context;
    writeln!(output, "Server: {}", super::safe(scrubber, &config.name))?;
    let display_url = displayable_url(&config.url);
    match &display_url {
        Some(url) => writeln!(output, "Destination: {}", super::safe(scrubber, url))?,
        None => writeln!(
            output,
            "Destination: hidden because the URL contains unsafe components; repair ~/.nosis/mcp.toml"
        )?,
    }
    writeln!(
        output,
        "Authentication: {} | trust: {}",
        auth_status_label(config, scrubber),
        super::trust_label(config.trust)
    )?;
    let effective_config = effective.iter().find(|item| item.name == config.name);
    let canonical_destination = display_url.as_deref();
    let unsafe_url = canonical_destination.is_none();
    let audience_status = canonical_destination.map_or(CredentialAudience::Ready, |destination| {
        credential_audience_status(config, policy, destination)
    });
    let missing_audience = match &audience_status {
        CredentialAudience::Missing { entry, origin } => Some((*entry, origin.clone())),
        CredentialAudience::Ready | CredentialAudience::Invalid => None,
    };
    let invalid_auth = matches!(audience_status, CredentialAudience::Invalid);
    let send_blocked = canonical_destination.is_some_and(|destination| {
        nh_vault::host_of(destination)
            .is_none_or(|host| matches!(policy.send_verdict(&host), Verdict::Block(_)))
    });
    let trust_blocked = config.trust == McpTrust::Block
        || effective_config.is_some_and(|item| item.trust == McpTrust::Block);
    let effective_refused = effective_config.is_none() && missing_audience.is_none();
    let policy_ready = !unsafe_url
        && !invalid_auth
        && missing_audience.is_none()
        && !trust_blocked
        && !send_blocked
        && !effective_refused;
    let credential = policy_ready.then(|| presence(config));
    let review_name_valid = super::validate_state_name(&config.name).is_ok();
    let review = if review_name_valid {
        Some(super::read_prior_review(home, &config.name)?)
    } else {
        None
    };
    let (state_label, enabled, next_action) = match review {
        None => ("review state unavailable for this server name", 0, "review"),
        Some((None, _)) => ("not reviewed; tools disabled", 0, "review"),
        Some((Some(_), None)) => ("review state malformed; tools disabled", 0, "review"),
        Some((Some(_), Some(ref state))) if !state.connection_matches(config) => {
            ("configuration changed; tools disabled", 0, "review")
        }
        Some((Some(_), Some(ref state))) if state.enabled.is_empty() => {
            ("disabled; zero tools enabled", 0, "review")
        }
        Some((Some(_), Some(ref state))) => (
            "ready locally; connectivity not checked",
            state.enabled.len(),
            "check",
        ),
    };
    if unsafe_url {
        writeln!(
            output,
            "State: configuration URL must be repaired before review or use"
        )?;
    } else if invalid_auth {
        writeln!(
            output,
            "State: authentication settings or credential destination transport must be repaired before review or use"
        )?;
        writeln!(
            output,
            "Next: inspect ~/.nosis/mcp.toml; tool review cannot repair authentication settings"
        )?;
    } else if let Some((entry, origin)) = &missing_audience {
        writeln!(
            output,
            "State: credential is not approved for the exact destination"
        )?;
        print_audience_repair(output, scrubber, home, entry, origin)?;
    } else if trust_blocked || send_blocked || effective_refused {
        writeln!(
            output,
            "State: policy or effective configuration blocks this server"
        )?;
        writeln!(
            output,
            "Next: review user law and repository restrictions; changing tool review cannot remove this block"
        )?;
    } else if let Some(CredentialPresence::Missing(entry)) = &credential {
        writeln!(
            output,
            "State: credential is missing; tools remain unavailable"
        )?;
        writeln!(
            output,
            "Next: {}",
            super::safe(
                scrubber,
                &crate::cmd_setup::command_line(executable, &["key", "add", "--", entry])
            )
        )?;
    } else if matches!(&credential, Some(CredentialPresence::Unavailable)) {
        writeln!(
            output,
            "State: credential presence could not be checked; local readiness is unknown"
        )?;
        writeln!(
            output,
            "Next: unlock or repair the OS credential store, then run status again"
        )?;
    } else {
        writeln!(output, "State: {state_label}")?;
    }
    let prerequisites_ready =
        policy_ready && matches!(&credential, Some(CredentialPresence::Ready));
    if prerequisites_ready {
        writeln!(output, "Enabled reviewed tools: {enabled}")?;
    } else {
        writeln!(
            output,
            "Enabled reviewed tools: {enabled} (inactive until the prerequisites above are repaired)"
        )?;
    }
    if prerequisites_ready {
        if !review_name_valid {
            writeln!(
                output,
                "Next: rename this server in ~/.nosis/mcp.toml using only ASCII letters, digits, underscores, or hyphens before review"
            )?;
        } else {
            let command = if next_action == "check" {
                server_command(executable, "check", &config.name)
            } else {
                server_command(executable, "review", &config.name)
            };
            writeln!(output, "Next: {}", super::safe(scrubber, &command))?;
        }
    }
    Ok(())
}

fn print_unconfigured_status(
    home: &Path,
    executable: &Path,
    name: &str,
    output: &mut dyn std::io::Write,
    scrubber: &nh_vault::Scrubber,
) -> anyhow::Result<()> {
    writeln!(output, "Server: {}", super::safe(scrubber, name))?;
    let saved = if super::validate_state_name(name).is_ok() {
        Some(super::read_prior_review(home, name)?)
    } else {
        None
    };
    writeln!(output, "State: unconfigured; no destination is active")?;
    if let Some((Some(_), state)) = saved {
        if let Some(state) = state {
            writeln!(
                output,
                "Saved review: {} enabled tool(s), inactive without configuration",
                state.enabled.len()
            )?;
        } else {
            writeln!(output, "Saved review: malformed and inactive")?;
        }
        let path = home
            .join(".nosis")
            .join("mcp-reviews")
            .join(format!("{name}.json"));
        writeln!(
            output,
            "Next: inspect and deliberately remove {} before reconnecting under this name, or choose another name",
            super::safe(scrubber, &path.display().to_string())
        )?;
    } else {
        writeln!(
            output,
            "Next: {}",
            super::safe(
                scrubber,
                &crate::cmd_setup::command_line(executable, &["mcp", "connect"])
            )
        )?;
    }
    Ok(())
}

fn check_with_io(
    root: &Path,
    home: &Path,
    policy: &Policy,
    executable: &Path,
    server: &str,
    output: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
    let (_, raw_configs) = read_user_configs(home)?;
    if !raw_configs.iter().any(|config| config.name == server) {
        anyhow::bail!(
            "MCP server is not configured in ~/.nosis/mcp.toml; no network request was sent"
        )
    }
    let (prior_text, prior) = super::read_prior_review(home, server)?;
    let mut warnings = Vec::new();
    let configs = crate::cmd_run::load_and_vet_mcp_configs(root, Some(home), policy, &mut warnings);
    let config = configs
        .iter()
        .find(|config| config.name == server)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "MCP server is blocked or unavailable in the effective trusted configuration; no network request was sent"
            )
        })?;
    if config.trust == McpTrust::Block {
        anyhow::bail!("MCP server is blocked by effective trust; no network request was sent")
    }
    let host = nh_vault::host_of(&config.url)
        .ok_or_else(|| anyhow::anyhow!("MCP server URL has no reviewable host"))?;
    if matches!(policy.send_verdict(&host), Verdict::Block(_)) {
        anyhow::bail!("MCP server destination is blocked by law; no network request was sent")
    }

    let snapshot = nh_tools::inspect_mcp_server(config)?;
    print_check_result(output, executable, &snapshot, prior.as_ref())?;
    if prior_text.is_some() {
        writeln!(output, "Saved review state was not changed.")?;
    } else {
        writeln!(output, "No review state was created.")?;
    }
    Ok(())
}

fn print_check_result(
    output: &mut dyn std::io::Write,
    executable: &Path,
    snapshot: &McpReviewSnapshot,
    prior: Option<&McpReviewState>,
) -> anyhow::Result<()> {
    let scrubber = nh_vault::Scrubber::new(Vec::new());
    let current = snapshot
        .tools
        .iter()
        .map(|tool| tool.name())
        .collect::<BTreeSet<_>>();
    let mut new = 0_usize;
    let mut changed = 0_usize;
    let mut unchanged = 0_usize;
    for tool in &snapshot.tools {
        match prior.and_then(|state| state.tool(tool.name())) {
            None => new += 1,
            Some(saved) if saved.fingerprint != tool.fingerprint => changed += 1,
            Some(_) => unchanged += 1,
        }
    }
    let removed = prior.map_or(0, |state| {
        state
            .snapshot
            .iter()
            .filter(|tool| !current.contains(tool.name()))
            .count()
    });
    let connection_changed =
        prior.is_some_and(|state| state.connection_fingerprint != snapshot.connection_fingerprint);
    writeln!(
        output,
        "Checked metadata for {}. No tool was invoked.",
        super::safe(&scrubber, &snapshot.server)
    )?;
    writeln!(
        output,
        "Tools: {} current; {new} new; {changed} changed; {unchanged} unchanged; {removed} removed; {} omitted as unreviewable.",
        snapshot.tools.len(),
        snapshot.omitted_tools
    )?;
    writeln!(
        output,
        "Connection settings changed: {}",
        if connection_changed { "yes" } else { "no" }
    )?;
    writeln!(
        output,
        "Review before use: {}",
        super::safe(
            &scrubber,
            &server_command(executable, "review", &snapshot.server)
        )
    )?;
    Ok(())
}

fn disable_with_io(
    home: &Path,
    server: &str,
    input: &mut dyn BufRead,
    output: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
    let (prior_text, prior) = super::read_prior_review(home, server)?;
    let Some(prior_text) = prior_text else {
        writeln!(
            output,
            "No saved MCP review exists; no tools are enabled for this server."
        )?;
        return Ok(());
    };
    let Some(mut state) = prior else {
        anyhow::bail!(
            "saved MCP review state is malformed; inspect or remove it deliberately; review state was not changed"
        )
    };
    if state.enabled.is_empty() {
        writeln!(
            output,
            "All reviewed tools are already disabled for this server."
        )?;
        return Ok(());
    }
    state.enabled.clear();
    let replacement = state.to_compact_json()?;
    let scrubber = nh_vault::Scrubber::new(Vec::new());
    write!(
        output,
        "Disable all reviewed tools for {}? [y/N] ",
        super::safe(&scrubber, server)
    )?;
    output.flush()?;
    if !read_yes(input)? {
        return cancelled(output, "Disable cancelled; review state unchanged.");
    }
    let filename = format!("{server}.json");
    let file = PrivateStateFile::open(
        home,
        Some("mcp-reviews"),
        &filename,
        server,
        nh_tools::MAX_MCP_REVIEW_BYTES,
        false,
    )?
    .ok_or_else(|| anyhow::anyhow!("saved MCP review disappeared; nothing was changed"))?;
    file.publish(Some(&prior_text), &replacement)?;
    writeln!(output, "Disabled all reviewed tools for this server.")?;
    writeln!(
        output,
        "Already running chat or TUI sessions keep their startup tools; start a new session to apply this change."
    )?;
    Ok(())
}

fn read_user_configs(home: &Path) -> anyhow::Result<(Option<String>, Vec<McpServerConfig>)> {
    let path = home.join(".nosis").join("mcp.toml");
    let Some(file) = PrivateStateFile::open(
        home,
        None,
        "mcp.toml",
        CONFIG_STEM,
        nh_tools::MAX_MCP_CONFIG_BYTES,
        false,
    )
    .map_err(|_| {
        anyhow::anyhow!(
            "refused MCP configuration at {}; inspect that path before retrying",
            path.display()
        )
    })?
    else {
        return Ok((None, Vec::new()));
    };
    match file.read().map_err(|_| {
        anyhow::anyhow!(
            "refused MCP configuration at {}; inspect that path before retrying",
            path.display()
        )
    })? {
        PrivateStateRead::Absent => Ok((None, Vec::new())),
        PrivateStateRead::Interrupted => anyhow::bail!(
            "MCP configuration update is incomplete near {}; inspect and resolve the recovery files before retrying",
            path.display()
        ),
        PrivateStateRead::Text(text) => {
            let configs = nh_tools::load_mcp_config(&text)?;
            Ok((Some(text), configs))
        }
    }
}

fn append_server_table(existing: Option<&str>, table: &str) -> String {
    let mut replacement = existing.unwrap_or_default().to_string();
    if !replacement.is_empty() {
        if !replacement.ends_with('\n') {
            replacement.push('\n');
        }
        replacement.push('\n');
    }
    replacement.push_str(table);
    replacement
}

fn print_connect_next_steps(
    output: &mut dyn std::io::Write,
    scrubber: &nh_vault::Scrubber,
    home: &Path,
    executable: &Path,
    policy: &Policy,
    config: &McpServerConfig,
    origin: &str,
) -> anyhow::Result<()> {
    if let McpAuth::ApiKey { vault_entry } = &config.auth {
        writeln!(output, "Add the API key in a hidden prompt:")?;
        writeln!(
            output,
            "{}",
            super::safe(
                scrubber,
                &crate::cmd_setup::command_line(executable, &["key", "add", "--", vault_entry])
            )
        )?;
        if !nh_vault::audience_allows(origin, &policy.approved_audiences(vault_entry)) {
            print_audience_repair(output, scrubber, home, vault_entry, origin)?;
        }
    }
    writeln!(output, "Review metadata and explicitly select tools:")?;
    writeln!(
        output,
        "{}",
        super::safe(
            scrubber,
            &server_command(executable, "review", &config.name)
        )
    )?;
    Ok(())
}

fn server_command(executable: &Path, action: &str, server: &str) -> String {
    crate::cmd_setup::command_line(executable, &["mcp", action, "--", server])
}

fn print_audience_repair(
    output: &mut dyn std::io::Write,
    scrubber: &nh_vault::Scrubber,
    home: &Path,
    entry: &str,
    origin: &str,
) -> anyhow::Result<()> {
    writeln!(
        output,
        "In {}, merge this exact-origin credential permission:",
        super::safe(
            scrubber,
            &home.join(".nosis").join("law.toml").display().to_string()
        )
    )?;
    writeln!(
        output,
        "[credential.\"{}\"]\naudience = [\"{}\"]",
        super::safe(scrubber, entry),
        super::safe(scrubber, origin)
    )?;
    Ok(())
}

enum CredentialAudience<'a> {
    Ready,
    Missing { entry: &'a str, origin: String },
    Invalid,
}

fn credential_audience_status<'a>(
    config: &'a McpServerConfig,
    policy: &Policy,
    canonical_destination: &str,
) -> CredentialAudience<'a> {
    let entry = match &config.auth {
        McpAuth::None => return CredentialAudience::Ready,
        McpAuth::ApiKey { vault_entry } | McpAuth::OAuth2 { vault_entry, .. } => vault_entry,
    };
    if validate_guided_vault_entry(entry).is_err() {
        return CredentialAudience::Invalid;
    }
    let check_target = |target: &str| {
        let Ok(target) = nh_tools::guided_mcp_url(target) else {
            return CredentialAudience::Invalid;
        };
        let Some(origin) = nh_vault::normalized_origin(&target) else {
            return CredentialAudience::Invalid;
        };
        if nh_vault::audience_allows(&target, &policy.approved_audiences(entry)) {
            CredentialAudience::Ready
        } else {
            CredentialAudience::Missing { entry, origin }
        }
    };
    match check_target(canonical_destination) {
        CredentialAudience::Ready => match &config.auth {
            McpAuth::OAuth2 { token_url, .. } => check_target(token_url),
            McpAuth::None | McpAuth::ApiKey { .. } => CredentialAudience::Ready,
        },
        other => other,
    }
}

#[derive(Clone)]
enum CredentialPresence {
    Ready,
    Missing(String),
    Unavailable,
}

fn credential_presence(config: &McpServerConfig) -> CredentialPresence {
    let entries = match &config.auth {
        McpAuth::None => return CredentialPresence::Ready,
        McpAuth::ApiKey { vault_entry } => vec![vault_entry.clone()],
        McpAuth::OAuth2 { vault_entry, .. } => {
            vec![
                format!("{vault_entry}-refresh"),
                format!("{vault_entry}-secret"),
            ]
        }
    };
    for entry in entries {
        match credential_entry_present(&entry) {
            Ok(true) => {}
            Ok(false) => return CredentialPresence::Missing(entry),
            Err(()) => return CredentialPresence::Unavailable,
        }
    }
    CredentialPresence::Ready
}

fn credential_entry_present(entry: &str) -> Result<bool, ()> {
    let vault = nh_vault::EnvFallbackVault {
        inner: nh_vault::KeyringVault,
    };
    match vault.entry_presence(entry) {
        nh_vault::EntryPresence::Present => Ok(true),
        nh_vault::EntryPresence::Absent => Ok(false),
        nh_vault::EntryPresence::StoreUnavailable => Err(()),
    }
}

fn displayable_url(raw: &str) -> Option<String> {
    let scrubber = nh_vault::Scrubber::new(Vec::new());
    let reviewed = nh_tools::guided_mcp_url(raw).ok()?;
    (scrubber.scrub(&reviewed) == reviewed).then_some(reviewed)
}

fn validate_guided_name(name: &str) -> anyhow::Result<()> {
    if name.len() > MAX_GUIDED_NAME_BYTES || name.starts_with('-') {
        anyhow::bail!(
            "MCP server name must be 1-64 ASCII letters, digits, underscores, or hyphens and cannot start with a hyphen"
        )
    }
    super::validate_state_name(name).map_err(|_| {
        anyhow::anyhow!(
            "MCP server name must be 1-64 ASCII letters, digits, underscores, or hyphens and cannot start with a hyphen"
        )
    })
}

fn validate_guided_vault_entry(entry: &str) -> anyhow::Result<()> {
    if entry.len() > 128
        || entry.starts_with('-')
        || !entry
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        anyhow::bail!(
            "vault entry name must use at most 128 ASCII letters, digits, dots, underscores, or hyphens and cannot start with a hyphen"
        )
    }
    Ok(())
}

fn validate_guided_url(raw: &str) -> anyhow::Result<(String, String)> {
    if raw.len() > MAX_GUIDED_URL_BYTES {
        anyhow::bail!("MCP server URL is too long; configuration was not changed")
    }
    reject_shaped_input("MCP server URL", raw)?;
    let reviewed = nh_tools::guided_mcp_url(raw).map_err(|_| {
        anyhow::anyhow!(
            "MCP server URL must be an explicit HTTP(S) URL with a host and omit credentials, query, and fragment"
        )
    })?;
    let origin = nh_vault::normalized_origin(&reviewed).ok_or_else(|| {
        anyhow::anyhow!("MCP server URL must use HTTPS, or HTTP with a literal loopback host")
    })?;
    let host = nh_vault::host_of(&reviewed)
        .ok_or_else(|| anyhow::anyhow!("MCP server URL must include a host"))?;
    if nh_vault::is_link_local_or_metadata(&host) {
        anyhow::bail!("literal link-local and metadata destinations are not supported")
    }
    Ok((reviewed, origin))
}

fn reject_shaped_input(label: &str, value: &str) -> anyhow::Result<()> {
    let scrubber = nh_vault::Scrubber::new(Vec::new());
    if scrubber.scrub(value) != value {
        anyhow::bail!("{label} looks like a credential; configuration was not changed")
    }
    Ok(())
}

fn auth_status_label(config: &McpServerConfig, scrubber: &nh_vault::Scrubber) -> String {
    match &config.auth {
        McpAuth::None => "none".into(),
        McpAuth::ApiKey { vault_entry } => {
            format!("API key selector {}", super::safe(scrubber, vault_entry))
        }
        McpAuth::OAuth2 { vault_entry, .. } => {
            format!("OAuth 2 selector {}", super::safe(scrubber, vault_entry))
        }
    }
}

fn require_interactive_mutation(
    action: &str,
    input_terminal: bool,
    output_terminal: bool,
) -> anyhow::Result<()> {
    if !input_terminal || !output_terminal {
        anyhow::bail!(
            "MCP {action} needs an interactive terminal; no configuration or review state was changed"
        )
    }
    Ok(())
}

fn prompt(
    input: &mut dyn BufRead,
    output: &mut dyn std::io::Write,
    text: &str,
) -> anyhow::Result<Option<String>> {
    write!(output, "{text}")?;
    output.flush()?;
    read_bounded_line(input)
}

fn read_yes(input: &mut dyn BufRead) -> anyhow::Result<bool> {
    Ok(read_bounded_line(input)?
        .is_some_and(|line| matches!(line.to_ascii_lowercase().as_str(), "y" | "yes")))
}

fn read_bounded_line(input: &mut dyn BufRead) -> anyhow::Result<Option<String>> {
    let mut bytes = Vec::new();
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return Ok(None);
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let content_len = newline.unwrap_or(available.len());
        if bytes.len().saturating_add(content_len) > MAX_GUIDED_INPUT_BYTES {
            anyhow::bail!(
                "MCP input exceeds the {} byte limit; no files were changed",
                MAX_GUIDED_INPUT_BYTES
            )
        }
        bytes.extend_from_slice(&available[..content_len]);
        let consumed = newline.map_or(available.len(), |index| index + 1);
        input.consume(consumed);
        if newline.is_some() {
            while matches!(bytes.last(), Some(b'\r')) {
                bytes.pop();
            }
            let line = String::from_utf8(bytes)?;
            return Ok(Some(line.trim().to_string()));
        }
    }
}

fn cancelled(output: &mut dyn std::io::Write, message: &str) -> anyhow::Result<()> {
    writeln!(output, "{message}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::{Cursor, Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    fn test_policy(root: &Path) -> Policy {
        nh_law::load(root, &nh_law::LoadOptions { cli_autonomy: None }).policy
    }

    fn executable() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            r"C:\Program Files\Nosis Harness\nh.exe"
        } else {
            "/opt/nosis harness/nh"
        })
    }

    fn read_rpc_request(stream: &mut TcpStream) -> Value {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 4096];
        let header_end = loop {
            if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                break index;
            }
            let read = stream.read(&mut chunk).unwrap();
            assert_ne!(read, 0);
            bytes.extend_from_slice(&chunk[..read]);
        };
        let header = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = header
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
        let mut body = bytes[header_end + 4..].to_vec();
        while body.len() < content_length {
            let read = stream.read(&mut chunk).unwrap();
            assert_ne!(read, 0);
            body.extend_from_slice(&chunk[..read]);
        }
        serde_json::from_slice(&body[..content_length]).unwrap()
    }

    fn metadata_peer(
        description: &'static str,
    ) -> (
        McpServerConfig,
        Arc<Mutex<Vec<String>>>,
        std::thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let methods = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&methods);
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_rpc_request(&mut stream);
            recorded
                .lock()
                .unwrap()
                .push(request["method"].as_str().unwrap().to_string());
            let body = json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": {
                    "tools": [{
                        "name": "lookup",
                        "description": description,
                        "inputSchema": {"type": "object"},
                        "annotations": {"readOnlyHint": true}
                    }]
                }
            })
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        (
            McpServerConfig {
                name: "fixture".into(),
                url: format!("http://{address}/mcp"),
                spec: nh_tools::MCP_SPEC_VERSION.into(),
                auth: McpAuth::None,
                scopes: Vec::new(),
                default_mode: None,
                trust: McpTrust::Ask,
            },
            methods,
            peer,
        )
    }

    fn nonconforming_peer() -> (McpServerConfig, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_rpc_request(&mut stream);
            let body = json!({
                "jsonrpc": "2.0",
                "id": request["id"].as_u64().unwrap() + 1,
                "result": {"tools": []}
            })
            .to_string();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        (
            McpServerConfig {
                name: "fixture".into(),
                url: format!("http://{address}/mcp"),
                spec: nh_tools::MCP_SPEC_VERSION.into(),
                auth: McpAuth::None,
                scopes: Vec::new(),
                default_mode: None,
                trust: McpTrust::Ask,
            },
            peer,
        )
    }

    fn write_config(home: &Path, config: &McpServerConfig) -> String {
        let text = nh_tools::render_mcp_server_config(config).unwrap();
        std::fs::create_dir_all(home.join(".nosis")).unwrap();
        std::fs::write(home.join(".nosis/mcp.toml"), &text).unwrap();
        text
    }

    fn save_review(home: &Path, state: &McpReviewState) -> String {
        let text = state.to_compact_json().unwrap();
        std::fs::create_dir_all(home.join(".nosis/mcp-reviews")).unwrap();
        std::fs::write(home.join(".nosis/mcp-reviews/fixture.json"), &text).unwrap();
        text
    }

    #[test]
    fn guided_connect_cancels_without_changes_then_appends_exact_existing_bytes() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".nosis")).unwrap();
        let original = "# keep this comment\n[servers.other]\nurl = \"http://127.0.0.1:9/mcp\"\n";
        std::fs::write(home.path().join(".nosis/mcp.toml"), original).unwrap();
        let policy = test_policy(root.path());

        let mut cancel = Cursor::new(b"fixture\nhttp://127.0.0.1:8765/mcp\nnone\nn\n".to_vec());
        let mut output = Vec::new();
        connect_with_io(
            home.path(),
            &policy,
            &executable(),
            &mut cancel,
            &mut output,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(home.path().join(".nosis/mcp.toml")).unwrap(),
            original
        );

        let mut save = Cursor::new(b"fixture\nhttp://127.0.0.1:8765/mcp\nnone\ny\n".to_vec());
        output.clear();
        connect_with_io(home.path(), &policy, &executable(), &mut save, &mut output).unwrap();
        let saved = std::fs::read_to_string(home.path().join(".nosis/mcp.toml")).unwrap();
        assert!(saved.starts_with(original));
        let parsed = nh_tools::load_mcp_config(&saved).unwrap();
        assert_eq!(
            parsed.iter().filter(|item| item.name == "fixture").count(),
            1
        );
        assert!(!home.path().join(".nosis/mcp-reviews/fixture.json").exists());
        let output = String::from_utf8(output).unwrap();
        assert!(
            output.contains("No server or model request was sent."),
            "{output}"
        );
        assert!(output.contains("zero enabled tools"), "{output}");

        let api_home = tempfile::tempdir().unwrap();
        let mut api = Cursor::new(
            b"api_server\nhttps://mcp.example.invalid/service\napi-key\nmy.integration\ny\n"
                .to_vec(),
        );
        let mut api_output = Vec::new();
        connect_with_io(
            api_home.path(),
            &policy,
            &executable(),
            &mut api,
            &mut api_output,
        )
        .unwrap();
        let api_saved = std::fs::read_to_string(api_home.path().join(".nosis/mcp.toml")).unwrap();
        let api_config = nh_tools::load_mcp_config(&api_saved).unwrap();
        assert_eq!(
            api_config[0].auth,
            McpAuth::ApiKey {
                vault_entry: "my.integration".into()
            }
        );
        assert!(!api_home.path().join(".nosis/law.toml").exists());
        let api_output = String::from_utf8(api_output).unwrap();
        assert!(api_output.contains("key"), "{api_output}");
        assert!(
            api_output.contains("[credential.\"my.integration\"]"),
            "{api_output}"
        );
        assert!(
            api_output.contains("https://mcp.example.invalid:443"),
            "{api_output}"
        );
    }

    #[test]
    fn guided_connect_rejects_opaque_or_schemeless_urls_and_stores_canonical_http_url() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let policy = test_policy(root.path());
        for (index, invalid) in [
            "alice:hunter2@example.invalid/mcp",
            "localhost:8765/mcp",
            "mailto:x@example.invalid",
            "https:example.invalid/mcp",
            "https:/example.invalid/mcp",
            "http:/203.0.113.5/mcp",
            "https:\t//example.invalid/mcp",
        ]
        .into_iter()
        .enumerate()
        {
            let mut input = Cursor::new(format!("invalid{index}\n{invalid}\n").into_bytes());
            let error = connect_with_io(
                home.path(),
                &policy,
                &executable(),
                &mut input,
                &mut Vec::new(),
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("explicit HTTP(S) URL"), "{error}");
            assert!(!error.contains("hunter2"), "{error}");
            assert!(!home.path().join(".nosis/mcp.toml").exists());
        }

        let mut input =
            Cursor::new(b"canonical\nhttps://EXAMPLE.invalid:443/m\tcp\nnone\ny\n".to_vec());
        connect_with_io(
            home.path(),
            &policy,
            &executable(),
            &mut input,
            &mut Vec::new(),
        )
        .unwrap();
        let saved = std::fs::read_to_string(home.path().join(".nosis/mcp.toml")).unwrap();
        let parsed = nh_tools::load_mcp_config(&saved).unwrap();
        assert_eq!(parsed[0].url, "https://example.invalid/mcp");
        assert!(!saved.contains('\t'));
    }

    #[test]
    fn guided_connect_refuses_leading_dash_secret_shape_and_orphan_review() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let policy = test_policy(root.path());
        for name in [
            "-leading".to_string(),
            format!("{}{}", "sk-", "abcdefghijklmnopqrstuvwxyz123456"),
        ] {
            let mut input = Cursor::new(format!("{name}\n").into_bytes());
            let error = connect_with_io(
                home.path(),
                &policy,
                &executable(),
                &mut input,
                &mut Vec::new(),
            )
            .unwrap_err()
            .to_string();
            assert!(
                error.contains("cannot start") || error.contains("looks like a credential"),
                "{error}"
            );
        }
        let review_dir = home.path().join(".nosis/mcp-reviews");
        std::fs::create_dir_all(&review_dir).unwrap();
        std::fs::write(review_dir.join("orphan.json"), "malformed but preserved").unwrap();
        let mut orphan = Cursor::new(b"orphan\n".to_vec());
        let error = connect_with_io(
            home.path(),
            &policy,
            &executable(),
            &mut orphan,
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("review state already exists"), "{error}");
        assert_eq!(
            std::fs::read_to_string(review_dir.join("orphan.json")).unwrap(),
            "malformed but preserved"
        );
        assert!(!home.path().join(".nosis/mcp.toml").exists());

        let race_home = tempfile::tempdir().unwrap();
        let review_dir = race_home.path().join(".nosis/mcp-reviews");
        let mut confirmed = Cursor::new(b"late\nhttp://127.0.0.1:8765/mcp\nnone\ny\n".to_vec());
        let error = connect_with_io_before_publish(
            race_home.path(),
            &policy,
            &executable(),
            &mut confirmed,
            &mut Vec::new(),
            || {
                std::fs::create_dir_all(&review_dir)?;
                std::fs::write(review_dir.join("late.json"), "late competing state")?;
                Ok(())
            },
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("appeared while connection setup was open"),
            "{error}"
        );
        assert!(!race_home.path().join(".nosis/mcp.toml").exists());
        assert_eq!(
            std::fs::read_to_string(review_dir.join("late.json")).unwrap(),
            "late competing state"
        );
    }

    #[test]
    fn guided_connect_conflict_malformed_and_oversize_failures_preserve_prior_bytes() {
        let root = tempfile::tempdir().unwrap();
        let policy = test_policy(root.path());

        let conflict_home = tempfile::tempdir().unwrap();
        let conflict_config = McpServerConfig {
            name: "fixture".into(),
            url: "http://127.0.0.1:8765/mcp".into(),
            spec: nh_tools::MCP_SPEC_VERSION.into(),
            auth: McpAuth::None,
            scopes: Vec::new(),
            default_mode: None,
            trust: McpTrust::Ask,
        };
        let conflict_prior = write_config(conflict_home.path(), &conflict_config);
        let error = connect_with_io(
            conflict_home.path(),
            &policy,
            &executable(),
            &mut Cursor::new(b"fixture\n".to_vec()),
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("already exists"), "{error}");
        assert_eq!(
            std::fs::read_to_string(conflict_home.path().join(".nosis/mcp.toml")).unwrap(),
            conflict_prior
        );

        let malformed_home = tempfile::tempdir().unwrap();
        let malformed_prior = "[servers.bad\nurl = \"unterminated";
        std::fs::create_dir_all(malformed_home.path().join(".nosis")).unwrap();
        std::fs::write(
            malformed_home.path().join(".nosis/mcp.toml"),
            malformed_prior,
        )
        .unwrap();
        assert!(connect_with_io(
            malformed_home.path(),
            &policy,
            &executable(),
            &mut Cursor::new(b"new_server\n".to_vec()),
            &mut Vec::new(),
        )
        .is_err());
        assert_eq!(
            std::fs::read_to_string(malformed_home.path().join(".nosis/mcp.toml")).unwrap(),
            malformed_prior
        );

        let oversize_home = tempfile::tempdir().unwrap();
        let oversize_prior = format!("#{}\n", "x".repeat(nh_tools::MAX_MCP_CONFIG_BYTES - 2));
        assert_eq!(oversize_prior.len(), nh_tools::MAX_MCP_CONFIG_BYTES);
        std::fs::create_dir_all(oversize_home.path().join(".nosis")).unwrap();
        std::fs::write(
            oversize_home.path().join(".nosis/mcp.toml"),
            &oversize_prior,
        )
        .unwrap();
        let error = connect_with_io(
            oversize_home.path(),
            &policy,
            &executable(),
            &mut Cursor::new(b"new_server\nhttp://127.0.0.1:8765/mcp\nnone\n".to_vec()),
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("would exceed"), "{error}");
        assert_eq!(
            std::fs::read_to_string(oversize_home.path().join(".nosis/mcp.toml")).unwrap(),
            oversize_prior
        );
    }

    #[test]
    fn offline_status_hides_unreviewable_urls_without_contacting_them() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".nosis")).unwrap();
        let text = r#"
[servers.userinfo]
url = "https://user:hidden-user@example.invalid/mcp"
[servers.query]
url = "https://example.invalid/mcp?hidden-query"
[servers.fragment]
url = "https://example.invalid/mcp#hidden-fragment"
[servers.opaque]
url = "alice:hunter2@example.invalid/mcp"
[servers.schemeless]
url = "localhost:8765/mcp"
[servers.mail]
url = "mailto:x@example.invalid"
[servers.special]
url = "https:example.invalid/mcp"
[servers.oneslash]
url = "https:/example.invalid/mcp"
[servers.http_oneslash]
url = "http:/203.0.113.5/mcp"
[servers.canonical]
url = "https://EXAMPLE.invalid:443/m\tcp"
"#;
        std::fs::write(home.path().join(".nosis/mcp.toml"), text).unwrap();
        let policy = test_policy(root.path());
        let mut output = Vec::new();

        status_with_io(
            root.path(),
            home.path(),
            &policy,
            &executable(),
            None,
            &mut output,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(
            output.contains("offline; connectivity was not checked"),
            "{output}"
        );
        assert_eq!(output.matches("unsafe components").count(), 9, "{output}");
        for hidden in [
            "hidden-user",
            "hidden-query",
            "hidden-fragment",
            "hunter2",
            "localhost:8765",
            "mailto:x",
            "203.0.113.5",
        ] {
            assert!(!output.contains(hidden), "leaked {hidden}: {output}");
        }
        assert!(
            output.contains("Destination: https://example.invalid/mcp"),
            "{output}"
        );
        assert!(!output.contains('\t'), "{output:?}");
    }

    #[test]
    fn offline_status_distinguishes_audience_key_store_policy_and_changed_review_states() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let policy = test_policy(root.path());
        let scrubber = nh_vault::Scrubber::new(Vec::new());

        let missing_audience = McpServerConfig {
            name: "fixture".into(),
            url: "https://mcp.example.invalid/mcp".into(),
            spec: nh_tools::MCP_SPEC_VERSION.into(),
            auth: McpAuth::ApiKey {
                vault_entry: "custom-entry".into(),
            },
            scopes: Vec::new(),
            default_mode: None,
            trust: McpTrust::Ask,
        };
        let mut output = Vec::new();
        print_server_status_with_presence(
            ServerStatusContext {
                home: home.path(),
                policy: &policy,
                executable: &executable(),
                effective: &[],
                scrubber: &scrubber,
                presence: &|_| -> CredentialPresence {
                    panic!("credential presence must not be queried before audience approval")
                },
            },
            &missing_audience,
            &mut output,
        )
        .unwrap();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("not approved for the exact destination"));
        assert!(
            rendered.contains("audience = [\"https://mcp.example.invalid:443\"]"),
            "{rendered}"
        );
        assert!(rendered.contains("inactive until the prerequisites"));

        for malformed in [
            "https:example.invalid/mcp",
            "https:/example.invalid/mcp",
            "http:/203.0.113.5/mcp",
        ] {
            let mut malformed_config = missing_audience.clone();
            malformed_config.url = malformed.into();
            let mut output = Vec::new();
            print_server_status_with_presence(
                ServerStatusContext {
                    home: home.path(),
                    policy: &policy,
                    executable: &executable(),
                    effective: std::slice::from_ref(&malformed_config),
                    scrubber: &scrubber,
                    presence: &|_| -> CredentialPresence {
                        panic!("malformed destinations must not trigger credential lookup")
                    },
                },
                &malformed_config,
                &mut output,
            )
            .unwrap();
            let rendered = String::from_utf8(output).unwrap();
            assert!(
                rendered.contains("configuration URL must be repaired"),
                "{rendered}"
            );
            assert!(!rendered.contains("credential permission"), "{rendered}");
            assert!(!rendered.contains("https://http:443"), "{rendered}");
        }

        let lan_no_auth = McpServerConfig {
            name: "lan".into(),
            url: "http://192.0.2.1/mcp".into(),
            spec: nh_tools::MCP_SPEC_VERSION.into(),
            auth: McpAuth::None,
            scopes: Vec::new(),
            default_mode: None,
            trust: McpTrust::Ask,
        };
        let mut output = Vec::new();
        print_server_status_with_presence(
            ServerStatusContext {
                home: home.path(),
                policy: &policy,
                executable: &executable(),
                effective: std::slice::from_ref(&lan_no_auth),
                scrubber: &scrubber,
                presence: &|_| CredentialPresence::Ready,
            },
            &lan_no_auth,
            &mut output,
        )
        .unwrap();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("Destination: http://192.0.2.1/mcp"));
        assert!(rendered.contains("not reviewed; tools disabled"));
        assert!(rendered.contains(&server_command(&executable(), "review", "lan")));
        assert!(!rendered.contains("configuration URL must be repaired"));

        let mut lan_with_key = lan_no_auth.clone();
        lan_with_key.auth = McpAuth::ApiKey {
            vault_entry: "deepseek".into(),
        };
        let mut output = Vec::new();
        print_server_status_with_presence(
            ServerStatusContext {
                home: home.path(),
                policy: &policy,
                executable: &executable(),
                effective: std::slice::from_ref(&lan_with_key),
                scrubber: &scrubber,
                presence: &|_| -> CredentialPresence {
                    panic!("insecure credential transport must not trigger credential lookup")
                },
            },
            &lan_with_key,
            &mut output,
        )
        .unwrap();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains(
            "authentication settings or credential destination transport must be repaired"
        ));
        assert!(!rendered.contains("credential permission"));
        assert!(!rendered.contains("<unreviewable destination>"));

        let key_config = McpServerConfig {
            name: "fixture".into(),
            url: "https://api.deepseek.com/mcp".into(),
            spec: nh_tools::MCP_SPEC_VERSION.into(),
            auth: McpAuth::ApiKey {
                vault_entry: "deepseek".into(),
            },
            scopes: Vec::new(),
            default_mode: None,
            trust: McpTrust::Ask,
        };
        for (presence, expected) in [
            (
                CredentialPresence::Missing("deepseek".into()),
                "credential is missing",
            ),
            (
                CredentialPresence::Unavailable,
                "credential presence could not be checked",
            ),
        ] {
            let mut output = Vec::new();
            print_server_status_with_presence(
                ServerStatusContext {
                    home: home.path(),
                    policy: &policy,
                    executable: &executable(),
                    effective: std::slice::from_ref(&key_config),
                    scrubber: &scrubber,
                    presence: &|_| presence.clone(),
                },
                &key_config,
                &mut output,
            )
            .unwrap();
            let rendered = String::from_utf8(output).unwrap();
            assert!(rendered.contains(expected), "{rendered}");
            assert!(rendered.contains("inactive until the prerequisites"));
        }

        let blocked_root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(blocked_root.path().join(".nosis")).unwrap();
        std::fs::write(
            blocked_root.path().join(".nosis/law.toml"),
            "[send]\nblock = [\"blocked.example\"]\n",
        )
        .unwrap();
        let blocked_policy = test_policy(blocked_root.path());
        let blocked = McpServerConfig {
            name: "fixture".into(),
            url: "https://blocked.example/mcp".into(),
            spec: nh_tools::MCP_SPEC_VERSION.into(),
            auth: McpAuth::None,
            scopes: Vec::new(),
            default_mode: None,
            trust: McpTrust::Ask,
        };
        let mut output = Vec::new();
        print_server_status_with_presence(
            ServerStatusContext {
                home: home.path(),
                policy: &blocked_policy,
                executable: &executable(),
                effective: std::slice::from_ref(&blocked),
                scrubber: &scrubber,
                presence: &|_| -> CredentialPresence {
                    panic!("credential presence must not be queried for a blocked destination")
                },
            },
            &blocked,
            &mut output,
        )
        .unwrap();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("policy or effective configuration blocks"));
        assert!(rendered.contains("inactive until the prerequisites"));

        let changed_home = tempfile::tempdir().unwrap();
        let (original, methods, peer) = metadata_peer("Public lookup.");
        let snapshot = nh_tools::inspect_mcp_server(&original).unwrap();
        peer.join().unwrap();
        assert_eq!(*methods.lock().unwrap(), ["tools/list"]);
        let state = McpReviewState::from_snapshot(snapshot, ["lookup".to_string()]).unwrap();
        save_review(changed_home.path(), &state);
        let mut changed = original;
        changed.url.push_str("-changed");
        let mut output = Vec::new();
        print_server_status_with_presence(
            ServerStatusContext {
                home: changed_home.path(),
                policy: &policy,
                executable: &executable(),
                effective: std::slice::from_ref(&changed),
                scrubber: &scrubber,
                presence: &|_| CredentialPresence::Ready,
            },
            &changed,
            &mut output,
        )
        .unwrap();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("configuration changed; tools disabled"));
        assert!(rendered.contains("Enabled reviewed tools: 0"));
        assert!(rendered.contains(&server_command(&executable(), "review", "fixture")));

        let mut invalid_auth = key_config.clone();
        invalid_auth.auth = McpAuth::OAuth2 {
            token_url: "https:auth.example.invalid/token".into(),
            client_id: "client".into(),
            vault_entry: "deepseek".into(),
        };
        let mut invalid_selector = key_config.clone();
        invalid_selector.auth = McpAuth::ApiKey {
            vault_entry: "invalid\"selector".into(),
        };
        for invalid_auth in [invalid_auth, invalid_selector] {
            let mut output = Vec::new();
            print_server_status_with_presence(
                ServerStatusContext {
                    home: home.path(),
                    policy: &policy,
                    executable: &executable(),
                    effective: std::slice::from_ref(&invalid_auth),
                    scrubber: &scrubber,
                    presence: &|_| -> CredentialPresence {
                        panic!("invalid authentication settings must not trigger credential lookup")
                    },
                },
                &invalid_auth,
                &mut output,
            )
            .unwrap();
            let rendered = String::from_utf8(output).unwrap();
            assert!(rendered.contains(
                "authentication settings or credential destination transport must be repaired"
            ));
            assert!(!rendered.contains("<unreviewable destination>"));
            assert!(!rendered.contains("credential permission"));
        }

        let invalid_name = McpServerConfig {
            name: "a.b".into(),
            url: "https://example.invalid/mcp".into(),
            spec: nh_tools::MCP_SPEC_VERSION.into(),
            auth: McpAuth::None,
            scopes: Vec::new(),
            default_mode: None,
            trust: McpTrust::Ask,
        };
        let mut output = Vec::new();
        print_server_status_with_presence(
            ServerStatusContext {
                home: home.path(),
                policy: &policy,
                executable: &executable(),
                effective: std::slice::from_ref(&invalid_name),
                scrubber: &scrubber,
                presence: &|_| CredentialPresence::Ready,
            },
            &invalid_name,
            &mut output,
        )
        .unwrap();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("rename this server"), "{rendered}");
        assert!(!rendered.contains(&server_command(&executable(), "review", "a.b")));
    }

    #[test]
    fn unconfigured_malformed_review_points_to_deliberate_state_removal() {
        let home = tempfile::tempdir().unwrap();
        let directory = home.path().join(".nosis/mcp-reviews");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("fixture.json"), "malformed but preserved").unwrap();
        let mut output = Vec::new();
        print_unconfigured_status(
            home.path(),
            &executable(),
            "fixture",
            &mut output,
            &nh_vault::Scrubber::new(Vec::new()),
        )
        .unwrap();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("malformed and inactive"), "{rendered}");
        assert!(rendered.contains("deliberately remove"), "{rendered}");
        assert!(!rendered.contains("mcp connect"), "{rendered}");
        assert!(!rendered.contains("mcp disable"), "{rendered}");
    }

    #[test]
    fn metadata_check_only_lists_tools_and_never_creates_review_state() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let (config, methods, peer) = metadata_peer("Public lookup.");
        let original = write_config(home.path(), &config);
        let policy = test_policy(root.path());
        let mut output = Vec::new();

        check_with_io(
            root.path(),
            home.path(),
            &policy,
            &executable(),
            "fixture",
            &mut output,
        )
        .unwrap();
        peer.join().unwrap();

        assert_eq!(*methods.lock().unwrap(), ["tools/list"]);
        assert_eq!(
            std::fs::read_to_string(home.path().join(".nosis/mcp.toml")).unwrap(),
            original
        );
        assert!(!home.path().join(".nosis/mcp-reviews/fixture.json").exists());
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("No tool was invoked."), "{output}");
        assert!(output.contains("No review state was created."), "{output}");
    }

    #[test]
    fn metadata_check_connection_and_protocol_failures_preserve_review_and_configuration() {
        let root = tempfile::tempdir().unwrap();
        let policy = test_policy(root.path());

        let closed_home = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let closed = McpServerConfig {
            name: "fixture".into(),
            url: format!("http://{address}/mcp"),
            spec: nh_tools::MCP_SPEC_VERSION.into(),
            auth: McpAuth::None,
            scopes: Vec::new(),
            default_mode: None,
            trust: McpTrust::Ask,
        };
        let closed_config = write_config(closed_home.path(), &closed);
        let closed_state = "malformed review stays byte-for-byte";
        std::fs::create_dir_all(closed_home.path().join(".nosis/mcp-reviews")).unwrap();
        std::fs::write(
            closed_home.path().join(".nosis/mcp-reviews/fixture.json"),
            closed_state,
        )
        .unwrap();
        assert!(check_with_io(
            root.path(),
            closed_home.path(),
            &policy,
            &executable(),
            "fixture",
            &mut Vec::new(),
        )
        .is_err());
        assert_eq!(
            std::fs::read_to_string(closed_home.path().join(".nosis/mcp.toml")).unwrap(),
            closed_config
        );
        assert_eq!(
            std::fs::read_to_string(closed_home.path().join(".nosis/mcp-reviews/fixture.json"))
                .unwrap(),
            closed_state
        );

        let protocol_home = tempfile::tempdir().unwrap();
        let (protocol, peer) = nonconforming_peer();
        let protocol_config = write_config(protocol_home.path(), &protocol);
        let protocol_state = "another malformed review remains unchanged";
        std::fs::create_dir_all(protocol_home.path().join(".nosis/mcp-reviews")).unwrap();
        std::fs::write(
            protocol_home.path().join(".nosis/mcp-reviews/fixture.json"),
            protocol_state,
        )
        .unwrap();
        let error = check_with_io(
            root.path(),
            protocol_home.path(),
            &policy,
            &executable(),
            "fixture",
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();
        peer.join().unwrap();
        assert!(error.contains("response id"), "{error}");
        assert_eq!(
            std::fs::read_to_string(protocol_home.path().join(".nosis/mcp.toml")).unwrap(),
            protocol_config
        );
        assert_eq!(
            std::fs::read_to_string(protocol_home.path().join(".nosis/mcp-reviews/fixture.json"))
                .unwrap(),
            protocol_state
        );
    }

    #[test]
    fn disable_is_offline_default_no_and_works_after_configuration_is_removed() {
        let home = tempfile::tempdir().unwrap();
        let (config, methods, peer) = metadata_peer("Public lookup.");
        let snapshot = nh_tools::inspect_mcp_server(&config).unwrap();
        peer.join().unwrap();
        assert_eq!(*methods.lock().unwrap(), ["tools/list"]);
        let state = McpReviewState::from_snapshot(snapshot, ["lookup".to_string()]).unwrap();
        let original = save_review(home.path(), &state);

        let mut no = Cursor::new(b"\n".to_vec());
        disable_with_io(home.path(), "fixture", &mut no, &mut Vec::new()).unwrap();
        let path = home.path().join(".nosis/mcp-reviews/fixture.json");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        assert!(!home.path().join(".nosis/mcp.toml").exists());

        let mut yes = Cursor::new(b"yes\n".to_vec());
        let mut output = Vec::new();
        disable_with_io(home.path(), "fixture", &mut yes, &mut output).unwrap();
        let disabled = McpReviewState::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(disabled.enabled.is_empty());
        assert!(String::from_utf8(output)
            .unwrap()
            .contains("start a new session"),);
    }

    #[test]
    fn mutation_commands_require_both_terminal_sides() {
        assert!(require_interactive_mutation("disable", true, true).is_ok());
        for (input, output) in [(false, true), (true, false), (false, false)] {
            let error = require_interactive_mutation("disable", input, output)
                .unwrap_err()
                .to_string();
            assert!(error.contains("interactive terminal"), "{error}");
            assert!(error.contains("no configuration or review state was changed"));
        }
    }
}
