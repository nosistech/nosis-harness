# Continue Here — authoritative checkpoint

**Updated:** 2026-07-29 America/Guatemala
**Resume phrase:** `continue`
**Works for:** Codex or Claude in `C:\Users\capv2\Desktop\nosis-Harness`

This file supersedes every older checkpoint and "next task" in `CURRENT_TASK.md`. Historical records
remain useful for provenance, but do not execute their stale instructions.

## THE HEADLINE: FEEL PASSED and the release candidate is COMMITTED (2026-07-29)

The owner passed the Windows FEEL gate against the rebuilt binary — the last subjective blocker. Six
findings came out of the pass; all six are captured in wave 4 (below) and none blocked the verdict.

The owner then authorized the commit. **`cba2444 feat: complete the v0.1.0 release candidate` —
90 files, +6985/−3980. The working tree is CLEAN.** Everything that had accumulated uncommitted
(Slice G, the responsibility refactor, and three waves) is now in git history.

**NOTHING HAS BEEN PUSHED.** The remote is still an empty public repository, so there is no
`origin/main` ref yet (`git log origin/main..HEAD` fails — that is expected, not a fault).

**The next action is to ask the owner whether to push.** Pushing is outward-facing and makes the
source public at `github.com/nosistech/nosis-harness`. `continue` alone is NOT push authorization —
the owner must say so explicitly.

Pre-commit guards that were run and passed, so they need not be repeated for `cba2444`: a six-pattern
secret-shape scan over the staged diff (`sk-`, Bearer+token, JWT, 40+ hex, AWS key, private-key
block) returned **0 matches**; no `.nosis/`, `target/`, log, `.env`, or stray artifact was staged;
and the gate was green at commit time. Two modules listed in the previous checkpoint
(`nh-routes/src/profiles/compile.rs`, `nh-tui/src/worker/session.rs`) were **consolidated away by the
waves, not lost** — neither exists on disk, neither is ignored, and the gate passes.

## First action on `continue`

Read this file in full. Then verify with read-only commands:

```powershell
git status --short          # expect CLEAN (zero entries)
git log -1 --oneline        # expect cba2444
git remote -v               # expect https://github.com/nosistech/nosis-harness.git
gh auth status -h github.com
gh repo view nosistech/nosis-harness --json isEmpty,visibility   # expect isEmpty=true until pushed
Get-Process -Name nh,codex -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,StartTime,Path
```

If `git status` is dirty, someone has worked since this checkpoint — read the diff before acting.

Do not restart the audit, redo the refactor, create another repository, or re-run any completed wave.

## Hard constraints

- Preserve the entire uncommitted tree.
- **Do not commit, push, or tag until the owner explicitly authorizes it.** FEEL passing does not
  authorize the commit; only an explicit owner "yes" does.
- **Do not run write-mode `cargo fmt`.** `cargo fmt --all --check` is allowed and is a gate step.
- Do not expose MCP publicly. Loopback-only, bearer-guarded, preview.
- Do not delete or truncate `.nosis/` receipts or Fleet state.
- Do not add dependencies or broaden scope to continue the release.
- Never print, persist, or upload credentials. GitHub auth lives in the OS keyring.
- **Never run two nosis codexes.** The owner runs his own (e.g. PIDs 30868, 5160).
- Roles: **Sol max = executor** (all code, one wave at a time, via `codex exec`), **Opus 5 =
  orchestrator** (briefs, gates, review, commits; writes no feature code), **Fable 5 = research**.
- Follow root `AGENTS.md`, `05-ai-collaboration/AGENTS.md`, and THE LAW.

## Exact state

- Branch `main`, HEAD **`cba2444 feat: complete the v0.1.0 release candidate`** (parent `0056a07`)
- **Working tree CLEAN — zero uncommitted entries.**
- Remote `https://github.com/nosistech/nosis-harness.git` — **public, still empty, never pushed**.
  No `origin/main` ref exists yet.
- Committed on `main` deliberately: branch protection cannot be configured until `main` exists on the
  remote, so the release plan requires main. Do not retroactively rewrite this onto a branch.
- **Gate: `GATE: PASS` — 546 passed / 0 failed / 1 ignored `--release`**; clippy `-D warnings` clean;
  `fmt --check` clean; `cargo deny --locked check` green (advisories/bans/licenses/sources), nothing
  suppressed. Baseline was 514 — 32 tests added this session.
- Binary `target\release\nh.exe` is current with the source as of the wave-3 gate.

## What shipped this session (2026-07-28 → 29), all gated

Three Sol waves, each verified by the orchestrator running `.\gate.ps1` independently.

