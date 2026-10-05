//! `nh mcp` - loopback preview serving and operator-owned remote tool review.

mod integration;

pub(crate) use integration::{check, connect, disable, status};

use std::collections::BTreeSet;
use std::io::IsTerminal as _;
use std::path::Path;

use nh_tools::{McpReviewSnapshot, McpReviewState, McpServerConfig, McpTrust};
use nh_vault::Vault as _;

use crate::private_state::{PrivateStateFile, PrivateStateRead};

const MAX_REVIEW_INPUT_BYTES: usize = 4 * 1024;

pub fn serve(addr: &str, token_entry: Option<&str>) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (root, catalog) = crate::cmd_run::find_catalog(&cwd)?;
    let law = nh_law::load_checked(&root, &nh_law::LoadOptions { cli_autonomy: None })?;
    crate::cmd_fleet::print_law_warnings(&law.warnings);
    let token = match token_entry {
        Some(entry) => {
            let vault = nh_vault::EnvFallbackVault {
                inner: nh_vault::KeyringVault,
            };
            Some(vault.get(entry)?)
        }
        None => None,
    };
    let parsed_addr: std::net::SocketAddr = addr.parse().map_err(|_| {
        anyhow::anyhow!("invalid --addr '{addr}' - use host:port, e.g. 127.0.0.1:8765")
    })?;
    nh_mcp::serve(nh_mcp::ServeConfig {
        addr: parsed_addr,
        catalog,
        law,
        default_route: "deepseek-v4-flash".into(),
        run_root: root,
        token,
        max_workers: 4,
    })
}

pub fn review(server: &str) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    require_interactive_review(stdin.is_terminal(), stdout.is_terminal())?;
    validate_state_name(server)?;
    let cwd = std::env::current_dir()?;
    let (root, _) = crate::cmd_run::find_catalog(&cwd)?;
    let law = nh_law::load_checked(&root, &nh_law::LoadOptions { cli_autonomy: None })?;
    crate::cmd_fleet::print_law_warnings(&law.warnings);
    let home = nh_law::user_home_dir()
        .ok_or_else(|| anyhow::anyhow!("home directory is unavailable - cannot save MCP review"))?;
    let mut warnings = Vec::new();
    let configs =
        crate::cmd_run::load_and_vet_mcp_configs(&root, Some(&home), &law.policy, &mut warnings);
    let scrubber = nh_vault::Scrubber::new(Vec::new());
    for warning in warnings {
        eprintln!(
            "warning: {}",
            crate::cmd_run::safe_line(&scrubber, &warning)
        );
    }
    let config = configs
        .iter()
        .find(|config| config.name == server)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "MCP server is not available from the effective trusted configuration; check ~/.nosis/mcp.toml and repository restrictions"
            )
        })?;
    if config.trust == McpTrust::Block {
        anyhow::bail!(
            "MCP server is blocked by the effective configuration; review was not contacted"
        )
    }
    let Some(host) = nh_vault::host_of(&config.url) else {
        anyhow::bail!("MCP server URL has no reviewable host")
    };
    if matches!(law.policy.send_verdict(&host), nh_law::Verdict::Block(_)) {
        anyhow::bail!("MCP server destination is blocked by law; review was not contacted")
    }

    let (prior_text, prior) = read_prior_review(&home, server)?;
    let snapshot = nh_tools::inspect_mcp_server(config)?;
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    review_snapshot_with_io(
        &home,
        config,
        snapshot,
        prior_text.as_deref(),
        prior.as_ref(),
        &mut input,
        &mut output,
    )
}

fn require_interactive_review(input_terminal: bool, output_terminal: bool) -> anyhow::Result<()> {
    if !input_terminal || !output_terminal {
        anyhow::bail!("MCP review needs an interactive terminal; no review state was changed")
    }
    Ok(())
}

