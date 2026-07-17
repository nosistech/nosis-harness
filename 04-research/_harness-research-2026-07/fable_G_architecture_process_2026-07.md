# LENS G — Architecture / THE LAW / Build Process — NOSIS HARNESS improvement research
**Date:** 2026-07-16/17 · **Analyst:** Fable 5 research pass · **Repo HEAD:** `bd35b4d` (M4 Slice D implemented, uncommitted)

## 0. Ground truth observed in the repo

- Workspace: 9 crates (`nh-core, nh-routes, nh-tools, nh-vault, nh-law, nh-tui, nh-fleet, nh-mcp, nh-cli`), ~15,100 lines of Rust total (`wc -l crates/*/src/*.rs`). Every crate is a **single-file lib.rs** except nh-cli and nh-tools; `nh-tui/src/lib.rs` = **4,107 lines / 172 fns**, `nh-fleet/src/lib.rs` = 2,243, `nh-tools/src/mcp.rs` = 1,603, `nh-core/src/lib.rs` = 1,419 (contains three inline modules: `wire`, `receipt`, `agent`).
- Single-RouteResolver invariant is real and clean: `RouteResolver` in `crates/nh-routes/src/lib.rs:471` is the only minting point; `ResolvedRoute` carries wire/price/modality/dialect; `is_banned` at line 256 enforces the banned-string rule.
- 2-wire adapter design is honored: `nh-core/src/lib.rs` `pub mod wire` — `make_client` (line 123) picks `OpenAiCompatClient` (166) or `AnthropicMessagesClient` (412); "The ONE place (dialect, effort) → OpenAI-wire params lives" (line 300).
- **No CI at all**: no `.github/` directory, no workflows, no pinned `rust-toolchain.toml`, no `deny.toml`, no `[workspace.lints]` in the root `Cargo.toml`, empty `.cargo/` dir. Gating is 100% manual (`cargo test --workspace` + `cargo clippy -- -D warnings` run by the orchestrator per CONTRACTS_M4 §6).
- **Frozen-crate discipline is enforced socially, not mechanically**: CONTRACTS_M4 §0.1 freezes nh-core/nh-tools/nh-law/nh-routes/nh-vault; verification is manual `git diff --numstat` reading ("numstat = truth"). BUILD_LOG.md line 339 records a scare where `git status` showed frozen crates modified and numstat had to prove zero delta.
- **Verification docs are empty templates**: `02-architecture/EVALUATION_PLAN.md` and `FAILURE_MODES.md` are unfilled scaffolds, while the *actual* verification discipline (E1–E4 exit criteria, verify-live ledger CONTRACTS_M4 §7, FEEL gate) lives scattered in CONTRACTS_M*.md and CURRENT_TASK.md.
- **The M4 finale exists only as loose working-tree state** + a patch in `%TEMP%` (CURRENT_TASK.md lines 8–13) — one Kaspersky quarantine, disk hiccup, or accidental `git checkout --` away from loss.
- Escalation-ladder default **hard-codes route IDs in Rust** (`crates/nh-fleet/src/lib.rs:95–107`: `"deepseek-v4-flash"`, `"kimi-k2.7-code"`, `"deepseek-v4-pro"`), in tension with AGENTS.md's hard rule "Catalog/pricing data is data (TOML) — never hard-code model IDs or prices in Rust."
- Windows-first pain is real and recurring: Kaspersky blocks freshly built `nh.exe` (os error 5) failing `fleet_kill_resume` + `m2_exit`; running `nh.exe` locks `target\debug\nh.exe` (link failures); PowerShell OEM-codepage mojibake in probes (CURRENT_TASK "Environment gotchas").

---

## Finding 1 — Commit-to-WIP-branch rule: never leave milestone work as loose working-tree state (process, S)

**Problem.** Right now the entire M4 finale (Slice D OAuth2, +367/−39 in frozen `nh-tools/src/mcp.rs`) lives ONLY as an uncommitted working tree plus a patch file in `C:\Users\capv2\AppData\Local\Temp` — a directory that Windows Storage Sense / temp cleaners / AV quarantine can purge. The FEEL-gate rule ("do NOT commit until Carlos approves") was interpreted as "don't put it in git at all," which conflates *publishing to main* with *durability*.

**Recommendation.** One-line process amendment in AGENTS.md / CONTRACTS ground rules: at any session end (or before any risky operation), gated-but-unapproved work is committed to a `wip/<slice>` branch (or `git stash create` + tag). Main stays FEEL-gated exactly as today; the object database becomes the durable store; the Temp patch becomes a belt-and-suspenders copy, not the primary. Resume = `git switch wip/slice-d` or cherry-pick. Zero new tooling.

