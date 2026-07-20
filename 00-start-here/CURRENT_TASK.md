# Current Task

## Immediate Goal — **W2 "TOOL EGRESS + EXEC" DONE + committed `e903ef0`** (2026-07-19). W3 done (`2b68163`), W1 done (`d95a8d6`); Slice A–D + fmt/gate/toolchain done; FULL Fable 5 audit done (75 findings; `d868f16`). M5 "Slice F: HARDEN" order W1✓→W3✓→W2✓→**W5**→W4. **NEXT = W5 "FLEET RELIABILITY" (nh-fleet)** — the one wave that REQUIRES amendment **A-M5-8** (nh-fleet is frozen). Not yet briefed. Sequence: (1) draft + ratify the A-M5-8 CONTRACTS_M5 §8 amendment defining nh-fleet's mutable surface (incl. the RunFailed ledger event W2 deferred to it) → (2) write the W5 brief seam-by-seam → (3) owner GO → (4) launch the single nosis Sol run → (5) post-Sol cycle (gate → adversarial review → owner → commit). A bare "continue" = ground the nh-fleet seams + draft the A-M5-8 amendment, then ask before the Sol launch.

**⇒⇒ W1 DONE + committed `d95a8d6` (2026-07-19).** Sol (codex exec, xhigh) implemented all 13 items
W1-1..W1-13 (14 audit findings: 1 high / 4 med / 7 low / 2 nit); orchestrator gated + adversarially reviewed
+ committed to main. **Gate: 363/0/1 `--release`, clippy `-D` clean, `cargo fmt --all --check` clean.**
8 files: nh-vault+nh-law src, nh-cli cmd_run/cmd_chat, root Cargo.toml + nh-vault/Cargo.toml, Cargo.lock
(only the nh-vault→url edge), + 1 owner-approved scope amendment (`nh-cli/tests/m2_exit.rs` — isolated temp
user-law declares the synthetic entry's audience; W1-6 fail-closed required it; security-neutral, also fixed
a real-home leak). Frozen signatures byte-stable (`Scrubber::new(Vec<String>)`, `send_verdict(&self,&str)`);
**nh-fleet untouched → NO A-M5-8** (reserved for W5). `url` = zero build weight (transitive via reqwest
2.5.8). Key fixes: url-crate host parity closes the `\@` exfil differential + `pub normalized_host`;
fail-closed audience; typed `AudienceRefused` + downcast; `\b`-anchored key shapes; bidi escape both
sanitizers; `Zeroizing` literals; keyring error surfaced; ITERATIVE glob/segment matchers (O(1) stack, no
60k-segment/200k-char DoS, semantics byte-identical); send_verdict + exec normalize. **Sol correctly STOPPED
CLEAN once** on a real scope conflict (M2 exit test's undeclared synthetic entry → W1-6 rightly refused);
the amendment resolved it. Full detail in BUILD_LOG 2026-07-19; brief archived
`Temp/slice-f-w1-brief-v1.txt`, amendment prompt `Temp/w1-amend-prompt.txt`.

**⇒⇒ W3 DONE + committed `2b68163` (2026-07-19).** Sol (codex exec, xhigh) implemented W3-1..W3-14 across
nh-core (turn loop + wire math) + nh-routes (honest routing/cost); orchestrator gated + adversarially reviewed
+ committed to main. **Gate: 377/0/1 `--release`, clippy `-D` clean, `cargo fmt --all --check` clean** (orch
ran the normalizing `cargo fmt` post-Sol to clear 3 hand-reflow drifts). 7 files: nh-core/lib.rs,
nh-routes/lib.rs+profiles.rs, nh-cli/cmd_run+cmd_chat+cmd_profile, nh-tui/lib.rs. Key fixes: compaction
cost-guard DROPPED (high-1 — now fires on a normal uniform history, counts `max(provider,estimate)`);
cross-currency normalized only via FRESH fx, stale = REFUSE not raw ¥-vs-$ (high-2, trace prints native);
`resolve_effort` wire-aware (A-M5-9, AnthropicMessages→None honest provider-default); receipt-append non-fatal
(paid answer + real provider error both survive); `cache_hit_pct` honest None on inconsistent usage; both HTTP
clients propagate body-read errors; anthropic `tool_use` requires nonempty id+name; +`push_user_block`/`min_cap`
reuse. **Owner ratified 3 design calls** (high-1 drop compaction guard; med-2 A-M5-9 wire-aware effort; high-2
USD-normalize via fresh fx, fail-safe REFUSE on stale). **+ owner-approved W3b addendum** (extend A-M5-9 glue
to `nh chat` + `nh profile` — display-only, Anthropic wire never serializes effort; folded `effort_for` to take
`Wire`, dropped `effort_for_wire`). **nh-fleet untouched → still NO A-M5-8** (reserved for W5). W3 + W3b both
PASS (one self-corrected compile slip in W3b). Briefs archived `Temp/slice-f-w3-brief-v1.txt` +
`slice-f-w3b-brief-v1.txt`; self-reports `Temp/w3-last-message.txt` + `w3b-last-message.txt`. Full detail in
BUILD_LOG 2026-07-19 (W3). §8 amendment **A-M5-9** (+ glue-extension note) in CONTRACTS_M5.

**⇒⇒ W2 DONE + committed `e903ef0` (2026-07-19).** Sol (codex exec, xhigh) implemented all 18 items W2-1..W2-18
across nh-tools + nh-mcp in ONE run (2 high / 3 med / 8 low / 5 nit), no deferrals; orchestrator gated +
adversarially reviewed + committed to main. **Gate: 395/0/1 `--release`, clippy `-D` clean, `cargo fmt --all
--check` clean** (Sol ran no fmt; no drift this wave). +18 tests over 377. 8 files, +981/−175: nh-tools/{lib.rs,
mcp.rs}, nh-mcp/{lib.rs,Cargo.toml}, cmd_run+cmd_chat (1-line `with_scrubber` glue), nh-tui/lib.rs, Cargo.lock.
Key fixes: high-3 ALL tool egress through `ToolResultEnvelope`(+ctx scrubber) at the single MCP-adapter choke
point; high-4 Windows exec `raw_arg` verbatim; medium-4 dep-free exec 300s timeout + null stdin + whole-tree
kill; medium-11 synchronous `fleet_run` preflight rejected to caller before spawn; low-6 MCP egress gated on
`Access::Send` (**Block stops egress before trust; default Allow byte-identical**); getrandom CSPRNG token +
subtle constant-time bearer; nh-mcp `State`→`Runtime` refactor; exact-route-only (404 else) + 1 MiB body cap
(413); OAuth refresh persist-warn + coalescing mutex. **Owner ratified 4 design calls** (low-6 `[send]` gate
Block-stops / getrandom+subtle vetted primitives / nit-8/9 State→Runtime refactor / medium-4 dep-free timeout).
**Two sound deviations** (both improvements, in the commit): (a) W2-17 `ServeConfig.token` stays
`Option<String>` (caller input = "mint one if absent"; the guaranteed-`Some` bound token lives on the new
`Runtime` struct → public config type unchanged for scoped callers); (b) W2-12/W2-10 use nh_fleet's REAL
`.nosis/fleet/{run_id}` ledger path, not the brief's approximate `run_root.join(run_id)`. **nh-fleet untouched
→ still NO A-M5-8** (built via `ToolCtx::new(...).with_guard(...)`, takes default scrubber). New deps getrandom
0.2.17 + subtle 2.6.1 (nh-mcp direct edges; already transitive → zero build weight, §0.4 W2 exception).
Adversarial review: **zero blocking issues**. Brief archived `Temp/slice-f-w2-brief-v1.txt`; self-report
`Temp/w2-last-message.txt`. Full detail in BUILD_LOG 2026-07-19 (W2).
- **Deferred to W5/A-M5-8** (owner-authorized frozen-boundary stop): a `fleet_run` that fails AFTER the
  synchronous preflight now emits a scrubbed warning, but cannot be returned to the original caller without an
  nh-fleet **RunFailed ledger event** — nh-fleet is frozen, so that surfacing is now formally W5 work.

**⇒⇒ NEXT: W5 "FLEET RELIABILITY" (nh-fleet)** — ratified order W1✓→W3✓→W2✓→**W5**→W4. NOT yet briefed. This is
the ONE wave that requires an amendment first: nh-fleet is M4-FROZEN, so W5 needs **A-M5-8** to define its
mutable surface before any Sol code. Sequence:
1. **Draft + ratify A-M5-8** (CONTRACTS_M5 §8): define nh-fleet's exact mutable surface for W5, INCLUDING the
   **RunFailed ledger event** contract W2 deferred (so a post-preflight fleet-run failure can reach the caller).
2. **Ground the seams + write the W5 brief** seam-by-seam (file:line) with adversarial tests — same cycle W1/W3/W2
   got. W5 targets (§0.1 Slice F plan): #6 budget-halt hang; #7 ledger torn-read; single-writer lock; resume
   hardcodes Native/drops offpeak; run_id unvalidated; receipts-without-usage never trip budget; run() dup
   validation. Pull exact finding IDs from `04-research/AUDIT_2026-07_fable5-full.md`.
3. **Owner GO** on the design calls (recommendation + why on each, per [[decisions-explain-rec-then-owner-decides]]),
   then **ask before launching** the Sol run.
- **Held for W4 (NOT W5):** low-16 (from_vault skip-signal), medium-20 (install_client key-literal union).
- **LAUNCH INVOCATION** (background, xhigh, single codex — never two nosis codexes at once):
  `codex exec --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox -m gpt-5.6-sol -c model_reasoning_effort=xhigh -o <last-msg-file> - < <brief-file>`
  (W1/W2 used this form — brief piped via stdin `-`, `-o` captures Sol's machine-readable self-report; the older
  `-s workspace-write` sandbox form also works — pick per environment. Verify `19820`-style stray codex
  sessions are yours before launching a second codex; NEVER two *nosis* codexes at once.)
- **After Sol returns:** kill nh.exe → `git diff --stat` (scope) → authoritative gate via `gate.ps1`
  (fmt --check + clippy + test --release; redirect + `echo $?`, NEVER `| tail`; **note the gate's real
  verdict is its `GATE: PASS/FAIL` summary line, NOT the background wrapper's exit code**; if `fmt --check`
  flags Sol's hand-written reflow drift, orch runs the normalizing `cargo fmt` under pinned 1.96.0 then
  re-gates) → adversarial security review → commit → next wave (W4, the last).

**PRIOR IN FLIGHT (Slice D — DONE, historical):**

**IN FLIGHT (2026-07-18):** Slice D "LEVER" briefed to Sol (gpt-5.6 xhigh, `codex exec` background). Brief
= `slice-d-brief-v1.txt` (+ `slice-d-preamble-v2.txt` for handoff #2). **Handoff #1 (`b123jcp3f`) STOPPED
clean** at a real frozen boundary (the `Receipt.effective_profile` field forces `effective_profile: None`
glue in FROZEN nh-fleet test literals slice_b.rs:405/490 that A-M5-7 hadn't authorized — Sol changed no
files, good catch). Fixed via the **A-M5-7 addendum** (§8: authorizes the full `Receipt`-literal ripple —
nh-fleet:405/490 + nh-tui:1373/3067 = `None`; make_receipt:1622 sets the real value; + a blanket clause so
trivial glue for the two field-adds never stops Sol again). **Handoff #2 = `bfan4b9wi` DONE + GATED GREEN** (357 pass / 0 fail / 1 ignored `--release`, clippy `-D
warnings` clean, verified independently; +18 tests over 339; scope exactly the surface — nh-fleet got its
3 authorized glue lines; EOL-noise on cmd_init/cmd_key/m2_exit restored). Adversarial review passed:
profiles.rs tighten-only correct, resolve_effort matrix honest (AlwaysThinking can't disable / None can't
enable), clamp_route only touches max_out (audience gate preserved), balanced==today default path,
all surfaces scrubbed. **FEEL gate:** owner ran `nh profile` demo + approved **resolve-thinking-to-route**
(the honest display: show effective effort none/low/high/max per route, not the abstract posture — same
"meter must not lie" principle as A-M5-6). **Handoff #3 = `b1dwlm8r2` DONE** (display-only FEEL refinement:
resolved-effort display none/low/high/max, shared `effort_label`). Re-gated independently GREEN
(**357/0/1** `--release`, clippy clean) + owner FEEL-approved the honest `nh profile` output. **SLICE D
COMMITTED `d6e2c7f` on `3a5df91`** (feat; CURRENT_TASK.md deliberately held out for the docs-close).
Amendment **A-M5-7** logged (§8: `Receipt.effective_profile` + `AgentLoop.profile` + frozen nh-fleet:1227
`profile: None` ripple, + the Receipt-literal-ripple addendum). **Owner ratified 3 design calls:** (1) two levers only — thinking tier + output cap; NO route
selection (M6), NO currency hard-stop; (2) frugal = route thinking floor + cap ≤16384 + off-peak pref;
balanced = today byte-for-byte; max-quality = route thinking ceiling (High) + cap = route.max_out; (3)
route capability immutable (AlwaysThinking always thinks, no-toggle never thinks; law never weakened).
**Design:** apply at the caller (clone route w/ clamped `max_out` → existing `make_client`, NO sig change
[5 callers incl frozen nh-fleet:1203]; set live `AgentLoop.thinking`); nh-routes new `profiles` module
(Profiles + EffectiveExecutionPolicy + ThinkingPosture, layered bundled→user→repo tighten-only mirroring
nh-law:200-325); nh-core `resolve_effort` posture×dialect helper + receipt field; nh-cli `--profile` +
`nh profile`; nh-tui `/profile` + HUD chip + live re-apply (SetProfile worker cmd).

**✓ DONE (2026-07-18, this session — the Slice E fmt piece pulled forward + more):**
- **Fmt normalized + gated** (commit `bc2a1b1`): one-time `cargo fmt --all` cleared the 37-hunk / 7-file
  backlog (behavior-preserving); added `gate.ps1` mechanizing fmt --check + clippy -D warnings + test
  --release with per-step exit-code aggregation (never `| tail`, whose 0 masks a failure). Re-gate GREEN:
  **357 pass / 0 fail / 1 ignored** `--release`, clippy clean.
- **Build hygiene** (commit `a71eb23`): `rust-toolchain.toml` pins 1.96.0 + rustfmt/clippy (stops fmt
  drift recurrence at the root — a newer rustfmt can no longer silently reflow); `.gitattributes` EOL
  policy; `deny.toml` DORMANT cargo-deny policy (cargo-deny NOT installed — wire `cargo deny check` into
  gate.ps1 after `cargo install cargo-deny --locked`).
- **FULL Fable 5 high audit DONE** (background workflow run `wf_72da5ecf-6f6`, 95 agents, ~3M tokens; report
  committed `d868f16` = `04-research/AUDIT_2026-07_fable5-full.md`). **86 raw → 75 confirmed / 11 refuted:
  0 critical, 7 high, 21 med, 30 low, 17 nit.** 3 highs orchestrator-confirmed vs source (#2 nh-routes
  cross-currency compare, #3 nh-tools MCP egress unbounded/unscrubbed, #5 nh-vault backslash audience
  bypass). Pre-scan: `unsafe` = **0**; `#[allow]` = 10 (benign).

**⇒ M5 "Slice F: HARDEN" — full audit remediation (owner chose EVERYTHING ACTIONABLE). Sol implements each
wave; owner FEEL-approves every human-facing surface BEFORE commit; nh-fleet is M4-FROZEN → its fixes need
a logged CONTRACTS amendment (A-M5-8). Ground each brief seam-by-seam w/ file:line + ASK owner before each
Sol launch. Full finding detail in the audit report.**
- **W1 SECURITY FLOOR — nh-vault + nh-law:** #5 backslash bypass; IPv6-audience-always-refused, `sk-`
  no-left-boundary, bidi spoof; empty-audience fail-open, Scrubber/Zeroize contract, make `normalized_host`
  pub (kill nh-cli `host_of` dup); law glob-recursion stack-overflow, `send_verdict` fail-open + host-norm,
  `exec_block` first-token sidestep; BUNDLED_LAW parsed-twice, repo_tries_to_weaken false-warn.
- **W2 TOOL EGRESS + EXEC — nh-tools + nh-mcp:** #3 MCP results through ToolResultEnvelope; #4 Windows exec
  `raw_arg`; ExecShell timeout+stdin, tools/list timeout, envelope literal-scrub, Send-verdict unenforced,
  OAuth refresh persist/race; nh-mcp CSPRNG token + constant-time compare + body cap + accept_loop signal,
  fleet_status 'starting', scrubber-recompile / State-dup / loose-routing.
- **W3 METER TRUTH — nh-core + nh-routes:** #1 compaction-dead (fold cost-check into candidate selection +
  realistic test); #2 cross-currency (partition-by-currency / fx-normalize + honest trace); compaction
  stale count, Anthropic wire drops resolved effort, deepseek think-low display; cache_hit_pct clamp,
  receipt-append destroys outcome, HTTP-body→empty, tool_use missing id; read_optional_profiles error + nits.
- **W4 SURFACES — nh-tui + nh-cli [FEEL-GATED]:** approval modifier-keys, wrapped_rows clip, failed-turn
  metering; worker-abandoned-on-quit, panic-hook, input-dead-while-Working; Esc-interrupt legend lie,
  heartbeat reset; usage-missing-as-fact `$0.00`, whole-run single-instant pricing, install_client scrub
  drop, ANSI passthrough, catalog injection, session-total omits unpriced, host_of dup, max-turns 0.
- **W5 FLEET RELIABILITY — nh-fleet [FROZEN → A-M5-8]:** #6 budget-halt hang, #7 ledger torn-read;
  single-writer lock, resume hardcodes Native/drops offpeak, run_id unvalidated, receipts-without-usage
  never trip budget, run() dup validation.
- **Order — OWNER-RATIFIED 2026-07-18:** W1 (security) → W3 (the meter thesis) → W2 (egress) → W5 (fleet)
  → W4 (surfaces, FEEL, last — consumes the fixed lower layers). A bare **"continue"** = ground + draft the
  **W1** Sol brief (nh-vault + nh-law) seam-by-seam, then **ask the owner before launching the Sol run**.

**Still queued (mechanical, no Sol — do opportunistically, e.g. while a Sol wave runs):**
`[workspace.lints]` + `#![forbid(unsafe_code)]` (audit done reading → unblocked; codify `unsafe`=0);
rest of Slice E "LOOP" (keyless CI, `codex exec --output-schema`, nextest+AV canary, gate.ps1
frozen-surface sensor). Then M5 DONE → write the ≥5 launch posts ([[why-best-in-category-2026]]).

**Slice C "VISIBLE" SHIPPED (E3 met — THE FEEL GATE PASSED).** Committed `a0a4036` on `e97ec1f`. Gate
green: **339 pass / 0 fail / 1 ignored** `--release`, clippy `-D warnings` clean (319 → +20: Slice C +17,
sub-cent fix +3). Built by Sol (two handoffs: the full slice, then a sub-cent honesty fix live-testing
exposed); gated + adversarially reviewed + **live-verified with the owner's real GLM key** (free
glm-4.7-flash end-to-end → honest `$0.00`; paid glm-5.2 → clean "insufficient balance" error; `nh why`
live) + **owner FEEL-approved before commit**. Delivered: money HUD (currency cached/miss/output split +
per-currency session total, replacing token-only; token budget hard-stop kept); THE counterfactual
savings line (`cost … — saved N% vs no-cache` + peak/no-cache/top-tier breakdown; cold turn makes no
false claim); approximate USD gloss (native = billed truth, `≈$` omitted when stale/absent, NEVER
FX-summed across CNY/USD); adaptive money precision (a real sub-cent spend never renders `$0.00` — only a
genuinely-free route does); `/why` (TUI + `nh why` CLI) off live `resolve_capable` + `RejectionTrace`;
approval cluster L6 fix (y/a/n/Esc only, any other key = no-op never a silent deny, always-this-session
rule, visible legend, Esc-to-interrupt); working heartbeat; OSC 9;4 Windows taskbar; errors-that-teach.
**drop-if-hard call:** "Esc to stop" while working was dropped (no truthful cooperative-cancel path — the
heartbeat shows `● WORKING · Ns` without claiming an interrupt). Amendment **A-M5-6** (USD gloss + `[fx]`
catalog data + no-cache-headline FEEL ruling) logged in `CONTRACTS_M5.md` §8. **FEEL finding worth
remembering:** at 2 dp a real ~$0.003 turn rounded to `$0.00` — fixed with adaptive precision (congruent
with "the meter must not lie"). Live money/savings % demo still pending a key with paid balance (free GLM
caps at `$0.00`; DeepSeek/Kimi or a funded GLM key would light up "saved 93%").