**Wave 1 — FEEL fixes, 8/8** (`GATE: PASS` 529/0/1)
- F-1 free GLM routes got `context` + `max_out` with dated z.ai sources — they are now selectable
  candidates instead of being skipped as "unknown context". **`context` and `max_out` had to land
  together**: `glm-4.6v-flash` has a 32K output cap and the client's `DEFAULT_MAX_TOKENS` is 65,536.
- F-2 bare `/model`, `/provider`, `/profile` open pickers (arrows/Enter/Esc). `/provider` lists only
  providers with usable credentials, checked before terminal takeover.
- F-3 `TOOL <name> · <elapsed>s` indicator — **tool calls only, not model turns** (see W4-7).
- F-4 tool-result authority rule in `agent.rs:74`, shared by `nh run`, `nh chat`, TUI.
- F-5 `mcp serve --help` names all six tools. F-6 `--max-turns` range 1–100 with readable refusal.
- F-7 `approve_on_stdin` checks `IsTerminal` — piped input cannot approve a shell command.
- F-8 `nh run` metering to stderr; stdout is answer-only.

**Wave 2 — provider truth, 7/8** (`GATE: PASS` 538/0/1)
- All three reasoning-dishonesty bugs closed: MiMo now sends an explicit thinking toggle; DeepSeek
  preserves reasoning while thinking and disables default thinking on its Anthropic wire; GLM
  disables thinking at `none` and sends truthful normalized High/Max effort.
- K2.6 sends `thinking.keep = "all"`. `kimi-k3` added (1,048,576 ctx, $0.30/$3.00/$15.00,
  `price_confidence = confirmed`, `valid_until = 2026-08-02`). GLM finish reasons classified.
- **P-4 deliberately NOT implemented** — the Kimi cache-hit live probe was not run. Kimi documents
  top-level `usage.cached_tokens`; `WireUsage` parses only two other names. If Kimi uses only the
  documented shape, every Kimi input token is metered at cache-miss (~5× overstatement). **Do not
  implement this from documentation alone — it is only correct if the field is actually populated.**

**Wave 3 — local lane, 5/5** (`GATE: PASS` 546/0/1)
- `class = "local"`: selectable via `--model`/`/model`; excluded from `resolve_capable`,
  cheapest-capable, provider defaults, automatic escalation, and the `top_tier` anchor. Enforced to
  the OpenAI wire and a loopback origin.
- Meter copy: `Local: no billed tokens; hardware and power are not metered.`
- L-1 `#[serde(alias = "reasoning")]` on `WireMessage.reasoning_content` — Ollama's field name. **The
  only wire-client change the local lane needed.**
- L-2 commented llama.cpp/Ollama catalog templates; `model_id`/`context`/`max_out` left as
  user-filled placeholders per the catalog's "never guessed" rule.
- L-3/L-4/L-5 `06-operations/LOCAL_MODELS.md` — verification procedure, licence traps, sizing, and
  **the Ollama silent-truncation hazard**; llama.cpp documented as the fail-closed reference path.

## Live verifications performed (real API calls, ~$0.0002 total)

- **F-8 confirmed**: `nh run "say hi" --model deepseek-v4-flash > out.txt` → `out.txt` contained
  exactly one line, the answer. All progress/turn/cost lines went to stderr.
- **F-7 confirmed**: same run produced `approval refused: stdin is not a terminal; piped input cannot
  approve shell commands` on a real exec attempt.
- **F-4 confirmed**: the model reported the refusal honestly instead of claiming success.
- **Identity correct in a fresh session**: "I'm nosis on deepseek-v4-flash".
- **GLM free tier returned HTTP 429** on the first attempt — free-tier limits are real and
  unpublished. This also confirmed live that there are **zero retries anywhere**: one 429 killed the run.

## QUEUED WORK — briefs written, in `<scratchpad>\`

Scratchpad root:
`C:\Users\capv2\AppData\Local\Temp\claude\C--Users-capv2-Desktop-nosis-Harness\a5323c10-de58-40bd-9eff-f92a1441ca56\scratchpad`

**`wave3b-drop-anthropic-routes-brief.md` — AUTHORIZED, NOT LAUNCHED.** Owner said "if we don't need
them, remove them" (2026-07-29). Removes `deepseek-v4-flash-anthropic` and
`deepseek-v4-pro-anthropic` from `catalog.toml`. **Retains `wire/anthropic.rs` and the `Wire` enum
unchanged** — deleting the client is a separate post-1.0 decision. Was not launched only because the
owner was about to `/clear`; a Sol run would have been orphaned. **Launch this when work resumes**,
then gate, then rebuild.