**LAW fit:** safe (work preservation is AGENTS.md "Preserve user work"), auditable (WIP has a hash + message), small. No tension.
**Evidence:** `00-start-here/CURRENT_TASK.md:8–13`; 2026 agent-governance guidance that every agent task needs a rollback plan and audit trail ([Totalum orchestration patterns 2026](https://www.totalum.app/blog/ai-agent-orchestrator-totalum-2026)).
**Effort:** S. **Value:** high (it protects the highest-value uncommitted asset in the project *today*).

## Finding 2 — MCP 2026-07-28 conformance gap: the RC mandates `Mcp-Method`/`Mcp-Name`/`MCP-Protocol-Version` HTTP headers and the reverse-DNS `clientInfo` meta key; nosis implements neither (code, S/M, deadline 2026-07-28)

**Problem.** Differentiator #5 is "MCP 2026-07-28 stateless-native," and Decision 5 pins the RC. The official RC post confirms statelessness (SEP-2575 no initialize, SEP-2567 no `Mcp-Session-Id` — both correctly implemented in nosis) **but also**: **SEP-2243 mandates `Mcp-Method` and `Mcp-Name` HTTP headers for routing**, requires `MCP-Protocol-Version: 2026-07-28` as a header, moves client info to `"_meta":{"io.modelcontextprotocol/clientInfo":{...}}` (reverse-DNS key), and adds `cacheScope` beside `ttlMs` (SEP-2549). The final ships **July 28, 2026** — 11 days out — and "a Standards Track SEP can no longer reach Final status until a matching scenario lands in the conformance suite," i.e. an official conformance suite exists to test against. Beta SDKs for the RC are published.

Nosis today (verified by grep): client sends `params._meta.protocolVersion` + plain `clientInfo` key (`crates/nh-tools/src/mcp.rs:315–316`), no `Mcp-Method`/`Mcp-Name`/`MCP-Protocol-Version` headers anywhere in `nh-tools/src/mcp.rs` or `nh-mcp/src/lib.rs`; server routes purely on the JSON-RPC body. It handles `ttlMs` (mcp.rs:247, nh-mcp lib.rs:275) and `.well-known/mcp.json` (mcp.rs:294, nh-mcp lib.rs:142) correctly, but knows nothing of `cacheScope`, `InputRequiredResult` multi-round-trip (SEP-2322), or the Tasks extension (SEP-2663 — which is *exactly* the shape of `fleet_run`/`fleet_status`).

**Recommendation.** A small M5 slice, additive on both sides of the already-congruent client↔server pair: (1) client `rpc()` adds the three headers (values are method names — they pass the existing `lint_headers` secret lint, which currently *rejects* `Mcp-*` headers carrying JWT-shaped values, mcp.rs:1455 — verify the lint allowlists these two known header names with non-secret values); (2) client `_meta` key renamed to `io.modelcontextprotocol/clientInfo`; (3) nh-mcp server accepts-and-ignores-or-routes on `Mcp-Method` and echoes `MCP-Protocol-Version`; (4) parse `cacheScope` next to `ttlMs`; (5) after 7/28, run the official conformance suite once against `nh mcp serve` and log the result in the verify-live ledger. Requires a frozen-crate amendment for nh-tools (precedent: A-M4-1) — plan it in CONTRACTS_M5 up front. Later/optional: advertise `fleet_run` as a Tasks-extension task.

**LAW fit:** congruent (Decision 5 "conformance check in CI"), auditable, small (headers + one key rename). No tension.
**Evidence:** [MCP 2026-07-28 RC post](https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/); [Beta SDKs post](https://blog.modelcontextprotocol.io/posts/sdk-betas-2026-07-28/); `crates/nh-tools/src/mcp.rs:315–316,247,294,1455`; `crates/nh-mcp/src/lib.rs:142,275`; `02-architecture/ARCHITECTURE_DECISIONS.md` Decision 5.
**Effort:** M (mostly because of the frozen-crate amendment ceremony). **Value:** high — it's a headline differentiator with a dated deadline.

## Finding 3 — Mechanize the frozen-crate + allowed-files gate: a 30-line script replaces numstat eyeballing (process+code, S)

**Problem.** The build loop's central safety invariant — "Sol touched ONLY the files the brief allows; frozen crates byte-identical" — is verified by a human reading `git diff --numstat`. It works (Slice D attempt 1 was correctly caught STOPPING; the BUILD_LOG line-339 scare was resolved by numstat), but it is the single most repeated, most safety-critical manual step in every gate, and 2026 spec-driven practice is explicit that task contracts should state **allowed files / forbidden files** and that gates should be **deterministic sensors**, not prose review.

**Recommendation.** Add `tools/gate.ps1` (or a `just gate` task): reads a per-slice `slice.allow` manifest (list of allowed path globs, checked into the brief/contract), then (a) `git diff --numstat <anchor>..HEAD` must be a subset of the manifest — else FAIL with the offending paths; (b) for frozen crates, `git diff --quiet <anchor> -- crates/nh-core crates/nh-tools crates/nh-law crates/nh-routes crates/nh-vault` unless an `A-M*-n` amendment line in the manifest names the exact file; (c) optionally run [cargo-public-api](https://github.com/cargo-public-api/cargo-public-api) diff on frozen crates to prove "source-compatible, additive-only" instead of asserting it ("Detect breaking API changes and semver violations via CI or a CLI" is its stated purpose; [cargo-semver-checks](https://github.com/obi1kenobi/cargo-semver-checks) with 245 lints is the heavier alternative being merged into cargo itself per the [2026 project goal](https://rust-lang.github.io/rust-project-goals/2026/cargo-semver-checks.html)). The orchestrator still reads the diff for security review — the script removes the mechanical half and makes gate results reproducible artifacts.

**LAW fit:** auditable (gate output is a receipt), secure, small. Tension: none — it encodes existing law rather than adding process.
**Evidence:** `CONTRACTS_M4.md §0.1, §8`; `00-start-here/BUILD_LOG.md:339`; `00-start-here/CURRENT_TASK.md` (attempt-1 STOP story); [cargo-public-api](https://github.com/cargo-public-api/cargo-public-api); [SDD task-contract practice](https://github.blog/ai-and-ml/generative-ai/spec-driven-development-with-ai-get-started-with-a-new-open-source-toolkit/); [Totalum 2026 gate patterns](https://www.totalum.app/blog/ai-agent-orchestrator-totalum-2026).
**Effort:** S. **Value:** high — faster gates, zero-drift enforcement of the loop's #1 invariant.

## Finding 4 — Stand up minimal CI (the repo has none) (process+code, S)

**Problem.** ARCHITECTURE_OVERVIEW promises "CI runs headless `nh exec` with the free GLM-4.7-Flash route ($0 test suite)" — but there is no `.github/workflows/`, no CI of any kind. Every gate runs only on the ASUS box, subject to Kaspersky interference, exe locks, and human availability. Verification-first discipline without CI means a regression can only be caught when someone remembers to run the suite.

**Recommendation.** Smallest viable: one GitHub Actions workflow, `windows-latest` (Windows-first: CI should test the wedge platform) + `ubuntu-latest`, steps: checkout → `dtolnay/rust-toolchain` pinned → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo nextest run --workspace` (or `cargo test`) → the Finding-3 gate script. The deterministic 292-test suite needs **no API keys** (loopback mocks throughout — that discipline already exists). The promised keyed GLM smoke lane is a separate, later, optional job (keyRequired=GLM/Z.ai; we hold no GLM key today) — do NOT block on it. A CI run on clean Windows also permanently disambiguates "Kaspersky env failure" from "real regression" for the two AV-blocked spawn tests.

**LAW fit:** auditable, safe, congruent (the docs already claim CI). Tension: none if kept to one workflow file.
**Evidence:** absence of `.github/` (verified); `02-architecture/ARCHITECTURE_OVERVIEW.md:33`; [Modern Rust tooling 2026](https://blog.rajpoot.dev/posts/rust/rust-tooling-cargo-2026/) (clippy -D warnings + nextest as the 2026 CI baseline).
**Effort:** S. **Value:** high.

## Finding 5 — Structured Sol handoff: `codex exec --output-schema` + brief manifests (process, S)

**Problem.** Sol's handoff today is free-form: the orchestrator re-derives what happened from numstat + test runs + reading the diff. The Codex CLI's non-interactive mode (the exact `codex exec` surface the loop already uses) supports `--output-schema <file.json>` to force the **final agent message to conform to a JSON Schema**, and it streams progress to stderr while printing only the final message to stdout — purpose-built for exactly this loop shape ("useful for automated workflows that need stable fields").

**Recommendation.** Add a 10-field schema to every brief: `{files_touched[], tests_added[], tests_run, pass, fail, clippy_clean, stopped: bool, stop_reason, amendments_needed[], notes}`. Invocation becomes `codex exec ... --output-schema /c/Users/capv2/AppData/Local/Temp/sol-report-schema.json`. The gate script (Finding 3) then cross-checks Sol's self-report against numstat truth — disagreement itself is a signal (the Slice-D attempt-1 STOP would have surfaced as `stopped:true, stop_reason:"brief forbids nh-tui touch"` in seconds instead of an empty-numstat investigation). Also consider `codex exec resume` for two-stage briefs (implement → self-review) instead of fresh sessions.

**LAW fit:** auditable (machine-readable receipts from the builder), simple, harmonic with the receipts-everywhere design. Tension: none.
**Evidence:** [Codex non-interactive docs](https://developers.openai.com/codex/noninteractive) (`--output-schema`, stdout/stderr split, `exec resume`); `00-start-here/CURRENT_TASK.md` executor invocation + attempt-1 story.
**Effort:** S. **Value:** high — richer, faster, safer gates with zero new deps.

## Finding 6 — Harden the Windows gate: cargo-nextest (retries, per-test process isolation, flaky marking) + an AV/exe-lock preflight (code+process, S/M)

**Problem.** The loop repeatedly loses time to Windows environment failures that masquerade as code failures: Kaspersky blocking the freshly built `nh.exe` (os error 5) failed `fleet_kill_resume` + `m2_exit` this week; a running `nh.exe` locks `target\debug\nh.exe` and kills the build; earlier, `wire_clients` test exes were AV-blocked. This is the documented Windows story: cargo's own tracker has the exact "Access is denied (os error 5) on freshly compiled exe" AV issue, and nextest's Windows docs call out Defender/AV interference with per-test process spawning and recommend Dev Drive.

**Recommendation.** (a) Switch the gate runner to [cargo-nextest](https://nexte.st/): per-test **process isolation** (a wedged/AV-blocked spawn test can't poison the run), `--retries N` with automatic **flaky** marking so a transient AV block reads as `FLAKY` not `FAIL`, and 2–3× faster suites = faster loop iterations; put the two real-binary spawn tests (`fleet_kill_resume`, `m2_exit`) in a nextest test-group with `retries = 2` + serial execution. (b) Add a preflight to the gate script: kill stray `nh.exe`, then build + spawn a trivial fresh canary exe — if the canary gets os error 5, print "AV is blocking fresh binaries — environment, not code" and mark spawn tests env-blocked *before* running the suite. (c) Document (don't silently apply) the Kaspersky/Defender exclusion for `target\` and the Dev Drive option in a WINDOWS.md gotchas section — the CURRENT_TASK "Environment gotchas" list is session-scoped memory that should graduate to a repo doc.

**LAW fit:** safe, lightweight (nextest is a dev-tool, not a dependency), congruent with Windows-first (Decision 8). Tension: one more tool to install — justified by the recurring cost.
**Evidence:** [nextest retries/flaky](https://nexte.st/docs/features/retries/); [nextest Windows + AV + Dev Drive](https://nexte.st/docs/installation/windows/); [process-per-test rationale](https://sunshowers.io/posts/nextest-process-per-test/); [cargo os-error-5 AV issue](https://github.com/rust-lang/cargo/issues/11544); [JetBrains nextest 2026](https://blog.jetbrains.com/rust/2026/05/01/faster-rust-tests-with-cargo-nextest/); `00-start-here/CURRENT_TASK.md` gate section + gotchas.
**Effort:** S (nextest adoption) + S (preflight). **Value:** high.

## Finding 7 — Declarative workspace hygiene: `[workspace.lints]`, pinned `rust-toolchain.toml`, `cargo-deny` (code, S)

**Problem.** The `-D warnings` clippy policy lives only in the gate command line; crates carry no inherited lint table; there's no pinned toolchain (Claude, Sol, and any future CI can build with different rustc versions — a real source of "works here, clippy-fails there" gate noise); no license/advisory/duplicate-dep audit despite THE LAW's "lightweight" tenet and the security posture.

**Recommendation.** Three small additions: (1) `[workspace.lints.rust]` + `[workspace.lints.clippy]` in the root Cargo.toml with `[lints] workspace = true` in each crate (note the Cargo book's explicit warning that inheritance is **not** implicit — each member needs the stanza); encode the current de-facto policy (`warnings = "deny"` at CI level, keep local builds warn). (2) `rust-toolchain.toml` pinning the channel so orchestrator/executor/CI compile identically. (3) `deny.toml` + `cargo deny check` in the gate: advisories (RUSTSEC), licenses, and **bans on duplicate/heavy transitive deps** — a mechanical guard for "lightweight" (it would, e.g., flag if a future dep dragged in tokio against the no-async posture that CONTRACTS_M4 §0.4 currently enforces by prose).
 
**LAW fit:** lightweight, secure, congruent, readable — this is THE LAW as configuration. Tension: none.
**Evidence:** root `Cargo.toml` (no lints table, verified); [Cargo lints reference](https://doc.rust-lang.org/nightly/cargo/reference/lints.html); [RFC 3389 manifest-lint](https://rust-lang.github.io/rfcs/3389-manifest-lint.html); [Rust Project Primer lints](https://rustprojectprimer.com/checks/lints.html); [2026 tooling baseline](https://blog.rajpoot.dev/posts/rust/rust-tooling-cargo-2026/); `CONTRACTS_M4.md §0.4`.
**Effort:** S. **Value:** med-high.

## Finding 8 — Escalation ladder is hard-coded route IDs in Rust — move it to data (code, S)

**Problem.** AGENTS.md hard rule: "Catalog/pricing data is data (TOML) — never hard-code model IDs or prices in Rust." Yet the default escalation ladder embeds `"deepseek-v4-flash"`, `"kimi-k2.7-code"`, `"deepseek-v4-pro"` as string literals in `crates/nh-fleet/src/lib.rs:95–107`. When K3/V5 land (the exact scenario Decision 3 anticipates: "Catalogs rot… new models must be a TOML entry, not a release"), the routing *policy* still requires a recompile. Same congruence smell as if prices were in code.

**Recommendation.** Add a `[policy.ladder]` array-of-tables to `catalog.toml` (or `.nosis/policy.toml`): `[[policy.ladder]] route = "deepseek-v4-flash", effort = "none"` …, loaded beside the routes; nh-fleet's `default_ladder()` becomes "read policy table, error with a friendly line if absent" (honest, never guessed — same posture as prices). Config override stays. ~40 lines net, no frozen crate touched (nh-fleet is not frozen; catalog is data by charter).

**LAW fit:** congruent (the finding *is* a congruence repair), simple, auditable. Tension: none.
**Evidence:** `crates/nh-fleet/src/lib.rs:95–107` (verified literals); `05-ai-collaboration/AGENTS.md` (data rule); `02-architecture/ARCHITECTURE_DECISIONS.md` Decision 3.
**Effort:** S. **Value:** med.

## Finding 9 — Split the monolith files before they become the loop's bottleneck (code, M)

**Problem.** `nh-tui/src/lib.rs` is 4,107 lines / 172 functions in one file; `nh-fleet` 2,243; `nh-tools/src/mcp.rs` 1,603; `nh-core` holds `wire`+`receipt`+`agent` inline in one 1,419-line file; `nh-cli/cmd_chat.rs` is 980. This is technical debt accruing precisely where the *build process* pays for it: every Sol brief scoped to "one file" now means shipping a 4k-line context to the executor; every orchestrator review re-reads whole files; slice collision risk (two slices touching different features that live in the same file) grows each milestone — the M3 Slices D/E/F all landed in `nh-tui/src/lib.rs`, and M4 Slice D's forced 2-line `nh-tui` adaptation shows how a single file couples slices. The project's own KV-cache-first philosophy (stable prefixes, minimal dynamic context) argues for smaller, stable modules in its dev loop too.

**Recommendation.** Pure mechanical module splits, no public-API change (re-export from lib.rs), one crate per mini-slice so the diff is verifiably move-only (`git diff --color-moved` + `cargo public-api` proves zero API delta — synergy with Finding 3): nh-tui → `{app, render, worker, modal, commands, identity}`; nh-core → `{wire.rs (or wire/{openai,anthropic}.rs), receipt.rs, agent.rs}`; nh-fleet → `{ledger, workers, schedule, ladder, swarm}`. Do it as an explicit "refactor slice" with a locked no-behavior-change contract — the ideal low-risk Sol task type.

**LAW fit:** readable, modular (two tenets directly); small in *units* even though the diff is large (move-only). Tension: churn risk in git blame — acceptable, do it once between milestones.
**Evidence:** `wc -l crates/*/src/*.rs` (verified counts); `00-start-here/CURRENT_TASK.md` M3/M4 slice descriptions.
**Effort:** M. **Value:** med-high (compounds over every future slice).

## Finding 10 — Consolidate the real verification discipline into the empty EVALUATION_PLAN.md / FAILURE_MODES.md (docs/process, S)

**Problem.** The project genuinely practices verification-first (per-slice E-criteria mapped to real tests, a verify-live ledger for what mocks can't prove — CONTRACTS_M4 §7, "numstat = truth", FEEL gate) — but the canonical architecture docs meant to hold that discipline are empty templates (`EVALUATION_PLAN.md` has literally blank sections; `FAILURE_MODES.md` is a 20-line stub). The real knowledge lives in per-milestone CONTRACTS files and CURRENT_TASK.md, which are *churned* every milestone; the verify-live ledger from M4 will be buried once CONTRACTS_M5 starts. Environment failure modes (AV block, exe lock, codepage mojibake) live in a session-scoped "gotchas" list.

**Recommendation.** One writing pass, no new process: EVALUATION_PLAN.md gets (a) the standing gate definition (test+clippy+gate-script+FEEL), (b) a **cumulative verify-live ledger** (rows migrated from CONTRACTS_M1–M4 §7s: E2 real off-peak dispatch, E4 real OAuth server, Kimi swarm live, KORVIN real connection, DeepSeek peak-window recheck at catalog `valid_until` 2026-07-24), each with status + how-to-verify. FAILURE_MODES.md gets the five already-known modes (AV-blocked fresh exe; nh.exe target lock; codepage mojibake; Bash-tool cd persistence; delegate subprocess fragility from Decision 4) in the doc's own What/Detection/Recovery format. This is exactly the SDD premise: the spec/verification docs are the durable center, not the chat transcript.

**LAW fit:** auditable, congruent (docs promise what practice does), harmonic. Tension: none.
**Evidence:** `02-architecture/EVALUATION_PLAN.md` + `FAILURE_MODES.md` (verified empty); `CONTRACTS_M4.md §7`; `00-start-here/CURRENT_TASK.md` gotchas; [GitHub SDD toolkit rationale](https://github.blog/ai-and-ml/generative-ai/spec-driven-development-with-ai-get-started-with-a-new-open-source-toolkit/); [Spec Kit](https://github.com/github/spec-kit).
**Effort:** S. **Value:** med.

---

## Cross-cutting notes

- **What's healthy and should NOT change:** the single-RouteResolver choke point; the 2-wire-only adapter set with quirks-as-catalog-data; the no-tokio/blocking posture (tiny_http, std threads) — all congruent and cheap; the loopback-mock test style that keeps 292 tests keyless; the amendment ceremony for frozen crates (A-M4-1/2 worked exactly as designed, including the attempt-1 STOP); receipts + scrubber-on-every-output-path.
- **Sequencing suggestion:** Finding 1 immediately (it's a one-command habit); 2 before 2026-07-28; 3+5 together as "gate v2" before CONTRACTS_M5 is written; 4, 6, 7 as one infra mini-slice; 8, 9, 10 opportunistically between M5 slices.
- **Key-reality check:** everything above needs **no new API keys**. The only keyed item mentioned (GLM $0 CI smoke lane from ARCHITECTURE_OVERVIEW) is explicitly deferred and flagged keyRequired=GLM/Z.ai.

## Sources
- https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/
- https://blog.modelcontextprotocol.io/posts/sdk-betas-2026-07-28/
- https://nexte.st/docs/features/retries/
- https://nexte.st/docs/installation/windows/
- https://sunshowers.io/posts/nextest-process-per-test/
- https://blog.jetbrains.com/rust/2026/05/01/faster-rust-tests-with-cargo-nextest/
- https://github.com/rust-lang/cargo/issues/11544
- https://github.com/cargo-public-api/cargo-public-api
- https://github.com/obi1kenobi/cargo-semver-checks
- https://rust-lang.github.io/rust-project-goals/2026/cargo-semver-checks.html
- https://doc.rust-lang.org/nightly/cargo/reference/lints.html
- https://rust-lang.github.io/rfcs/3389-manifest-lint.html
- https://rustprojectprimer.com/checks/lints.html
- https://blog.rajpoot.dev/posts/rust/rust-tooling-cargo-2026/
- https://developers.openai.com/codex/noninteractive
- https://github.com/github/spec-kit
- https://github.blog/ai-and-ml/generative-ai/spec-driven-development-with-ai-get-started-with-a-new-open-source-toolkit/
- https://www.totalum.app/blog/ai-agent-orchestrator-totalum-2026
- https://arxiv.org/html/2605.18747v1
