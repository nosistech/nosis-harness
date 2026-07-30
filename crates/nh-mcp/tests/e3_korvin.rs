//! End-to-end MCP/Fleet exercise for the debug-only echo provider seam.
//!
//! Release builds deliberately compile that seam out; the CLI integration suite
//! separately proves a release binary refuses `NH_FLEET_TEST_PROVIDER`.
#![cfg(debug_assertions)]

use std::collections::BTreeSet;
use std::io::{Read as _, Write as _};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use nh_mcp::{McpServer, ServeConfig};

struct FleetTestEnv;

impl FleetTestEnv {
    fn echo() -> Self {
        std::env::set_var("NH_FLEET_TEST_PROVIDER", "echo");
        std::env::set_var("NH_FLEET_TEST_SLEEP_MS", "40");
        Self
    }
}

impl Drop for FleetTestEnv {
    fn drop(&mut self) {
        std::env::remove_var("NH_FLEET_TEST_PROVIDER");
        std::env::remove_var("NH_FLEET_TEST_SLEEP_MS");
        std::env::remove_var("NH_NH_MCP_E3_KEY");
    }
}

#[test]
fn e3_korvin_starts_and_polls_a_stateless_fleet_run() -> anyhow::Result<()> {
    let tempdir = tempfile::tempdir()?;
    let catalog =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../catalog.toml")).to_string();
    let law = nh_law::load(tempdir.path(), &nh_law::LoadOptions { cli_autonomy: None });
    let fleet_env = FleetTestEnv::echo();
    let server = McpServer::start(ServeConfig {
        addr: "127.0.0.1:0".parse()?,
        catalog,
        law,
        default_route: "deepseek-v4-flash".into(),
        run_root: tempdir.path().to_path_buf(),
        token: None,
        max_workers: 2,
    })?;
    let token = server.token().to_string();
    std::env::set_var("NH_NH_MCP_E3_KEY", &token);
    let cfg = nh_tools::mcp::McpServerConfig {
        name: "nh-mcp".into(),
        url: format!("http://{}/mcp", server.addr()),
        spec: "2026-07-28".into(),
        auth: nh_tools::mcp::McpAuth::ApiKey {
            vault_entry: "nh-mcp-e3".into(),
        },
        scopes: vec![],
        default_mode: None,
        trust: nh_tools::mcp::McpTrust::Ask,
    };
    let client = nh_tools::mcp::McpClient::new(cfg).expect("test HTTP clients initialize");

    let names: BTreeSet<_> = client
        .list_tools()?
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    assert_eq!(
        names,
        BTreeSet::from([
            "fleet_run".to_string(),
            "fleet_status".to_string(),
            "receipts".to_string(),
            "route_cost".to_string(),
            "route_resolve".to_string(),
            "why".to_string(),
        ])
    );

    let route = client.call_tool("route_resolve", serde_json::json!({}))?;
    assert!(route.contains("deepseek-v4-flash"));

    let started = client.call_tool(
        "fleet_run",
        serde_json::json!({
            "tasks": [{"task":"echo one"}, {"task":"echo two"}],
            "budget": 1_000
        }),
    )?;
    let run_id = started
        .split_whitespace()
        .find_map(|token| token.strip_prefix("run_id="))
        .filter(|run_id| !run_id.is_empty())
        .ok_or_else(|| anyhow::anyhow!("fleet_run did not return a run_id handle"))?
        .to_string();

    let deadline = Instant::now() + Duration::from_secs(10);
    let finished = loop {
        let status = client.call_tool("fleet_status", serde_json::json!({ "run_id": run_id }))?;
        if status.contains("finished") {
            break status;
        }
        if Instant::now() >= deadline {
            anyhow::bail!("fleet_status did not reach finished within 10 seconds");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(finished.contains("2 done"), "{finished}");

    let raw = raw_tools_list(server.addr(), &token)?;
    let headers = raw.split("\r\n\r\n").next().unwrap_or(&raw);
    assert!(!headers.to_ascii_lowercase().contains("mcp-session-id"));

    server.shutdown()?;
    drop(fleet_env);
    Ok(())
}

fn raw_tools_list(addr: SocketAddr, token: &str) -> anyhow::Result<String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "tools/list",
        "params": {}
    })
    .to_string();
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(request.as_bytes())?;
    stream.shutdown(Shutdown::Write)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}
