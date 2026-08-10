# Contributing to Nosis Harness

Thanks for helping build `nh` - an honest, metered, multi-model terminal agent.
Be respectful in issues, reviews, and discussions; we keep this a friendly place to work.

## Build

- **Rust 1.96.0, pinned.** [`rust-toolchain.toml`](rust-toolchain.toml) pins the channel
  (with `rustfmt` and `clippy` components), so `rustup` selects the right toolchain
  automatically - builds, lints, and formatting are identical on every machine.
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

[`gate.ps1`](gate.ps1) mechanizes the five checks that define "clean" for this workspace,
in order:

1. `cargo fmt --all --check` - no formatting drift.
2. `cargo clippy --locked --workspace --all-targets --release -- -D warnings` - zero warnings, all targets.
3. `cargo doc --locked --workspace --no-deps` with `RUSTDOCFLAGS=-D warnings` - every public API documents without warnings.
4. `cargo deny --locked check` - advisories, bans, licenses, and sources.
5. `cargo test --locked --workspace --release --no-fail-fast` - the full workspace suite.
   `--no-fail-fast` matters. Without it cargo stops at the first failing test binary, so one
   failure hides every failure after it and the reported count is short.

It captures each step's real exit code, prints a per-step summary, and exits non-zero if
**any** step fails - all five must be green. On Windows it first stops any running
`nh.exe`, which would otherwise lock `target\release\nh.exe` and fail the build.

**Prefer to run the steps yourself?** Run the same five cargo commands above, in the same
order, under the pinned toolchain. Do that if you are not on PowerShell, or in any
environment where running a script that stops processes is not appropriate. The five
commands are the gate. The script is a convenience that runs them for you.

Format your code with `cargo fmt` - the gate enforces `fmt --check`, it does not reformat
for you.

## Quality bar

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
| `nh-mcp`   | Local MCP server (preview; loopback `127.0.0.1` only - must not be exposed on a public interface, and this is not a restriction that lapses on a date) |
| `nh-cli`   | The `nh` binary                         |

**Disjoint-file discipline:** parallel work touches disjoint files. If another change is
in flight in a crate, do not edit that crate - coordinate first.

## Pull requests

- Branch from `main`.
- Keep the diff focused - one concern per PR.
- Update [CHANGELOG.md](CHANGELOG.md) under `[Unreleased]`.
- Run [`./gate.ps1`](gate.ps1) and make sure all five steps pass.
- **Never commit secrets.** Keys belong in the OS vault (`nh key add`), never in files.
- Flag security-sensitive changes (vault, approval/guardrail verdicts, tool egress,
  MCP surface) explicitly in the PR description so they get a security-minded review.
- By contributing you agree your contribution is licensed under the [MIT License](LICENSE).

## Reporting security issues

Do **not** open a public issue for a vulnerability. See [SECURITY.md](SECURITY.md) and
email **info@nosistech.com** - we respond within 5 business days.