**Slice A "TRUTH" SHIPPED (E1 met).** Committed `9c96259` on `0bd1d7f` on `fe04ce5`. Full workspace gate
green: **306 pass / 0 fail / 1 ignored** `--release`, clippy `-D warnings` clean (baseline 292 → +14
Slice A tests). No FEEL gate needed (no human-facing surface). Built by Sol (two handoffs: truth-math +
resolver, then an Anthropic-wire fix); gated + adversarially reviewed by the orchestrator.
- **Delivered:** L1 explicit thinking-disable + kimi-toggle dialect; L2 state-aware reasoning replay
  (K2.6 thinking+tools no longer errors); L7 cache-safe compaction (elision note = new msg, retained
  msgs byte-identical); L8 reasoning+tool-spec token counting; L9 output cap on both wires; L12
  all-builds PrefixSeal + cache-break signal; effective_context clamp; native cache-field fallback;
  nh-routes `resolve_capable` + `RejectionTrace` (cheapest context-fitting priced route + audit trace).
- **Amendments logged (all in `CONTRACTS_M5.md` §8):** A-M5-1 (KimiToggle variant + preserve_when_thinking
  field, nh-routes); A-M5-2 (KimiToggle compile-compat arm in nh-fleet/nh-tui/nh-cli `effort_for` — the
  enum-variant ripple; orchestrator glue); A-M5-3 (build_anthropic_body consecutive-user merge — fixes an
  L7 regression the review caught: two consecutive user messages post-compaction 400 the Anthropic wire).
