# Environment

`nh` is a local CLI. There is no NosisTech staging or production server in v0.1.

## Local Setup

Requirements:

- Rust **1.96.0** through `rustup`; `rust-toolchain.toml` pins rustfmt and clippy.
- Git.
- Windows needs no extra native package for the selected keyring backend.
- Ubuntu CI installs `libdbus-1-dev` and `pkg-config` for Linux Secret Service.
- A desktop keyring/Secret Service session is needed to use `nh key add` on Linux.

Commands:

```sh
cargo build --locked --release
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
```

## Environment Variables

Do not put real values in this file, shell history, issue reports, or test fixtures.

| Name | Purpose | Notes |
|---|---|---|
| `NH_<ENTRY>_KEY` | CI/headless fallback when the OS vault has no entry | Replace hyphens with underscores and uppercase the entry. Prefer the OS vault interactively. |
| `HOME` | User-global config root on Linux/macOS | `~/.nosis/law.toml`, `catalog.toml`, and `mcp.toml` may live below it. |
| `USERPROFILE` | User-global config root on Windows | Same purpose as `HOME`. |

Test-only Fleet variables exist behind debug assertions and are not a supported runtime
configuration surface.

## Local State

- Repository policy/runtime data: `<repo>/.nosis/`
- Receipts: `<repo>/.nosis/receipts.jsonl`
- Fleet state: `<repo>/.nosis/fleet/`
- User-global trusted configuration: `~/.nosis/`
- Provider and MCP keys: OS credential store, service name `nosis-harness`

## Troubleshooting

- `no key found`: run `nh key add <entry>`, or confirm the headless environment contains
  the correctly named fallback.
- Linux keyring errors: confirm a Secret Service implementation and D-Bus session are active.
- Catalog refused: restore the bundled catalog or put the exact reviewed replacement at
  `~/.nosis/catalog.toml`.
- Stale price warning: reverify first-party provider data and advance `valid_until`; do not
  suppress the warning.
- MCP bind refusal: v0.1 accepts only `127.0.0.1`; this is intentional.
