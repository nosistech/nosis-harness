# CONTRACTS_M5.md — Locked surface for Milestone M5 ("The Honest Meter")

**Status: LOCKED (orchestrator Opus 4.8; owner scope-ratified 2026-07-17).** Builder = GPT-5.6 Sol
xhigh via `codex exec`. Claude plans + gates + adversarially reviews; Sol implements EXACTLY the
enumerated seams below. Amendments go through the orchestrator only, logged §8.

**Spec source:** `00-start-here/RESEARCH_2026-07_harness.md` (§1 top-15, §3 live-issues L1–L12, §7
master backlog, §10 sequencing), `00-start-here/CURRENT_TASK.md` (owner direction 2026-07-17),
`02-architecture/SECURITY_MODEL.md`. Both research engines (Fable 5 high, Sol xhigh) independently
converged on the identity and the priority below — that convergence is the spine.

**The identity M5 serves (every seam must be congruent to it):**
> nosis is the agent harness with a meter: it routes every task to the cheapest CAPABLE model — by
> clock, cache, modality, thinking budget — and hands you the receipt.

**M5 thesis:** make the meter **TRUE, SAFE, and VISIBLE** (and its routing choice HONEST) before
adding any autonomy, learning, or providers. **One verb — *meter*. No second verb.** M5 wins the
beachhead (honesty + visibility + safety); M6 wins the moat (intelligence + reliability + resume).

**Owner scope rulings (2026-07-17) — the four ratified decisions:**
1. **Five slices A–E** (TRUTH / FLOOR / VISIBLE / LEVER / LOOP), re-slotted by *seam* for congruence.
2. **Thin honest-routing IS in** (Slice A) — makes "cheapest capable" true, not aspirational; powers
   `/why`. **One addition, not two:** the pre-run forecast / `cost_estimate` are OUT (M6-adjacent).
3. **Two DEFERS held out of M5:** (a) MCP TOFU/hash-pinning → M7 (`extensions.lock` does it
   provenance-wide; M5 keeps only ANSI/invisible **sanitize**); (b) jurisdiction routing + `governance`
   catalog metadata + privacy-router filter → M6 (M5 ships only the `[read]`/`[send]` law **class**).
4. **Behavior-corrections authorized, enumerated (§0.1).** M5 canNOT stay "additive only" like M4 —
   fixing the meter bugs *changes what the wire sends*. Public **type** signatures stay
   source-compatible; **wire behavior** changes only at the enumerated seams, each pinned by a new test.

---

## 0. Ground rules (bind every builder)

### 0.1 The M5 mutable surface + amendment list (UP FRONT — the A-M4-1 lesson)

M4 froze five crates whole. **M5 REOPENS them — but each crate is open ONLY for the enumerated seams
below; every other line in them stays frozen.** This list IS the pre-authorization: Sol implements
these seams without stopping. Any need to touch a seam NOT on this list → **STOP, amend §8 first.**
Each seam is tagged **[+]** additive (source-compatible) or **[Δ]** behavior-correcting (bug fix; the
wire changes; a new test pins the corrected behavior).

**`nh-core`** (Slice A truth-math; D policy application):
| Seam | Ref | Tag | Change |
|---|---|:--:|---|
| `apply_thinking` | lib.rs ~301-326 | Δ | emit `thinking:{type:disabled}` for None/Low on disable-capable dialects; always send explicit effort where required; add `kimi-toggle` dialect handling. |
| `reasoning_to_send` + `OpenAiPolicy` | ~283-298, ~130 | Δ | reasoning replay conditional on **effective** thinking state (`preserve_when_thinking`), not a static flag. |
| `compact_history` | ~1332-1368 | Δ | insert elision note as a **NEW appended message**; never mutate `history[1]`/the prefix; cache-aware trigger (only when recache cost < projected savings). |
| `estimate_tokens` | ~1316-1328 | Δ | count `reasoning_content` (when the route preserves it) + serialized tool-spec bytes. |
| Anthropic `max_tokens` + OpenAI `build_body` | ~138-143, ~505, ~225-274 | Δ | budget-aware output cap (not hard 8192); **send `max_tokens` on the OpenAI wire** (currently none); map effort→`output_config` where supported. |
| `PrefixSeal` / `debug_assert` sites | ~1151-1239 | Δ | promote prefix byte-stability to an **all-builds** `PrefixSeal` check + a cache-break detector. |
| `effective_context` clamp | new | + | per-route context clamp guarding the compaction trigger (context-rot guard). |
| native cache-field parse | usage extraction | + | parse `prompt_cache_hit_tokens`/`prompt_cache_miss_tokens` as a fallback. |
| `EffectiveExecutionPolicy` application | request build | + | apply the clamped policy (output cap, thinking tier) at build time (Slice D consumer). |

**`nh-routes`** (Slice A resolver; C cost helpers; D profiles):
| Seam | Ref | Tag | Change |
|---|---|:--:|---|
| `resolve_capable` (new) + `RejectionTrace` | alongside `resolve`/`provider_default` ~529-560 | + | capability(**context-fit**)-filtered, expected-cost-ordered resolver returning the chosen route **+ an auditable rejection trace** ("skipped X: ctx 32K<45K; skipped Y: 4× price"). Existing entry points UNCHANGED. **No jurisdiction, no learning** (M6). |
| `naive_cost` / cost helper (new) | near `price_at` ~173 | + | pure `cost(price, tokens)` + `naive_cost` (peak × cache-miss × top-tier over same tokens) — the counterfactual line's math. Takes primitives (no nh-core dep). |
| `Profiles` + `EffectiveExecutionPolicy` (new) | new module | + | parse `profiles.toml` layered like law; produce the policy that **clamps profile wishes to route caps** (repo may only *tighten*). |

**`nh-tools`** (Slice B floor):
| Seam | Ref | Tag | Change |
|---|---|:--:|---|
| `Access` enum | lib.rs 16-19 | + | add `Read(&str)` and `Send(&str)` variants (currently Write/Exec only). |
| `ReadFile::execute` | lib.rs 154-181 | Δ | consult `ctx.guard` with `Access::Read` before reading; return a bounded `ToolResultEnvelope`. |
| exec tool result + spawn | exec path | Δ | bounded envelope on output; **min-env allowlist** on the spawned process (not full env). |
| `ToolResultEnvelope` (new) | new | + | `{ excerpt, handle, digest }` — bounds every tool result (DoW / injection / token surface). |
| `parse_tool` | mcp.rs ~493-517 | + | `sanitize_untrusted_text()` on descriptions/schemas (strip ANSI / invisible chars). **No TOFU pin** (M7). |
| OAuth token request | M4 Slice D code | + | add `resource` param (RFC 8707) to the refresh/token grant. |

**`nh-law`** (Slice B floor):
| Seam | Ref | Tag | Change |
|---|---|:--:|---|
| `read_verdict` + `send_verdict` (new) | mirror `write_verdict` 88-104 | + | pattern-match `[read] block` / `[send] block` lists; reuse existing `Verdict::{Allow,Ask,Block}` (no new variant). |
| `law.toml` `[read]`/`[send]` sections | data | + | data-only additive sections. |