- **Two `[VERIFY-LIVE §7]` guesses to confirm with a live key later:** DeepSeek explicit non-thinking
  wire shape (`thinking:{type:disabled}`) + Kimi K2.6 toggle shape (`thinking:{type:enabled|disabled}`).
- **PROCESS LESSON (bit us this slice):** the workspace was clippy-clean but NEVER `cargo fmt`-clean;
  running `cargo fmt --all` mid-gate reformatted the ENTIRE workspace and polluted the diff across frozen
  crates. Recovered by reverting fmt-only churn to HEAD + re-applying the glue. **RULE: never `cargo fmt
  --all` mid-slice — use scoped `cargo fmt -p <crate>`; and always `git diff --stat` after a Sol run to
  confirm scope.** Slice E must add a scoped `cargo fmt --check` gate + a one-time workspace normalization.

**Slice B "FLOOR" SHIPPED (E2 met).** Committed `1a9d92a` on `70a2f9d`. Gate green: **319 pass / 0 fail /
1 ignored** (`--release`), clippy `-D warnings` clean (306 → +13). Built by Sol (single handoff), gated +
adversarially reviewed + live-demoed (audience redirect refused 3 ways incl. userinfo `@`-trick + suffix
spoof; nh-mcp unauth `fleet_run`→401, cross-Origin/Host→403, loopback→200). Delivered: L3 read guard
(`Access::Read`/`Send` + `[read]`/`[send]`/`[credential]` law classes mirroring `write_verdict`) +
`ToolResultEnvelope` (bounds + shape-scrubs tool output); L4 `get_scoped` credential-audience broker
(**trusted-law** source — bundled/user, repo can't grant; host-only compare; refuses BEFORE materialize)
wired on CLI routes + MCP config-load; L5 nh-mcp fail-closed OS-seeded token + Host/Origin (DNS-rebind);
L10 min-env exec allowlist; L11 MCP text sanitize; F8 scrubber widen (ghp_/AKIA/AIza/xox…) + `from_vault`;
F7 OAuth `resource` (RFC 8707). Amendments **A-M5-4** (Access-variant ripple; frozen nh-fleet + nh-tui got
exactly +2 guard arms each) + **A-M5-5** (audience broker call-sites; owner-ratified trusted-law source)
logged in `CONTRACTS_M5.md` §8.

**NEXT = Slice C "VISIBLE" (E3) — THE FEEL GATE (the milestone is won or lost here).** Crates: `nh-tui` +
`nh-cli` + the `nh-routes` cost helpers. **Graded by FEEL first, tests second; `drop-if-hard` per sub-item;
owner FEEL-approves every human-facing surface BEFORE commit.** Items (see `CONTRACTS_M5.md` Slice C): money
cost HUD (currency split over cached/miss/output + running session total + budget hard-stop, replacing the
token-only HUD); **THE counterfactual savings line** (`cost ¥0.11 — saved 93% vs naive (peak ¥0.44 ·
cache-miss ¥1.62 · pro-tier ¥3.90)` — the launch screenshot) via a new nh-routes `naive_cost`; `/why`
route-explain (CLI + TUI chip + receipt) using Slice A's `RejectionTrace`; approval cluster (explicit
y/n/Esc + visible legend — fixes L6 — prefix-rule approvals, Esc-to-interrupt, working heartbeat); OSC 9;4
Windows taskbar semáforo; "errors that teach" as a tested invariant. Slice C consumes Slice A's
`RejectionTrace` (shipped) + Slice B's floor (shipped). Brief Sol the SAME way that worked for A and B:
read the real nh-tui/nh-cli/nh-routes seams first, ground the brief seam-by-seam with file:line, define +
pre-authorize the mutable surface UP FRONT, enumerate cross-crate ripples BEFORE briefing, then ASK the
owner before launching the Sol run. Because C is the FEEL gate, plan to run the built `nh.exe` / TUI and
have the owner approve the FEEL before committing.