fn read_prior_review(
    home: &Path,
    server: &str,
) -> anyhow::Result<(Option<String>, Option<McpReviewState>)> {
    validate_state_name(server)?;
    let filename = format!("{server}.json");
    let state_path = home.join(".nosis").join("mcp-reviews").join(&filename);
    let Some(file) = PrivateStateFile::open(
        home,
        Some("mcp-reviews"),
        &filename,
        server,
        nh_tools::MAX_MCP_REVIEW_BYTES,
        false,
    )
    .map_err(|_| {
        anyhow::anyhow!(
            "refused MCP review state at {}; inspect that path and move or remove it deliberately before rerunning `nh mcp review {server}`; review state was not changed",
            state_path.display()
        )
    })?
    else {
        return Ok((None, None));
    };
    let state = file.read().map_err(|_| {
        anyhow::anyhow!(
            "refused MCP review state at {}; inspect that path and move or remove it deliberately before rerunning `nh mcp review {server}`; review state was not changed",
            file.path().display()
        )
    })?;
    match state {
        PrivateStateRead::Absent => Ok((None, None)),
        PrivateStateRead::Interrupted => anyhow::bail!(
            "MCP review state at {} is absent while interrupted update files remain in {}; inspect and resolve the recovery files deliberately before rerunning `nh mcp review {server}`; review state was not changed",
            file.path().display(),
            file.path()
                .parent()
                .expect("review file has a directory")
                .display()
        ),
        PrivateStateRead::Text(text) => {
            let parsed = McpReviewState::parse(&text)
                .ok()
                .filter(|state| state.server == server);
            Ok((Some(text), parsed))
        }
    }
}

fn review_snapshot_with_io(
    home: &Path,
    config: &McpServerConfig,
    snapshot: McpReviewSnapshot,
    prior_text: Option<&str>,
    prior: Option<&McpReviewState>,
    input: &mut dyn std::io::BufRead,
    output: &mut dyn std::io::Write,
) -> anyhow::Result<()> {
    let scrubber = nh_vault::Scrubber::new(Vec::new());
    let connection_changed =
        prior.is_some_and(|state| state.connection_fingerprint != snapshot.connection_fingerprint);
    writeln!(
        output,
        "MCP tool review for {}",
        safe(&scrubber, &snapshot.server)
    )?;
    writeln!(
        output,
        "Destination: {}",
        safe(
            &scrubber,
            snapshot.connection["url"]
                .as_str()
                .unwrap_or("<unavailable>")
        )
    )?;
    writeln!(
        output,
        "Protocol: {} | auth: {} | trust: {}",
        safe(
            &scrubber,
            snapshot.connection["protocol"]
                .as_str()
                .unwrap_or("<unavailable>")
        ),
        auth_label(config),
        trust_label(config.trust)
    )?;
    if prior_text.is_some() && prior.is_none() {
        writeln!(
            output,
            "Existing review state is malformed. It stays unchanged unless this review is confirmed."
        )?;
    } else if connection_changed {
        writeln!(
            output,
            "Connection settings changed. Previously enabled tools remain disabled until this review is confirmed."
        )?;
    }
    if snapshot.omitted_tools > 0 {
        writeln!(
            output,
            "{} tools had secret-bearing, invalid, or oversized metadata and cannot be enabled.",
            snapshot.omitted_tools
        )?;
    }

    let current_names = snapshot
        .tools
        .iter()
        .map(|tool| tool.name())
        .collect::<BTreeSet<_>>();
    for (index, tool) in snapshot.tools.iter().enumerate() {
        let status = tool_status(tool, prior, connection_changed);
        let description = tool.description().unwrap_or("(no description)");
        writeln!(
            output,
            "{}. {} [{}] - {}",
            index + 1,
            safe(&scrubber, tool.name()),
            status,
            safe(&scrubber, description)
        )?;
    }
    if let Some(prior) = prior {
        for removed in prior
            .snapshot
            .iter()
            .filter(|tool| !current_names.contains(tool.name()))
        {
            writeln!(
                output,
                "- {} [removed; disabled]",
                safe(&scrubber, removed.name())
            )?;
        }
    }
    if snapshot.tools.is_empty() {
        writeln!(output, "No reviewable tools were returned.")?;
    }
    write!(
        output,
        "Tool numbers to enable (comma-separated or ranges, `none` for zero, Enter cancels): "
    )?;
    output.flush()?;
    let Some(selection_line) = read_bounded_line(input)? else {
        writeln!(output, "Cancelled; review state unchanged.")?;
        return Ok(());
    };
    let selection_line = selection_line.trim();
    if selection_line.is_empty()
        || selection_line.eq_ignore_ascii_case("q")
        || selection_line.eq_ignore_ascii_case("quit")
    {
        writeln!(output, "Cancelled; review state unchanged.")?;
        return Ok(());
    }
    let selected_indexes = parse_selection(selection_line, snapshot.tools.len())?;
    let enabled = selected_indexes
        .iter()
        .map(|index| snapshot.tools[*index].name().to_string())
        .collect::<Vec<_>>();

    for index in &selected_indexes {
        let tool = &snapshot.tools[*index];
        if tool_status(tool, prior, connection_changed) == "enabled" {
            continue;
        }
        writeln!(
            output,
            "\nMetadata to approve for {}:",
            safe(&scrubber, tool.name())
        )?;
        if let Some(before) = prior.and_then(|state| state.tool(tool.name())) {
            writeln!(output, "Before:")?;
            write_descriptor(output, &scrubber, &before.descriptor)?;
        }
        writeln!(output, "After:")?;
        write_descriptor(output, &scrubber, &tool.descriptor)?;
    }
    if enabled.is_empty() {
        writeln!(output, "\nSelected: no tools")?;
    } else {
        writeln!(
            output,
            "\nSelected: {}",
            safe(&scrubber, &enabled.join(", "))
        )?;
    }
    let state = McpReviewState::from_snapshot(snapshot, enabled)?;
    let replacement = state.to_compact_json()?;
    write!(output, "Enable these selected tools? [y/N] ")?;
    output.flush()?;
    let Some(confirm) = read_bounded_line(input)? else {
        writeln!(output, "Cancelled; review state unchanged.")?;
        return Ok(());
    };
    if !matches!(confirm.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        writeln!(output, "Cancelled; review state unchanged.")?;
        return Ok(());
    }

    let filename = format!("{}.json", state.server);
    let file = PrivateStateFile::open(
        home,
        Some("mcp-reviews"),
        &filename,
        &state.server,
        nh_tools::MAX_MCP_REVIEW_BYTES,
        true,
    )?
    .expect("review directory was created");
    file.publish(prior_text, &replacement)?;
    writeln!(
        output,
        "Saved review: {} enabled tool(s). Start a new chat or TUI session to use it.",
        state.enabled.len()
    )?;
    Ok(())
}

