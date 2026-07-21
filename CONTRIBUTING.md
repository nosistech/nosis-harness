# Contributing to Nosis Harness

Thanks for helping build `nh` — an honest, metered, multi-model terminal agent.
Be respectful in issues, reviews, and discussions; we keep this a friendly place to work.

## Build

- **Rust 1.96.0, pinned.** [`rust-toolchain.toml`](rust-toolchain.toml) pins the channel
  (with `rustfmt` and `clippy` components), so `rustup` selects the right toolchain
  automatically — builds, lints, and formatting are identical on every machine.
- Build the workspace:

  ```sh
  cargo build --release
  ```

- The binary lands at `target/release/nh` (`target\release\nh.exe` on Windows).

## The gate

Run the workspace gate before every PR:

```powershell
./gate.ps1
```

[`gate.ps1`](gate.ps1) mechanizes the three checks that define "clean" for this workspace,
in order:

1. `cargo fmt --all --check` — no formatting drift.
2. `cargo clippy --workspace --all-targets --release -- -D warnings` — zero warnings, all targets.
3. `cargo test --workspace --release` — the full workspace suite.

It captures each step's real exit code, prints a per-step summary, and exits non-zero if
**any** step fails — all three must be green. (On Windows it also kills any running
`nh.exe` first, which would otherwise lock the target directory and fail the link.)

Not on PowerShell? Run the same three cargo commands above, in the same order, under the
pinned toolchain.

Format your code with `cargo fmt` — the gate enforces `fmt --check`, it does not reformat
for you.

## Quality bar — THE LAW

Every change is held to THE LAW: **small, simple, secure, safe, lightweight, readable,
auditable, modular, congruent, harmonic.** See
[03-execution/QUALITY_BAR.md](03-execution/QUALITY_BAR.md) for what that means in practice.

Keep diffs small and auditable. A reviewer should be able to hold your whole change in
their head.

## Crate map

Nine crates, one responsibility each:

| Crate      | Responsibility                          |
| ---------- | --------------------------------------- |
| `nh-core`  | Agent turn loop + wire/cost math        |
| `nh-routes`| Honest routing + pricing                |
| `nh-tools` | Tool exec + egress scrubbing            |
| `nh-vault` | OS keyring + secret scrubber            |
| `nh-law`   | Approval/guardrail verdicts             |
| `nh-tui`   | Full-screen terminal UI                 |
| `nh-fleet` | Durable worker fleet                    |
| `nh-mcp`   | Local MCP server (preview; loopback `127.0.0.1` only — must not be exposed publicly before the final MCP spec lands on 2026-07-28) |
| `nh-cli`   | The `nh` binary                         |

**Disjoint-file discipline:** parallel work touches disjoint files. If another change is
in flight in a crate, do not edit that crate — coordinate first.

## Pull requests

- Branch from `main`.
- Keep the diff focused — one concern per PR.
- Update [CHANGELOG.md](CHANGELOG.md) under `[Unreleased]`.
- Run [`./gate.ps1`](gate.ps1) and make sure all three steps pass.
- **Never commit secrets.** Keys belong in the OS vault (`nh key add`), never in files.
- Flag security-sensitive changes (vault, approval/guardrail verdicts, tool egress,
  MCP surface) explicitly in the PR description so they get a security-minded review.
- By contributing you agree your contribution is licensed under the [MIT License](LICENSE).

## Reporting security issues

Do **not** open a public issue for a vulnerability. See [SECURITY.md](SECURITY.md) and
email **info@nosistech.com** — we respond within 5 business days.