**M5 "The Honest Meter" — scope ratified & contract locked (2026-07-17, this session):**
- **Five slices A–E**, re-slotted by *seam* (not theme) for congruence: A TRUTH (nh-core meter-math
  + nh-routes thin honest-routing) / B FLOOR (nh-tools/law/vault/mcp) / C VISIBLE (nh-tui/cli — THE
  FEEL gate) / D LEVER (profiles) / E LOOP (build hardening). See `CONTRACTS_M5.md` (LOCKED).
- **Ratified calls:** thin honest-routing IN (Slice A; context-fit + expected-cost + rejection trace;
  NO jurisdiction/learning); forecast/`cost_estimate` OUT (one addition, not two); TWO defers held out
  (MCP TOFU-pin → M7 keep only sanitize; jurisdiction/governance/privacy-router → M6 keep only the
  `[read]`/`[send]` law class); behavior-corrections authorized enumerated (§0.1 mutable surface).
- **Positioning captured:** `01-product/WHY_BEST_IN_CATEGORY_2026.md` — the best-in-category thesis +
  seeds for ≥5 launch posts; append new article ideas there as found (write posts when M5 is "done").
- **NEXT:** write the Slice A Sol brief → `codex exec` (background) → gate (`cargo test --workspace
  --release` + clippy) + adversarial review → owner FEEL-approve → commit (`wip/slice-a` → main).