fn tool_status(
    tool: &nh_tools::ReviewedTool,
    prior: Option<&McpReviewState>,
    connection_changed: bool,
) -> &'static str {
    if connection_changed {
        return "connection changed; disabled";
    }
    match prior.and_then(|state| state.tool(tool.name()).map(|saved| (state, saved))) {
        None => "new; disabled",
        Some((_, saved)) if saved.fingerprint != tool.fingerprint => "changed; disabled",
        Some((state, _)) if state.is_enabled(tool.name()) => "enabled",
        Some(_) => "disabled",
    }
}

fn write_descriptor(
    output: &mut dyn std::io::Write,
    scrubber: &nh_vault::Scrubber,
    descriptor: &serde_json::Value,
) -> anyhow::Result<()> {
    let rendered = serde_json::to_string_pretty(descriptor)?;
    writeln!(output, "{}", crate::cmd_run::safe_text(scrubber, &rendered))?;
    Ok(())
}

fn parse_selection(text: &str, count: usize) -> anyhow::Result<Vec<usize>> {
    if text.eq_ignore_ascii_case("none") {
        return Ok(Vec::new());
    }
    let mut selected = BTreeSet::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            anyhow::bail!("invalid empty tool selection")
        }
        if let Some((start, end)) = part.split_once('-') {
            let start = parse_selection_number(start, count)?;
            let end = parse_selection_number(end, count)?;
            if start > end {
                anyhow::bail!("tool selection range start must not exceed its end")
            }
            selected.extend(start..=end);
        } else {
            selected.insert(parse_selection_number(part, count)?);
        }
    }
    Ok(selected.into_iter().map(|number| number - 1).collect())
}

fn parse_selection_number(text: &str, count: usize) -> anyhow::Result<usize> {
    let number = text
        .trim()
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("tool selections must be numbers or ranges"))?;
    if number == 0 || number > count {
        anyhow::bail!("tool selection {number} is outside 1..={count}")
    }
    Ok(number)
}