**`nh-vault`** (Slice B floor):
| Seam | Ref | Tag | Change |
|---|---|:--:|---|
| `Scrubber` `KEY_SHAPES` | lib.rs 85-89 | Δ | widen shapes (add `ghp_`/`AKIA`/`AIza`/`xox…`). Deterministic → cache-safe. |
| `Scrubber` constructor / registry | ~94-100 | + | seed literals from a shared registry of **all** vault entries. |
| credential audience broker (new) | new | + | `get_scoped(entry, audience)` validates host **before** the secret materializes (closes repo-config credential redirect). |

**`nh-mcp`** (Slice B floor — new in M4, reopened):
| Seam | Ref | Tag | Change |
|---|---|:--:|---|
| `authorized()` | lib.rs 189-198 | Δ | default-mint a token when none set (no silent open door); validate Host/Origin (DNS-rebind guard). |

**`nh-tui`** (Slice C visible; D lever — NOT frozen, fully open for these seams):
| Seam | Ref | Tag | Change |
|---|---|:--:|---|
| `hud_line` / `render_hud` | ~401-428, ~1840 | Δ | add **currency cost** (cached/miss/output split), session total, budget hard-stop, the **counterfactual savings line**, a profile chip. |
| `reduce_key` + approval row | ~1355 | Δ | explicit `y`/`n`/`Esc` only + a visible legend (fix L6); prefix-rule approvals (y / always-this-session / no); Esc-to-interrupt. |
| working heartbeat | new | + | `WORKING · 34s · Esc to stop` live suffix. |
| OSC 9;4 taskbar semáforo | new | + | yellow taskbar = "waiting on you" (Windows-first, zero deps). |
| `/why`, `/profile` commands | command dispatch | + | route-explain (uses the `RejectionTrace`) + profile toggle. |
| "errors that teach" helper | new | + | tested invariant (the Slice-D OAuth line is the template). |

**`nh-cli`** (Slice C, D — open):
| Seam | Tag | Change |
|---|:--:|---|
| cost display in `nh run`/`nh chat`; `nh why` | + | print the savings line + `/why` on the CLI path (not just TUI). |
| `--profile` / `nh profile` | + | select/show the active profile. |

**Data (always allowed — data, not frozen code):** `catalog.toml` (kimi-toggle dialect flag,
`preserve_when_thinking`, cache-field map, output caps), `law.toml` (`[read]`/`[send]`),
`profiles.toml` (NEW: frugal / balanced / max-quality).

**Repo tooling (Slice E — no runtime crate):** `gate.ps1`, `.github/workflows/*`, `deny.toml`,
`rust-toolchain.toml`, `[workspace.lints]`, nextest config.