**M4 is CLOSED.** HEAD `6de331a` (docs: close M4) on `d3cac39` (research) on `aa751f4` (Slice D feat).
All four M4 slices committed: A `347bce6` (fleet), B `ecadc0a` (scheduler/ladder/swarm-seam),
C `26c6a22` (nh-mcp), D `aa751f4` (OAuth2, E4). `CONTRACTS_M4.md` LOCKED; §8 has the as-implemented
A-M4-1 clarification. Working tree clean after this session's 3 commits.

The **deep improvement research** is committed (`d3cac39`) — read it before planning M5:
`00-start-here/RESEARCH_2026-07_harness.md` (report) + `04-research/_harness-research-2026-07/`
(14 raw files: Sol xhigh 60-item backlog + Fable 5 high lenses A-M, 265 sources). Two models
independently converged on the identity and the priority; that convergence is the plan's backbone.

### The product identity both models converged on (this is the spine — keep everything congruent to it)
> **nosis is the agent harness with a meter: it routes every task to the cheapest CAPABLE model —
> by clock, cache, modality, thinking budget (and, new, jurisdiction) — and hands you the receipt.**

Design test for any feature: *can it produce a receipt / does it serve "priced, routed, receipted,
and you can see why"?* If not, it isn't nosis. The 60-second "aha" = the **counterfactual savings
line** (actual cost next to naive cost — a line no incumbent can print because their router can't
see their cache). This is the thing to build the whole milestone around.

