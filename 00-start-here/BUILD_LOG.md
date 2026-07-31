# Build Log

Record every meaningful session here.

## 2026-07-31: Wave M2 "TOOL FLOOR" — write_file, grep_files, glob_files

What changed (`2e0fea0`, 9 files, +1317/−14, all 9 items, no deferrals):

- `write_file`, **create-only**: refuses an existing path (naming `edit_file` as the alternative)
  and refuses a missing parent directory rather than creating one. Publication reuses `edit_file`'s
  temp-file + fsync + atomic-rename path with cleanup on every error branch.
- `grep_files`: **literal substring, not regex** — stated in the tool description so the model does
  not send `\d+` and silently get nothing. NUL-sniff binary skip, oversized skip, 300-char line
  truncation with an honest `(+N more chars)`.
- `glob_files`: segment-wise with `**` spanning directories, results sorted lexicographically.
  Determinism is deliberate — `read_dir` order is filesystem-dependent and a non-deterministic tool
  result poisons the prefix cache.
- `builtin_tools()` now returns read_file, write_file, edit_file, grep_files, glob_files,
  exec_shell — cheapest and safest first, shell last.
- `nh-core`: `progress_line` reads the `"pattern"` argument key. One line, nothing else.

The case-folding bypass is closed:

- The WARNING left at `nh-tools/src/lib.rs:277` for exactly this wave is **discharged**, and the
  comment updated rather than left to mislead the next reader.
- `creation_guard_verdict` consults the typed path, its ASCII-folded form, and — where an existing
  parent resolves through an in-workdir alias — the actual path and its folded form too.
  `merge_guard_verdict` takes the strictest answer: **Block beats Ask beats Allow**.
- `read_file`, `edit_file` and `load_image` are unchanged; they touch only existing files, so
  `canonicalize` already hands the guard the true on-disk case.
- Two hardenings beyond the brief: `symlink_metadata` makes a symlink at the destination count as
  existing, so create-only refuses it; and the parent directory is canonicalized before creation,
  closing a symlinked-parent escape.

Honest search, no approval storm:

- The walk consults the law per file. `Allow` includes; `Ask` and `Block` **exclude silently and
  increment a counter**. `approve` is never called in the walk — hundreds of per-file prompts would
  train blind approval, a worse security outcome than an excluded file.
- Footer counts, all eight: matches, files visited, files excluded by law, binary skipped,
  oversized skipped, symlinks skipped, and **law-pruned versus default-pruned directories tracked
  separately** — separately because counting files inside a law-blocked directory would mean
  entering it.
- Bounded and iterative: explicit stack, no recursion, no symlink following, 20,000 files and 500
  matches maximum, cap named in the footer when it stops the walk. Nothing silently truncated.
- `target`, `node_modules`, `.venv`, `dist`, `build` pruned by default, disclosed in both the tool
  description and the footer.

Verification:

- `GATE: PASS` — **620 passed / 0 failed / 1 ignored, `--release`** (599 → 620, +21 tests).
- All five steps green: `fmt --check`, `clippy -D warnings`, `rustdoc -D warnings`,
  `cargo deny --locked`, `test --release`. Orchestrator ran the scoped `cargo fmt -p` normalize on
  the four touched crates, per protocol.

Process note — **three clean stops, and every one of them was right:**

- Stop 1: the brief required `nh_law::glob_matches` while forbidding manifest edits. `nh-tools` had
  no `nh-law` dependency, so the brief was unsatisfiable.
- Stop 2: the brief claimed nh-cli asserts tool names but never counts.
  `nh-cli/src/cmd_chat/tests.rs:815` asserted exactly three lines.
- Both errors came from the orchestrator reading the tree and reading it wrong. Sol changed nothing
  and fabricated nothing on either stop. The scope-by-crate rule from wave M1 held: no stop occurred
  for a file inside a fully-in-scope crate.
- **Two more obsolete assertions were found passing** (`nh-tools` three-name list, nh-cli `/tools`
  three-line count), which is the M1 lesson repeating: a passing test proves consistency, not truth.

CI on the pushed commits: Windows, macOS and Supply-chain all green; **ubuntu-latest cancelled at
the 35-minute ceiling for the fifth time.** The hang remains the one hard blocker for `v0.1.0`.

## 2026-07-30: Wave M1 "IMAGES IN" — image input on OpenAI-wire vision routes

What changed (`05c53cc`, 26 files, +948/−41, all 11 items, no deferrals):

- `nh run --image <path>` (repeatable, maximum 4) and `/image <path>` in `nh chat` attach PNG or
  JPEG images to the next user message. The text-only path is unchanged.
- `ChatMessage` gained an optional `parts: Vec<ContentPart>` with `ContentPart { Text, ImageB64 }`.
  When `parts` is absent the serialized request is **byte-identical** to before, asserted literally
  by `parts_free_request_bytes_remain_identical`, so the prefix cache and the PrefixSeal invariant
  are unaffected.
- The OpenAI wire emits the content array only when parts exist, and always as a full
  `data:<mime>;base64,<data>` URI.
- `load_image` in `nh-tools` reuses `read_file`'s workdir boundary and law guard, allowlists PNG and
  JPEG by extension, **verifies magic bytes** so a mislabelled file is refused rather than guessed
  at, and caps raw size at 3.5 MiB. Base64 is a dependency-free RFC 4648 encoder tested against the
  specification vectors.
- Image capability is read from the live catalog and **fails closed before any HTTP call**, at three
  independent layers, naming the image-capable routes in the refusal.
- Compaction's pre-send estimate counts 32 tokens per image part, which brackets the measured 18–29
  token deltas. It is documented as an estimate used only to trigger compaction — never for billing,
  and never shown as measured.
- `nh-fleet`, `nh-tui` and the Anthropic wire received only a one-line `parts: None` initializer. No
  image logic entered any of them.

Catalog correction:

- `mimo-v2.5-pro` declared `modality = ["text","image","video","audio"]`. Xiaomi documents it as
  text-only and a live probe returned `404 No endpoints found that support image input`. Corrected
  to `["text"]` with a dated citation. The `nh-routes` test
  `mimo_routes_preserve_reasoning_and_are_omni_modal` had been **passing** while asserting the false
  claim; it was updated and renamed. **A passing test proves consistency, not truth.**

Wire facts established by live probe on 2026-07-30, not from documentation:

| route | text-only prompt tokens | with image | delta |
|---|---|---|---|
| `kimi-k2.6` | 14 | 43 | +29 |
| `mimo-v2.5` | 254 | 272 | +18 |
| `glm-4.6v-flash` | 13 | 40 | +27 |

- All three fold image tokens into `usage.prompt_tokens`; none exposes a separate image-token field.
  `Usage` already parses that, so **receipts are honest for images with zero costing change.**
- `kimi-k2.6` rejects a bare base64 string — the `data:` prefix is mandatory. `glm-4.6v-flash`
  accepts both forms, so one code path serves all three.
- Images coexist with a `tools` array on `kimi-k2.6` (verified, prompt=72).
- Free `glm-4.6v-flash` is heavily rate-limited; the probe needed four retries at 6/12/24/48s.
- Total probe spend was well under $0.01.

Verification:

- `GATE: PASS` — **599 passed / 0 failed / 1 ignored, `--release`** (579 → 599, +20 tests).
- All five gate steps green: `fmt --check`, `clippy -D warnings`, tests, `rustdoc -D warnings`,
  `cargo deny --locked`.

Known limitations, both non-blocking:

- Part ordering is emitted text-first but was only measured image-first. One sub-cent call settles
  whether ordering changes the token count.
- MiMo documents an 8192-pixel minimum per image and the harness does not decode image dimensions,
  so a very small but otherwise valid image may be refused by the provider rather than by `nh`.

Next step:

- `05c53cc` and the checkpoint commit `3e40c36` are **committed and not pushed**. `origin/main` is
  still at `52314a3`. The push needs owner action.

## 2026-07-29: Wave 4 strict release Clippy gate cleared

What changed:

- Replaced the one-pattern JSON parse `match` in `agent/tool_repair.rs` with the equivalent
  `if let` fast path.
- Grouped the private `make_receipt` helper's six per-run values into a private `ReceiptFields`
  struct. Receipt fields and cache-hit percentage derivation remain unchanged.
- Added no lint suppression, dependency, test change, commit, push, or tag.

Verification:

- `cargo clippy --locked --workspace --all-targets --release -- -D warnings` — PASS.
- `cargo fmt --all --check` — PASS. Write-mode `cargo fmt` was not run.
- `cargo test --locked --workspace` — PASS: 579 passed / 0 failed / 1 ignored.

Next step:

- The orchestrator reviews and commits the complete Wave 4 working tree.

## 2026-07-29: Env-gated raw provider-usage diagnostic

What changed:

- Added the display-only `NH_DEBUG_USAGE=1` diagnostic to both OpenAI-compatible and Anthropic
  Messages clients. Each response writes one stderr record prefixed with route, wire, and
  request sequence, followed by the provider's raw top-level `usage` value.
- Preserved the raw JSON value through serde's borrowed `RawValue`, so unknown fields, ordering,
  spacing, and nested provider data survive without changing `WireUsage`, `Usage`, metering,
  pricing, receipts, routing, or control flow.
- Routed the complete diagnostic line through `nh_vault::Scrubber` with the active credential
  literal before stderr. Only the `usage` value is observed; response content and headers are
  never included. Missing usage renders the explicit `usage absent` message.
- Kept the disabled path allocation-, parsing-, and formatting-free at request time. Route and
  scrubber state are created only when the environment value is exactly `1`, and stderr write
  failures are ignored so the observer cannot replace a provider result.
- Documented the switch in the existing operator environment-variable table. No new dependency
  was added; the existing `serde_json` dependency enables its `raw_value` feature, and
  `Cargo.lock` is unchanged.

Verification:

- Focused raw-usage diagnostics â€” PASS: 4 passed / 0 failed.
- `cargo fmt --all --check` â€” PASS. Write-mode `cargo fmt` was not run.
- `cargo test --locked --workspace` â€” PASS: 550 passed / 0 failed / 1 OS-keyring test ignored.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` â€” PASS.
- `git diff --check` â€” PASS.
- No commit, push, tag, release, pricing change, usage-field acceptance change, or frozen
  `crates/nh-mcp/tests/e3_korvin.rs` edit was made.

Next step:

- Run the three owner-controlled live provider probes with `NH_DEBUG_USAGE=1`, then use the
  measured raw field shapes as the sole evidence for the separate metering-fix wave.

## 2026-07-29: Explicit local-model lane wave

What changed:

- Added the ratified `local` route class. Local routes remain explicitly selectable through
  `--model` and `/model`, while API-only defaulting, capable-route resolution, cheapest-capable
  comparison, escalation, and the top-tier cost anchor exclude them.
- Restricted local routes to the existing OpenAI wire at literal-loopback origins and kept the
  normal exact-origin vault lookup. No listener, credential bypass, discovery, or new wire was
  added.
- Accepted Ollama's `message.reasoning` as an additive alias for the existing
  `reasoning_content` field, with coverage for both response shapes.
- Rendered the ratified local meter copy verbatim instead of presenting unmetered hardware cost as
  `$0.00`, across run, chat, price, session, and TUI surfaces.
- Added commented llama.cpp and Ollama catalog templates. Machine-dependent `model_id`, `context`,
  and mandatory `max_out` remain symbolic for the user to fill rather than guessed.
- Added `06-operations/LOCAL_MODELS.md` covering the existing vault flow, llama.cpp as the
  fail-closed reference path, Ollama's undetectable silent-truncation hazard, manual artefact
  verification and licensing, reference-machine sizing, KV-cache math, dated model-selection
  examples, and the owner-run live checks.

Verification:

- Focused affected-crate suites — PASS.
- `cargo fmt --all --check` — PASS. Write-mode `cargo fmt` was not run.
- `cargo test --locked --workspace` — PASS: 546 passed / 0 failed / 1 OS-keyring test ignored.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — PASS.
- `git diff --check` — PASS; existing line-ending warnings remain informational.
- No dependency, commit, push, tag, release, or frozen `crates/nh-mcp/tests/e3_korvin.rs` change was
  made by this wave.

Next step:

- The owner fills one commented local template with the exact loaded model limits, copies the
  trusted catalog into the user-global catalog, grants its exact loopback origin in the user-global
  law, and runs the five documented live checks against the chosen server.

## 2026-07-28: Provider-truth metering wave

What changed:

- Reclassified both MiMo routes to the existing exact Kimi thinking-toggle wire shape, so the
  normal no-thinking posture explicitly disables MiMo's provider-default thinking.
- Enabled state-aware reasoning replay on all four DeepSeek routes and explicitly disabled
  provider-default thinking on DeepSeek's Anthropic wire.
- Added GLM-5.2 disable and normalized High/Max effort controls, and made the effective effort
  resolver report the tier actually sent.
- Engaged K2.6 preserved thinking with `thinking.keep = "all"` only while thinking is enabled;
  MiMo's shared toggle dialect deliberately emits no `keep` field.
- Classified GLM sensitive/context/network finish reasons and DeepSeek resource interruption into
  the existing filtered, context, and constraint receipt classes.
- Added the first-party-verified `kimi-k3` route with its dated 1,048,576-token limits, multimodal
  capability, always-thinking Low/High/Max effort dialect, preserved reasoning, and confirmed
  $0.30/$3.00/$15.00 per-million-token price data.
- Left P-4 completely unchanged: no live Kimi cache-hit probe was supplied, so the documented
  top-level `usage.cached_tokens` field remains intentionally unimplemented.

Verification:

- Focused `nh-routes`, `nh-core`, and `nh-fleet` suites — PASS.
- `cargo fmt --all --check` — PASS. Write-mode `cargo fmt` was not run.
- `cargo test --locked --workspace` — PASS: 537 passed / 0 failed / 1 OS-keyring test ignored.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — PASS.
- No dependency, commit, push, tag, release, or frozen `crates/nh-mcp/tests/e3_korvin.rs` change was
  made by this wave.

Next step:

- The owner runs the three sub-cent live probes. A confirmed Kimi cache hit is required before
  implementing P-4; the MiMo token-field and DeepSeek Anthropic model-identity probes remain
  separate provider verification items.

## 2026-07-28: Owner FEEL wave v0.1.0 blockers fixed

What changed:

- Added the supplied conservative `context` and `max_out` caps for all three free GLM routes,
  keeping each pair together and adding a resolver regression for capability selection.
- Added reusable TUI model/provider/profile pickers. Model rows keep every catalog route and mark
  relative price, currency, stale or unknown price, and unknown context honestly. Provider
  discovery checks route-scoped credential usability before terminal takeover and lists only
  usable providers. Typed slash-command behavior and model-switch history remain unchanged.
- Wrapped TUI tools with exact start/finish events and render the active tool name plus elapsed
  seconds while it is running; no percentage or synthetic progress is shown.
- Centralized a tool-result authority rule in the agent identity constitution: contradicted
  process/server/file/system state cannot be asserted, and timeout, kill, and non-zero exits must
  be reported as failures. `nh run`, `nh chat`, route switches, and the TUI share the rule.
- Corrected `nh mcp serve --help` to list all six runtime tools and replaced the raw integer
  `--max-turns` bound with the designed inclusive range 1–100.
- Made CLI shell approval fail closed before reading when stdin is not a terminal. Interactive
  explicit yes remains unchanged. This is defense-in-depth: exploitation required
  attacker-influenced content to already be piped into `nh run`.
- Moved `nh run` metering to stderr, leaving stdout answer-only, and updated its end-to-end
  contract test.

Verification:

- Focused nh-routes regression, full nh-tui suite, and full nh-cli suite — PASS.
- `cargo fmt --all --check` — PASS. Write-mode `cargo fmt` was not run.
- `cargo test --locked --workspace` — PASS; one OS-keyring test remains intentionally ignored.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` — PASS.
- No dependency, commit, push, tag, release, or frozen `crates/nh-mcp/tests/e3_korvin.rs` change was
  made by this wave.