**Explicitly STILL FROZEN (M5 does NOT open):** **`nh-fleet`** (the escalation ladder stays as-is —
M5's resolver is *initial* cheapest-capable selection, a different concern from fleet fallback); and
every seam in any crate NOT enumerated above. Touching either → STOP, amend §8.

### 0.1-F Slice F "HARDEN" — audit-remediation seams (2026-07-18, owner chose EVERYTHING ACTIONABLE)

Slice F remediates the 75-finding Fable 5 audit (`04-research/AUDIT_2026-07_fable5-full.md`) in 5 waves,
owner-ratified order **W1→W3→W2→W5→W4**. Each wave opens the seams below (in already-open crates unless a
frozen-crate row is marked). Same discipline as §0.1: Sol implements these without stopping; anything not
listed → STOP, amend §8. Full finding detail (why/fix/verifier) lives in the audit report; the wave here is
the authorization + file:line index.

**W1 — SECURITY FLOOR (nh-vault + nh-law + minimal nh-cli glue). No frozen crate touched → no §8 amendment.**
Public signatures frozen `nh-fleet` depends on stay byte-stable (`Scrubber::new(Vec<String>)`,
`Policy::send_verdict(&self,&str)->Verdict`). Two owner-ratified calls: **(Q1)** host-parse uses the `url`
crate (§0.4 exception below), not a hand-rolled splitter; **(Q2)** undeclared vault entries are **fail-closed**.

| # | Ref (audit) | Crate:seam | Tag | Change |
|---|---|---|:--:|---|
| W1-1 | high-5 + low-15 | nh-vault `normalized_host` L141 | Δ | parse host via `url::Url` (reqwest parity, kills the backslash exfil differential); make `pub`; delete nh-cli `host_of` dup (cmd_run.rs ~256 + test ~397) and rewire its 3 sites. |
| W1-2 | medium-8 | nh-vault `get_scoped`/`audience_allows` L184 | Δ | fixed by W1-1 idempotent parser (IPv6 audiences approve). |
| W1-3 | medium-9 | nh-vault `KEY_SHAPES` L93 | Δ | `\b`-anchor `sk-`/`csk-` (stop mangling "risk-…"). |
| W1-4 | medium-10 | nh-vault `sanitize_line`/`sanitize_untrusted_text` L199,228 | Δ | escape bidi controls (U+202A-E, U+2066-9, U+061C). |
| W1-5 | low-12 | nh-vault `Scrubber.literals` L103 | Δ | field → `Vec<Zeroizing<String>>`; **`new(Vec<String>)` signature unchanged** (wrap internally → no nh-fleet ripple). |
| W1-6 | low-14 | nh-vault `audience_allows` L167 | Δ | empty approved list → **REFUSE** (fail-closed). |
| W1-7 | low-13 | nh-vault `EnvFallbackVault::get` L56 | Δ | preserve the inner keyring error in the miss message. |
| W1-8 | low-29 | nh-vault (new) `AudienceRefused` + nh-cli cmd_chat.rs L117 | + | typed error replaces the fragile `starts_with("refused:")` coupling. |
| W1-9 | medium-7 | nh-law `glob_matches`/`segment_matches` L415,450 | Δ | iterative two-pointer rewrite (kill stack-overflow DoS); drop-if-hard: clamp oversized input to the fail-safe verdict. |
| W1-10 | low-10 | nh-law `send_verdict` L119 | Δ | ASCII-lowercase + strip trailing dot before matching (signature unchanged — frozen nh-fleet calls it). |
| W1-11 | low-11 | nh-law `exec_pattern_matches` L406 | Δ | case-fold first token + test common wrapper/chain prefixes (Ask default kept). |
| W1-12 | nit-6 | nh-law `ConstitutionSources.bundled`/`assemble_constitution` L83,172 | Δ | carry pre-extracted text (`Option<String>`), parse once; nh-law-internal only. |
| W1-13 | nit-7 | nh-law `repo_tries_to_weaken` L321 | Δ | warn only when a repo field would actually grant. |

**Held for W4 (not W1):** low-16 (`from_vault` skip-signal) + medium-20 (install_client key-literal union) —
both nh-cli display/rebuild surface. W2/W5 seam tables to be appended when each wave is briefed.

**W3 — METER TRUTH (nh-core + nh-routes; +2-line resolve_effort glue in nh-cli/nh-tui). Owner-ratified three
design calls 2026-07-19: (Q1) med-2 fixed via wire-aware `resolve_effort` → §8 amendment A-M5-9; (Q2) high-1
drops the compaction cost guard; (Q3) high-2 normalizes cross-currency to USD via FRESH fx, fail-safe when
stale.** Only signature change is `resolve_effort` gaining a trailing `wire` param (nh-fleet does NOT call it).
Brief: `Temp/slice-f-w3-brief-v1.txt`.

| # | Ref (audit) | Crate:seam | Tag | Change |
|---|---|---|:--:|---|
| W3-1 | high-1 | nh-core `compact_history` L1836 | Δ | drop the `elided<=retained` guard (compaction only runs post-trigger; overflow-avoidance beats cache-warmth). Realistic uniform-turn test. |
| W3-2 | medium-1 | nh-core compaction trigger L1586 | Δ | `input_tokens = max(latest_prompt_tokens, estimate_request_tokens(...))` so the trigger sees this turn's tool-result additions. |
| W3-3 | medium-3 | nh-core `resolve_effort` explicit branch L107 | Δ | coerce DeepseekNhm explicit Low → None (display==wire; mirrors GlmHm). `nh run` path only; TUI `/effort` is W4. |
| W3-4 | medium-2 | nh-core `resolve_effort` L100 (+ cmd_run.rs:64, nh-tui:1456 glue) | Δ | **A-M5-9**: add trailing `wire` param; AnthropicMessages → None (honest provider-default). OpenAi unchanged. |
| W3-5 | low-1 | nh-core `cache_hit_pct` L152 | Δ | return None when cached>prompt (no fabricated 100%). Update blessing test L747. |
| W3-6 | low-2 | nh-core `run_with_history` receipt append L1623,1649,1678 | Δ | receipt-write failure is NON-FATAL: keep the real Ok/answer or the original provider Err; warn via `emit`. |
| W3-7 | low-3 | nh-core wire clients `resp.text()` L252,545 | Δ | `.map_err(send_error)?` instead of `unwrap_or_default()` (no silent empty-body). |
| W3-8 | low-4 | nh-core `parse_anthropic_response` L691 | Δ | tool_use block missing id/name → parse Err (clear local error, not a downstream 400). |
| W3-9 | nit-1 | nh-core `WireUsage.prompt_cache_miss_tokens` L460,501 | Δ | delete dead field + binding (miss derived in cost_of). |
| W3-10 | nit-2 | nh-core `build_anthropic_body` L577,609 | Δ | extract one `push_user_block` helper (dedupe the merge logic). |
| W3-11 | high-2 | nh-routes `resolve_capable` L848 | Δ | single-currency compares native; cross-currency normalizes to USD via FRESH fx, fail-safe refuse when stale; trace prints native amounts cross-currency. |
| W3-12 | low-5 | nh-routes `read_optional_profiles` L285,292 | Δ | include the io/parse error in the warning (surface the actionable ThinkingPosture message). |
| W3-13 | nit-3 | nh-routes `from_toml` L639 | Δ | validate one-currency-per-provider at parse (fail-closed; shipped catalog is clean). |
| W3-14 | nit-4 | nh-routes `Profiles::effective`/`clamp_route` profiles.rs L207,247 | Δ | one shared `min_cap` helper (one truth; defensive re-min preserved). |

### 0.2 THE LAW + UX-first
- THE LAW (top authority): small, simple, secure, safe, lightweight, readable, auditable, modular,
  congruent, harmonic. **Reuse over duplication** — the resolver reuses `price_at`/`peak_status`; the
  cost line reuses catalog price data; `read_verdict` mirrors `write_verdict`; the outbound scrubber
  reuses the existing `Scrubber`.
- **UX-first STILL governs — and Slice C is where the milestone is won or lost.** The single
  determinant of best-in-category is that the money HUD + savings line + `/why` + approvals **FEEL
  effortless**. "Pretty but frustrating" = failure. `drop-if-hard` on any C sub-item that can't be
  made calm. See [[ux-first-and-the-law]]. Owner FEEL-approves every human-facing surface before commit.

### 0.3 Security invariants (carry from M0–M4; M5 raises the floor)
- **Every rendered/logged/persisted/EGRESSED string passes `nh_vault::Scrubber` first** — HUD, savings
  line, `/why`, receipts, envelopes, rejection traces, OAuth lines. M5 ADDS the egress path (`[send]`)
  to this rule.
- **exec_shell stays approval-gated; `read_file` now guarded too** (`Access::Read`). No tool returns
  unbounded output (envelope). Approvals show the **full** action (never approve a truncated action).
- **nh-mcp does NOT ship publicly before the MCP final spec (2026-07-28)** — binds `127.0.0.1`, carries
  the preview banner; M5 only *closes the auth hole*, it does not expose the server.
- **The meter must not lie about safety:** a leaked key in any surface, an unguarded read, an
  unbounded tool result, or an unauthenticated money-spend is an M5 exit-blocking defect.

### 0.4 Dependency additions (orchestrator-authorized here)
- **No new runtime crates.** M5 adds NO external dependencies to the runtime workspace — every seam is
  std + the already-vendored `serde`/`regex`/`chrono`/`anyhow`. (The savings math, envelope, resolver,
  profiles, and law classes are all pure Rust over existing types.)
- **Slice E tooling only:** `cargo-nextest` + `cargo-deny`/`cargo-audit` are dev/CI tools (not workspace
  deps); `deny.toml` + `rust-toolchain.toml` + `[workspace.lints]` are config. No runtime impact.
- **Slice F exception (owner-ratified 2026-07-18):** `url` becomes a **direct** dep of `nh-vault` (W1-1)
  so the credential broker parses hosts with the exact same parser as `reqwest` — closing the parser-
  differential exfil class (high-5), not just the one backslash trick. `url` v2.5.8 is already compiled
  (transitive via reqwest), so this is a dependency-*declaration* change with **zero build weight**. This
  is a deliberate, scoped override of "no new runtime crates" for one security-critical seam.

### 0.5 M5 exit criteria (each maps to one slice + a real headless test)
- **E1 — TRUTH (Slice A).** On a mock provider: a None/Low-effort turn's built body has thinking
  disabled + an explicit output cap **on both wires**; a compaction event leaves the prefix bytes
  **byte-identical** (PrefixSeal holds in release) and APPENDS the elision note; `estimate_tokens`
  counts reasoning + tool specs; `resolve_capable` returns the cheapest **context-fitting** route plus
  a `RejectionTrace` naming why each costlier/incapable route was skipped. Kimi-k2.6 thinking+tools
  round-trips reasoning_content conditionally **without erroring**.
- **E2 — FLOOR (Slice B).** A `read_file` of `.env`/`*.pem` is **blocked** by the guard (`[read]`
  verdict); an over-cap tool result returns a bounded envelope; nh-mcp **rejects** an
  unauthenticated / cross-Origin `fleet_run`; a fake key literal in any tool output / egress is
  `[REDACTED]`.
- **E3 — VISIBLE (Slice C).** After a mock turn the HUD shows **currency** cost (cached/miss/output) +
  session total; the **counterfactual savings line** prints from catalog price × JSONL tokens; `/why`
  explains the route using Slice A's `RejectionTrace`; the approval row accepts **only** y/n/Esc with a
  visible legend. (Graded by FEEL, then the test.)
- **E4 — LEVER (Slice D).** Switching profile (frugal ↔ max-quality) changes the
  `EffectiveExecutionPolicy` → a different output cap / route ceiling on the next turn's **built body** +
  HUD chip + receipt field; a **repo** profile can only *tighten*, never loosen, the user/law caps.
- **E5 — LOOP (Slice E).** `gate.ps1` **fails** a simulated out-of-surface edit and **passes** a
  within-surface one; CI runs the full mock test suite keyless on windows-latest + ubuntu;
  nextest + the AV canary classify an AV-blocked exe as `EnvironmentBlocked`, not `FAIL`.

---

## Slice A — TRUTH: every number AND the routing choice is honest and provable (E1)

**Crates:** `nh-core` (the meter-math) + `nh-routes` (the honest resolver). One theme, two crates. The
heaviest slice — Sol MAY split it into up to 3 sequential handoffs (thinking/reasoning → cache/context →
resolver), each gated, but it is ONE contract section.

### A.1 Thinking + reasoning truth (L1, L2)
- **L1:** the governor tier maps HONESTLY to the wire. None/Low on a disable-capable dialect (deepseek-nhm)
  sends `thinking:{type:disabled}` — it must not silently buy full high thinking. A new `kimi-toggle`
  dialect (catalog data) handles K2.6's toggle. Always send an **explicit** effort where the dialect
  requires one (DeepSeek normalizes `low`→`high` and auto-escalates recognized harnesses to `max` — so
  omission is a cost bug). `[VERIFY-LIVE §7]`.
- **L2:** `reasoning_content` replay is conditional on the **effective** thinking state
  (`preserve_when_thinking`). K2.6 thinking+tools ERRORS today when prior reasoning isn't replayed;
  DeepSeek's contract requires it. The static `preserve_reasoning` flag becomes state-aware.

### A.2 Cache + context + token truth (L7, L8, L9, L12, clamp, cache-fields)
- **L7 (cost bug):** `compact_history` inserts the elision note as a **new appended message** —
  `history[1]` and the whole prefix stay byte-identical, so the next turn is a cache HIT, not a ~120×
  MISS. Compaction only fires when recache cost < projected savings.
- **L12:** prefix byte-stability is enforced in **all** builds (`PrefixSeal`, not `debug_assert`), with a
  cache-break detector that surfaces drift instead of silently eating a cache miss.
- **L8:** `estimate_tokens` counts `reasoning_content` (when preserved) + serialized tool specs — so
  compaction fires on time and the provider never overflows.
- **`effective_context` clamp:** compaction arms at a per-route *effective* context (context-rot guard),
  not the raw window (arming at 700K on a 1M route is self-defeating).
- **L9:** output is capped on **both** wires — the OpenAI wire (currently no `max_tokens`) and a
  budget-aware Anthropic cap (not the hard 8192); effort maps to `output_config` where supported.
- **Cache fields:** parse `prompt_cache_hit_tokens`/`prompt_cache_miss_tokens` natively as a fallback
  (ecosystem tools get this wrong) — the honest denominator for the savings line.

### A.3 Honest routing — the thin resolver (the ratified addition)
- `resolve_capable(task_estimate, allowed_set) -> (ResolvedRoute, RejectionTrace)`: filter routes by
  **context-fit** (route window ≥ estimated prompt+output), order the survivors by **expected cost**
  (honest `price_at` × L8 token estimate), pick the cheapest, and record a `RejectionTrace` — a
  structured, scrubbed list of every rejected route + the reason ("ctx 32K < 45K", "4.0× price").
- **No jurisdiction** (needs M6 governance metadata) and **no learning** (needs M6 receipts fold). "Cheapest
  capable" becomes TRUE for the dimensions that exist in M5's world (clock, cache, price, context-fit).
- Existing `resolve`/`provider_default` are UNCHANGED (source-compatible); this is a NEW entry point.

### A.4 Tests (headless, mock provider — E1)
- Built-body assertions: None/Low → thinking disabled on deepseek-nhm; explicit effort present; output
  cap on BOTH wires. Kimi-toggle turn round-trips reasoning conditionally without a provider error.
- Compaction: after a compaction, `message_bytes(history[0])` and the retained prefix are byte-identical;
  the elision note is a NEW message; PrefixSeal passes in a `--release` test build.
- `estimate_tokens`: a message with `reasoning_content` + tool specs estimates strictly higher than the
  old byte/4 count; the delta ≈ the counted bytes.
- `resolve_capable`: a task whose estimate exceeds a cheap route's window selects the next-cheapest
  fitting route and the trace names the skip reason; ties break to lowest expected cost.

---

## Slice B — FLOOR: the meter is safe/auditable (E2)

**Crates:** `nh-tools`, `nh-law`, `nh-vault`, `nh-mcp`. **Adversarial security review on EVERY item.**

- **L3 — read guard + `[read]`/`[send]` law CLASS.** `Access::Read`/`Access::Send` added; `ReadFile`
  consults the guard (closes the Lethal-Trifecta read leg — secrets can't be read into a CN-bound
  prompt); `read_verdict`/`send_verdict` mirror `write_verdict`; the outbound (`[send]`) path is
  Scrubber-checked. **Mechanism only** — the privacy-*routing* filter is M6.
- **Tool-result envelope.** `read_file`/`exec` return `{ excerpt, handle, digest }`, bounded — closes a
  denial-of-wallet + prompt-injection + token surface. The full content stays retrievable by handle.
- **L11 — sanitize (only).** `sanitize_untrusted_text()` strips ANSI + invisible Unicode from MCP
  tool descriptions/schemas before the model sees them. **TOFU/hash pinning is M7** (`extensions.lock`).
- **L4 — credential audience binding.** `get_scoped(entry, audience)` validates the host against the
  entry's approved audience BEFORE the secret materializes — a repo checkout can no longer redirect a
  real vault credential to an attacker origin.
- **L5 — nh-mcp inbound auth.** `authorized()` default-mints a token (no silent open door) and validates
  Host/Origin (DNS-rebind). An unauthenticated / cross-Origin `fleet_run` is refused (it spends money).
- **Scrubber widen + registry.** Add `ghp_`/`AKIA`/`AIza`/`xox…` shapes; seed literals from all vault
  entries. Deterministic (cache-safe).
- **Min-env exec allowlist.** The spawned exec process gets an allowlisted env, not the full parent env.
- **OAuth `resource` (RFC 8707).** Add the resource indicator to the M4 OAuth grant.

### B tests (E2)
- Guard: `read_file(".env")` / `*.pem` → `Block`; a normal source read → `Allow`. Egress of a secret →
  `[send]` block. Envelope: an over-cap `exec` output returns a bounded excerpt + a resolvable handle.
- nh-mcp: `fleet_run` with no token → 401/refused; with a mismatched Origin/Host → refused; with the
  minted token + loopback → runs. Scrubber: `ghp_…`/`AKIA…` literals `[REDACTED]` in every surface.
- Audience: a `get_scoped(entry, "https://evil.example")` where the entry's audience is the real API →
  refused before materialization.

---

## Slice C — VISIBLE: the meter felt (E3) — THE UX GATE

**Crates:** `nh-tui`, `nh-cli` + the `nh-routes` cost helpers (A). **This is where best-in-category is
won.** Graded by FEEL first, tests second. `drop-if-hard` per sub-item.

- **Money cost HUD.** Currency cost per turn split over cached / miss / output tokens, a running session
  total, and a budget hard-stop — replacing the token-only HUD. Honest-stale flag on any `verify_live`
  price.
- **The counterfactual savings line (THE aha — the launch screenshot).** After a turn:
  `cost ¥0.11 — saved 93% vs naive (peak ¥0.44 · cache-miss ¥1.62 · pro-tier ¥3.90)`. Pure function over
  catalog price + JSONL tokens (via `naive_cost`). No incumbent CAN print it (their router can't see
  their cache).
- **`/why` route-explain (CLI + TUI chip + receipt).** Explains the chosen route using Slice A's
  `RejectionTrace` — the last clause of the identity ("you can see why") made real.
- **Approval cluster (fixes L6 + fatigue).** Explicit `y`/`n`/`Esc` + a visible legend; prefix-rule
  approvals (y / **always-this-session** / no); Esc-to-interrupt; a live working heartbeat.
- **OSC 9;4 Windows taskbar semáforo.** Yellow taskbar icon = "waiting on you" — visceral, Windows-first,
  zero deps.
- **"Errors that teach" — a tested invariant.** Every human-facing error names the cause + the next
  action (the Slice-D OAuth line is the template).

### C tests (E3)
- HUD/savings snapshot from a mock turn matches the expected currency line (deterministic price × tokens).
- `/why` prints the chosen route + at least one rejection reason from the trace.
- Approval reducer: `y`→approve, `n`/`Esc`→deny, any OTHER key → **no-op** (not a silent deny); the legend
  string is present. Heartbeat updates on a tick.

---

## Slice D — LEVER: savings selectable (E4) — the owner's ask

**Crates:** `nh-routes` (profiles + policy), `nh-core` (policy application), `nh-tui` (`/profile` + chip).

- **`profiles.toml`** (frugal / balanced / max-quality) layered like `law.toml`: bundled → user → repo.
  A **repo** profile may only *tighten* spend, never loosen (like law).
- **One `EffectiveExecutionPolicy`** clamps every profile wish to route capability: route ceiling, output
  cap (sets Slice A's mechanism), thinking tier, off-peak preference, compaction aggressiveness. Route-
  **required** behavior (e.g. Kimi reasoning replay) and law stay immutable — a profile can never weaken them.
- **`/profile` toggle + HUD chip + receipt field.** The active profile is visible and recorded.
- **The single owner of every user-selectable cost lever** — no cost knob lives anywhere else.

### D tests (E4)
- `frugal` vs `max-quality` produce different built-body output caps / route selections on the same task.
- A repo `profiles.toml` attempting to RAISE a cap above the user/law ceiling is clamped down (tighten-only).
- The receipt carries the effective profile; the HUD chip reflects it.

---

## Slice E — LOOP HARDENING: the build loop is durable + gated (E5)

**No runtime crate.** Repo tooling only — congruent with the meta-process (the M4 finale nearly lived
only in Temp).

- **`wip/<slice>` commit rule.** Gated-but-unapproved slice work is committed to a `wip/<slice>` branch
  before the owner FEEL-review — durability, no more one-AV-quarantine-from-loss.
- **`gate.ps1`** — mechanizes the §0.1 frozen-surface / allowed-files check (a diff touching a
  non-enumerated seam FAILS the gate); runs `cargo test --workspace --release` + clippy `-D warnings` +
  (optional) `cargo public-api` additive proof.
- **Minimal keyless CI** — one GitHub Actions workflow on windows-latest + ubuntu running the full mock
  suite (no keys, no network). Green = mergeable.
- **`codex exec --output-schema`** — Sol handoffs return a machine-readable self-report (files touched,
  tests added, gate result) for a deterministic post-run check.
- **`cargo-nextest` + AV canary preflight** — classifies a Kaspersky-blocked `nh.exe` as
  `EnvironmentBlocked`/`FLAKY`, not `FAIL` (turns env noise into a signal, not a red gate).
- **`[workspace.lints]` + pinned `rust-toolchain.toml`** — reproducible builds, lint policy as data.
- **Supply-chain gate** — `deny.toml` + `cargo-audit`/`cargo-deny` (crates.io is under active attack,
  RUSTSEC-2026-0155).

### E tests (E5)
- `gate.ps1` unit: a simulated diff touching `nh-fleet` (frozen) → non-zero exit; a diff touching only
  `nh-core::apply_thinking` (enumerated) → zero exit.
- CI dry-run green on both OSes with the mock suite; nextest AV-canary classifies a forced os-error-5 as
  `EnvironmentBlocked`.

---

## 6. Slice order + gating
1. **Slice A** (E1 — truth-math + resolver) — the foundation; every other slice reads its honest numbers
   / trace. Gate on the built-body + PrefixSeal + `resolve_capable` tests. **Sol may split A into ≤3
   handoffs.**
2. **Slice B** (E2 — floor) — independent of A; adversarial review per item. Can follow A directly.
3. **Slice C** (E3 — visible) — needs A (cost helper + `RejectionTrace`). **The FEEL gate; owner approves
   before commit.**
4. **Slice D** (E4 — lever) — needs A (output-cap mechanism) + C (HUD chip surface).
5. **Slice E** (E5 — loop) — can land anytime; ideally *before* A so the gate mechanizes the rest.
- Gate EACH slice: `cargo test --workspace --release` (≥292 pass, 0 fail) + `cargo clippy --workspace
  --all-targets -- -D warnings` clean; adversarial review; **owner FEEL-approve** any human-facing
  surface; then commit per-slice (via `wip/<slice>` → `main`). Kill any `nh.exe` before builds (it locks
  `target\debug\nh.exe`).

## 7. Verify-live ledger (M5) — confirm before building on (carried from report §8)
- **DeepSeek peak/off-peak windows** — still not first-party as of 2026-07-16; re-check at catalog
  `valid_until=2026-07-24`; the catalog peak block is `verify_live`, not `confirmed`, until then.
- **DeepSeek thinking** — confirm omission = thinking-on; `low`→`high` normalization; Anthropic-wire
  `output_config.effort` + thinking-block replay. (Gates A.1/A.2 wire assertions vs a real key later.)
- **Kimi** — K2.6 thinking-toggle + mandatory `reasoning_content` in tools; `kimi-toggle` dialect shape.
  (A.1/A.2 are mock-tested; real Kimi key is live-pending.)
- **Cache accounting** — assert `cached_tokens>0` on a repeated-prefix second call, per provider; exact
  fields for the savings-line denominator. (A.2 mock-tested; live-pending.)
- **MCP 2026-07-28 final** — reconcile the assumed wire; nh-mcp stays local-only until then regardless.
- **GLM / jurisdiction / learning** — OUT of M5 (M6/M7). No key acquired this milestone.

## 8. Integration amendments (append here, dated, orchestrator authority)

**A-M5-1 (2026-07-17, orchestrator Opus 4.8) — Slice A catalog-schema seams in `nh-routes`.**
Writing the Slice A brief surfaced that L1's `kimi-toggle` dialect and L2's state-aware reasoning replay
both need catalog schema that the §0.1 `nh-routes` table did not enumerate (it listed only
`resolve_capable`, `naive_cost`, `Profiles`). The §0.1 "Data (always allowed)" note already anticipates
these catalog *flags*; this amendment makes the *parsing code* explicit so Sol never faces a
break-scope-or-duplicate choice (the A-M4-1 lesson). Both seams are **[+] additive / source-compatible**
(new enum variant + new `#[serde(default)]` field — no existing signature changes, no behavior change
for routes that don't set them):

| Crate | Seam | Ref | Tag | Change |
|---|---|---|:--:|---|
| `nh-routes` | `ThinkingDialect::KimiToggle` | enum ~28-38, `as_str` ~41, `from_toml` validate ~480 | + | new variant carrying K2.6's thinking toggle; `from_toml` parses the catalog string `"kimi-toggle"`; `as_str` round-trips it. Existing variants + their wire behavior unchanged. |
| `nh-routes` | `preserve_when_thinking` field | `RawRoute` ~274-297, `ResolvedRoute` ~144-165 | + | `#[serde(default)] bool` on both structs; carried onto the resolved route so nh-core can gate L2 reasoning replay on the *effective* thinking state. Default `false` = today's static behavior. |

Consumers stay inside the already-enumerated nh-core seams (`apply_thinking`, `reasoning_to_send` /
`OpenAiPolicy`). Catalog data edits (set `kimi-k2.6` → `thinking_dialect="kimi-toggle"`,
`preserve_when_thinking=true`) are already "always allowed" (§0.1 Data). No other `nh-routes` line opens.

**A-M5-2 (2026-07-17, orchestrator Opus 4.8) — compile-compat ripple of the KimiToggle variant into two
frozen crates.** A-M5-1 under-scoped: adding a public enum variant forces every *exhaustive* `match
ThinkingDialect` in the workspace to gain an arm, or the crate fails to compile (E0004). Sol correctly
STOPPED at the frozen boundary instead of editing these; the full-workspace build fails in exactly three
identical spots (Sol's self-report named two; the orchestrator's full-workspace gate surfaced the third,
`nh-cli`) until they gain the arm:
| Crate | Seam | Ref | Tag | Change |
|---|---|---|:--:|---|
| `nh-fleet` | `effort_for(dialect)` match | lib.rs ~1478-1482 | Δ | add `KimiToggle` to the existing `DeepseekNhm \| None => ThinkingEffort::None` arm. |
| `nh-tui` | `effort_for(dialect)` match | lib.rs ~1198-1203 | Δ | same one-token addition. |
| `nh-cli` | `effort_for(_, dialect)` match | cmd_run.rs ~55-58 | Δ | same one-token addition (default-effort branch). |
All three are **behavior-preserving compile-compat glue**, not design: a toggle model (K2.6) defaults to
*no-thinking*, identical to `DeepseekNhm`/`None` — congruent with Slice A ("never silently buy
thinking"). This is the *only* line each frozen crate gains; `nh-fleet`'s escalation ladder and
`nh-tui`'s render logic are otherwise untouched, and everything else in both crates **stays frozen**.
Applied by the orchestrator as gating/integration glue (Sol owns all substantive Slice A logic).

**A-M5-3 (2026-07-17, orchestrator Opus 4.8) — `build_anthropic_body` consecutive-user merge (fixes an
L7 regression on the Anthropic wire).** Adversarial review + an added regression test empirically proved
that the L7 fix (elision note inserted as a separate message) makes `build_anthropic_body` emit **two
consecutive `user` messages** after a compaction: the second system note degrades to a user block (the
`_ =>` arm) and lands immediately before the first retained user turn — output `["user","user",...]`,
which the Anthropic Messages API rejects ("roles must alternate"). Reachable on the `deepseek-*-anthropic`
routes (1M window, now compacting earlier under the 256K `effective_context` cap).
| Crate | Seam | Ref | Tag | Change |
|---|---|---|:--:|---|
| `nh-core` | `build_anthropic_body` message-assembly loop | lib.rs ~515-585 | Δ | merge **consecutive same-role `user` messages** into one (content-block concat — generalizing the existing consecutive-`tool` merge). |
The §0.1 nh-core row opened `build_anthropic_body` for `max_tokens` only; this opens its message loop for
the merge. L7/cache-safety is PRESERVED — the note stays a separate message and retained real messages
stay byte-identical; only the transient Anthropic wire body coalesces. Pinned by the orchestrator-authored
test `anthropic_body_roles_alternate_after_compaction` (already in the tree, currently failing = the
proof). This is Slice A follow-up handoff #2 (the contract pre-authorized Sol to split Slice A into ≤3).

**A-M5-4 (2026-07-18, orchestrator Opus 4.8) — `Access::{Read,Send}` variant ripple into the four
policy-backed guard closures (Slice B L3).** §0.1 opens the nh-tools `Access` enum to add `Read(&str)` /
`Send(&str)` **[+]** and `ReadFile::execute` to consult `Access::Read` **[Δ]**. Adding two variants to the
public enum forces every *exhaustive* `match Access` in the workspace to gain arms or fail to compile
(E0004) — the A-M5-2 pattern. The non-wildcard matches live in four crates the §0.1 nh-tools row does not
enumerate. Unlike A-M5-2 these arms are **behavior-adding** (they wire the new read/send verdicts live),
which is the point: the read guard must bite in every real path, most of all the unattended fleet path.
| Crate | Seam | Ref | Tag | Change |
|---|---|---|:--:|---|
| `nh-tui` | `verdict_to_guard` guard closure | lib.rs ~1012-1014 | Δ | add `Access::Read(p) => verdict_to_guard(policy.read_verdict(p))` + `Access::Send(t) => verdict_to_guard(policy.send_verdict(t))`. |
| `nh-fleet` | guard closure (**FROZEN** crate) | lib.rs ~1218-1220 | Δ | same two arms — closes the read leg on the autonomous fleet path. The only lines nh-fleet gains; ladder/workers stay frozen. |
| `nh-cli` | `guard_from` closure (`nh run`) | cmd_run.rs ~105-108 | Δ | same two arms via `guard_from`. |
| `nh-cli` | `guard_from` closure (`nh chat`) | cmd_chat.rs ~107-109 | Δ | same two arms via `guard_from`. |
| `nh-tools` | default guard (`ToolCtx::new`) | lib.rs 55-58 | Δ | `Access::Read(_) => Guard::Allow`, `Access::Send(_) => Guard::Allow` — preserves M0/M1 no-law behavior (reads allowed when no policy is installed; the bundled `[read]` block bites only through the policy-backed closures above). |
Everything else in nh-fleet and nh-tui stays frozen; this is the only line each gains (mirrors A-M5-2).
Applied by the orchestrator as integration glue if Sol stops at the frozen boundary.

**A-M5-5 (2026-07-18, orchestrator Opus 4.8) — L4 credential-audience broker: trusted-law source +
call-site ripple + `[read]`/`[send]`/`[credential]` law schema.** §0.1 nh-vault lists only the new
`get_scoped` broker; it does not enumerate the *trusted* source of an entry's approved audience nor the
callers, and closing the redirect hole (`find_catalog` walks up to a repo-controlled `catalog.toml`;
`.nosis/mcp.toml` is repo-controlled) requires opening key-materialization choke points outside the
enumerated surface. **Owner-ratified 2026-07-18:** the trusted source is **law** (bundled/user; repo
cannot add — reuses the `repo_tries_to_weaken` / `compile_policy` layering, nh-law:196/297, exactly as
`write.auto` is repo-refused). Pre-authorized surface:
| Crate | Seam | Ref | Tag | Change |
|---|---|---|:--:|---|
| `nh-law` | `[read]` / `[send]` / `[credential]` sections + `ReadRules`/`SendRules`/`CredentialRules` | LawFile 440-446, compile_policy 297-327 | + | data-only additive sections. `[credential.<entry>] audience=[hosts]` layered **bundled→user only** (repo entries dropped by the weaken-guard, like `write.auto`). Bundled defaults for the four shipped provider entries (deepseek→api.deepseek.com, kimi→api.moonshot.ai, mimo→api.xiaomimimo.com, glm→api.z.ai — from catalog.toml). |
| `nh-law` | `read_verdict` / `send_verdict` / `approved_audiences(entry)` | mirror `write_verdict` 88-104 | + | two two-tier verdicts (Block/Allow, no Ask); a pure accessor returning an entry's approved hosts. |
| `nh-vault` | `audience_allows(host, approved)` + `get_scoped(entry, requested_host, approved)` | new, alongside `Vault` | + | pure host-compare helper (normalize both sides to host) + broker that **refuses before the secret materializes** when `approved` is non-empty and the host ∉ it; empty (undeclared) → returns as today. No nh-law dep — caller passes `approved`. |
| `nh-cli` | primary-key materialization | cmd_run.rs 90, cmd_chat.rs 69 | Δ | `vault.get(&route.vault_entry)` → `get_scoped(entry, host_of(route.base_url), law.approved_audiences(entry))`; refusal is one friendly, secret-free line. |
| `nh-cli` | MCP config-load audience gate | cmd_chat.rs `load_mcp` ~364, cmd_tui.rs `load_mcp_palette` ~58 | Δ | before building each `McpClient`, drop+warn any server whose `url` host (and, for oauth2, `token_url` host) is not in `law.approved_audiences(vault_entry)` — validates the pairing **without materializing the secret** (fetch is lazy in `request_headers`). |
| `nh-tools` | OAuth `resource` (RFC 8707) | mcp.rs `refresh_oauth` form 446-454 | + | `form.push(("resource", config.url.trim_end_matches('/')))` — the only nh-tools/mcp.rs L4 change (the audience gate is enforced at nh-cli load, so the MCP client needs no policy threading). |
`Access::Send` ships as the **[send] mechanism** (variant + `send_verdict` + `[send]` law + the A-M5-4
guard arms + a unit test that a `[send]`-blocked host → `Block`); the concrete "a secret cannot egress to
a non-approved host" enforcement is `get_scoped` + the MCP load gate. The **broad** egress consult site is
M6's privacy-router (§0 ruling 3b) — M5 wires no live `Access::Send` producer into the MCP adapter
(mcp.rs:621 keeps its deliberate no-`ctx.guard` stance). Host comparison is **host-only** (scheme/path/
port stripped) so DeepSeek's dual wires — `api.deepseek.com/v1` (OpenAI) and `api.deepseek.com/anthropic`
(Anthropic, nh-core:506) — both satisfy one audience entry; pinned by a test.

**A-M5-6 (2026-07-18, orchestrator Opus 4.8; owner-ratified) — Slice C approximate USD gloss + `[fx]`
catalog data.** The open-weight routes mix currencies (DeepSeek/most = CNY; `kimi-k2.7-code` = USD,
catalog:162). A Western user has no gut feel for `¥`, so the owner ratified adding an **approximate USD
gloss** (`¥0.11 (≈$0.02)`) to the money surfaces. This is additive to the already-open nh-routes
"`naive_cost` / cost helper (new)" row (§0.1) and the "Data (always allowed): catalog.toml" note; it is
logged here for auditability because it introduces a new data block + type. **The meter-must-not-lie
invariant governs it:** CNY stays the billed source of truth; USD is `≈`-marked, **never** exact;
sessions **never** FX-sum across currencies (per-currency subtotals only — the gloss is display, never a
summation basis); a stale/absent rate → the gloss is **omitted**, never guessed.
| Crate | Seam | Ref | Tag | Change |
|---|---|---|:--:|---|
| `catalog.toml` | `[fx]` block | data | + | `usd_per_cny` + `valid_until` + `price_confidence` — reuses the price honesty machinery (`valid_until`/`confidence`/stale). A catalog with no `[fx]` still parses; gloss omitted. |
| `nh-routes` | `Fx` type + `RouteResolver::fx()` + `to_usd_approx` + `money`/`money_with_gloss` | near `price_at` ~181 | + | pure, deterministic (cache-safe); `to_usd_approx` returns `Some` only for a fresh CNY→USD (never for USD-native or a stale/absent rate). |
Ratified FEEL/format calls (not surface — recorded so the FEEL gate has a fixed target): savings
headline baseline = **no-cache** (same model, zero cache — the honest "our caching saved you N%";
top-tier/peak are breakdown context, never the headline); dual-currency = native primary + `≈$` gloss
on the paid number (per-turn headline, HUD session total, `/why`), naive breakdown native-only. `/profile`
+ the profile HUD chip stay **Slice D** (they need the not-yet-built `Profiles` module) and are OUT of C.

**A-M5-7 (2026-07-18, orchestrator Opus 4.8; owner-ratified) — Slice D receipt-profile field +
`AgentLoop.profile` + the AgentLoop-literal ripple into frozen nh-fleet.** §0.1's nh-core row opens
"`EffectiveExecutionPolicy` application | request build" for the output-cap + thinking clamp, but Slice
D's E4 also requires "the receipt carries the effective profile" (D tests) — a seam in the `Receipt`
struct + the receipt-recording path, NOT request-build. Recording the active profile needs (a) a new
`Receipt.effective_profile` field and (b) `AgentLoop` to carry the profile name so `make_receipt` can
write it. Adding a public field to `AgentLoop` forces every *exhaustive* struct-literal site to add it or
fail to compile (E0063) — the A-M5-2/-4 pattern. The production sites in the D-open crates set the real
profile name; the **frozen** nh-fleet site + the nh-core test literals get the behavior-preserving `None`.
**Owner-ratified design calls (2026-07-18):** profiles clamp **thinking tier + output cap** on the
user-chosen route only (route *selection* by profile → M6 auto-router); frugal = route thinking floor +
output cap ≤16 384 + off-peak preferred; balanced = today's behavior exactly; max-quality = route thinking
ceiling + cap = route.max_out. **No currency session hard-stop in D** (held to a separate lever / M6).
| Crate | Seam | Ref | Tag | Change |
|---|---|---|:--:|---|
| `nh-core` | `Receipt.effective_profile` | lib.rs `Receipt` 1313-1325 | + | `#[serde(default, skip_serializing_if = "Option::is_none")] pub effective_profile: Option<String>`. Additive; pre-D `receipts.jsonl` still parse (`default`). |
| `nh-core` | `AgentLoop.profile` + `make_receipt` | lib.rs 1403-1422, `make_receipt` 1613-1632 | + | new `pub profile: Option<String>` field; `make_receipt` copies `self.profile.clone()` onto every receipt. `None` = today's behavior (no profile line). |
| `nh-fleet` | `AgentLoop { … }` literal (**FROZEN** crate) | lib.rs 1227 | Δ | add `profile: None` — the only line nh-fleet gains (ladder/workers stay frozen); fleet runs are profile-agnostic in M5. Compile-compat glue, mirrors A-M5-2/-4. |
| `nh-core` | test `AgentLoop` literals | tests/agent_loop.rs:99, tests/context_engine.rs:118 | Δ | add `profile: None` (test-only compile glue). |
Applied by the orchestrator as integration glue if Sol stops at the frozen boundary; Sol owns all
substantive Slice D logic (the nh-routes `profiles` module + `EffectiveExecutionPolicy`, the thinking +
output-cap clamps, and the `/profile` / `--profile` / `nh profile` surfaces + HUD chip). The nh-cli
(`cmd_run.rs:128`, `cmd_chat.rs:120`/`:668`) and nh-tui (`lib.rs:1214`) `AgentLoop` sites are inside
D-open crates and set the real profile name — no amendment needed for them. **Application stays at the
caller** (clamp a `ResolvedRoute` clone's `max_out` → the existing `make_client`; set the live
`AgentLoop.thinking` field) so `AgentLoop` stays policy-free and `make_client`'s signature (5 callers incl.
frozen nh-fleet:1203) is untouched — the §0.1 nh-core "application" row is satisfied in spirit (the
clamped values ARE applied at build time) without opening a new nh-core function.

**A-M5-7 addendum (2026-07-18, orchestrator Opus 4.8) — the `Receipt`-literal ripple (Sol correctly
stopped here on the first handoff).** Adding the public `Receipt.effective_profile` field forces every
*exhaustive* `Receipt { … }` literal in the workspace to gain `effective_profile: None`, exactly as the
`AgentLoop` field forces the `profile:` glue (E0063). A-M5-7's tables enumerated the `AgentLoop` literals
but not the `Receipt` literals; a full-workspace grep finds the complete set — the real constructor
`nh-core::make_receipt` (lib.rs:1622) sets the live value, and the remaining four literals (two in FROZEN
nh-fleet's test file, two in nh-tui) take the behavior-preserving `None`:
| Crate | Seam | Ref | Tag | Change |
|---|---|---|:--:|---|
| `nh-fleet` | `MockSwarm::submit_and_collect` Receipt literal (**FROZEN** test) | tests/slice_b.rs:405 | Δ | add `effective_profile: None` — test-only compile glue; the swarm mock is profile-agnostic. |
| `nh-fleet` | `failed_receipt` helper (**FROZEN** test) | tests/slice_b.rs:490 | Δ | add `effective_profile: None`. |
| `nh-tui` | `failed_timeline_summary` synthetic Receipt | lib.rs:1373 | Δ | add `effective_profile: None` — a connection-failure placeholder, no active turn/profile. |
| `nh-tui` | test `receipt` helper | lib.rs:3067 | Δ | add `effective_profile: None`. |
**Blanket (prevents a further stop-and-wait on pure glue):** any *additional* exhaustive-literal site that
the `Receipt.effective_profile` or `AgentLoop.profile` field additions force to compile is pre-authorized
as behavior-preserving glue — Sol **applies** the trivial `effective_profile: None` / `profile: None`
(or the real active-profile value at a live caller site) and **reports** it in the self-report, rather
than stopping. This blanket covers ONLY the two field-addition ripples of A-M5-7; every other frozen line
still requires an amendment.

**A-M5-9 (2026-07-19, orchestrator Opus 4.8; owner-ratified) — Slice F W3 wire-aware `resolve_effort`
(medium-2 fix).** The two anthropic-wire deepseek routes (`deepseek-v4-pro-anthropic`,
`deepseek-v4-flash-anthropic`; `wire="anthropic"`, `thinking_dialect="deepseek-nhm"`) displayed a resolved
thinking tier (High under Ceiling/explicit) that `build_anthropic_body` never sends — a meter-honesty lie in
the display. The honest fix makes `nh_core::wire::resolve_effort` **wire-aware**: it gains a trailing
`wire: nh_routes::Wire` parameter and returns `ThinkingEffort::None` for `Wire::AnthropicMessages` regardless
of posture/explicit (None is the only tier that matches a wire sending no thinking directive); `Wire::OpenAi`
behavior is byte-for-byte unchanged. This is a **public type-signature change** (source-compat break) to a
`pub fn`, so it forces its call-sites to update — the A-M5-2 ripple pattern, but a signature widen rather than
an enum arm. Authorized call-site glue (both non-frozen, route/wire already in scope, no logic change):

| Crate | Seam | Ref | Tag | Change |
|---|---|---|:--:|---|
| `nh-cli` | `effort_for` call to `resolve_effort` | cmd_run.rs:64 | Δ | pass `route.wire` as the new trailing arg. |
| `nh-tui` | posture→effort call to `resolve_effort` | lib.rs:1456 | Δ | pass `route.wire` as the new trailing arg. |

**nh-fleet does NOT call `resolve_effort`** (verified by workspace grep — it uses `effort_for`/`SetEffort`),
so the FROZEN crate takes NO ripple from this signature change. The real Anthropic thinking-wire mapping stays
deferred to a live-verify (report §7); until then None (provider-default) is the honest displayed tier. The
matching display-honesty fix for the TUI `/effort low` direct-set path (medium-3's TUI caveat) is a separate
W4 concern, not covered here.

**A-M5-9 glue extension (2026-07-19, orchestrator Opus 4.8; owner-approved during the W3 gate).** After W3
implementation + gate PASS, the wire-aware glue — ratified above as `nh run` (cmd_run) + TUI only — was found
to leave two more effort-resolving surfaces wire-blind: `nh chat` (cmd_chat.rs:169/371 + a test fixture) and
`nh profile` (cmd_profile.rs:31). Owner ratified extending the glue to both so every surface reports the
effort identically. **Display-only:** `build_anthropic_body` never serializes the effort (nh-core lib.rs
~519; `apply_thinking` runs only in the OpenAI `build_body`), so the wire bytes are unchanged — this corrects
only the reported tier on those two surfaces. Implementation folded `cmd_run::effort_for` to take the trailing
`Wire` and dropped the transient `effort_for_wire` wrapper (mirrors nh-tui's `effort_for`, leaves no
test-only helper → no `clippy -D` dead-code trap). Delivered as the W3b addendum (Sol, xhigh;
`Temp/slice-f-w3b-brief-v1.txt`; self-report `Temp/w3b-last-message.txt`). Committed with W3 in `2b68163`.