## M5 = "The Honest Meter" (owner-picked direction 2026-07-17; RATIFY, then contract + brief)
Thesis: **make the meter TRUE, SAFE, and VISIBLE before adding autonomy or providers.** Both models
proposed this exact order (fix security + measurement first). Five congruent slices, each a Sol brief:

- **Slice A — TRUTH (the meter must not lie).** Fixes the live cost/correctness bugs found in the
  research (report §3): L1 thinking-defaults (governor None/Low silently buys full high thinking;
  DeepSeek normalizes low→high, auto-escalates harnesses to max — send `thinking:{type:disabled}`,
  new `kimi-toggle` dialect); L2 `reasoning_content` conditional replay (kimi-k2.6 thinking+tools
  ERRORS today); L8 `estimate_tokens` counts reasoning + tool-spec bytes; L9 Anthropic-wire output
  cap + `output_config` effort; parse DeepSeek native cache fields as fallback. Touches nh-core +
  catalog. **These are the highest per-dollar fixes in the whole report.**
- **Slice B — FLOOR (the meter must be safe/auditable).** L3 `read_file` guard + `[read]`/`[send]`
  law verdict class (closes the Lethal-Trifecta read leg); L4 credential-audience binding (repo
  config can currently redirect a vault secret to any URL); L5 nh-mcp inbound token-default +
  Host/Origin; L6 fix any-key-silently-denies approval bug; L11 MCP tool TOFU pinning + ANSI/
  invisible-char sanitize; min-env exec allowlist; widen Scrubber shapes + registry from all vault
  entries; supply-chain `deny.toml` + cargo-audit/deny; OAuth `resource` param (RFC 8707).
  Touches nh-tools/nh-law/nh-vault/nh-mcp.
- **Slice C — VISIBLE (the meter felt).** Money cost HUD (`turn_cost` over cached/miss/output) +
  session total + budget hard-stop; the **counterfactual savings line** (the aha); `/why` route-
  explain (CLI + TUI chip + receipt); honest activity suffix + Esc-to-interrupt + working heartbeat;
  OSC 9;4 Windows taskbar semáforo; "errors that teach" as a tested invariant. Mostly nh-tui + a
  tiny nh-routes helper.