fn read_bounded_line(input: &mut dyn std::io::BufRead) -> anyhow::Result<Option<String>> {
    let mut bytes = Vec::new();
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return Ok(None);
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let content_len = newline.unwrap_or(available.len());
        if bytes.len().saturating_add(content_len) > MAX_REVIEW_INPUT_BYTES {
            anyhow::bail!(
                "MCP review input exceeds the {} byte limit; review state unchanged",
                MAX_REVIEW_INPUT_BYTES
            )
        }
        bytes.extend_from_slice(&available[..content_len]);
        let consumed = newline.map_or(available.len(), |index| index + 1);
        input.consume(consumed);
        if newline.is_some() {
            while matches!(bytes.last(), Some(b'\r')) {
                bytes.pop();
            }
            return String::from_utf8(bytes).map(Some).map_err(Into::into);
        }
    }
}

fn validate_state_name(name: &str) -> anyhow::Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        anyhow::bail!("MCP server name cannot be used for review state")
    }
    Ok(())
}

fn auth_label(config: &McpServerConfig) -> &'static str {
    match config.auth {
        nh_tools::McpAuth::None => "none",
        nh_tools::McpAuth::ApiKey { .. } => "API key",
        nh_tools::McpAuth::OAuth2 { .. } => "OAuth 2",
    }
}

fn trust_label(trust: McpTrust) -> &'static str {
    match trust {
        McpTrust::Auto => "auto",
        McpTrust::Ask => "ask",
        McpTrust::Block => "block",
    }
}