**`wave4-repair-brief.md` — 7 items, PREPARED, NOT AUTHORIZED.** Do not launch before v0.1.0 ships
unless the owner directs otherwise. W4-1 tolerant edit-match cascade + nearest-match failure message;
W4-2 malformed tool-call repair cascade + name aliases through the same guard path; W4-3 surface
cache-hit %; W4-4 local runtime flags doc; W4-5 route-switch note in history; W4-6 absolute price
display; W4-7 model-turn wait indicator. It carries an evidence-backed **DO NOT** list — no
grammar-constrained decoding, no temperature lowering, no udiff/whole-file edit format, no new tools,
no LLM-based repair.

**`provider-truth-brief.md`** — reference for the deferred P-4.

## The six FEEL findings (all in wave 4, none blocked the pass)

1. **Model confabulated its identity after a mid-conversation route switch.** Status line was correct;
   `worker.rs:382-384` correctly rebuilds the constitution and replaces the system message. **The
   harness is right — do not "fix" the switch path.** Proof: `identity_constitution` derives id and
   provider from one route, and `glm-4.7-flash`+`mimo` is not a valid pairing, so the harness cannot
   emit it. The model blended its own stale self-description from preserved history. Gap = the model
   gets no in-context signal that the route changed → W4-5.
2. **`/why` and picker rows collapsed to bare "higher price".** `resolver.rs:422-425` cannot compute a
   ratio when the chosen route costs `0.0`, and after F-1 the cheapest is usually free. F-1 silently
   destroyed the price ladder's information content → W4-6.
3. **Model turns are an unindicated blank screen.** `wire/openai.rs:35` — no streaming. F-3 covered
   tool calls only → W4-7.
4. Four DeepSeek picker rows where two would do → wave 3b.
5. FEEL script errors (mine): A2 claimed "reply streams in" (it does not); B3 was written as one
   malformed line. Script at `C:\Users\capv2\AppData\Local\Temp\feel-gate.ps1` is corrected.
6. `mcp serve` appearing "stuck" is correct behaviour — it is a server; Ctrl+C exits.

## Remaining path to v0.1.0

1. **Ask the owner whether to push `cba2444`.** The commit is done; the push is the open gate.
   Pushing is what finally runs CI and gives the first honest Linux/macOS answer — expect a real
   failure, which is better found on an untagged commit than after a tag.
2. Launch wave 3b → gate → rebuild → second commit.
3. **Live probes still owed** (sub-cent each): Kimi cache-hit field name (unblocks P-4); MiMo
   cached-tokens field name and whether it accepts `max_tokens` or `max_completion_tokens`; **K3
   `max_out`** — Moonshot documents `max_completion_tokens` default 131072, max 1048576, and the
   catalog declares 1048576, but `wire/openai.rs:117` sends it on every request and prompt + 1M may
   exceed the 1M context. Consider 131072. (The DeepSeek Anthropic-downgrade probe becomes moot once
   wave 3b lands.)
4. Inspect the full staged diff; repeat the secret guard without printing candidate values; confirm
   all new modules are staged and that `.nosis/`, `target/`, and stray artifacts are not.
5. **One coherent commit + push.** This is the real risk reduction — Slice G plus three waves exist
   only on this laptop.
6. Monitor GitHub Actions → **first honest Linux/macOS answer ever**. CI has never run (no remote
   history). Expect real work; better before the tag than after.
7. Protected `main` with the actual job contexts; verify direct pushes are blocked.
8. Recheck the MCP final spec (2026-07-28) before any public MCP statement. Its headline change —
   stateless request/response — already matches this design. **Skip the OAuth 2.0/OIDC work**: it
   targets enterprise identity, and this server is loopback + bearer by design. Extensions framework
   is optional. Stay loopback-preview through v0.1.0.
9. Confirm `info@nosistech.com` is monitored.
10. Only with explicit owner approval: tag `v0.1.0`, publish notes, verify the tag points at the
    tested commit.

## DEADLINE

**All 14 price blocks carry `valid_until = "2026-08-02"`.** After that the harness's own freshness
discipline flags every price as stale. Ship before then, or budget a day to re-verify 14 price blocks.

## Settled decisions — do not reopen

Recorded in full, with rejected alternatives and evidence, in `00-start-here/DECISION_LOG.md`
(2026-07-29 entries):

- **Provider scope is DeepSeek + Kimi + GLM + MiMo + local.** No Anthropic/OpenAI/Gemini API routes.
  Keep the commented `class = "delegate"` stubs at `catalog.toml:344-356`.