- **Slice D — LEVER (savings selectable — the owner's toggle-per-provider-by-profile ask).**
  `profiles.toml` (frugal / balanced / max-quality) layered like law.toml → one
  `EffectiveExecutionPolicy` clamping wishes to route caps; `/profile` + HUD chip + receipt field;
  output caps threaded to both wires; tool-result envelope (bounded, handle+digest); cache-aware +
  **append-only** compaction (L7 — never mutate the prefix); `effective_context` clamp (context-rot
  guard). nh-routes/nh-core/nh-tui.
- **Slice E — LOOP HARDENING (continuous; congruent with the build process, no frozen crates).**
  `wip/<slice>` commit rule (durability — Slice D nearly lived only in Temp); `gate.ps1` frozen-
  crate/allowed-files sensor; minimal keyless CI (windows-latest + ubuntu, 292 mock tests);
  `codex exec --output-schema` structured Sol handoff; cargo-nextest + AV canary preflight;
  `[workspace.lints]` + pinned `rust-toolchain.toml`.

**FROZEN-CRATE NOTE (important):** the M4 freeze was per-milestone. Many Slice A/B items live in
nh-core/nh-tools/nh-law/nh-routes/nh-vault. **CONTRACTS_M5.md must define its own mutable surface +
the amendment list UP FRONT** (learn from A-M4-1: pre-authorize the seams so Sol never faces a
break-scope-or-duplicate choice). Additive/behavior-preserving where possible; adversarial security
review on every Slice-B item.

**The arc beyond M5 (directional, re-plan after M5 — do NOT let it leak into M5 scope):**
- **M6 = the Learning + Private + Resilient meter** — Route Scorecard off receipts → outcome-weighted
  ladder + failure-class-aware next_step + pre-run forecasts + `nh bench` (the moat); privacy-aware
  routing (governance metadata + privacy profile + custody receipts + one-question `nh init` — the
  new differentiator); five-stage compaction + session ledger/`nh resume` + file-memory; reliability
  (typed RouteError + backoff, availability re-resolve, cooldown, $0 local Ollama floor).
- **M7 = ecosystem + launch** — `nh exec --output json` + GitHub Action ($0 GLM CI), MCP Tasks,
  `nh gateway`, SKILL.md + `extensions.lock`, `nh acp`, cargo-dist→winget; **GLM/Z.ai key** (free,
  best ratio — $0 CI + vision + FlashX + 3rd Anthropic wire + SG privacy lane); multimodal input;
  hash-chained receipts + `nh verify`; Windows sandbox tier; nosistech.com launch post.
- **Scope CUT ratified by the research + July-2026 news:** demote subscription delegates from marquee
  pillar to escalation-gate footnote (Anthropic moved programmatic use to API pricing 2026-06-15;
  Gemini CLI died as open delegate). Reposition: **"open-weight-first harness with a frontier review
  gate."** Keep the commented catalog delegate schema; don't build the full adapter class in v1.

### ON RESUME ("continue") — Slices A–D + fmt/gate/toolchain DONE; audit DONE; NOW = Slice F HARDEN remediation.
**State (2026-07-18):** M5 Slices A "TRUTH" (`9c96259`) + B "FLOOR" (`1a9d92a`) + C "VISIBLE" (`a0a4036`) +
**D "LEVER" (`d6e2c7f`)** shipped. This session (owner "continue" + expanded scope): fmt-normalize +
`gate.ps1` (`bc2a1b1`), toolchain pin + `.gitattributes` + `deny.toml` (`a71eb23`), docs-close (`0c14743`),
+ a **FULL Fable 5 high audit** (report `d868f16`: 75 confirmed = 0 crit / 7 high / 21 med / 30 low / 17
nit). Gate GREEN **357/0/1**, clippy clean. §8 amendments A-M5-1..**7** (A-M5-8 pending for W5 fleet fixes).
**Owner chose "EVERYTHING ACTIONABLE" → M5 "Slice F: HARDEN"** = full remediation in 5 Sol-implemented waves
(W1 security floor / W2 tool egress+exec / W3 meter truth / W4 surfaces [FEEL] / W5 fleet [frozen]). **W1
(`d95a8d6`), W3 (`2b68163`), W2 (`e903ef0`) are DONE** — order now W1✓→W3✓→W2✓→**W5**→W4. If the owner typed
**"continue"**: do the **⇒⇒ NEXT: W5** block at the TOP — ground the nh-fleet seams + draft the **A-M5-8**
amendment (nh-fleet is frozen; W5 needs it FIRST) incl. the RunFailed ledger event W2 deferred, then write the
W5 brief seam-by-seam, **ask the owner before launching the Sol run**, gate + adversarially review, then W4
(the last, FEEL-gated). Also queued (no Sol): `[workspace.lints]`+`forbid(unsafe_code)` + rest of Slice E.
Do NOT re-do Slices A–D, W1/W3/W2, or the fmt/hygiene commits. Read the top blocks first.

1. **Confirm clean:** `git log --oneline -1` = `a0a4036`; `git status` clean. Kill any `nh.exe` before builds.
2. **Read the real seams FIRST** for Slice D: `nh-routes` (a NEW `Profiles` module + `EffectiveExecutionPolicy`
   that clamps profile wishes to route caps — layered bundled→user→repo like law, repo may only *tighten*);
   `nh-core` (apply the clamped policy at request-build: output cap [Slice A's mechanism], thinking tier);
   `nh-tui`/`nh-cli` (`/profile` toggle + HUD **profile chip** [deferred from C] + receipt field). Ground the
   brief seam-by-seam with file:line, the way A/B/C worked.
3. **Pre-authorize the mutable surface + enumerate cross-crate ripples** BEFORE briefing (the A-M5-2/-4/-6
   lesson). `profiles.toml` is new DATA (frugal/balanced/max-quality). Log any needed §8 amendment first.
   Then **ask the owner** the genuine decisions (e.g. the three profiles' exact caps; whether a *currency*
   budget hard-stop lands here — it was held out of C as a D lever) and **ask before launching the Sol run**.
4. **Brief Sol** (gpt-5.6 xhigh, `codex exec`, background — invocation below; NEVER two nosis codexes at
   once) for Slice D per `CONTRACTS_M5.md` Slice D. `drop-if-hard` per sub-item.
5. **Gate:** `git diff --stat` for scope → kill nh.exe → `cargo test --workspace --release` (≥339 + new,
   0 fail) + `cargo clippy --workspace --all-targets --release -- -D warnings`. **GATE RULE:** never pipe
   `cargo test`/`clippy` through `| tail` (a pipeline's exit code is the LAST command's — `tail`'s 0 masks a
   real failure); redirect to a file + `echo $?`. Never `cargo fmt --all` (scoped `cargo fmt -p` only).
   Adversarial review. If Sol STOPS at a frozen boundary, apply the compile-glue as orchestrator + log §8.
6. **FEEL gate + commit:** switching frugal↔max-quality must visibly change the built body (output cap /
   route ceiling) + HUD profile chip + receipt; a **repo** profile may only *tighten*. Owner FEEL-approves
   the profile chip/`/profile` BEFORE commit, then commit per-slice to `main` + update this file + memory.
   Slice E (LOOP/gate.ps1+CI — can land anytime, ideally soon to mechanize the gate) is the last M5 slice.

**When M5 is "done" (shipped + FEEL-approved):** write the ≥5 launch posts from
`01-product/WHY_BEST_IN_CATEGORY_2026.md` (append new article seeds there as they surface). [[why-best-in-category-2026]]

## Roles (fixed) — [[m2-m5-codex-sol-directive]]
- **Orchestrator = Opus 4.8** (this session): plans, writes contracts/briefs, runs gates, adversarially
  reviews, commits, docs. Does NOT hand-write milestone code.
- **Executor = GPT-5.6 Sol xhigh** via `codex exec` — writes all milestone implementation.
- STOP (don't fall back to Terra) if gpt-5.6-sol stops resolving or Sol fails the same gate twice.

## Executor invocation (proven — used for both M4 Slice D and the July-2026 research pass)
```
codex exec --skip-git-repo-check -s workspace-write -m gpt-5.6-sol \
  -c model_reasoning_effort=xhigh "$(cat /c/Users/capv2/AppData/Local/Temp/<brief>.txt)" < /dev/null
```
Run in background (harness-tracked); verify empirically after (numstat = truth; EOL/CRLF flags = noise).
Do NOT start a second codex on nosis while one writes nosis. Consider adding `--output-schema` for a
machine-readable self-report (report Lens G finding 5).
**STANDING RULE (2026-07-18): every Sol brief must say "do NOT run `cargo fmt`."** The workspace is now
fmt-clean, toolchain-pinned (1.96.0), and gated (`gate.ps1` runs `cargo fmt --all --check`). Formatting is
the orchestrator/gate's job; a Sol `fmt` run only risks re-reflowing whatever file it touches (this is what
polluted the Slice A and Slice D diffs). Sol writes code; the gate formats + checks.

## UX is THE priority (see [[ux-first-and-the-law]])
"Pretty but frustrating" = failure. Judge by FEEL first, tests second. Self-teaching, no handholding,
delightful for small tasks. Do NOT commit milestone work until Carlos approves the FEEL.

## Environment gotchas (bit us before)
- A running `nh.exe` LOCKS `target\debug\nh.exe` → build/test link fails. Kill it first.
- Bash tool `cd` PERSISTS → use absolute paths or `cd /c/Users/capv2/Desktop/nosis-Harness`.
- PowerShell reads UTF-8 as OEM codepage → box-drawing/`—` look like mojibake; check raw BYTES.
- Kaspersky AV blocks freshly built `nh.exe` (os error 5) → `fleet_kill_resume` + `m2_exit` spawn
  tests fail on env, not code. cargo-nextest + an AV canary preflight (Slice E) will classify this.
- Do NOT change Carlos's Claude Code settings or Kaspersky settings without asking.

## Do Not Do
- Do NOT hand-write milestone code — Sol implements; Claude plans + gates.
- Do NOT commit milestone work until Carlos approves the FEEL (UX is the gate, not "tests pass").
- Do NOT touch a crate M5's contract hasn't opened, without a logged CONTRACTS_M5 amendment.
- Do NOT let the M6/M7 arc leak into M5 scope (scope creep is the plan's named #1 killer).
- Do NOT ship the nh-mcp server publicly before the MCP final spec lands (2026-07-28).
