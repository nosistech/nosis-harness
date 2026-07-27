//! `nh mcp serve` - loopback-only preview access to routes and fleet runs.

use nh_vault::Vault as _;

pub fn serve(addr: &str, token_entry: Option<&str>) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (root, catalog) = crate::cmd_run::find_catalog(&cwd)?;
    let law = nh_law::load(&root, &nh_law::LoadOptions { cli_autonomy: None });
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