Next step:

- Rebuild the optimized executable and repeat the owner FEEL gate before any release authorization.

## 2026-07-27: Cross-client continuation checkpoint saved

What changed:

- Replaced the stale 2026-07-26 `CONTINUE_HERE.md` with one authoritative, self-contained
  checkpoint covering the exact uncommitted tree, completed A+ work, verification evidence,
  public-empty GitHub repository/security state, release-process state, remaining owner gates, and
  ordered release continuation.
- Updated the repo-root `AGENTS.md` resume banner and added a minimal root `CLAUDE.md`, so typing
  `continue` in either Codex or Claude routes to the same checkpoint before any action.
- Added a current override to `CURRENT_TASK.md`; its older checkpoints remain historical provenance
  and no longer compete with the continuation instructions.
- No product code, dependency, Git commit, remote content, tag, or release changed. The agent did
  not stop the open harness; during final validation PID 91116 had exited independently, and no
  `nh` process remained.

Verification:

- `cargo test --workspace` — PASS: 514 passed / 0 failed / 1 OS-keyring test ignored.
- `cargo clippy --workspace --all-targets -- -D warnings` — PASS.
- Continuation pointers were read back as UTF-8 and checked for the required state, constraints,
  owner-only gates, and exact next sequence.

Next step:

- The owner can return in Codex or Claude and type `continue`. The next agent must read
  `CONTINUE_HERE.md` first, preserve the tree, verify no `nh` process has reopened, rebuild the
  canonical release, and launch the post-refactor Windows FEEL window. If a new harness is open,
  ask before stopping it.

## 2026-07-27: Public GitHub repository bootstrap

What changed:

- Reauthenticated GitHub CLI as `arparvar` and verified membership in the `nosistech`
  organization.
- Created the empty public repository `nosistech/nosis-harness` and attached it locally as
  `origin`. Verified the repository is public, empty, and administered by the authenticated
  account. No source, commit, tag, release, or secret was pushed.
- Enabled private vulnerability reporting, Dependabot vulnerability alerts and automated
  security updates, secret scanning, and secret-scanning push protection before the first push.
- Kept Issues enabled, disabled unused Wiki/Projects/Discussions surfaces, selected squash-only
  merges with automatic branch cleanup, and added accurate Rust/CLI/agent/metering topics.
- Left the existing release harness process open. Branch protection and required checks remain
  pending because the empty repository does not yet have a `main` branch.

Verification:

- `gh repo view nosistech/nosis-harness` — public, empty, `viewerPermission: ADMIN`.
- `origin` — `https://github.com/nosistech/nosis-harness.git` for fetch and push.
- GitHub API — private vulnerability reporting, Dependabot security updates, secret scanning,
  and push protection all enabled.
- `.github/dependabot.yml` — Cargo and GitHub Actions weekly update ecosystems are configured
  locally and will activate after the first push.
- `cargo test --workspace` — PASS: 514 passed / 0 failed / 1 OS-keyring test ignored.
- `cargo clippy --workspace --all-targets -- -D warnings` — PASS.

Next step:

- Replace the open stale release executable with a canonical build, complete the owner's Windows
  FEEL pass, then obtain explicit authorization to commit and push. After `main` exists, run remote
  CI and enforce its green jobs with branch protection before tagging.

## 2026-07-27: A+ readability, auditability, and release-mode hardening

What changed:

- Preserved every public command, wire shape, receipt/ledger shape, and route decision while
  splitting mixed implementation files at responsibility boundaries:
  - `nh-core::wire::{http,openai,anthropic}` and `agent::context`;
  - `nh-routes::resolver::catalog`;
  - `nh-cli::cmd_run::{config,meter}` and `cmd_chat::startup`;
  - `nh-tools::mcp::client::oauth`;
  - `nh-fleet::scheduler`;
  - `nh-tui::input::commands`, `render::transcript`, and a `WorkerSession` state machine.
- Moved remaining large inline test bodies out of production modules and replaced production
  wildcard imports with explicit imports. Crate roots remain thin public facades.
- Removed every Rust `#[allow(...)]` exception. Named callback types replace repeated trait-object
  signatures; Fleet dispatch and budget transitions now live with the scheduler state they mutate.
- Removed avoidable runtime panics at HTTP-client construction, MCP token creation, poisoned
  synchronization boundaries, worker joins, route ordering, terminal input queues, and process
  result handling. Added poisoned MCP cache/OAuth regression tests. Static compiled-data
  invariants remain explicit `expect` sites.
- Kept `unsafe_code = "forbid"` workspace-wide; no first-party `unsafe` block exists.
- Added strict rustdoc to CI and `gate.ps1`, and removed two stale unused license allowances from
  `deny.toml`.
- Documented the exact module ownership map and runtime-file behavior. Normal execution creates no
  generated source or cache: only intentional, gitignored append-only receipts/Fleet audit state,
  which remains operator-retained until deletion.
- The release gate exposed and fixed one refactor regression: debug-only Fleet test-provider
  constants were imported in optimized non-test builds. Imports are now gated identically to the
  seam, and the release-only test proves the switch is unavailable.
- Added no dependency. No automatic artifact pruning was introduced because it would weaken the
  durable audit trail.

Verification:

- `cargo fmt --all --check` — PASS. Write-mode `cargo fmt` was not run.
- `cargo test --workspace` — PASS: 514 passed / 0 failed / 1 OS-keyring test ignored.
- `cargo clippy --workspace --all-targets -- -D warnings` — PASS with zero local lint allowances.
- `cargo clippy --locked --workspace --all-targets --release -- -D warnings` — PASS.
- `cargo test --locked --workspace --release` — PASS: 515 passed / 0 failed / 1 ignored, including
  the release-only test-provider refusal. The live `target/release/nh.exe` was left untouched; the
  suite used an isolated ignored target directory, which was deleted immediately afterward.
- `RUSTDOCFLAGS="-D warnings" cargo doc --locked --workspace --no-deps` — PASS.
- `cargo deny --locked check --hide-inclusion-graph` — advisories, bans, licenses, and sources
  PASS. Remaining notices are upstream transitive duplicate versions already governed as warnings.
- The live harness process remained open and responsive throughout verification. No commit was
  created.

## 2026-07-26: v0.1.0 local release-candidate evidence

What changed:

- Rolled the complete first-public-release changelog into `[0.1.0] - 2026-07-26` and left a new
  empty `[Unreleased]` section.
- Updated the release checklist only for controls backed by current evidence. Remote CI, public
  repository settings, the owner's terminal FEEL pass, release commit/push/tag, and publication
  remain deliberately unchecked.
- No product logic, dependency, catalog, wire-format, or persistence-format change was made.

Verification:

- Ran the complete checked-in `gate.ps1` on `e42a5bc`: formatting check, locked strict release
  Clippy, `cargo deny` advisories/bans/licenses/sources, and locked release tests all PASS.
  Tests: 512 passed / 0 failed / 1 OS-keyring test ignored. The first sandboxed attempt could not
  lock the read-only user advisory cache; the identical full script passed with normal cache access.
- Built the exact optimized binary with `cargo build --locked --release`.
- Release binary SHA-256:
  `225248E64F16050072201E42398692F8F9812E800E44912C9BBD22E62D1C337C`.
- Smoke-tested the optimized binary:
  - `nh --version` prints `nh 0.1.0`.
  - `nh why "quick task"` returns a cheapest-capable explanation without network or key access.
  - `nh init` is idempotent in an isolated Git repository.
  - A pre-existing user hook is preserved byte-for-byte and produces the manual-chaining warning.
  - `nh mcp serve --addr 0.0.0.0:8765` refuses before binding.
  - Both isolated smoke repositories were removed after verification.
- Scanned all 67 reachable commits with the canonical key-shape family without printing candidate
  values. Reviewed 2,663 historical occurrences across 259 unique redacted contexts; all were
  ordinary-word substring false positives or explicit redaction/security test fixtures. No
  credential was found.
- Audited every freshness date: production catalog data remains valid through 2026-08-02; fixtures
  evaluated for freshness use far-future dates or injected clocks, while the remaining dated
  fixture is parse-only and cannot age into a behavior change.

Remaining before tag:

- Complete and record the owner's subjective terminal FEEL pass.
- Configure the intended public remote and valid GitHub authentication, replace local changelog
  version links, push `main`, and obtain green Windows/Ubuntu/macOS/supply-chain CI.
- Configure branch protection, private vulnerability intake, repository metadata, and required
  checks; then tag and publish.

## 2026-07-26: Responsibility-boundary modularity refactor

What changed:

- Reorganized the largest first-party crates by responsibility while preserving their public
  facades and behavior:
  - `nh-tui`: input, render, session, state, palette, timeline, terminal, and worker.
  - `nh-core`: agent loop, wire protocol, receipts, credentials, and runtime paths.
  - `nh-fleet`: model, engine, preparation, ledger, and run/resume orchestration.
  - `nh-mcp`: protocol dispatch, route tools, receipt tools, Fleet tools, responses, and transport.
  - `nh-tools`: MCP config/client/adapter, shell execution, and the built-in tool facade.
  - `nh-routes`: route vocabulary, pricing, resolver, profiles, and the banned-ID boundary.
  - `nh-law`: policy model, layered loading/compilation, and pure matchers.
- Extracted embedded tests from production modules throughout the workspace, including the CLI and
  vault. Large test suites remain intentionally grouped where that improves invariant visibility;
  they no longer inflate or couple the production modules.
- Updated `02-architecture/ARCHITECTURE_OVERVIEW.md` with the exact internal ownership map so the
  code boundaries are discoverable without reverse-engineering the module tree.
- Kept existing external paths available through thin re-exports. This was a structural change,
  not a command, wire-format, persistence-format, or entitlement change.
- Strengthened the central route invariant by colocating `ResolvedRoute` with `RouteResolver`;
  its fields remain private, and tests now exercise public accessors instead of weakening the
  resolver-only construction boundary.
- Added no dependency and made no manifest change in this refactor. The owner then explicitly
  passed the FEEL/commit gate and requested the coherent commit; no tag was created.

Verification:

- `cargo test --workspace` — PASS (512 passed, including the resolver compile-fail doctest;
  1 OS-keyring test ignored, 0 failures).
- `cargo clippy --workspace --all-targets -- -D warnings` — PASS.
- `cargo test --workspace --release` — PASS with the same totals.
- `cargo clippy --workspace --all-targets --release -- -D warnings` — PASS.
- Direct `rustfmt --check` over every crate Rust source — PASS. `cargo fmt` was deliberately not
  run; direct `rustfmt` was scoped to files created or structurally rewritten in this session.

Result:

- Production code is now organized around stable responsibilities rather than historical file
  growth. The public crate architecture and the internal module architecture both satisfy the
  repository's small/simple/readable/auditable/modular principles; this supersedes the earlier
  note below that named Fleet, MCP, route, and TUI coordinators as future split candidates.

## 2026-07-26: Public-v0.1 hardening — audit remediation, safe modularization, and release gate

What changed:

- Closed the pre-release audit's load-bearing security paths with regression coverage:
  exact HTTPS origin/port authorization before provider-key materialization; resolver-only route
  minting; symlink/size/containment checks for repository instructions and runtime state; mandatory
  approval at the `exec_shell` operation boundary; bounded remote/request/tool/task/output bodies;
  typed finish-reason handling; checked usage arithmetic; locked, scrubbed receipts; worker/server
  shutdown ownership; and repository MCP/catalog configuration that can only tighten trusted
  operator configuration.
- Replaced ordinary credential copies with zeroizing `SecretValue`/`SecretRegistry` ownership for
  active routes. Added `nh key remove <entry>` and made the generated Git hook consume the same
  canonical secret-shape registry as runtime redaction.
- Added per-call output caps, Fleet task/count/id/file caps, mandatory Fleet budgets, and an MCP
  Fleet budget ceiling. Provider-side account alerts/hard limits remain operator-owned controls.
- Reverified the production catalog against first-party provider pages on 2026-07-26, normalized
  current prices to USD, removed unsupported historical peak windows, and set 2026-08-02 as the
  next mandatory recheck date. Synthetic peak-pricing tests now use far-future fixture dates.
- Kept routing behavior honest: execution remains explicitly selected; `nh why` is advisory and
  cannot silently dispatch a different route.
- Removed Telegram end to end in the following owner-directed slice (documented separately below).
- Made focused modularity extractions instead of a high-risk cosmetic rewrite:
  `nh-core::{credential,runtime_path}` and `nh-tui::{terminal,worker}` now own shared security and
  lifecycle invariants. Large Fleet, MCP, route, and TUI coordinator modules remain candidates for
  later characterization-first splits; they are not launch-blocking correctness changes.
- Hardened GitHub Actions with exact action commit pins, read-only permissions, no persisted
  checkout credential, locked dependencies, timeouts, concurrency cancellation, Windows/Linux/
  macOS jobs, a supply-chain job, and weekly Dependabot configuration.
- Replaced blank operations templates and corrected security, privacy, architecture, product,
  release, and master-plan claims so they describe the current local CLI rather than planned
  hosted, delegate, sandbox, telemetry, snapshot, or automatic-routing features.
- Marked all workspace crates `publish = false`; the documented distribution is a reviewed source
  release, not nine independently publishable crates.io packages.
- Made the debug-only MCP/Fleet echo integration test explicitly debug-only. Release builds retain
  and pass the separate proof that the test-provider switch is unavailable.

Verification and evidence:

- Personally re-derived and executed the top three negative regressions:
  exact-origin mismatch is refused before vault access; an out-of-repository `AGENTS.md` symlink is
  refused; and `Guard::Allow` still cannot bypass explicit shell approval.
- Scanned all 66 reachable commits using the canonical key-shape registry. Only deliberate
  fake/redaction fixtures matched; no candidate live credential was found.
- `cargo test --locked --workspace`: 512 passed, 0 failed, 1 ignored (real OS-keyring mutation).
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: pass.
- `cargo fmt --all --check`: pass. Formatting drift was corrected manually; `cargo fmt` was not run.
- `cargo clippy --locked --workspace --all-targets --release -- -D warnings`: pass.
- `cargo deny --locked check`: advisories, bans, licenses, and sources all pass. It reports only
  non-blocking duplicate transitive Windows-support crates and unused license allowances.
- `cargo test --locked --workspace --release`: 512 passed, 0 failed, 1 ignored.
- Release binary smoke: `nh 0.1.0`; `nh why "quick task"` succeeds without a key; `nh init` is
  idempotent in a disposable Git repository; `nh mcp serve --addr 0.0.0.0:8765` refuses before
  serving; the disposable repository was removed and no audit artifact remains.

External release gates still owned by the operator:

- This checkout has no Git remote, so the configured Windows/Ubuntu/macOS CI has never run on a
  pushed release commit. Branch protection, required checks, private vulnerability intake, and the
  public repository metadata therefore cannot yet be verified.
- The owner must complete the Windows FEEL pass. Linux and macOS remain unverified and must not be
  advertised until their remote jobs and platform smoke tests pass.
- Provider-account hard spend limits/alerts and a post-hardening live provider smoke are external
  checks. No paid provider call was made in this hardening session.
- The official MCP project still labels 2026-07-28 as an RC on 2026-07-26. `nh mcp serve` remains a
  loopback-only preview; the normal CLI/TUI release is independent.

No commit or tag was created, `cargo fmt` was not run, and the owner-held uncommitted FEEL-gate tree
was preserved.

## 2026-07-26: Public-v0.1 hardening — remote notifications removed by owner decision

What changed:

- Removed the TUI Telegram surface end to end: `NotifyConfig`/parser, `notify.toml` loading, bot
  credential lookup, sender thread, HTTP POST, failure channel, and notification-specific tests.
  The existing local approval bell and taskbar/status transitions remain unchanged.
- Removed `nh-tui`'s now-unused direct `reqwest` and `toml` dependencies.
- Updated the canonical plan, roadmap, milestone and architecture views. Historical M3 records were
  preserved and marked superseded. Recorded why the feature was removed and the narrow conditions
  for a future explicit opt-in integration.

Why:

- Remote notification is outside the harness's central invariant and added credential,
  destination-config, privacy, dependency, background-thread, and outbound-network attack surface.
  Its strongest walk-away justification was not wired to headless Fleet, and the real send remained
  verify-live. Public v0.1 keeps the local signal without owning that remote surface.

Checks run:

- `cargo test -p nh-tui`: 69 passed, 0 failed.
- `cargo test -p nh-cli cmd_tui`: 3 passed, 0 failed.
- Full workspace test/clippy and release gates remain pending for the larger uncommitted hardening
  session.

No commit was created and `cargo fmt` was not run.

## 2026-07-20: Release Slice — LIVE provider tests (launch evidence) — no commit-of-code; docs-only

Orchestrator (Opus 4.8) ran the four-provider live smoke against the shipped `target/release/nh.exe`
(the MCP-tools build). Identical tiny prompt each time ("Answer in one short sentence: what does a
compiler do?"), `--max-turns 2`. Keys resolved from the OS vault (env `NH_*_KEY` fallbacks all unset).
`.nosis/receipts.jsonl` is gitignored → the appended receipts leave the tree clean.

Results (all `outcome: pass`, 1 turn, 0 tool calls):

| Provider | Route | This-turn cost | usd_approx | top-tier counterfactual | wire verified |
|---|---|---|---|---|---|
| GLM / Z.ai | `glm-4.7-flash` | **$0.00** (free) | — | $0.0024 | free path OK |
| DeepSeek | `deepseek-v4-flash` | **¥0.0025** | ≈$0.0003 | ¥0.0074 | `thinking:{type:disabled}` (`--think none`) ✅ |
| Kimi / Moonshot | `kimi-k2.6` | **$0.0009** | (USD native) | $0.0018 | `kimi-toggle` (default think) ✅ |
| MiMo / Xiaomi | `mimo-v2.5` | **$0.0002** | (USD native) | $0.0025 | dialect `none` OK |

**Total real spend ≈ $0.0014 across all four** — priciest single provider (Kimi $0.0009) is ~2200× under
the $2/provider HARD CAP.

Honest-meter invariants verified LIVE (launch evidence):

- **Cross-currency refusal** — `nh why "refactor a 200-line rust module and add unit tests"` picked
  `mimo-v2.5` (cheapest capable, ~1037 tok, $0.0003 est) and REFUSED to rank the DeepSeek routes:
  "¥0.0041 vs chosen $0.0003 — different currency, not directly comparable" (no fake FX for the compare).
  It also honestly skipped the three GLM free routes as "unknown context" (no `context` field → can't
  confirm the task fits → not auto-selected; explicit `--model glm-4.7-flash` still runs).
- **`usd_approx` only on fresh fx** — DeepSeek ¥0.0025 → ≈$0.0003 because catalog `[fx]` `valid_until
  2026-07-24` is fresh (today 07-20); ¥0.0025 × 0.139 = $0.000348 ✓. Native CNY stays the billed truth.
- **Peak pricing applied honestly** — the DeepSeek turn stamped 03:36Z = 11:36 Asia/Shanghai, inside the
  09:00–12:00 peak window, so the 2× is already in the actual cost and the `peak` counterfactual equals it
  (¥0.0025) — an honest "we ARE at peak", not a fabricated markup. Base would have been ~¥0.0012.
- **No invented savings** — free GLM prints a real `$0.00` next to `top-tier $0.0024`; every run renders
  the full `cost · peak · no-cache · top-tier` line; `no-cache == cost` at 0% cache is correct.
- **VERIFY-LIVE §7 wire shapes CONFIRMED** — the two guesses carried since Slice A both accepted by the
  real APIs with no 400: DeepSeek `thinking:{type:disabled}` (via `--think none`) and the Kimi K2.6 toggle.
- **Typed receipts written** — each run appended `{ts_utc, model_id, task, turns, tool_calls, outcome,
  usage{prompt/completion/cached}, effective_profile:"balanced"}` to `.nosis/receipts.jsonl`.

Not exercised (out of scope for a cost smoke): identity honesty on the contaminated providers
(DeepSeek/MiMo self-ID as "Claude") — the prompt was purely factual. Release Slice remainder: **W4**
(nh-tui+nh-cli surfaces, FEEL — the last Sol code wave) + the cosmetic `print_banner` 6-tool fix.

---

## 2026-07-20: Release Slice — Section B: engineering tail (forbid-unsafe + workspace lints + MIT license + cargo-deny gate + keyless CI) — committed `cccb2dc`

Builder:

- Codex (GPT-5.6 Sol xhigh) — from `Temp/section-b-brief-v1.txt`. Status PASS, all 4 parts, no
  deferrals. Self-report `Temp/section-b-last-message.txt`.

What changed (release engineering tail; METADATA / CONFIG / CI ONLY — no source, no behavior, no new deps):

- **Forbid unsafe (DRY, one source of truth):** root `[workspace.lints.rust] unsafe_code = "forbid"` +
  `[lints] workspace = true` in all 9 crates (chosen over per-file `#![forbid]` for auditability;
  confirmed clean — zero direct `unsafe` in-tree, nothing needed a code change).
- **License:** `license = "MIT"` in `[workspace.package]` + `license.workspace = true` in each crate.
- **Supply-chain gate REAL + GREEN:** deny.toml activated (dormant note removed); cargo-deny 0.20.2 =
  advisories/bans/licenses/sources all ok. Only policy delta = allow `CDLA-Permissive-2.0` (webpki-roots
  trust-anchor data); **no RustSec advisory found or ignored** (`[advisories] ignore` stays empty —
  nothing suppressed). Wired as a 4th `Invoke-GateStep 'deny check' { cargo deny check }` in gate.ps1,
  between clippy and test; existing steps + exit-code aggregation untouched.
- **Path-dep versions:** 27 internal path deps gained `version = "0.1.0"` (cargo-deny `wildcards=deny`
  treats a versionless path dep as `*`); dependency resolution + Cargo.lock unchanged.
- **Keyless CI** (`.github/workflows/ci.yml`): `contents: read`; a `checks` matrix over win+ubuntu on
  pinned 1.96.0 running the gate's 3 checks (+ Linux `libdbus-1-dev` for the keyring backend) + a
  separate `supply-chain` cargo-deny 0.20.2 job. No secrets — the suite is offline/mocked.

Orchestrator (Opus 4.8): briefed the wave, launched the single background Sol run (xhigh), ran the
authoritative gate.ps1 (now FOUR steps — fmt+clippy+deny+test all exit 0, **GATE: PASS 416/0/1**),
adversarially reviewed the diffs (forbid-unsafe in place; deny genuinely green with NOTHING suppressed;
gate aggregation intact; CI keyless + minimal perms), and committed. Docs-close also refreshes
RELEASE_CHECKLIST (cargo-deny now wired; forbid via workspace lints). Deferred to backlog:
cargo-nextest, AV canary, gate frozen-surface sensor.

Gate: **416 pass / 0 fail / 1 ignored** (`--release`), clippy -D + fmt clean, **cargo deny check green**
(advisories/bans/licenses/sources ok). 13 files, no source changes. Release Slice next: **W4**
(nh-tui+nh-cli surfaces, FEEL — last Sol wave) + live provider tests (<$2/provider). MCP not public
before 2026-07-28.

---

## 2026-07-20: Release Slice — MCP metered-service expansion (why/route_cost/receipts + structuredContent) — committed `7c2b2c4` (feat) + `b708b8c` (docs C+D)

Builder:

- Codex (GPT-5.6 Sol) — MCP executor from `Temp/release-mcp-brief-v2.txt` (v1 brief + the
  owner-authorized amendment header). The FIRST run STOPPED CLEAN and reverted on a frozen test
  (`crates/nh-mcp/tests/e3_korvin.rs` asserts the exact old 3-tool set); the amendment authorized
  widening ONLY that assertion. **This run resolved effort = `xhigh`** (owner switched from `max`
  after the Fable window closed). Self-report `Temp/mcp-last-message-v2.txt` (`-o` capture); status
  PASS, no deviations.

What changed (expresses nosis's one differentiator — "priced, routed, receipted, and you can see
why" — as first-class MCP tools with STRUCTURED output; additive, existing tool TEXT byte-compatible):

- **why (flagship; mirrors `nh why`):** `resolve_capable(prompt,output,allowed,now)` over the caller's
  allowed set (defaults to available priced API routes) → `naive_cost` + `saved_pct`. structuredContent
  = route / cost (usd_approx only on FRESH fx) / savings (OMITTED when naive None) / rejected[] from the
  RejectionTrace. Text never claims a saving it did not compute.
- **route_cost:** `resolve(model|default)` → `price_at` → `cost_of(prompt,cached,output)`; quote + cost;
  usd_approx omitted when fx absent/stale.
- **receipts:** reads `<run_root>/.nosis/receipts.jsonl` READ-ONLY (limit clamp 1..=100; missing →
  count 0). Torn last line tolerated, file NEVER mutated; typed via `nh_fleet::LedgerEvent::TaskReceipt`
  (no new dep). Secrets in task text redacted in BOTH text + structuredContent.
- **route_resolve / fleet_status (enhanced):** text byte-identical (fleet_status finished line too);
  structuredContent added alongside (route + would_park_offpeak; run_id/state/counts/failed_reason).
- **Egress choke:** new `tool_result` scrubs text (`safe_line`) AND `scrub_json`'s the structured value
  (strings, array elements, object values AND keys) before returning content+structuredContent+isError.

Orchestrator (Opus 4.8): assembled the v2 brief, re-launched the single background Sol run (xhigh),
ran the authoritative `gate.ps1` (fmt drift → normalizing `cargo fmt -p nh-mcp` under pinned 1.96.0 →
re-gate; Sol ran no fmt), adversarially reviewed (ZERO blocking — every checkpoint invariant verified
vs code + a dedicated test: structuredContent always scrubbed; receipts read-only + never mutates +
redacts in both surfaces + missing→0; fleet_status finished line byte-identical; loopback + banner
unchanged; `why` == `resolve_capable`; usd_approx omitted when fx stale; `e3_korvin` edit = +3/−0 tool
names only), and committed. Docs C+D (CHANGELOG / README / PRIVACY / CONTRIBUTING / RELEASE_CHECKLIST)
were drafted by a **Fable 5 ultracode docs workflow** (5 writers, each independently accuracy-verified
against the real CLI), orchestrator spot-checked (gate.ps1, rust-toolchain.toml, catalog.toml) and
committed `b708b8c`.

Gate: **416 pass / 0 fail / 1 ignored** (`--release`), clippy `-D warnings` clean,
`cargo fmt --all --check` clean; +6 nh-mcp tests; nh-mcp ONLY (`src/lib.rs` +826/−19, `e3_korvin.rs`
+3), no Cargo.toml, no new deps. Server stays stateless, loopback-only, preview (NOT public before
2026-07-28). Release Slice next: live-verify MCP → live provider tests (<$2/provider) → Section B
(forbid-unsafe / cargo-deny / keyless CI) → W4 (nh-tui+nh-cli surfaces, FEEL).

---

## 2026-07-20: M5 Slice F Wave 5 — FLEET RELIABILITY (audit W5-1..W5-11) — committed `441727b`

Builder:

- Codex (GPT-5.6 Sol xhigh) — W5 executor from the seam-by-seam brief
  (`Temp/slice-f-w5-brief-v1.txt`). All 11 items W5-1..W5-11 landed in **one** Sol run, no
  deferrals. Machine-readable self-report `Temp/w5-last-message.txt` (`-o` capture). Lock
  strategy shipped = std `File::try_lock` (the PRIMARY, not the heartbeat fallback).

What changed (makes the fleet layer crash-safe and honest: two coordinators can't corrupt one
ledger, a killed run auto-recovers, torn writes are read-tolerant without mutation, a
budget-halted fleet can't hang, and a dead run is a first-class failure the caller sees). First
substantive reopening of the M4-frozen nh-fleet crate, under amendment A-M5-8:

- **Single-writer lock (W5-1/W5-3):** OS advisory lock on `<run_dir>/coordinator.lock` via std
  `File::try_lock`. `run`/`run_with_id` fail-fast; `resume` bounded-retries ~2s then errors
  "run appears live (pid N, started …)". `truncate(false)` + `set_len(0)` only AFTER acquisition
  (never clobbers a live coordinator's diagnostics); RAII drop + OS-release-on-kill = auto
  recovery. Index appends serialized under a separate blocking `index.lock`.
- **Readers never mutate (W5-2):** `read_ledger`/`read_run_ledger`/`latest_incomplete_run` now
  `fs::read` + pure `parse_jsonl`; `repair_uncommitted_tail` confined to write paths under the
  lock. Torn final line (unparseable + no trailing `\n`) skipped read-only; mid-file /
  committed-invalid lines stay hard errors.
- **RunFailed ledger event (W5-7):** `run_with_id`/`resume` split into a lock-acquiring outer +
  inner; on Err the inner best-effort appends `LedgerEvent::RunFailed{run_id,reason}` (scrubbed).
  Closes the surfacing W2 deferred (CLI + MCP). `status_from_ledger`: a later `RunFinished`
  supersedes it (`failed_reason` = None once complete).
- **Budget drain + honesty (W5-8/W5-9):** `halt_remaining_for_budget` fires every over-budget
  iteration (drains re-queued in-flight failures that hung the run — high-6). A `usage=None`
  receipt adds zero tokens (never fabricated), warns "usage unreported — budget cannot count …",
  and surfaces a derived `FleetStatus.unmetered` count (low-25).
- **Resume fidelity + path safety (W5-4/W5-5/W5-6):** `TaskQueued` gains `#[serde(default)]`
  `defer_offpeak`/`backend` + a `QueuedTask` carrier → resume restores the real backend + off-peak
  intent (was hardcoded Native/config). `latest_incomplete_run` validates the run_id it reads
  (rejects `../evil`). `run()`'s duplicate validation deleted — `run_with_id` is the single check
  (so an empty / `max_workers==0` run now records a RunFailed under the lock; intended).
- **Surfaces (W5-10/W5-11):** nh-cli `--escalate`/`--defer-offpeak` → `Option<bool>`
  (`flag_or_file` = CLI-over-file; `--defer-offpeak=false` now overrides a file `true`). nh-mcp
  `fleet_status` renders `failed: <reason>` + `· N unmetered`; finished line byte-identical.

Orchestrator (Opus 4.8): briefed the wave (A-M5-8), launched the single background Sol run,
ran the authoritative `gate.ps1` (fmt drift → normalizing `cargo fmt` under pinned 1.96.0 →
re-gate; Sol ran no fmt), adversarially reviewed (ZERO blocking issues — all six focus areas
verified vs code + a dedicated test: lock blocks a 2nd coordinator + auto-recovers on kill; the
high-6 drain ends the hang via a 30s `recv_timeout` test; readers never mutate the ledger;
`RunFinished` supersedes `RunFailed`; `tokens_in(usage=None)==0`; finished-status shape
byte-identical), and committed. **Public surface additive:** `LedgerEvent += RunFailed`;
`FleetStatus += failed_reason/unmetered`; `TaskQueued += defer_offpeak/backend` (serde-default,
old ledgers still decode). `fleet_kill_resume.rs` untouched; no Cargo.toml; no new deps.

Gate: **410 pass / 0 fail / 1 ignored** (`--release`), clippy `-D warnings` clean,
`cargo fmt --all --check` clean; +15 tests; 6 files +976/−76. nh-fleet is now REOPENED under
A-M5-8 — the last frozen crate touched by Slice F. Order W1✓→W3✓→W2✓→W5✓→**W4** (surfaces,
FEEL), now folded into the owner-directed Release Slice.

---

## 2026-07-19: M5 Slice F Wave 2 — TOOL EGRESS + EXEC (audit W2-1..W2-18) — committed `2e09513`

Builder:

- Codex (GPT-5.6 Sol xhigh) — W2 executor from the seam-by-seam brief
  (`Temp/slice-f-w2-brief-v1.txt`). All 18 items W2-1..W2-18 landed in **one** Sol run, no
  deferrals, one self-corrected compile slip. Self-report `Temp/w2-last-message.txt`.

What changed (makes tool egress + local exec SAFE: every byte a tool sends outbound goes
through one capped, scrubbed choke point; every command runs bounded and killable):

- **nh-tools (egress + exec):** ALL tool results now pass through `ToolResultEnvelope`
  (bound + shape-scrub) at the single MCP-adapter egress choke point, using the session
  `ToolCtx` scrubber (W2-1, high-3). Windows `ExecShell` runs the approved command via
  `cmd /C` + `raw_arg` verbatim — no re-quoting differential (W2-2, high-4). Exec gets null
  stdin, concurrent pipe drains, a dep-free 300s timeout, and whole-tree kill+reap on
  deadline (W2-3, medium-4). `ToolCtx` gains a `scrubber` field + `with_scrubber` builder
  (`ToolCtx::new` signature UNCHANGED so nh-fleet is untouched); the three interactive
  callers (cmd_run, cmd_chat, nh-tui) install key-literal scrubbers, nh-fleet keeps the
  default shape-only scrubber (W2-4, medium-5). Startup `tools/list`/discover uses a distinct
  10s HTTP client, not the 600s live-call timeout (W2-5). MCP egress consults
  `Access::Send(url)`: **Block stops the call before trust; default Allow is byte-identical**
  to before, and Ask reuses the existing approval (W2-6, low-6). Rotated refresh-token
  persist failure emits one secret-free scrubbed warning instead of swallowing (W2-7); a
  dedicated refresh `Mutex` coalesces concurrent 401 refreshes to a single token POST (W2-8).
  read_file and each exec stream retain at most 2 MiB before the 32k envelope elision (W2-9).
- **nh-mcp (inbound server):** `fleet_run` runs a **synchronous** route/task/key/run-directory
  preflight and rejects a bad config to the caller before spawning (W2-10, medium-11). The
  bearer token is 32 `getrandom` CSPRNG bytes → 64 hex (W2-11, low-17), compared with
  `subtle::ConstantTimeEq` on equal-length input (W2-13, low-19). `fleet_status` distinguishes
  an unknown run (no directory) from an existing-but-empty run = starting (W2-12, low-18).
  Request bodies are capped at 1 MiB → 413 on overflow (W2-14); a non-shutdown accept error
  emits one scrubbed warning before exit (W2-15); one `Scrubber` is built at bind and reused
  (W2-16). The `State`/`From` duplication is replaced by a `Runtime` holding
  `Arc<ServeConfig>`, a plain bound token `String`, and the shared scrubber (W2-17, nit-9);
  routing accepts ONLY exact `GET /.well-known/mcp.json` + `POST /mcp`, else 404 (W2-18).

Frozen surfaces preserved byte-stable: **nh-fleet untouched → still NO A-M5-8** (it builds
`ToolCtx` via `ToolCtx::new(...).with_guard(...)` and takes the default scrubber);
`ToolCtx::new`'s signature is unchanged. New public surface is additive only:
`ToolCtx.scrubber` field + `ToolCtx::with_scrubber` builder.

Owner ratified 4 design calls: (1) low-6 GATE MCP egress with `[send]` — a `Block` verdict
stops egress before trust; a default/`Allow` path is byte-identical to pre-W2; (2) low-17/-19
use `getrandom` (CSPRNG token) + `subtle` (constant-time bearer) — vetted primitives over
hand-rolled crypto; (3) nit-8/-9 INCLUDE the nh-mcp `State`→`Runtime` refactor + scrubber-once;
(4) medium-4 dep-free exec timeout (300s), null stdin, whole-tree kill.

Two sound deviations from the brief (both improvements, both in the commit): (a) W2-17 keeps
`ServeConfig.token` as `Option<String>` — the caller's input means "mint one if absent"; the
guaranteed-`Some` bound token lives on the new `Runtime` struct instead, so the public config
type is unchanged for scoped callers; (b) W2-12/W2-10 use nh_fleet's REAL `.nosis/fleet/{run_id}`
ledger path rather than the brief's approximate `run_root.join(run_id)`.

Deferred to W5/A-M5-8 (owner-authorized frozen-boundary stop): a `fleet_run` that fails AFTER
the synchronous preflight now emits a scrubbed warning, but cannot be returned to the original
caller without an nh-fleet **RunFailed ledger event** — nh-fleet is frozen, so that surfacing
is now formally W5 work under amendment A-M5-8.

New deps (nh-mcp direct edges only): `getrandom` 0.2.17 + `subtle` 2.6.1 — both already
transitive, so zero added build weight (§0.4 W2 exception). Cargo.lock change is exactly those
two edges.

Gate: **395 pass / 0 fail / 1 ignored** (`--release`), clippy `-D warnings` clean,
`cargo fmt --all --check` clean (Sol did not run fmt; no drift to normalize this wave). +18 new
tests over 377. 8 files, +981/−175. Adversarially reviewed by the orchestrator (Opus 4.8):
**zero blocking issues** — the egress choke point is the only path to `render()`, `[send]` Block
provably stops the call before trust, timeout kills the whole tree + prevents a late marker,
constant-time compare only on equal-length input, exact-route matching rejects everything else.

Next: **W5 "FLEET RELIABILITY" (nh-fleet)** — the one wave that REQUIRES amendment **A-M5-8**
(nh-fleet is frozen), including the RunFailed ledger contract W2 deferred above. Order now
W1✓→W3✓→W2✓→**W5**→W4.

## 2026-07-19: M5 Slice F Wave 3 — METER TRUTH (audit W3-1..W3-14 + A-M5-9) — committed `73d278b`

Builder:

- Codex (GPT-5.6 Sol xhigh) — W3 executor from the seam-by-seam brief
  (`Temp/slice-f-w3-brief-v1.txt`), plus a gated **W3b** addendum
  (`Temp/slice-f-w3b-brief-v1.txt`) that extended the A-M5-9 glue to `nh chat` + `nh profile`.

What changed (makes the meter — the numbers nosis shows and bills against — TRUE;
"the meter must not lie, in EITHER direction"):

- **nh-core (turn loop + wire math):** DROP the compaction cost guard that defeated the
  0.70 trigger — compaction now fires on a normal uniform-turn history instead of only at
  ~100% (post-overflow hard-fail) (W3-1, high); the trigger counts `max(provider prompt
  count, fresh local estimate)` so a just-appended large tool result is seen (W3-2).
  `resolve_effort` gains a trailing `Wire`: `AnthropicMessages` → `None` ("provider-default",
  the only tier matching a wire that sends no thinking directive) (W3-4, A-M5-9); DeepSeek
  explicit `Low` → disabled tier, matching the wire (W3-3). `cache_hit_pct` → `None` (not a
  fabricated 100%) when `cached > prompt` (W3-5). Receipt-append failures warn via `emit`
  without discarding a paid answer or shadowing the real provider error (W3-6). Both HTTP
  clients propagate a response-body read failure through `send_error` instead of an empty
  body (W3-7). Anthropic `tool_use` fails locally on a missing/empty id or name (W3-8).
  Removed the unused `prompt_cache_miss_tokens` field (W3-9); extracted `push_user_block`
  (W3-10).
- **nh-routes (honest routing/cost):** `resolve_capable` compares native costs within one
  currency and normalizes cross-currency only through FRESH catalog FX; stale FX → REFUSE
  the non-comparable route (trace `"fx stale"`) rather than compare ¥ against $ — the
  `"x price"` ratio is emitted only same-currency, and the trace prints native amounts
  cross-currency (W3-11, high). Catalog parsing rejects a provider that mixes currencies
  (W3-13). Optional-profile warnings carry the underlying read/parse error (W3-12); a shared
  `min_cap` helper replaces two clamp match blocks (W3-14).
- **nh-cli + nh-tui (A-M5-9 glue, incl. W3b):** wire-aware effort threaded through EVERY
  surface that resolves it — `nh run`, TUI, `nh chat` (cmd_chat), `nh profile` (cmd_profile).
  `cmd_run::effort_for` folded the `Wire` param in and dropped the transient
  `effort_for_wire` wrapper (mirrors nh-tui's `effort_for`, no test-only helper). W3b is
  display-only: the Anthropic wire body never serializes the effort, so the bytes are
  unchanged.

Frozen surfaces preserved byte-stable: **nh-fleet untouched → still NO A-M5-8** (it has its
own `effort_for` and calls neither `resolve_effort` nor `cache_hit_pct`); `resolve_capable`
and `cache_hit_pct` return types unchanged; the only public signature change is
`resolve_effort`/`effort_for` gaining a trailing `Wire`.

Owner ratified 3 design calls: (high-1) drop the compaction cost guard — avoiding a hard
context overflow beats keeping the prefix cache warm; (med-2, A-M5-9) wire-aware
`resolve_effort` returning `None` for AnthropicMessages; (high-2) normalize cross-currency to
USD via FRESH fx, fail-safe REFUSE when fx is stale, trace in native amounts. Plus the
owner-approved W3b glue extension (A-M5-9 → cmd_chat + cmd_profile).

Process: first W3 gate FAILed on `fmt --check` only (3 hand-reflow drifts); orchestrator ran
the normalizing `cargo fmt` under pinned 1.96.0 and re-gated PASS (Sol never runs fmt). W3b
had one self-corrected compile slip (new test referenced `ThinkArg` at the crate root; fixed
to `crate::cmd_run::ThinkArg`). 7 files, 676/107.

Gate (W3 + W3b combined): **377 pass / 0 fail / 1 ignored** (`--release`), clippy `-D warnings`
clean, `cargo fmt --all --check` clean. Adversarially reviewed by the orchestrator (Opus 4.8):
compaction fires on a realistic uniform history, no raw ¥-vs-$ compare + fail-safe on stale fx,
OpenAi effort cells unchanged / AnthropicMessages → None, receipt-append non-fatal (paid answer
+ real provider error both survive a directory-path append failure).

Next: **W2 "TOOL EGRESS + EXEC" (nh-tools + nh-mcp)** per the ratified order W1→W3→**W2**→W5→W4.

## 2026-07-19: M5 Slice F Wave 1 — SECURITY FLOOR (audit W1-1..W1-13)

Builder:

- Codex (GPT-5.6 Sol xhigh) — W1 executor from the locked seam-by-seam brief
  (`Temp/slice-f-w1-brief-v1.txt`), with one owner-approved mid-run scope amendment.

What changed (hardens the credential broker + policy engine against a hostile repo;
every fix fails safe — refuse/escape/block on ambiguity):

- nh-vault: `normalized_host` now parses with the `url` crate (WHATWG parity with
  reqwest) and is `pub` — closing the credential-exfil differential where
  `https://evil.example\@api.deepseek.com/v1` passed the broker as `api.deepseek.com`
  while reqwest dialed `evil.example` (W1-1). `audience_allows` fails CLOSED for
  undeclared/unparseable entries (W1-6); IPv6 audiences match via one idempotent
  normalization (W1-2). Refusals carry a typed `AudienceRefused` error, ending the
  `"refused:"` string coupling (W1-8). Key-shape alternation is `\b`-anchored so
  `risk-`/`desk-` no longer corrupt displays/ledger (W1-3); both sanitizers escape
  bidi format chars (Trojan-Source, W1-4); stored literals are `Zeroizing<String>`
  (W1-5); `EnvFallbackVault` surfaces the real key-store error (W1-7).
- nh-law: glob/segment matchers rewritten ITERATIVELY (two-pointer backtrack, O(1)
  stack) — a 60k-segment path / 200k-char segment can no longer overflow the
  main-thread stack (W1-9); semantics byte-identical. `send_verdict` normalizes its
  target (lowercase + trailing dot) so a block binds (W1-10); exec matching
  case-folds and unwraps cmd/sh/bash wrappers and `&&`/`;`/`|` chains (W1-11).
  BUNDLED_LAW parsed once, not twice (W1-12); repo-weaken warning fires only on an
  actual grant, not field presence (W1-13).
- nh-cli: deleted the duplicate `host_of` parser; all three uses route through
  `nh_vault::normalized_host` (W1-1e). cmd_chat classifies a fatal refusal by
  downcast to `AudienceRefused`, not a string prefix (W1-8).
- Scope amendment (owner-approved): `nh-cli/tests/m2_exit.rs` gained an isolated
  temp user-law declaring the synthetic entry's audience — required by W1-6
  fail-closed, security-neutral, and it removed the test's prior leak to the real
  home dir.

Frozen surfaces byte-stable: `Scrubber::new(Vec<String>)`, `send_verdict(&self,&str)`;
nh-fleet untouched (no A-M5-8). `url` is a direct dep of nh-vault now (already
transitive via reqwest 2.5.8 → zero build weight); Cargo.lock gains only that edge.

Tests/checks (authoritative gate, `gate.ps1`, on the committed tree):

- `cargo fmt --all --check`: clean (orchestrator ran the normalizing `cargo fmt`
  post-Sol under pinned 1.96.0 — Sol never runs fmt; file-set did not expand).
- `cargo clippy --workspace --all-targets --release -- -D warnings`: clean.
- `cargo test --workspace --release`: **363 passed / 0 failed / 1 ignored**.

Next step:

- Ground + brief W3 "METER TRUTH" (nh-core + nh-routes), the next wave in the
  ratified order W1→W3→W2→W5→W4.

## 2026-07-18: M5 Slice E — LOOP hygiene, PARTIAL (E5) — committed `68f71cd` + `059a00e`

*Backfilled 2026-07-25 from the commit record (`68f71cd`, `059a00e`, docs-close `6a11f32`);
no BUILD_LOG entry was written at the time.*

Builder:

- Not recorded at the time. Neither commit message nor the docs-close names an executor.
  Both commits are repo-tooling/formatting only and touch no crate logic. (The same session
  logged the standing rule that Sol must NOT run `cargo fmt` — formatting is the gate's job.)

What changed:

- `68f71cd` — one-time `cargo fmt --all` normalization of the 37-hunk / 7-file backlog that
  had accumulated because the workspace was never fmt-clean, so any scoped `cargo fmt -p`
  reflowed pre-existing code and polluted slice diffs (it bit Slice A and Slice D). Pure
  behavior-preserving reflow across nh-cli (`cmd_init`, `cmd_key`, `m2_exit`), nh-fleet,
  nh-tools (`lib`, `mcp`), and nh-vault.
- `68f71cd` — added `gate.ps1`, mechanizing the three checks that define "clean": `fmt
  --check`, `clippy -D warnings`, `test --release`, with per-step exit-code aggregation
  (never `| tail`, whose 0 would mask a real failure). This is the Slice E "fmt --check gate"
  item, pulled forward to kill the reflow pitfall at its root.
- `059a00e` — `rust-toolchain.toml` pins 1.96.0 + rustfmt/clippy so fmt/clippy/build are
  reproducible on every machine and in CI, and a future rustfmt cannot silently re-introduce
  the reflow drift the normalize just cleared.
- `059a00e` — `.gitattributes` makes line-ending handling explicit and portable (LF in repo,
  native checkout; Windows scripts CRLF) so fmt never churns on EOL.
- `059a00e` — `deny.toml`, a DORMANT cargo-deny supply-chain policy (advisories / bans /
  licenses / sources). cargo-deny was not installed; wiring `cargo deny check` into `gate.ps1`
  was left as follow-up. No crate source touched.

Tests/checks:

- After `68f71cd`: `cargo test --workspace --release`: **357 passed / 0 failed / 1 ignored**;
  clippy `-D warnings` clean.
- For `059a00e`: no gate run is recorded — the commit touches no crate source.
- Owner FEEL gate: not recorded at the time (Slice E changes no human-facing surface).

Not delivered in this pass — Slice E is PARTIAL and E5 is NOT met:

- The `gate.ps1` frozen-surface / allowed-files sensor, which is the E5 acceptance test (an
  out-of-surface edit must fail the gate). `gate.ps1` as shipped here runs fmt/clippy/test only.
- Keyless CI (windows + ubuntu); `codex exec --output-schema` structured Sol self-report;
  `cargo-nextest` + AV-canary preflight; `[workspace.lints]` + `forbid(unsafe_code)`; wiring
  cargo-deny into the gate. All recorded as "rest of Slice E" in `6a11f32`.
- Later record: forbid-unsafe, workspace lints, the cargo-deny gate step, and keyless CI
  landed in the Release Slice Section B commit `cccb2dc` (see its entry above). The
  frozen-surface sensor, nextest/AV canary, and `--output-schema` remain undelivered.

Next step:

- Owner expanded the scope: a full Fable 5 high read-only audit was launched in the background
  (workflow run `wf_72da5ecf-6f6`). Triage its findings, then `[workspace.lints]` +
  `forbid(unsafe_code)`, then the rest of Slice E "LOOP".

## 2026-07-18: M5 Slice D — LEVER (E4 implementation)

Builder:

- Codex (GPT-5.6 Sol xhigh) — Slice D executor from the locked seam-by-seam brief.

What changed:

- Added embedded, user, and repository execution profiles with deterministic
  tighten-only repository layering. Profiles clamp only thinking posture and
  output cap on the already-selected route.
- Added the route-legal posture/effort matrix, receipt profile field, and
  profile-aware client construction on run, chat, route-switch, reconnect, and
  TUI worker paths.
- Added `--profile` to run/chat/TUI, the read-only `nh profile` listing,
  `/profile` live switching, and the HUD profile chip.
- Refined the `nh profile` and TUI `/profile` displays to report the route-resolved
  effective thinking effort (`none` / `low` / `high` / `max`) instead of the
  abstract profile posture; added the shared additive `nh-core::wire::effort_label`.
- Applied the A-M5-7 compile glue to exhaustive `AgentLoop` and `Receipt`
  literals, including the three authorized frozen nh-fleet additions.

Tests/checks:

- Scoped `cargo fmt -p` was run for nh-routes, nh-core, nh-cli, nh-tui, and
  nh-fleet; pre-existing formatter-only drift outside the slice was removed.
- `cargo test --workspace --release`: **357 passed / 0 failed / 1 ignored**
  after the display refinement.
- `cargo clippy --workspace --all-targets --release -- -D warnings`: clean
  after the display refinement.
- Keyless `nh profile` smoke on a toggle-capable route printed effective
  thinking efforts `none` / `none` / `high` for the three bundled profiles.
- `cargo test --workspace`: **357 passed / 0 failed / 1 ignored**.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- Orchestrator adversarial review and owner FEEL approval of `nh profile`, the
  `/profile` confirmation, and the HUD chip before the Slice D commit.

## 2026-07-18: M5 Slice C — VISIBLE (E3 implementation) — THE FEEL GATE — committed `a0f77be`

*Backfilled 2026-07-25 from the commit record (`a0f77be`, docs-close `213ed0a`); no BUILD_LOG
entry was written at the time.*

Builder:

- Codex (GPT-5.6 Sol xhigh) — Slice C executor from a seam-by-seam brief, in two handoffs
  (the full slice, then a sub-cent honesty fix that live testing exposed); gated,
  adversarially reviewed, and live-verified by the orchestrator.

What changed:

- Money HUD: currency cost (cached/miss/output split) + per-currency session total, replacing
  the token-only line; an honest-stale (`*`) flag on verify_live prices; the token budget
  hard-stop kept.
- The counterfactual savings line (the aha): cost + "saved N% vs no-cache" over catalog price
  × JSONL tokens, with a peak / no-cache / top-tier breakdown. A cold turn makes no false
  claim. New pure `cost_of` / `naive_cost` / `saved_pct` in nh-routes.
- Approximate USD gloss: native currency stays the billed truth; `~$` is omitted when the rate
  is stale or absent and is never FX-summed across CNY/USD (per-currency subtotals). New
  `[fx]` catalog data + `Fx` type (A-M5-6).
- Adaptive money precision: a real sub-cent spend never renders as `$0.00` — only a genuinely
  free route shows `$0.00`, so the meter cannot lie by rounding.
- `/why` (TUI) + `nh why` (CLI): live `resolve_capable` + `RejectionTrace` explain the chosen
  route and every route it beat.
- Approval cluster (L6 fixed): y/a/n/Esc only, any other key is a no-op (never a silent deny);
  always-this-session rule; visible legend; Esc-to-interrupt.
- Working heartbeat; OSC 9;4 Windows taskbar semaforo; an errors-that-teach helper.
- drop-if-hard: "Esc to stop" while working was dropped — there is no truthful cooperative
  cancel path, so the harness does not claim an interrupt that does not work.

Tests/checks:

- `cargo test --workspace --release`: **339 passed / 0 failed / 1 ignored** (319 → +20:
  Slice C +17, sub-cent fix +3).
- `cargo clippy --workspace --all-targets --release -- -D warnings`: clean.
- Live-verified with the owner's real GLM key: free `glm-4.7-flash` ran end-to-end and printed
  an honest `$0.00`; paid `glm-5.2` returned a clean "insufficient balance" error; `nh why`
  ran live.
- **Owner FEEL-approved before commit.** FEEL finding worth keeping: at 2 dp a real ~$0.003
  turn rounded to `$0.00`, fixed by the adaptive precision above.
- Still open at close: a live money / savings-% demo needs a key with paid balance (a free GLM
  key caps the display at `$0.00`).
- Amendment A-M5-6 (USD gloss + `[fx]` catalog data + the no-cache-headline FEEL ruling)
  logged in CONTRACTS_M5 §8.

Next step:

- Brief Sol for Slice D "LEVER" (E4, profiles).

## 2026-07-18: M5 Slice B — FLOOR (E2 implementation)

Builder:

- Codex (GPT-5.6 Sol xhigh) — Slice B executor from the locked seam-by-seam brief.

What changed:

- Added law-backed read/send verdicts and credential audiences. `read_file` now
  consults the guard before I/O; bundled law blocks repository metadata, runtime
  state, environment files, private keys, and certificates from reads.
- Bounded and shape-scrubbed built-in tool results with head/tail excerpts and a
  full-result digest; approved shell commands now inherit only a minimal environment.
- Sanitized untrusted MCP descriptions/schema strings, widened secret-shape
  redaction, and seeded session scrubbers from all resolvable catalog vault entries.
- Added the credential-audience broker and enforced it on CLI routes and MCP
  configuration before secrets materialize; repository law cannot approve audiences.
- Made nh-mcp fail closed with an OS-seeded bearer token plus strict loopback
  Host/Origin checks while retaining the loopback bind and preview banner.
- Added the OAuth resource indicator and authenticated the existing E3 MCP fleet test.
- Kept nh-fleet frozen except for the two authorized read/send guard arms.

Tests/checks:

- `cargo test --workspace --release`: **319 passed / 0 failed / 1 ignored**.
- `cargo clippy --workspace --all-targets --release -- -D warnings`: clean.
- Scoped rustfmt check exposed the repository's known pre-existing formatting drift;
  no formatter was applied, avoiding unrelated changes in touched/frozen crates.

Next step:

- Orchestrator adversarial review and commit gate for Slice B, then Slice C VISIBLE.

## 2026-07-18: M5 Slice A — TRUTH (E1 implementation) — committed `68f91e6`

*Backfilled 2026-07-25 from the commit record (`68f91e6`, docs-close `7404878`); no BUILD_LOG
entry was written at the time.*

Builder:

- Codex (GPT-5.6 Sol xhigh) — Slice A executor from the locked seam-by-seam brief, in two
  handoffs (truth-math + resolver, then an Anthropic-wire fix); gated and adversarially
  reviewed by the orchestrator. Changes confined to the CONTRACTS_M5 §0.1 Slice A seams plus
  amendments A-M5-1/2/3.

What changed:

- nh-core meter-math, L1 `apply_thinking`: explicit `thinking:{type:disabled}` for None/Low on
  `deepseek-nhm` (it was omitting the field, so the provider auto-escalated), plus a new
  kimi-toggle dialect for K2.6.
- L2 reasoning replay is now conditional on the effective thinking state
  (`preserve_when_thinking`) — K2.6 thinking+tools no longer errors.
- L7 `compact_history`: the elision note is a NEW message and the retained messages stay
  byte-identical (cache-safe), with a cache-aware trigger. L8 `estimate_tokens` counts
  preserved reasoning + serialized tool specs. L9 output cap on BOTH wires (OpenAI now sends
  `max_tokens`; Anthropic is no longer clamped to 8192). L12 PrefixSeal enforced in ALL builds
  plus a cache-break signal (was debug-only).
- Added the `effective_context` clamp (context-rot guard) and a native
  `prompt_cache_hit/miss_tokens` fallback parse.
- `build_anthropic_body` merges consecutive user-role blocks, fixing an L7 regression that
  emitted two consecutive user messages after compaction (rejected by the Anthropic wire;
  found in review, A-M5-3).
- nh-routes honest resolver: `resolve_capable` + `RejectionTrace` — the cheapest
  context-fitting priced `Api` route by expected cost, with an auditable per-route skip trace
  (reuses `price_at`; no jurisdiction and no learning, those are M6). Added the `KimiToggle`
  dialect variant and the `preserve_when_thinking` route field (A-M5-1).
- Frozen-crate glue (A-M5-2): one behavior-preserving `KimiToggle` arm in `effort_for` in
  nh-fleet, nh-tui, and nh-cli (a toggle model defaults to no-thinking).

Tests/checks:

- Tests +14 over the 292 baseline.
- `cargo test --workspace --release`: **306 passed / 0 failed / 1 ignored**.
- `cargo clippy` with `-D warnings`: clean.
- No owner FEEL gate — Slice A has no human-facing surface (that is Slice C).
- Two `[VERIFY-LIVE §7]` wire shapes remained unconfirmed pending a live key: the DeepSeek
  explicit non-thinking shape and the Kimi K2.6 toggle shape.
- nh-core and nh-routes carry incidental cargo-fmt normalization of pre-existing code; the
  frozen crates were reverted to fmt-clean HEAD with only the glue arm re-applied.
- Process lesson recorded: the workspace was clippy-clean but never fmt-clean, so a
  `cargo fmt --all` run mid-gate reformatted the entire workspace and polluted the diff across
  frozen crates. Rule captured — never `cargo fmt --all` mid-slice; use scoped `cargo fmt -p`.
  Slice E is to add an `fmt --check` gate plus a one-time workspace normalization.

Next step:

- Brief Sol for Slice B "FLOOR" (E2).

## 2026-07-17: M4 CLOSED (Slice D committed `9344251`) + M5 direction research

**M4 is complete.** Slice D (OAuth2 MCP client, E4) committed `9344251` after a clean re-gate
(nh-tools 56/0 incl. the E4 crux, nh-tui 46/0, clippy clean; the only red is the 2 pre-existing
Kaspersky-AV-blocked spawn tests, os error 5, byte-identical nh-cli). CONTRACTS_M4 §8 gained the
as-implemented A-M4-1 clarification (OAuth2 is a struct variant; forced the authorized 2-line
`nh_tui::mcp_state` adaptation; A-M4-2 was a no-op — `Vault::set` existed since M0). All four M4
slices (A fleet, B scheduler/ladder/swarm-seam, C nh-mcp, D OAuth2) now committed.

**Deep improvement research (committed `a2c2b83`).** Owner-commissioned "deepest + richest" pass on
how to improve the harness (product + process), run on TWO models: **Fable 5 (high)** web-cited
July-2026 across 13 lenses (A-M), and **GPT-5.6 Sol (xhigh)** design pass over the crate code
(60-item backlog). 265 unique sources; exact line-number grounding. Report:
`00-start-here/RESEARCH_2026-07_harness.md`; raw files: `04-research/_harness-research-2026-07/`.
Both models independently converged on the product identity (**"the metered harness"**) and the top
priority (**make the meter true + visible + safe before adding autonomy or providers**). The pass
surfaced ~12 live code issues (thinking-defaults cost bug; kimi-k2.6 thinking+tools error path;
`read_file` has no law guard; credential-audience exfil; nh-mcp inbound no-auth; any-key-denies
approval bug; compaction mutates prefix → 120× cache miss; sessions RAM-only), a new differentiator
(**privacy-aware routing** — all keyed providers are Chinese/train-on-API; GLM=SG doesn't), the moat
(**learning router** off receipts already written), and one worthwhile new key (**GLM/Z.ai**, free).

**M5 direction chosen: "The Honest Meter"** (see CURRENT_TASK). Five congruent slices: A Truth
(fix the cost/correctness bugs), B Floor (security), C Visible (money HUD + savings line + /why),
D Lever (profiles + output caps + cache-aware compaction), E Loop-hardening (CI + gate.ps1 + wip
rule + nextest). Awaiting owner ratification, then CONTRACTS_M5 + Sol briefs. Build loop unchanged:
Claude plans + gates, Sol (gpt-5.6 xhigh) implements.

## 2026-07-16: M4 Slice C — nh-mcp stateless server + fleet handle seams (E3 gated)

Builder:

- Codex (GPT-5.6 Sol xhigh) — sole Slice C implementer from the locked brief.

What changed:

- Added the blocking, loopback-only `nh-mcp` library with the stateless 2026-07-28 JSON-RPC wire,
  optional bearer auth, well-known business card, scrubbed responses, and no session header or
  initialize handshake. The only tools are `route_resolve`, `fleet_run`, and `fleet_status`.
- Added the four locked nh-fleet seams: caller-provided run IDs, public ID minting, run-ledger reads,
  and a pure status fold. Existing `run` validation and behavior remain intact through delegation.
- Added `nh mcp serve` with the two-line local-preview banner, vault-backed optional token, default
  loopback address, and focused Clap parsing coverage.
- Added E3: the existing `nh_tools::mcp::McpClient` acts as KORVIN, starts a two-task echo fleet,
  polls it to `finished` with `2 done`, and proves the raw response has no session ID header.
- Frozen crates and repo-root `catalog.toml` are untouched.

Tests/checks:

- `cargo test --workspace --release`: **292 passed / 0 failed / 1 ignored** (+8 over Slice B).
- `cargo clippy --workspace --all-targets --release -- -D warnings`: clean.
- Sol's sandbox lacked network/TLS for the new crate; the orchestrator (Opus 4.8) re-ran both gates on a
  clean crates.io registry — `tiny_http 0.12.0` compiled against the Cargo.lock checksum (`389915df…`),
  292 pass / 1 ignored, clippy clean. No vendored source retained.
- FEEL driven through the real `nh mcp serve` binary over live HTTP: two-line banner; `tools/list` shows
  the 3 tools with readOnlyHint + ttlMs; `route_resolve` → one scannable line (`route <id> · <provider> ·
  <dialect> thinking · <peak line>`, `+ would park until off-peak` when preferred); `fleet_run` →
  `run_id=…`; `fleet_status` polled to `finished · 2 done`; `-32601`/`-32700` + `../escape` → `invalid
  fleet run id` + empty-tasks all one honest line; response headers carry no `Mcp-Session-Id`. Bind is
  127.0.0.1-only (non-loopback `--addr` hard-rejected).

## 2026-07-16: M4 Slice B — off-peak scheduler + escalation ladder + Kimi swarm seam (E2 gated), commit `25bd5b3`

Builder:

- Claude (Opus 4.8, Claude Code) — M4 orchestrator: briefed Sol from CONTRACTS_M4 §"Slice B",
  verified empirically (numstat = truth), adversarial review, FEEL demo through the real `nh` binary,
  gate, commit. Also authored the one-round follow-up brief (resume-continues-ladder + effort-in-line).
- Codex (GPT-5.6 Sol xhigh) — implementer, two `codex exec` background runs (main pass + follow-up).

What changed (all in `crates/nh-fleet` + additive `nh-cli`; **frozen crates + catalog.toml untouched**):

- **Off-peak scheduler (E2).** Injected `Clock` trait (`SystemClock` default) + pure
  `ready_to_dispatch(route, now)` reusing frozen `nh_routes::ResolvedRoute::price_at` (off-peak and
  no-price routes dispatch; peak routes park). Coordinator re-checks parked tasks on a 100 ms
  `recv_timeout` tick (no busy-spin). `--defer-offpeak` (run) + per-task `defer_offpeak`. One-line
  FEEL: `deferred <id> — peak 2x until <HH:MM local>, parked` (reuses the M3 `peak_status` chip).
  **E2 test:** injected `MockClock` parks at peak → advance to off-peak → dispatches → `TaskDone`.
- **Escalation ladder.** Pure `next_step(ladder, tier, attempt, outcome) → Retry/Escalate/Gate/Done`,
  ≤2 tries/tier; default ladder `flash/none → k2.7/high → v4-pro/high → v4-pro/max → Opus review-pause
  GATE`. Live-wired into `execute_tasks`: `Fail|Timeout` receipts climb (each `TaskEscalated` carries a
  typed reason; the failure `Receipt` is already durable as the preceding `TaskReceipt` — never a raw
  transcript); terminal `TaskGate` populates `RunReport.gated`. **Infra `Err` terminates immediately**
  (the ladder climbs model failures, not faults). `--escalate` opts in; per-task `model` rejected with
  one friendly line. Escalation line shows effort both sides (`…-pro/high → …-pro/max`).
- **Resume continues the climb.** `RunStarted` carries an additive `#[serde(default)] escalate` flag;
  `resume()` self-derives the effective ladder from the ledger (config wins) and reconstructs each
  interrupted task's tier via pure `ladder_position` — a killed escalation run resumes mid-ladder to
  exactly one terminal, gate count folding prior failures. Exactly-one-terminal + at-least-once hold.
- **Kimi swarm — MINIMAL seam** (owner: budget). `Backend{Native,KimiSwarm}` + `SwarmClient` trait;
  Native done; `KimiSwarm` one mock-receipt test + honest `PendingSwarmClient` `bail!("arrives live in
  M6")` stub. No polling/streaming, **no frozen wire touch**.

Tests/checks (orchestrator, independent; `--release`):

- `cargo test --workspace --release`: **284 passed / 0 failed / 1 ignored** (+11 over Slice A's 273:
  6 nh-fleet integration in `tests/slice_b.rs` — E2 park/dispatch, non-deferred control, live 8-attempt
  ladder→gate, no-ladder single-fail, kimi mock+stub, resume-continues-ladder — plus new lib units:
  `ready_to_dispatch`, exhaustive `next_step`, ladder-rejects-model, kimi serde, `ladder_position`).
- `cargo clippy --workspace --all-targets --release -- -D warnings`: clean.
- **E1 kill-9 resume integration test unmodified and green** (Slice A invariants intact).
- **FEEL owner-approved** — escalation ladder climb + off-peak parking driven through the real `nh`
  binary (deferred parking captured live during the actual Beijing peak window).

## 2026-07-15: M4 Slice A — nh-fleet (ledger + workers + idempotent resume) + /effort warmup (E1 gated)

Builder:

- Claude (Opus 4.8, Claude Code) — M4 orchestrator: drafted CONTRACTS_M4, briefed Sol, verified,
  adversarial review, gate, commit.
- Codex (GPT-5.6 Sol xhigh) — implementer (warmup + Slice A), via `codex exec` background runs.

What changed:

- **CONTRACTS_M4.md drafted + owner-scope-approved (LOCKED).** Carlos ruled the four open scope
  decisions: (1) OAuth2 in FROZEN nh-tools authorized — amendment **A-M4-1** (+ nh-vault keyring setter
  **A-M4-2**), the ONLY frozen-crate writes in M4; every other frozen need STOPS for an amendment.
  (2) Opus 4.8 gate = **review-pause** (no live delegate). (3) nh-mcp server = **tiny_http** (no tokio).
  (4) Kimi swarm = **minimal seam + verify-live** ("don't overdo it, budget"). Sliced A(fleet) /
  D(OAuth) / B(scheduler+ladder) / C(nh-mcp).
- **Warmup `b79c65d`** — `/effort` arg case-folded (`parse_effort` trims + ASCII-lowercases), so
  `/effort High|MAX|None` are accepted. nh-tui only. Also re-validated the `codex`/`gpt-5.6-sol`
  executor path before the big M4 briefs.
- **M4 Slice A — new crate `crates/nh-fleet`** (std threads + channels, no async, NO new external dep):
  - Append-only, **fsync-durable** ledger (`LedgerEvent` enum; ONE mutex-guarded writer does
    write→flush→`sync_all` per event; every line scrubbed like `ReceiptWriter`). Data under
    `.nosis/fleet/<run_id>/ledger.jsonl` + `.nosis/fleet/index.jsonl`.
  - Bounded worker pool reuses the `nh run` construction recipe (resolve route → `make_client` →
    `AgentLoop.run_with_history` over fresh per-task history); one scannable progress line per state
    change. **Durability ordering:** `TaskStarted` is fsync-committed BEFORE the task runs (worker
    blocks on a coordinator ack), so any observable side-effect implies a durable start record.
  - Stable task ids (caller `id` or `t{index:03}-{fnv8}`); id collisions rejected pre-run.
  - **Idempotent resume** — pure `plan_from_ledger` fold (done/failed/gated = terminal → never re-run;
    started-without-terminal → re-run at attempt+1; queued → run); `repair_uncommitted_tail` trims ONLY
    a torn non-newline tail; `ensure_single_terminal` guards exactly-one-terminal-per-task. Budget
    hard-stop carries across resume (sums prior receipts).
  - CLI (nh-cli, additive): `nh fleet run <tasks.json> [--max-workers N] [--budget T]` +
    `nh fleet resume [<run_id>] [--max-workers N]`; `defer_offpeak` politely rejected as Slice B.
  - Frozen crates (nh-core/nh-tools/nh-law/nh-routes/nh-vault) UNTOUCHED this slice.

Tests/checks (orchestrator, independent; `--release` to dodge the Kaspersky debug-exe block):

- `cargo test --workspace --release`: **273 passed / 0 failed / 1 ignored** (+12 over 261 baseline:
  7 nh-fleet unit + 2 cmd_fleet + the E1 kill-9 integration test + …).
- `cargo clippy --workspace --all-targets --release -- -D warnings`: clean.
- **E1 (kill-9 idempotent resume) GATED** — integration test spawns the real `nh` binary against a
  test-only echo provider (`NH_FLEET_TEST_PROVIDER=echo`, inert otherwise, cannot bypass the law gate),
  `Child::kill`s it mid-run after ≥3 durable `TaskDone`, runs `nh fleet resume`, asserts: all 10 reach
  exactly one `TaskDone`; no committed task re-`Started`; execution-log proves committed tasks ran
  exactly once. PASSES.
- Smoke: real `nh fleet run` (4 tasks, 2 workers) — clean scannable one-line-per-event output,
  parallelism visible, ledger append-only with typed nh-core receipts.

Two minor non-blocking notes: the echo test seam ships in the binary (inert without the exact env
opt-in; can't bypass the law gate — could later hide behind a cargo feature); a keyless fleet run exits
with an actionable `nh key add <entry>` rather than opening like `nh chat` (arguably better for batch).

Next step:

- **Slice B** — off-peak scheduler (reuse `nh_routes` peak_status/price_at + injected Clock; E2) +
  escalation ladder (Flash→K2.7→V4 Pro High→V4 Pro Max→Opus review-pause gate; 2 tries/tier, failure
  Receipt attached; pure `next_step` seam) + MINIMAL Kimi swarm seam. Then Slice C (nh-mcp, tiny_http,
  E3) + Slice D (OAuth2, E4, amendment A-M4-1/2). Spec: `CONTRACTS_M4.md`.

## 2026-07-15: M3 TUI UX overhaul (Slices D+E+F) — stress-test + FEEL-approved commit gate (M3 CLOSED)

Builder:

- Claude (Opus 4.8, Claude Code) — M3 orchestrator: verify, adversarial stress-test, gate, commit
- Codex (GPT-5.6 Sol) — Slices D/E/F implementer (from earlier sessions; committed here after FEEL approval)

What changed:

- Carlos rejected the content-complete-but-flat TUI on UX grounds (couldn't type tasks starting with
  t/l — bare-letter shortcuts collided; overlays bled into the transcript; no scroll; model/effort
  hidden; native mouse-copy broken; paste eaten). M3 was reopened and re-skinned + interaction-fixed
  across three slices (all had been sitting uncommitted on top of `b3503d9`):
  - **Slice D** — bordered outer frame + chat transcript (`❯ you` / `◆ nosis` roles, turn separation,
    visual gaps) + framed centered modals with Clear-before-draw (anti-bleed) + welcome empty-state +
    key-hint strip. (Spec: CONTRACTS_M3 §8.)
  - **Slice E** — type-freely slash-command input: `/` opens a live command menu; the colliding
    bare-letter t/l/? shortcuts are gone. Live `/model`/`/provider` route switch preserves history
    (only cache warmth resets, full resolver path) + `/effort none|low|high|max`. Keyboard scroll
    (`↑↓`/PageUp/PageDown/End) with `↑/↓ more` overflow hints. Honest identity system prompt
    (`nosis on <route>`, never Claude — fixes DeepSeek V4 Flash training contamination; routing
    verified via receipts, not misrouting). Mojibake fix. (Spec: CONTRACTS_M3 §9.)
  - **Slice F** — removed mouse capture (no `EnableMouseCapture`) so native click-drag copy works
    again with NO Shift; fixed paste via bracketed paste (`Event::Paste` → `reduce_paste`, multi-line
    collapses to one line, never auto-dispatches; `DisableBracketedPaste` added to the panic-safe
    restore sequence).
- Orchestrator adversarial stress-test (throwaway `#[cfg(test)] mod stress_probe` driving the private
  `reduce_key`/`reduce_paste`/`reduce_input_event`/`render` via ratatui `TestBackend` — ran, then
  DELETED; the tree stayed pristine D/E/F, never `git checkout`ed). 8 families all green: tiny
  terminals 1×1→80×1 across every overlay + full transcript/timeline (no split/clamp/cursor panic);
  200k-char + emoji/CJK + `\n\r\t` + pure-control paste (stays one line, never leaks a newline);
  backspace-underflow + 10k-char + wide-char cursor; no-match `/zzzzz`+Enter (friendly, never a task);
  100–200× arrow-spam nav stays in bounds; scroll saturates both ends; 20,000-event deterministic fuzz
  (keys+pastes+resizes) — zero panics, input never held a raw `\n`/`\r`. One FEEL nit surfaced (not a
  bug, fails safe): `/effort HIGH` uppercase is rejected because `parse_effort` is lowercase-only —
  left as-is (Carlos said commit; optional one-line follow-up).

Tests/checks run (orchestrator, independent):

- `cargo test --workspace`: **261 passed, 0 failed, 1 ignored** (+22 over Slice C's 239). With the
  temporary stress probe compiled in: 269 passed. Back to 46 nh-tui tests after the probe's removal.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- **Carlos approved the interactive FEEL re-smoke in Windows Terminal** (native mouse copy, real
  clipboard paste, WT glyph/frame render, `/quit` clean restore) — the binding UX gate.

Next step:

- **M3 CLOSED (UX-approved).** Proceed to M4: fleet + append-only ledger + off-peak scheduler +
  escalation ladder + nh-mcp server. Draft `CONTRACTS_M4.md`, brief Sol (GPT-5.6 xhigh), run the loop.

## 2026-07-14: M3 Slice C — orchestrator review + commit gate (M3 CONTENT-COMPLETE)

Builder:

- Claude (Opus 4.8, Claude Code) — M3 orchestrator: verify, adversarial review, gate, commit

What changed:

- No implementation code. Verified Slice C empirically: frozen nh-core/nh-tools/nh-law/nh-routes
  diffs are EMPTY; no new workspace dep (nh-tui uses the existing `reqwest.workspace`). Scope is
  nh-tui + cmd_tui + docs only.
- Adversarial review of the notify path (highest risk): the Telegram bot token is fetched on the
  side thread and used only inline in the URL; EVERY reqwest/vault error is mapped to a fixed
  "telegram notify failed" string, so a URL-with-token can never reach a rendered error. Redirects
  disabled (Policy::none()), fixed host, 3s/5s timeouts — no token-exfil-via-redirect, no hang. The
  body passes safe_line + a 160-char cap (proven redacted/control-safe). The POST runs on a
  short-lived thread drained via a failure channel — render never blocks; notify fires once per
  Waiting/Blocked transition, not on repeats. Timeline is strictly view-only: `R` shows only the
  deferral note (no restore, no snapshot store), every rail/detail line scrubbed (TestBackend render
  test with an sk- secret confirms), compaction flag is task-local. No confirmed defect → no
  hardening round-trip.

Tests/checks run (orchestrator, independent):

- `cargo test --workspace`: 239 passed, 0 failed, 1 ignored (+12 over Slice B).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- Commit Slice C — **M3 is content-complete** (all §1/§2/§3 surfaces present + green). Remaining M3
  exit item is the MANUAL three-terminal render smoke on the Predator (Windows Terminal + VS Code
  terminal + ConHost) and the live Telegram send (needs Carlos's KORVIN bot token). Then M4 (fleet).

## 2026-07-14: M3 Slice C — timeline view + Telegram notifications

Builder:

- Codex (GPT-5.6 Sol) — M3 Slice C implementer

What changed:

- Added the view-only `l` timeline overlay over in-memory task receipts and answers. Rows show the
  sequential turn, outcome, input/output/cached tokens, and the M2 compaction marker; Up/Down scrub,
  Enter inspects the selected receipt and answer, and `R` only shows the locked restore deferral.
  Added the timeline to the `?` palette. No snapshot store or restore path was added.
- Forwarded successful receipts through an additive nh-tui worker event while preserving the
  existing Usage/Answer flow. Failed tasks also receive one friendly fail projection without
  changing frozen nh-core. Compaction is detected solely from the existing `context NN% —
  compacted ...` progress line for the active task.
- Added optional `.nosis/notify.toml` loading once in `cmd_tui`, before terminal takeover. Missing or
  broken config becomes bell-only with one scrubbed stderr warning. Telegram tokens are fetched as
  vault entry `telegram` on a short-lived side thread; the fixed-host POST has short timeouts and
  redirects disabled, and URL/token details never enter UI errors.
- Added pure short notification bodies, transition-driven sends for Waiting/Blocked, an injectable
  sender seam, and one dim `telegram notify failed` transcript line per failed attempt. Every
  timeline and notification body string passes the shared nh-vault scrub/display-safety path.
- Added headless coverage for receipt projection, task-local compaction, timeline selection/inspect,
  disabled restore, timeline rendering safety, missing/broken/valid config, disabled zero-send,
  injected successful sends, short scrubbed messages, and one-line failure degradation. nh-core and
  nh-tools remain unchanged.

Tests/checks run:

- `cargo test --workspace`: 239 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- Orchestrator review/commit gate. Slice C's only verify-live item is the real Telegram send with
  Carlos's bot token/chat id. The previously-open M3 three-terminal renderer smoke remains manual.

## 2026-07-14: M3 Slice B — orchestrator review + commit gate

Builder:

- Claude (Opus 4.8, Claude Code) — M3 orchestrator: verify, adversarial review, gate, commit

What changed:

- No implementation code. Verified Slice B empirically: scope is exactly nh-tui + cmd_tui + the
  additive `nh_law::PolicyView` (+47, owned clones, fields still private, verdict/autonomy behavior
  unchanged) — `git diff HEAD` on nh-core/nh-tools is EMPTY (frozen surfaces honored).
- Adversarial review: `reduce_key -> UiAction` is a pure reducer; overlays only open from
  Idle/Blocked (guarded behind the Working/Waiting checks) so they can't open mid-task; dispatch is
  suppressed while an overlay is open (proven by test); palette windowing keeps the highlighted
  index correct; inset/scroll math is saturating. Security: MCP tools are describe-only (no
  invocation from the palette, §2.2); `mcp_state` derivation matches nh-tools' real warning format;
  every overlay string passes `nh_vault::safe_line`, with a `TestBackend` render test proving a
  secret + control chars in a tool description come back `[REDACTED]` with no raw `\r`/`\x1b` in the
  drawn buffer. Conservative styling + Clear-before-draw keep it ConHost-safe.
- No confirmed defect → no hardening round-trip (THE LAW: small/simple — don't spend a cycle on a
  non-finding). One drop-if-hard nicety noted for later: the trust dial doesn't scroll a very long
  rule list (law lists are small; correctness/security unaffected).

Tests/checks run (orchestrator, independent):

- `cargo test --workspace`: 227 passed, 0 failed, 1 ignored (+10 over Slice A).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `nh tui --help`: launches, exposes `--model` / `--budget`. The three-terminal render-artifact
  smoke on the Predator remains the manual M3 exit check.

Next step:

- Commit Slice B. Then Slice C (timeline view + notifications: bell + Telegram hook).

## 2026-07-14: M3 Slice B — trust dial + discoverability palette

Builder:

- Codex (GPT-5.6 Sol) — M3 Slice B implementer

What changed:

- Added the additive owned `nh_law::PolicyView` projection and `Policy::view()` accessor for the
  read-only trust dial; policy fields remain private and verdict/autonomy behavior is unchanged.
  Logged the pre-authorized §2.1 amendment in `CONTRACTS_M3.md` §7.
- Added pure nh-tui overlay state and reducers for the `t` trust-dial view and `?` palette. The
  palette has case-insensitive in-memory filtering, command activation, built-in tool descriptions,
  the visible deferred `R` note, and MCP server/tool rows with enabled/auth-ok/stale/discover-only
  startup state. Every overlay line passes the shared `nh_vault::safe_line` path.
- Made `nh tui` read and discover `.nosis/mcp.toml` once before terminal takeover through the
  existing nh-tools loader/toolset. Missing/empty config stays empty; malformed or unavailable MCP
  becomes warning-backed stale/discover-only palette data without crashing. The worker, approval
  gate, semáforo, HUD, nh-core, and nh-tools are unchanged.
- Added headless coverage for the owned policy projection, pure palette filtering and MCP state,
  empty/broken MCP config, trust-dial none rows, overlay dispatch suppression/Esc, palette command
  activation/tool description, and rendered overlay scrubbing/control safety.

Tests/checks run:

- `cargo test --workspace`: 227 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- Orchestrator review/gate for Slice B, then Slice C (timeline view + notifications). The three-
  terminal render-artifact smoke remains the manual M3 exit check.

## 2026-07-14: M3 Slice A — orchestrator review + commit gate

Builder:

- Claude (Opus 4.8, Claude Code) — M3 orchestrator: resume, verify, adversarial review, gate, commit

What changed:

- No implementation code (orchestrator does not hand-write milestone code). Resumed after a `/clear`
  with Slice A's disk state unknown; determined it empirically. A transient CRLF/format pass made
  `git status` show nh-core/nh-tools/nh-vault as modified — `git diff --numstat HEAD` proved ZERO
  content change (EOL noise only), so the frozen crates were never touched. Let the in-flight Sol
  codex finish rather than kill a valid build.
- Read the full nh-tui surface + wiring. Confirmed: RAII terminal guard + panic hook restore on
  every exit path; the `approve` closure is Mutex-backed (Send+Sync, no new dep) and default-deny on
  every failure path; exec still routed through the policy guard (never auto-approved); every
  rendered string passes `nh_vault::safe_line` (scrub + control-char escape); semáforo is a
  single-state pure reducer; budget is a hard stop; peak_status lifted (not copy-pasted).
- One finding fed back to Sol as a bounded hardening pass: the safe_line/sanitize_line display-safety
  primitive was duplicated in nh-tui — lifted into `nh_vault` (additive), reused by nh-cli + nh-tui
  (§5.2 congruence). Re-verified green after.

Tests/checks run (orchestrator, independent):

- `cargo test --workspace`: 217 passed, 0 failed, 1 ignored.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- Env note: Kaspersky/McAfee on-access scanning blocked execution of the freshly-linked 8 MB
  `wire_clients` test exe (frozen nh-core, green at M2 and in Sol's run); confirmed green after AV was
  paused. The three-terminal render-artifact smoke on the Predator remains the manual M3 exit check.

Next step:

- Commit Slice A. Then Slice B (trust-dial view + `?` palette).

## 2026-07-14: M3 Slice A display-safety hardening

Builder:

- Codex (GPT-5.6 Sol) - M3 Slice A hardening implementer

What changed:

- Lifted the canonical scrub-then-control-character-escape-then-truncate display helper into
  additive `nh_vault::safe_line` and `nh_vault::sanitize_line`, with the 500-character cap kept
  private to nh-vault.
- Made nh-cli's existing crate-private helper delegate to nh-vault and made nh-tui use the same
  implementation through its shared scrubber lock. Rendered output and cmd_chat/cmd_run behavior
  are unchanged.
- Moved the control-character and truncation tests to nh-vault while retaining the existing
  nh-cli redaction and nh-tui rendered-line safety regressions. Recorded the §5.2 congruence
  amendment in `CONTRACTS_M3.md` §7.

Tests/checks run:

- `cargo test --workspace`: 217 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- Orchestrator commit gate for M3 Slice A.

## 2026-07-13: M3 Slice A - TUI core

Builder:

- Codex (GPT-5.6 Sol) - M3 Slice A implementer

What changed:

- Added the `nh-tui` library crate: a channel-backed single-worker agent boundary, pure semaforo reducer, conservative full-screen layout, inline approval flow, scrubbed/sanitized rendering, cumulative usage and budget HUD, hard budget stop, keyless task errors, and panic-safe RAII terminal restoration.
- Added `nh tui [--model <id>] [--budget <tokens>]`, mirroring chat's catalog/law setup and warning order while keeping the terminal launch keyless.
- Lifted peak/off-peak status into `nh_routes::ResolvedRoute::peak_status` and shared it with chat and TUI; recorded the additive decision in `CONTRACTS_M3.md` section 7.
- Added headless reducer, approval, HUD, budget, redaction/control-character, terminal teardown, worker-history, and keyless-start coverage. Slice B/C keys remain reserved no-ops.

Tests/checks run:

- `cargo test --workspace`: 217 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `nh tui --help`: compiled command exposes the locked model and budget options.
- Live visual verification on Windows Terminal, VS Code terminal, and ConHost remains the human M3 Slice A exit check.

Next step:

- Run the three-terminal verify-live matrix, then orchestrator review and commit gate before Slice B.

## 2026-07-13: M2 orchestrator review + commit gate

Builder:

- Claude (Opus 4.8, Claude Code) — M2 orchestrator: verification, adversarial review, gate, commit

What changed:

- No implementation code (orchestrator does not hand-write milestone code). Read every M2 slice (nh-law, nh-core context engine, nh-tools guard, nh-cli wiring, m2_exit e2e) and independently re-ran the gate — confirmed green, not just Sol's self-report.
- Adversarial review vs THE LAW + SECURITY_MODEL. The write-hold holds: guard receives the workdir-relative `/`-joined path; `Verdict::Block` wins before the `is_file` check; symlinks resolve to their canonical target (escapes caught by `starts_with`); and structurally `exec_verdict` can only return `Block`/`Ask` — never `Allow` — so exec is never auto-approved even at `--autonomy auto`. Confirmed the Windows case-fold new-file bypass is NOT reachable through `EditFile` (it only mutates existing files, whose case `canonicalize` normalizes before the guard sees them; missing paths bail before any write).
- Fed three confirmed findings back to Sol as ONE bounded hardening pass (dead `exec_ask`, non-hermetic protected-path test, undocumented case-fold invariant); re-verified after.

Tests/checks run (orchestrator, independent):

- `cargo test --workspace`: 206 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- M2 exit #1 PROVEN by name: `stable_constitution_exceeds_sixty_percent_cache_hits_over_fifty_turns` = 97.70% (> 60%).
- M2 exit #2 PROVEN by name: `protected_path_is_blocked_at_auto_end_to_end` — real `nh run --autonomy auto`, model-readable law block, `.nosis/law.toml` byte-unchanged, exit 0.

Next step:

- Commit all of M2 together, then M3 (TUI).

## 2026-07-13: M2 bounded hardening pass

Builder:

- Codex (GPT-5.6 Sol) — M2 hardening executor

What changed:

- Removed the behaviorless compiled `exec_ask` state and matching work while preserving `[exec] ask` TOML compatibility; shell execution still blocks configured commands and asks for every other command.
- Made the bundled protected-path autonomy test independent of the developer's real home law.
- Documented why existing-file canonicalization makes the case-sensitive write-hold safe on case-insensitive filesystems, plus the required guard hardening for any future file-creation tool.

Tests/checks run:

- `cargo test --workspace`: 206 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- M2 reviewer and commit gate.

## 2026-07-13: M2 Slice C — nh-tools guard + nh-cli law wiring

Builder:

- Codex (GPT-5.6 Sol) — M2 Slice C executor

What changed:

- Added the locked nh-tools `Access` / `Guard` / `GuardFn` surface and `ToolCtx::new` / `with_guard`. `edit_file` now evaluates normalized workdir-relative forward-slashed paths before any file check or write; Block and Ask denials stay Ok-shaped. `exec_shell` evaluates the command before execution, while shipped policy still routes every non-blocked command through the existing approval gate. MCP adapters retain their independent M1 trust logic.
- Wired nh-law into both `nh run` and `nh chat`: scrubbed non-fatal warnings, byte-stable constitution, route context windows, policy-backed tool guards, and route-switch context refresh. Added optional `nh run --autonomy ask|auto` with one translation function.
- Added conditional cache-hit chips to the run summary and chat footer. `nh init` now creates `.nosis/law.toml` from the starter policy without overwriting it.
- Added the process-level M2 exit test `protected_path_is_blocked_at_auto_end_to_end`; it runs `nh run --autonomy auto`, observes the model-readable law block, and proves `.nosis/law.toml` is unchanged.

Tests/checks run:

- `cargo test --workspace`: 206 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- Keyless smoke: `echo /quit | nh chat` exited 0 with one actionable missing-key warning.

Next step:

- Orchestrator adversarial review and M2 commit gate.

## 2026-07-13: M2 Slice B — nh-core context engine

Builder:

- Codex (GPT-5.6 Sol) — M2 Slice B executor

What changed:

- Added the locked `AgentLoop.constitution` and `AgentLoop.context_limit` surfaces. A supplied constitution is installed verbatim only when session history is empty; the existing coding-agent/tool-list system message remains the `None` fallback.
- Added the pure `wire::cache_hit_pct` metric and mechanical 70% context compaction. Compaction preserves the byte-identical system prefix, retains at least two recent user turns from a user boundary, keeps complete tool-call/result groups and reasoning bytes, folds the audit marker into the first retained user message, and emits one concise event.
- Added context-engine integration coverage for prefix stability, usage-omitted token estimation, compaction invariants, disabled compaction, and the 50-turn prefix-cache exit criterion. The deterministic mock observed a 97.70% cumulative cache-hit rate.
- Updated the existing `AgentLoop` literals in nh-core tests and nh-cli with `constitution: None` and `context_limit: None`; nh-cli behavior is otherwise unchanged in this slice.

Tests/checks run:

- `cargo test --workspace`: 195 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- M2 Slice C: nh-tools guard enforcement and nh-cli law/autonomy/cache-HUD wiring.

## 2026-07-13: M2 Slice A — nh-law leaf crate

Builder:

- Codex (GPT-5.6 Sol) — M2 Slice A executor

What changed:

- Added the `nh-law` leaf crate with the locked constitution, policy, loader, and public types from `CONTRACTS_M2.md` section 1. Bundled and starter policy remain TOML data; the crate depends only on workspace `serde`, `toml`, and `anyhow` plus std.
- Implemented byte-stable constitution assembly, non-fatal layered loading, CLI/user/bundled autonomy precedence, unioned protections, the repo-cannot-weaken boundary, and the in-crate segment glob matcher. Repository autonomy and auto-approval attempts are ignored with the required warning.
- Added 11 unit tests covering assembly stability/order/omission, glob and exec matching, verdict precedence, bundled protected paths, source precedence, malformed input, unknown autonomy, and repo-law restrictions.

Tests/checks run:

- `cargo test --workspace`: 191 passed, 0 failed, 1 ignored (pre-existing keyring round-trip).
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.

Next step:

- M2 Slice B: nh-core stable-prefix cache discipline, cache metric, and compaction.

## 2026-07-13: M1 build finalized (Fable 5 multi-agent workflow)

Builder:

- Claude (Fable 5, Claude Code) — multi-agent workflow: architect + 4 parallel crate builders, integrator, 4 adversarial reviewers, hardening pass

What changed:

- M1 implemented end-to-end per CONTRACTS_M1.md: full 5-provider catalog with clock-aware pricing, Anthropic Messages wire adapter, thinking-mode dialects + `nh run --think`, stateless MCP client (2026-07-28 draft), and `nh chat` with mid-session `/model`/`/provider` switching and cost HUD. Integration green after merging the crate builders' outputs.
- 4 adversarial review findings addressed (fixes detailed in the M1 hardening entry below).
- Price verification pass (full detail: `../04-research/SOURCE_INDEX.md`, 2026-07-13 section): all four API providers checked against their OWN pricing/docs pages — DeepSeek, Kimi, MiMo, and GLM base prices and base URLs are all first-party CONFIRMED. MiMo plan-B.3 conflict RESOLVED first-party (current `mimo.mi.com/docs/pricing` matches the marketplace figures, superseding the May 27 cut notice; `verify_live` → `confirmed`). Two earlier Kimi "reported" figures were wrong (kimi-k2.6 is $0.16 hit / $0.95 miss / $4.00 out, not ~$0.55–0.60 in / $2.50–2.65 out; k2.7 highspeed bills 2x standard input) and the catalog's old MiMo host was wrong (fixed to `api.xiaomimimo.com`). Still open, honestly: DeepSeek's peak 2x windows are announcement-only (secondary press, not yet on the first-party pricing page — re-verify on/around 2026-07-24), and GLM free-tier rate limits remain unpublished.

Tests/checks run:

- cargo build --workspace: green. cargo test --workspace: 176 passed, 0 failed, 1 ignored (keyring round-trip; nh-cli 49, nh-core lib 21 + integration 5+4+49, nh-routes 38, nh-vault 10). cargo clippy --workspace --all-targets -- -D warnings: clean. M0 smoke: `nh --help` exit 0; `echo /quit | cargo run -p nh-cli -q -- chat` exit 0 with no key configured (friendly warning to stderr, stdout empty). Committed on main as fa5e986 'M1: full catalog, clock pricing, Anthropic wire, thinking dialects, MCP client, chat session'.

Next step:

- Live provider verification (DeepSeek keyed run + GLM free-route run), then M2: context engine + law.

## 2026-07-13: M1 hardening — adversarial-review fixes

Builder:

- Claude (Fable 5, Claude Code) — M1 hardening agent

What changed:

- Wire-client HTTP config (nh-core): both clients now build via one `http_client()` — explicit 600 s request timeout + 10 s connect timeout (reqwest's blocking default silently aborted every request at 30 s, killing long thinking turns) and `redirect::Policy::none()` (reqwest forwards custom headers like `x-api-key` across cross-host redirects — a redirecting endpoint is now a friendly HTTP error, never a key leak). Timeout failures get their own actionable line ("provider at <url> did not answer within 600s — retry, or switch to another route") instead of the misleading "could not reach provider". `McpClient` got the same explicit timeouts.
- `nh chat` keyless recovery (nh-cli): a keyless start now retries the real connection at the next task, so `nh key add <provider>` in another terminal works without restarting the session; reconnect registers the new key on every scrub path (shared `install_client` helper with `/model`/`/provider`).
- CONTRACTS_M1.md §7 amendment 4 (orchestrator authority): ratified `nh run --think none|low|high|max` + per-dialect defaults (always-thinking/glm-hm → High, deepseek-nhm/none → None) — closes the frozen-surface gap flagged in review.
- Deleted the reviewer's truncated `adv_redirect_probe.rs` (marked DELETE AFTER REVIEW, did not compile); its concern lives on as the `cross_host_redirects_are_refused_never_followed` regression test in `wire_clients.rs`.

Tests/checks run:

- `cargo test --workspace` (180 passed, 1 ignored keyring round-trip, 0 failed; 4 new tests: timeout error line, timeout floor guard, redirect refusal on both wires, keyless reconnect), `cargo clippy --workspace --all-targets -- -D warnings` clean. Smoke: `echo /quit | nh chat` exit 0 keyless; `nh run --help` surface unchanged.

Next step:

- M1 exit demo against a live key (unchanged from integration entry).

## 2026-07-13: M1 integration — workspace green, contracts reconciled

Builder:

- Claude (Fable 5, Claude Code) — M1 integrator

What changed:

- Reconciled the 3 nh-routes tests asserting pre-verification catalog values with the 2026-07-13 live-verified first-party prices (kimi-k2.6 output 4.00 confirmed, mimo B.3 conflict resolved confirmed, glm free tier confirmed); `mimo_prices_are_verify_live` renamed to `mimo_prices_are_confirmed_first_party`. Catalog is data-of-record — tests follow data.
- `nh chat` keyless startup: connect failure at start is now one `warning:` line plus a stand-in client that re-surfaces the vault error only when a task runs; commands work keyless and `echo /quit | nh chat` exits 0 on a fresh machine (new test).
- CONTRACTS_M1.md: §5.2 amendment 3 (`AgentLoop.thinking`), §5.3 nh-cli `serde_json` dev-dep, §6 ledger rows marked RESOLVED (base URLs, MiMo prices, Kimi K2.6 cache-hit $0.16) + DeepSeek 2026-07-24 re-verify row, new §7 integration amendments (keyless startup, `peak <mult>x until HH:MM` format blessed, nh-tools crate-root re-exports).

Tests/checks run:

- `cargo build --workspace`, `cargo test --workspace` (176 passed, 1 ignored keyring round-trip, 0 failed), `cargo clippy --workspace --all-targets -- -D warnings` — all green. M0 smoke: `nh --help` exit 0; `echo /quit | nh chat` exit 0 keyless; `/price` keyless shows live peak HUD.

Next step:

- M1 exit demo against a live key: mid-session `/model`+`/provider` switch, one stateless MCP call with handle passback; verify-live ledger items (`reasoning_effort`, `ttlMs`, GLM rate limits; DeepSeek prices on/around 2026-07-24).

## 2026-07-13: M1 nh-cli (`nh chat` REPL + `nh run --think`)

Builder:

- Claude (Fable 5, Claude Code) — nh-cli builder (CONTRACTS_M1.md §4)

What changed:

- New `nh chat [--model <id>]` (cmd_chat.rs): line REPL over an injected line-source/Write abstraction (piped-stdin friendly, BOM-tolerant for Windows pipes); prompt `nh> ` on stderr, stdout carries only answers and command output. Plain lines run through `AgentLoop::run_with_history` on ONE persistent history; `/model` and `/provider` switch routes mid-session preserving history and cumulative usage (M1 exit criterion); `/price` and the per-answer footer are the first cost HUD lines — peak indicator renders "peak 2x until HH:MM" with the window boundary converted to the user's local time; `/tools` lists builtin then MCP tools; unknown commands get the one-line help. MCP tools load at chat start from `.nosis/mcp.toml` via `nh_tools::mcp` (broken file = one warning, chat continues).
- Scrubbing: one shared `Arc<RwLock<Scrubber>>` across stdout/stderr/approval/receipt paths, rebuilt on every switch so switched-away keys stay redacted for the whole session.
- `nh run --think none|low|high|max` mapped to `ThinkingEffort` (`effort_for`): absent flag defaults per dialect — High for always-thinking/glm-hm, None for deepseek-nhm/none. `nh run` now uses `make_client` (both wires) and refuses delegate routes with the M4 message.
- Deps per §5.3: `chrono` (workspace) added to nh-cli; `serde_json` (workspace) added as dev-dependency so tests build `ChatMessage` via serde instead of struct literals (§5.2 grep rule).

Tests/checks run:

- `cargo test -p nh-cli` 48 passed (23 new chat tests: switching, price/footer formats, scrubbing, loop basics, MCP load); `cargo clippy -p nh-cli --all-targets -- -D warnings` clean; workspace clippy clean. Manual piped smoke: `/price` (live peak boundary in local time), friendly missing-key error keeps route, `/quit` exits 0. Workspace tests: all crates green EXCEPT 3 nh-routes tests asserting pre-verification catalog values (kimi-k2.6 output 2.65→4.00, mimo 0.87, glm confirmed) — catalog owner's reconciliation, not nh-cli.

Next step:

- nh-routes tests vs live-verified catalog.toml reconciliation; M1 live MCP call + `reasoning_effort` verify-live ledger.

## 2026-07-13: M1 nh-tools (MCP client, stateless 2026-07-28)

Builder:

- Claude (Fable 5, Claude Code) — nh-tools builder (CONTRACTS_M1.md §3)

What changed:

- New `nh_tools::mcp` module (re-exported at crate root; existing tools untouched): `.nosis/mcp.toml` parsing (`load_mcp_config`, unknown keys ignored, friendly errors naming valid auth/trust/spec values), `McpClient` (blocking JSON-RPC 2.0 over Streamable HTTP POST, `tools/list` with `ttlMs` cache — absent→60s, 0→no cache — `tools/call` text/non-text block rendering, discovery via GET `.well-known/mcp.json` with `server/discover` POST fallback).
- Statelessness invariant: no `initialize`, no `Mcp-Session-Id` ever; every request's params carry `_meta` (protocolVersion echoing config spec, clientInfo, capabilities). State handles are ordinary tool arguments (handle-passthrough test).
- Auth §3.4 (none / apikey via nh-vault per call / oauth2 deferred to M4 with one message) + §3.5 outbound header lint choke point (`Mcp-*`/`x-mcp-*` with Scrubber-shaped values refused; `Authorization` exempt).
- §3.6 adapters: `mcp_tools` builds `mcp__<server>__<tool>` adapters (`[MCP <server>] ` description prefix); trust=ask gates via `ctx.approve("mcp <server> <tool> <args>")` with Ok-shaped denial; trust=auto skips the gate only for server-annotated `readOnlyHint` tools; trust=block servers are never contacted and offer no tools; failing servers contribute one warning line, never a hard failure.
- Deps added to nh-tools per §5.3: `reqwest`, `toml` (workspace), `nh-vault` (path).

Tests/checks run:

- `cargo test -p nh-tools` 49 passed (37 new MCP tests against a hand-rolled `std::net::TcpListener` mock — no live calls, no heavy dev-deps); `cargo clippy -p nh-tools --all-targets -- -D warnings` clean. Workspace: nh-vault/nh-routes/nh-core lib+agent green; nh-cli has a pre-existing mid-flight compile error (`cmd_run::run` arity, cli owner's area); nh-core `wire_clients` test binary transiently file-locked by a concurrent build (os error 32), retried.

Next step:

- nh-cli `/tools` wires `mcp_tools` + warnings through the Scrubber; M1 live verify of `ttlMs` location against a real 2026-07-28 server.

## 2026-07-13: M1 nh-core (wire clients, thinking dialects, session history)

Builder:

- Claude (Fable 5, Claude Code) — nh-core builder (CONTRACTS_M1.md §2)

What changed:

- nh-core wire: `AnthropicMessagesClient` (POST `{base_url}/v1/messages`, `x-api-key` + `anthropic-version: 2023-06-01`, required `max_tokens`, full tool_use/tool_result mapping with consecutive-tool-result merge, usage from `input_tokens`/`output_tokens`/`cache_read_input_tokens`).
- nh-core wire: `make_client` factory — dispatches on `route.wire`, captures per-route policy (thinking dialect, `preserve_reasoning`, deepseek tool-replay quirk); `max_tokens = min(max_out, 8192)` on the anthropic wire.
- nh-core wire: `ThinkingEffort` enum + one-function `(dialect, effort)` mapping (deepseek-nhm → `reasoning_effort`, flagged verify-live; always-thinking/glm-hm/none → no toggle); `ChatRequest.thinking` and `ChatMessage.reasoning_content` amendments per §5.2; reasoning replay rules (preserve / strip / quirk empty-string, stored value wins) in one function.
- nh-core agent: `run_with_history` for `nh chat` sessions (`run` is now a thin wrapper); additive `AgentLoop.thinking` field — nh-cli's one literal updated in step (`thinking: None`).

Tests/checks run:

- `cargo test -p nh-core` 30 passed (incl. loopback mock-server tests for both wires — no live calls); `cargo clippy -p nh-core --all-targets -- -D warnings` clean; workspace clippy clean. Workspace tests: nh-core/nh-cli/nh-tools/nh-vault green; 3 pre-existing nh-routes failures from catalog.toml (2026-07-13 confirmed prices) vs nh-routes test expectations (verify_live/reported/2.65) — routes owner's area, untouched here.

Next step:

- Routes owner: reconcile nh-routes tests with the updated catalog.toml. nh-cli builder: `nh chat` on top of `run_with_history` + `make_client`.

## 2026-07-13: M0 hardening (adversarial review fixes)

Builder:

- Claude (Fable 5, Claude Code) — hardening agent

What changed:

- nh-cli: every stderr path now passes the Scrubber — progress lines, the approval prompt, and the final `nh:` error line; model-supplied text is also control-char-escaped (`sanitize_line`) so \r/ANSI cannot spoof the approval gate, with a visible truncation marker past 500 chars.
- nh-tools: `exec_shell` strips `NH_*_KEY` env vars from the child, closing the key-exfiltration-to-disk path via the env fallback.
- nh-routes: `from_toml` rejects banned route keys AND banned `model_id` values (clean alias can no longer smuggle a dead id onto the wire).
- nh-cli: `nh init` writes a starter catalog.toml (embedded repo-root catalog — still data), so `nh run` works in a fresh repo; missing-catalog error now says "run `nh init` to create one".

Tests/checks run:

- `cargo test --workspace` (62 passed, 1 ignored keyring round-trip), `cargo clippy --workspace --all-targets -- -D warnings` — green. Manual: `nh init` + `nh run` flow in a fresh temp dir reaches the key prompt.

Next step:

- M1: live route/pricing verification against providers.

## 2026-07-12: M0 build finalized (Fable 5 multi-agent workflow)

Builder:

- Claude (Fable 5, Claude Code) — multi-agent workflow: 5 parallel crate builders, integrator, 3 adversarial reviewers, hardening pass

What changed:

- M0 implemented end-to-end across all five crates via the multi-agent workflow; integration green after merging the crate builders' outputs.
- 6 adversarial review findings addressed (fixes detailed in the M0 hardening entry above).

Tests/checks run:

- 53 passed; 0 failed; 1 ignored (keyring_round_trip) across 6 test binaries: nh-core unit 12, nh-cli 8, nh-core integration 3, nh-routes 10, nh-tools 10, nh-vault 10 (+1 ignored); doc-tests 0; clippy -D warnings clean.

Next step:

- Verify live against DeepSeek (`nh key add deepseek`, then `nh run` on a sample repo), then M1.

## 2026-07-12: M0 implemented (turn loop, tools, vault, routes, CLI)

Builder:

- Claude (Fable 5, Claude Code) — 5 parallel crate builders + integrator

What changed:

- Implemented all locked `todo!()` contracts across `nh-core` (AgentLoop, OpenAiCompatClient, receipts), `nh-routes` (RouteResolver, catalog parsing, banned-string rejection), `nh-tools` (read_file / edit_file / exec_shell behind approval gate; denial is an Ok-shaped "user denied: <command>" tool result), `nh-vault` (OS keyring + env fallback + Scrubber), `nh-cli` (init / key / run).
- Sanctioned contract addition: `AgentLoop.on_event: Option<Box<dyn Fn(&str) + Send>>` for progress lines; nh-cli wires it to stderr. Field set is now frozen.

Tests/checks run:

- `cargo build --workspace`, `cargo test --workspace` (53 passed, 1 ignored keyring round-trip), `cargo clippy --workspace --all-targets -- -D warnings` — all green.

Next step:

- M1: live route/pricing verification against providers.

## 2026-07-12: Project OS created

Builder:

- Carlos + Claude (Fable 5, Claude Code)

What changed:

- Adapted ProjectStarterTemplate into this folder as the project operating system.
- Filled core docs from Master Plan v0.1: master context, current task, roadmap, milestones, decision log, product brief, architecture overview/decisions, security model, AI-collaboration set (AGENTS/CLAUDE/CODEX/MODEL_ROLES/CONTEXT_HANDOFF/PROMPT_LIBRARY), risk register, one-page summary.
- Re-pointed the M0 implementer prompt from "Codex 5.5" to GPT-5.6 (Terra default / Sol for hardest), added nh-vault to M0 scope per plan §A.10.7.

Files changed:

- All of `00-start-here/`, `05-ai-collaboration/`, plus README, PRODUCT_BRIEF, ARCHITECTURE_OVERVIEW, ARCHITECTURE_DECISIONS, SECURITY_MODEL, RISK_REGISTER, ONE_PAGE_SUMMARY.

Decisions made:

- Adopted the template as project OS (see DECISION_LOG 2026-07-12).

Tests/checks run:

- None (no code yet).

Next step:

- `git init`, root AGENTS.md, first commit, hand M0 to Codex (prompt in `../05-ai-collaboration/CODEX.md`).

Risks:

- All Appendix B prices are `reported`, not confirmed — verify live at M1. MiMo first-party pricing sources conflict.

## 2026-07-09 → 2026-07-11: Master Plan v0.1 + Appendices A/B

Builder:

- Carlos + Claude (research/planning)

What changed:

- Master Plan v0.1 written (verdict, capability matrix, architecture, routing brain, fleet, MCP 2026-07-28 strategy, UX, build plan M0–M5, risks, Codex first prompt).
- Appendix A: two-backend access architecture (API routes vs subscription delegates), per-provider deep dives, nh-vault spec.
- Appendix B: complete verified model catalog (July 11) with delta logs.

Next step:

- Organize project folder, then pre-M0 setup.