- **Never add a frontier price row as a `top_tier` anchor.** Verified at `resolver.rs:228-238` that
  one USD row inflates every savings line 3–6×. That is the fabricated-savings behaviour this product
  refuses.
- **Zero-price routes are a selectable tier, not a routing winner.**
- **The "router inside the harness" moat claim is retired.** The harness does not auto-route:
  `resolve_capable` is called only from `cmd_why.rs:51`, `input/commands.rs:190`,
  `route_tools.rs:101`. Positioning is the honest-meter bundle. Do-not-claim list is in the log.
- **Do not auto-route.** An advisory router that explains is more congruent with an auditability
  product than an automatic one that decides.
- **Do not add a quality axis to routing** — 0 of 104 small-model SWE-bench figures surveyed were
  independently verified.

## Chosen categories (positioning)

1. **Provable cost / auditability.** Still unbuilt and needed for the claim: receipts store only an
   ambiguous `model_id` and raw usage — **no `route_id`, no price snapshot** — so history silently
   reprices as the catalog changes. Plus `nh savings` from the counterfactuals already computed and
   discarded, and cache-aware comparison (`resolver.rs:359-361` prices all prompt tokens at
   `cache_miss`, blind to DeepSeek's ~120× hit/miss spread).
2. **Honest behaviour under failure.** Waves 1–3 are this.
3. **Getting the most out of cheap open-weight models.** Wave 4. Note the research finding: nosis is
   *already correct* on edit format (native tool-call search/replace) and tool minimalism (three flat
   tools). Both gaps are in the failure path. Also: **compaction never fires for single-turn runs**
   (`context.rs:112-114`, `nh run` has one user turn), and there are **zero retries anywhere**.

## Research corpus (this session) — `<scratchpad>\research\`

`01-ux-ui.md`, `02-engine.md`, `03-market.md`, `04-competitive.md`,
`05-providers-deepseek-mimo.md`, `06-providers-kimi-glm.md`, `07-providers-frontier.md`,
`08-local-models.md`, `09-local-runtimes.md`, `10-other-runtimes.md`, `11-huggingface.md`,
`12-hardware.md`, `13-competitor-patterns.md`, `14-weak-model-performance.md`.

Highlights worth not re-deriving:
- Every DeepSeek/MiMo/Kimi/GLM-5.2 price re-verified **exact** first-party 2026-07-28. **Do not
  "fix" them.** MiMo correctly has no off-peak block — its 0.8× discount is Token-Plan-exclusive on a
  different host; the 2026-07-17 F3 proposal is disproven and stays dead.
- Reference hardware: RTX 5070 Ti **Laptop** = 12 GB, 192-bit, **672 GB/s** (same bandwidth as the
  desktop RTX 5070; desktop 5070 Ti figures do NOT apply). Real budget ~10.5 GB after WDDM. Dense 14B
  Q4 is the resident ceiling; dense 20–24B is a cliff; MoE ~30–35B-A3B with `--n-cpu-moe` works. AC
  power required. The Core Ultra NPU is unusable for this.
- Local runtime: **llama.cpp is the reference path** — Ollama silently truncates history on context
  overflow with no client signal and no `/v1` opt-out. llama.cpp fails closed (HTTP 400). Run
  `llama-server --jinja --cache-reuse`.
- Do **not** add a Hugging Face token to the vault. HF disclosed a production breach 2026-07-16.
- Licence traps for a distributed product: **Gemma** (flow-down obligation + remote-restriction
  reservation), **Llama 4** (branding/agreement obligations), **Kimi K3 weights** (MaaS clause — note
  it governs the weights, NOT calling Moonshot's API). Safe: Apache-2.0 (Qwen3.x), MIT (DeepSeek V4,
  GLM-5.x).
- **Reject hooks.** Arbitrary code execution from config, two CVEs, a silent-exfiltration issue, and
  it inverts nh-law's tighten-only model. The useful 80% is an operator-configured verification
  command through the existing approval gate.
- On **native Windows** the sandbox gap is small: Codex's needs restricted tokens, local user
  creation, elevation and unsafe Win32 FFI (would cost `forbid(unsafe)`); Claude Code's is WSL2-only.

## Do not redo

- The five-lane audit, the Telegram removal, the responsibility refactor, the GitHub bootstrap.
- Any of waves 1, 2, or 3.
- Do not claim the empty remote has run CI. Do not claim Linux or macOS support.
- Do not clean receipts or add pruning.

Newest-first detail is in `00-start-here/BUILD_LOG.md`; the release truth table is
`03-execution/RELEASE_CHECKLIST.md`.