fn safe(scrubber: &nh_vault::Scrubber, text: &str) -> String {
    crate::cmd_run::safe_line(scrubber, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::{Cursor, Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};

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

    fn review_fixture() -> (McpServerConfig, McpReviewSnapshot) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_rpc_request(&mut stream);
            let body = json!({
                "jsonrpc": "2.0",
                "id": request["id"],
                "result": {
                    "tools": [{
                        "name": "peek",
                        "description": "Read one item.",
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
        let config = McpServerConfig {
            name: "sample".into(),
            url: format!("http://{address}/mcp"),
            spec: "2026-07-28".into(),
            auth: nh_tools::McpAuth::None,
            scopes: Vec::new(),
            default_mode: None,
            trust: McpTrust::Ask,
        };
        let snapshot = nh_tools::inspect_mcp_server(&config).unwrap();
        peer.join().unwrap();
        (config, snapshot)
    }

    #[test]
    fn review_requires_both_terminal_sides_before_work() {
        assert!(require_interactive_review(true, true).is_ok());
        for (input, output) in [(false, true), (true, false), (false, false)] {
            let error = require_interactive_review(input, output)
                .unwrap_err()
                .to_string();
            assert!(error.contains("interactive terminal"));
            assert!(error.contains("no review state was changed"));
        }
    }

    #[test]
    fn review_cancel_and_oversized_input_preserve_existing_state_bytes() {
        let home = tempfile::tempdir().unwrap();
        let state_dir = home.path().join(".nosis").join("mcp-reviews");
        std::fs::create_dir_all(&state_dir).unwrap();
        let path = state_dir.join("sample.json");
        std::fs::write(&path, "existing malformed bytes").unwrap();

        let (config, snapshot) = review_fixture();
        let mut cancel = Cursor::new(b"\n".to_vec());
        let mut output = Vec::new();
        review_snapshot_with_io(
            home.path(),
            &config,
            snapshot,
            Some("existing malformed bytes"),
            None,
            &mut cancel,
            &mut output,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "existing malformed bytes"
        );

        let (config, snapshot) = review_fixture();
        let mut oversized = Cursor::new(format!("{}\n", "1".repeat(MAX_REVIEW_INPUT_BYTES + 1)));
        let error = review_snapshot_with_io(
            home.path(),
            &config,
            snapshot,
            Some("existing malformed bytes"),
            None,
            &mut oversized,
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("input exceeds"), "{error}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "existing malformed bytes"
        );

        let (config, snapshot) = review_fixture();
        let mut eof_without_enter = Cursor::new(b"1\nyes".to_vec());
        review_snapshot_with_io(
            home.path(),
            &config,
            snapshot,
            Some("existing malformed bytes"),
            None,
            &mut eof_without_enter,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "existing malformed bytes"
        );
    }

    #[test]
    fn confirmed_selection_and_explicit_none_publish_valid_bounded_state() {
        let selected_home = tempfile::tempdir().unwrap();
        let (config, snapshot) = review_fixture();
        let mut input = Cursor::new(b"1\nyes\n".to_vec());
        review_snapshot_with_io(
            selected_home.path(),
            &config,
            snapshot,
            None,
            None,
            &mut input,
            &mut Vec::new(),
        )
        .unwrap();
        let selected =
            std::fs::read_to_string(selected_home.path().join(".nosis/mcp-reviews/sample.json"))
                .unwrap();
        let selected = McpReviewState::parse(&selected).unwrap();
        assert_eq!(selected.enabled, ["peek"]);

        let none_home = tempfile::tempdir().unwrap();
        let (config, snapshot) = review_fixture();
        let mut input = Cursor::new(b"none\ny\n".to_vec());
        review_snapshot_with_io(
            none_home.path(),
            &config,
            snapshot,
            None,
            None,
            &mut input,
            &mut Vec::new(),
        )
        .unwrap();
        let none = std::fs::read_to_string(none_home.path().join(".nosis/mcp-reviews/sample.json"))
            .unwrap();
        assert!(McpReviewState::parse(&none).unwrap().enabled.is_empty());
    }

    #[test]
    fn previously_disabled_tool_shows_full_metadata_before_it_can_be_enabled() {
        let home = tempfile::tempdir().unwrap();
        let (config, snapshot) = review_fixture();
        let mut disable = Cursor::new(b"none\ny\n".to_vec());
        review_snapshot_with_io(
            home.path(),
            &config,
            snapshot,
            None,
            None,
            &mut disable,
            &mut Vec::new(),
        )
        .unwrap();
        let path = home.path().join(".nosis/mcp-reviews/sample.json");
        let prior_text = std::fs::read_to_string(&path).unwrap();
        let prior = McpReviewState::parse(&prior_text).unwrap();
        assert!(prior.enabled.is_empty());

        let snapshot = McpReviewSnapshot {
            server: prior.server.clone(),
            connection: prior.connection.clone(),
            connection_fingerprint: prior.connection_fingerprint.clone(),
            tools: prior.snapshot.clone(),
            omitted_tools: 0,
        };
        let mut select = Cursor::new(b"1\ny\n".to_vec());
        let mut output = Vec::new();
        review_snapshot_with_io(
            home.path(),
            &config,
            snapshot,
            Some(&prior_text),
            Some(&prior),
            &mut select,
            &mut output,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Metadata to approve for peek:"), "{output}");
        assert!(output.contains("\"inputSchema\""), "{output}");
        assert!(output.contains("\"annotations\""), "{output}");
        let saved = McpReviewState::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved.enabled, ["peek"]);
    }

    #[test]
    fn refused_and_interrupted_prior_review_errors_name_safe_recovery_paths() {
        let refused_home = tempfile::tempdir().unwrap();
        let refused_path = refused_home.path().join(".nosis/mcp-reviews/sample.json");
        std::fs::create_dir_all(&refused_path).unwrap();
        let refused = read_prior_review(refused_home.path(), "sample")
            .unwrap_err()
            .to_string();
        assert!(
            refused
                .replace('\\', "/")
                .contains("/.nosis/mcp-reviews/sample.json"),
            "{refused}"
        );
        assert!(
            refused.contains("move or remove it deliberately"),
            "{refused}"
        );
        assert!(refused.contains("nh mcp review sample"), "{refused}");

        let interrupted_home = tempfile::tempdir().unwrap();
        let directory = interrupted_home.path().join(".nosis/mcp-reviews");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(".sample.nh-restore-1-0.tmp"), "prior").unwrap();
        let interrupted = read_prior_review(interrupted_home.path(), "sample")
            .unwrap_err()
            .to_string();
        assert!(
            interrupted
                .replace('\\', "/")
                .contains("/.nosis/mcp-reviews/sample.json"),
            "{interrupted}"
        );
        assert!(interrupted.contains("recovery files"), "{interrupted}");
        assert!(
            interrupted.contains("nh mcp review sample"),
            "{interrupted}"
        );
    }

    #[test]
    fn selection_parser_rejects_ambiguous_or_out_of_range_input() {
        assert_eq!(parse_selection("1,3-4", 4).unwrap(), [0, 2, 3]);
        assert!(parse_selection("none", 4).unwrap().is_empty());
        for invalid in ["", "0", "5", "3-2", "1,,2", "all"] {
            assert!(parse_selection(invalid, 4).is_err(), "accepted {invalid:?}");
        }
    }
}
