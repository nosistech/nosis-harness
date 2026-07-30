# Pre-release audit — Sol max (gpt-5.6-sol, effort max), READ-ONLY

> Run 2026-07-21. Auditor: **GPT-5.6 Sol (max effort)** via `codex exec` in a **read-only** sandbox. Orchestrated + gated by Claude (Opus 4.8).
> Trigger: the TUI left the terminal in raw / alternate-screen state after an unclean exit ("numbers flooding, could not type").
> Scope: working tree AS-IS (HEAD `c9863d1` + uncommitted M5 Slice F **W4** changes).

## Gate note (Claude) — independent verification of load-bearing findings

I re-read the actual code for the highest-impact claims. **All sampled findings are CONFIRMED and accurately located** — the report is not hallucinated:

- **Part 1 / H-01 (TUI teardown short-circuit)** — CONFIRMED. `restore_terminal()` (`crates/nh-tui/src/lib.rs:3024`) always disables raw mode, but `write_restore_commands` → `run_restore_sequence` uses `?` (`nh-tui:3039`), so a failure in `DisableBracketedPaste` skips `Show` + `LeaveAlternateScreen`; the outer error is discarded. Real latent leak (fires only when an earlier restore command errors). Setup enables only alt-screen / paste / hide-cursor — NOT mouse / kitty / CPR — so Sol correctly does NOT recommend blindly disabling those.
- **C-01 (audience drops scheme/port)** — CONFIRMED at vault layer: `normalized_host` (`nh-vault:161`) returns host only; `audience_allows` compares host only → http downgrade / arbitrary port on an approved host passes. Surface-breadth claim (TUI/fleet/MCP use raw `vault.get` not `get_scoped`) not re-verified line-by-line; credible and consistent with the W2/W3 scoping history.
- **C-02 (constitution symlink exfil)** — CONFIRMED. `read_optional_text` (`nh-law:288`) is a bare `fs::read_to_string` of repo `AGENTS.md` / `.nosis/memory.md` (and repo `law.toml`) with no symlink/containment/size guard; content is injected into the system prompt and sent to the provider. Exploitable where symlink creation is allowed (Unix; Windows dev-mode).
- **H-05 (ExecShell approval bypass)** — CONFIRMED. `Guard::Allow` executes with no approval (`nh-tools:489`). Shipped law returns `Ask`, so not live-exploitable today, but the boundary permits bypass on any future policy regression / external caller.
- **H-14 (scrubber prefix leak)** — CONFIRMED. `scrub` (`nh-vault:128`) replaces literals in insertion order; a shorter secret that is a prefix of a longer one, replaced first, leaves the suffix. Requires a prefix relationship (uncommon for random keys) but real.

**Calibration:** a few severities read slightly hot (H-14 needs a prefix relationship; H-05/C-02 are latent / privilege-gated), but every claim I checked is real and precisely located. The threat models are predominantly *hostile repo / malicious catalog / adversarial provider*, so for the current owner-run, loopback-preview posture the *live* exposure is lower than the CRITICAL labels imply — **but** THE LAW (secure, safe, congruent, auditable, modular) is genuinely not met, so the "not releasable as v0.1.0 public" verdict stands.

---

<!-- Full Sol deliverable appended below (unedited). Absolute file:line links are Sol's. -->

# Nosis Harness pre-release audit

## 1. Executive summary

- **Release verdict: NO — not releasable as v0.1.0 source-install.**
- Two confirmed critical trust-boundary failures can expose provider credentials or arbitrary local-file contents.
- The TUI invokes cleanup on normal, error, and panic paths, but cleanup short-circuits; terminal restoration is therefore not guaranteed. This is a real W4/FEEL blocker.
- TUI worker shutdown can deadlock indefinitely, including while waiting for an approval reply.
- Metering, finish-reason handling, receipts, MCP egress, process-tree termination, and fleet error cleanup contain confirmed fail-open or inaccurate behavior.
- Several important controls are solid: MCP’s generated bearer tokens, constant-time comparison, loopback binding, redirect disabling, fleet run-ID validation, and ledger locking.
- **UNVERIFIED:** I did not rerun the reported 432-test, clippy, or fmt gates because Cargo may write build artifacts and `cargo fmt` was explicitly prohibited.

## 2. PART 1 — TUI terminal teardown

### Definitive verdict

| Question | Answer |
|---|---|
| Restoration invoked after normal quit? | **YES.** `TerminalGuard::drop` runs after `ui_loop` returns. |
| Restoration invoked after `Err(...)`? | **YES.** Setup failures attempt immediate restoration, and later errors unwind through the guard. |
| Restoration invoked after a Rust panic mid-render? | **YES.** A process-wide panic hook calls restoration before the previous hook; normal unwinding also reaches the RAII guard. |
| Is terminal state successfully restored on every path? | **NO.** Restoration is fail-fast and errors are discarded, so one failed terminal command can prevent later cleanup commands. |
| Is cleanup only on the happy path? | **NO.** Both an RAII guard and a panic hook exist. |
| Is this a W4/FEEL blocker? | **YES.** The cleanup defect can directly leave alternate-screen/cursor/bracketed-paste state active after an abnormal exit. |

The TUI installs `PanicHookGuard` and `TerminalGuard` before entering its event loop; the loop propagates draw/input/channel errors, while the guard performs cleanup on scope exit ([crates/nh-tui/src/lib.rs:1063–1140](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:1063), [crates/nh-tui/src/lib.rs:1516–1570](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:1516), [crates/nh-tui/src/lib.rs:2962–2995](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:2962), [crates/nh-tui/src/lib.rs:3058–3104](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:3058)).

The actual setup sequence enables raw mode, enters the alternate screen, enables bracketed paste, and hides the cursor. It does **not** enable mouse capture, kitty keyboard enhancement, focus events, or a cursor-position-report mode ([crates/nh-tui/src/lib.rs:2998–3021](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:2998)). `HideCursor` is cursor visibility, not cursor-position reporting.

The restoration gap is in [crates/nh-tui/src/lib.rs:3024–3055](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:3024):

- `disable_raw_mode()` is always attempted.
- `DisableBracketedPaste`, `Show`, and `LeaveAlternateScreen` are then executed sequentially through a helper using `?`.
- If disabling bracketed paste fails, cursor restoration and leaving the alternate screen are skipped.
- If showing the cursor fails, leaving the alternate screen is skipped.
- The resulting error is discarded.

Thus, raw-mode disabling is attempted on every reachable cleanup, but bracketed paste, cursor visibility, and alternate-screen restoration are **not independently guaranteed**. Mouse, kitty-keyboard, and CPR cleanup commands are absent because those modes are not enabled by this implementation. They should only be added if setup starts enabling them; blindly popping a keyboard-enhancement stack the application did not push is not a sound fix.

### Minimal correct fix

At [crates/nh-tui/src/lib.rs:3024](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:3024):

1. Keep `TerminalGuard::Drop` as the primary cleanup mechanism.
2. Keep the panic hook calling that same idempotent restoration routine.
3. Attempt every inverse operation independently, retaining the first error only after all operations and the final flush have been attempted.
4. Track which setup operations succeeded and undo each owned mode.
5. Add failure-injection tests proving that failure of the first restore command does not suppress later commands. Current tests cover successful ordering, not cleanup failures or an actual panic path ([crates/nh-tui/src/lib.rs:4659–4714](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:4659)).

Hard kills, `process::abort`, OS termination, or power loss cannot be made universally recoverable by RAII. Catchable Rust panics and ordinary errors can and should be covered.

## 3. PART 2 — Findings

### CRITICAL

#### C-01 — CONFIRMED: credential authorization loses scheme and port, and several surfaces bypass scoping entirely

`normalize_host` reduces an audience to its hostname, while `audience_allows` compares only that value. Scheme and effective port are discarded ([crates/nh-vault/src/lib.rs:159–230](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-vault/src/lib.rs:159)). Route URLs are accepted from catalog data without an HTTPS/origin invariant ([crates/nh-routes/src/lib.rs:400–405](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-routes/src/lib.rs:400), [crates/nh-routes/src/lib.rs:697–703](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-routes/src/lib.rs:697)).

CLI `run` and `chat` use scoped retrieval, but TUI and fleet runtime/preflight paths retrieve raw vault entries:

- Scoped CLI paths: [crates/nh-cli/src/cmd_run.rs:110–126](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_run.rs:110), [crates/nh-cli/src/cmd_chat.rs:100–114](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_chat.rs:100)
- Raw TUI retrieval: [crates/nh-tui/src/lib.rs:1063–1075](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:1063)
- Raw fleet retrieval/preflight: [crates/nh-fleet/src/lib.rs:1382–1390](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:1382), [crates/nh-fleet/src/lib.rs:1536–1569](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:1536)
- Raw MCP fleet preflight: [crates/nh-mcp/src/lib.rs:931–963](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-mcp/src/lib.rs:931)

The resulting client transmits the credential in HTTP authorization headers ([crates/nh-core/src/lib.rs:250–265](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:250), [crates/nh-core/src/lib.rs:539–555](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:539)). A catalog can therefore redirect raw-retrieval surfaces to an attacker host. Even scoped paths accept an HTTP downgrade or arbitrary port on the approved hostname.

**Why it matters:** direct provider-key disclosure.

**Minimal fix:** centralize all credentialed client creation behind one API accepting only a legitimately minted `ResolvedRoute`; require HTTPS plus exact effective origin/port, with an explicit narrowly scoped loopback-HTTP exception. Perform `get_scoped` immediately before materialization in every runtime and preflight path. Remove direct `vault.get` calls from surfaces.

#### C-02 — CONFIRMED: repository instruction symlinks can exfiltrate arbitrary readable files

The constitution loader reads repository `AGENTS.md` and `.nosis/memory.md` using unrestricted `fs::read_to_string`, with neither a symlink check, canonical containment check, regular-file check, nor size limit ([crates/nh-law/src/lib.rs:203–263](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-law/src/lib.rs:203), [crates/nh-law/src/lib.rs:288–296](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-law/src/lib.rs:288)). That content is inserted into the system message and sent to the provider ([crates/nh-core/src/lib.rs:1638–1649](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:1638), [crates/nh-core/src/lib.rs:1684–1692](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:1684)).

On symlink-capable systems, a hostile checkout can point either file outside the repository and cause arbitrary readable local content to be uploaded.

**Why it matters:** local confidentiality compromise without invoking a guarded file tool.

**Minimal fix:** use `symlink_metadata`, reject symlinks and non-regular files, canonicalize and require containment under the repository, open no-follow where supported, and enforce a strict byte cap. Preserve provenance so repository instructions cannot silently acquire user-global trust.

### HIGH

#### H-01 — CONFIRMED: terminal cleanup short-circuits

See Part 1. The restore sequence stops after its first failed command and discards the error ([crates/nh-tui/src/lib.rs:3024–3055](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:3024)).

**Fix:** independent best-effort inversions with state tracking and failure-injection tests.

#### H-02 — CONFIRMED: TUI worker destruction can deadlock or wait for the full provider timeout

`Worker::drop` sends `Stop` and then unconditionally joins the worker thread ([crates/nh-tui/src/lib.rs:1150–1168](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:1150)). Approval handling can block indefinitely on `recv()` ([crates/nh-tui/src/lib.rs:1234–1256](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:1234)). Because `app` is declared before `worker`, reverse drop order joins the worker before dropping the application-owned approval sender ([crates/nh-tui/src/lib.rs:1104–1127](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:1104)).

A live provider call can similarly delay shutdown until its long HTTP timeout.

**Why it matters:** quit/error/panic handling can hang indefinitely. Terminal restoration happens earlier in current declaration order, but process shutdown is not bounded.

**Minimal fix:** explicit shutdown ordering that drops/cancels approval senders first, cancellation-aware approval waits, bounded provider operations, and a bounded worker-completion protocol. Never perform an unconditional `JoinHandle::join` from `Drop`.

#### H-03 — CONFIRMED: route changes leave the agent tool scrubber stale

The TUI constructs `ToolCtx` with a snapshot of the current scrubber ([crates/nh-tui/src/lib.rs:1262–1269](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:1262)). Route/profile/reconnect branches update shared and receipt scrubbers, but not `agent.ctx.scrubber` ([crates/nh-tui/src/lib.rs:1299–1370](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:1299)).

**Why it matters:** after changing routes, tool results containing the newly active secret can leave the tool boundary without that secret being redacted.

**Minimal fix:** use one shared `Arc`-backed scrubber across agent tools, receipts, and UI egress, or atomically replace every snapshot during a route change.

#### H-04 — CONFIRMED: repository MCP configuration can cause ungated SSRF and self-authorized tool calls

Repository MCP configuration accepts arbitrary URLs and `trust = "auto"` ([crates/nh-tools/src/mcp.rs:93–160](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/mcp.rs:93)). TUI/CLI load repository `.nosis/mcp.toml` ([crates/nh-cli/src/cmd_chat.rs:567–598](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_chat.rs:567), [crates/nh-cli/src/cmd_tui.rs:59–103](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_tui.rs:59)). Client initialization contacts every configured non-blocked server via `tools/list` before normal tool approval/send enforcement ([crates/nh-tools/src/mcp.rs:658–684](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/mcp.rs:658)).

For calls, the policy API expects a hostname, but the adapter passes the complete URL ([crates/nh-law/src/lib.rs:118–125](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-law/src/lib.rs:118), [crates/nh-tools/src/mcp.rs:703–729](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/mcp.rs:703)). Auto approval also trusts the server’s own `readOnlyHint` ([crates/nh-tools/src/mcp.rs:570–582](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/mcp.rs:570), [crates/nh-tools/src/mcp.rs:717–740](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/mcp.rs:717)).

**Why it matters:** opening a repository can initiate POST requests to loopback/private/link-local services; hostname blocks may not match; a hostile server can mislabel mutations as read-only.

**Minimal fix:** gate discovery as network egress, enforce URL/IP policy before DNS and after connection, normalize the actual hostname before policy evaluation, cap discovery work, and permit `auto` only from trusted user-global configuration. Never rely solely on a server-supplied read-only annotation.

#### H-05 — CONFIRMED: the hard “exec always requires approval” invariant is not enforced by `ExecShell`

`Guard` and `ToolCtx::with_guard` are public ([crates/nh-tools/src/lib.rs:34–82](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/lib.rs:34)). `ExecShell` executes immediately for `Guard::Allow`; only `Guard::Ask` invokes the approval callback ([crates/nh-tools/src/lib.rs:480–490](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/lib.rs:480)). The current law returns `Ask`, but the execution boundary itself permits bypass.

**Why it matters:** an external caller or future policy regression can violate the repository’s non-negotiable approval rule.

**Minimal fix:** inside `ExecShell`, treat `Block` as refusal and every other verdict as requiring explicit approval. Add a regression test showing `Guard::Allow` plus a rejecting approval callback does not execute.

#### H-06 — CONFIRMED: shell timeout does not reliably kill the whole process tree

Timeout termination shells out to `taskkill` or `kill`, ignores their exit status, and falls back to killing only the immediate child ([crates/nh-tools/src/lib.rs:439–469](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/lib.rs:439)). On Unix, negative-PID termination assumes a process group that the spawn path never creates. Output-drain threads then join without a deadline; a surviving descendant holding a pipe can keep them blocked after the nominal timeout ([crates/nh-tools/src/lib.rs:394–419](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/lib.rs:394), [crates/nh-tools/src/lib.rs:491–542](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/lib.rs:491)).

**Why it matters:** commands can survive timeout and the caller can hang indefinitely.

**Minimal fix:** create a real process group on Unix and a kill-on-close Job Object on Windows, verify termination, close/cancel pipes, and bound drain completion. Report termination failure instead of silently claiming timeout completion.

#### H-07 — CONFIRMED: fleet coordinator errors can detach active workers

Workers are created with ordinary `thread::spawn` handles ([crates/nh-fleet/src/lib.rs:892–919](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:892)). Numerous fallible operations follow before the happy-path join ([crates/nh-fleet/src/lib.rs:953–1153](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:953)). Returning through `?` drops `JoinHandle`s, which detaches their threads.

**Why it matters:** workers may continue provider/tool/receipt activity after the coordinator has returned and the run lock has been released.

**Minimal fix:** an RAII worker-pool guard must close work channels, signal cancellation, and join/drain every worker on every exit path before releasing the run lock.

#### H-08 — CONFIRMED: receipts are unlocked, symlink-following, and non-mandatory

`ReceiptWriter` directly opens an append path without file locking, no-follow protection, or durability synchronization ([crates/nh-core/src/lib.rs:1519–1541](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:1519)). Agent-loop append failures are discarded ([crates/nh-core/src/lib.rs:1808–1811](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:1808)). Parallel fleet workers each target the repository receipt file ([crates/nh-fleet/src/lib.rs:1413–1420](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:1413)).

**Why it matters:** records can be partially/interleavedly appended across writers, writes can follow a hostile symlink outside `.nosis`, and a run can report success without its required audit record.

**Minimal fix:** validate managed state paths without following symlinks, serialize all receipts through one locked writer, make each record durable as required, and surface receipt failure as an explicit unreceipted/failing outcome.

#### H-09 — CONFIRMED: invalid usage can produce a quote, overflow counters, or render non-finite cost as zero

`cost_of` uses `saturating_sub` when cached tokens exceed prompt tokens instead of rejecting impossible usage ([crates/nh-routes/src/lib.rs:170–182](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-routes/src/lib.rs:170)). Money formatting maps a non-finite value to `"0.00"` ([crates/nh-routes/src/lib.rs:249–262](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-routes/src/lib.rs:249)). Provider-controlled usage counters are accumulated with unchecked `u64 +=` in the core and chat paths ([crates/nh-core/src/lib.rs:1707–1714](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:1707), [crates/nh-cli/src/cmd_chat.rs:342–350](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_chat.rs:342)); the TUI uses saturation, which avoids a panic but silently loses truth ([crates/nh-tui/src/lib.rs:1457–1465](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:1457)). MCP exposes costing over caller-supplied usage ([crates/nh-mcp/src/lib.rs:657–733](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-mcp/src/lib.rs:657)).

**Why it matters:** costs and savings can be presented as authoritative despite invalid or overflowed inputs.

**Minimal fix:** validate `cached <= prompt`, use checked accumulation, and return an explicit incomplete/unavailable meter state on overflow or non-finite arithmetic. Never display invalid cost as zero.

#### H-10 — CONFIRMED: truncated provider answers are classified as successful passes

Wire parsers preserve `finish_reason` ([crates/nh-core/src/lib.rs:168–173](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:168), [crates/nh-core/src/lib.rs:476–514](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:476), [crates/nh-core/src/lib.rs:681–727](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:681)). The agent loop ignores it and returns `Outcome::Pass` whenever no tool call remains ([crates/nh-core/src/lib.rs:1717–1730](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:1717)).

**Why it matters:** `length`, token-limit, or content-filter termination can be recorded as completed work, preventing fleet escalation or retry.

**Minimal fix:** parse finish reasons into a typed enum. Only normal terminal reasons may produce `Pass`; length/limit must be partial or constrained, filtering must be explicit, and unknown reasons must fail closed.

#### H-11 — CONFIRMED: approval displays can omit the dangerous tail of the operation being approved

MCP arguments are capped at 120 display characters while the complete arguments are still executed ([crates/nh-tools/src/mcp.rs:33–34](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/mcp.rs:33), [crates/nh-tools/src/mcp.rs:728–740](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/mcp.rs:728)). CLI approval text passes through a 500-character sanitizer ([crates/nh-cli/src/cmd_run.rs:130–135](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_run.rs:130), [crates/nh-vault/src/lib.rs:239–257](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-vault/src/lib.rs:239)). TUI’s full-line scrubber escapes controls but not bidirectional-format characters ([crates/nh-tui/src/lib.rs:2924–2937](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:2924)).

**Why it matters:** the approver can authorize bytes they were not shown or whose visual order is misleading.

**Minimal fix:** one shared approval renderer must show the full canonical command/JSON, escape controls and bidi characters, allow scrolling/details, and bind any digest to the exact bytes later executed.

#### H-12 — CONFIRMED: remote responses are buffered without byte limits; server TTL can panic

Provider responses use unbounded `.text()` buffering ([crates/nh-core/src/lib.rs:250–261](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:250), [crates/nh-core/src/lib.rs:539–551](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:539)). MCP responses and OAuth JSON are similarly buffered without response-size caps ([crates/nh-tools/src/mcp.rs:322–382](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/mcp.rs:322), [crates/nh-tools/src/mcp.rs:503–517](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/mcp.rs:503)). A remote `ttlMs` is converted and added directly to `Instant`, which can overflow/panic for extreme values ([crates/nh-tools/src/mcp.rs:260–269](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/mcp.rs:260)).

**Why it matters:** a provider or MCP server can cause memory exhaustion or a process panic.

**Minimal fix:** stream with a hard maximum, reject oversized `Content-Length`, cap tool/schema counts, and clamp or `checked_add` all remote TTLs.

#### H-13 — CONFIRMED: `nh init` can write a hook outside the requested repository

Git directory discovery accepts `.git` directories, pointer files, symlinks, absolute paths, and `commondir` resolution without proving the resolved Git directory belongs to the repository ([crates/nh-cli/src/cmd_init.rs:68–143](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_init.rs:68)). Hook installation then creates/writes under the resolved directory ([crates/nh-cli/src/cmd_init.rs:97–110](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_init.rs:97)).

**Why it matters:** running `nh init` in a crafted checkout can create a pre-commit file in an arbitrary existing external directory.

**Minimal fix:** reject symlinked/arbitrary `.git` targets, validate the resolved common Git directory through a trusted Git query plus containment/relationship checks, and create hooks with no-follow semantics.

#### H-14 — CONFIRMED: scrubber replacement order can reveal a suffix of a longer secret

Literals are stored in insertion order, then replaced sequentially ([crates/nh-vault/src/lib.rs:119–135](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-vault/src/lib.rs:119)). If one secret is a prefix of another and the shorter is processed first, the longer value becomes `[REDACTED]` plus its remaining suffix.

**Why it matters:** arbitrary-format secrets can be partially disclosed despite the scrubber reporting successful replacement.

**Minimal fix:** deduplicate and sort literals longest-first, or use a simultaneous multi-pattern matcher. Test both insertion orders and overlapping secrets.

### MEDIUM

#### M-01 — CONFIRMED: scrubber construction materializes every catalog credential into ordinary strings

`Scrubber::from_vault` retrieves credentials for all catalog entries and copies them into a `Vec<String>` ([crates/nh-vault/src/lib.rs:139–156](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-vault/src/lib.rs:139)). TUI and fleet retain further ordinary-string copies ([crates/nh-tui/src/lib.rs:1219–1223](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:1219), [crates/nh-fleet/src/lib.rs:703–709](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:703)). This conflicts with the stated least-privilege and zeroization requirements ([05-ai-collaboration/SECURITY_MODEL.md:24–36](C:/Users/capv2/Desktop/nosis-Harness/05-ai-collaboration/SECURITY_MODEL.md:24)).

**Fix:** materialize only credentials needed by active routes and keep them in zeroizing secret types through client and scrubber ownership.

#### M-02 — CONFIRMED: missing price/FX expiry is treated as “valid forever”

FX `valid_until` and price validity are optional ([crates/nh-routes/src/lib.rs:438–457](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-routes/src/lib.rs:438)). Conversion refuses only an explicitly expired value; absent freshness metadata is accepted indefinitely ([crates/nh-routes/src/lib.rs:197–205](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-routes/src/lib.rs:197), [crates/nh-routes/src/lib.rs:297–312](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-routes/src/lib.rs:297), [crates/nh-routes/src/lib.rs:553–598](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-routes/src/lib.rs:553)).

The checked-in FX row is currently dated through 2026-07-24, so current staleness is **not** asserted ([catalog.toml:22–25](C:/Users/capv2/Desktop/nosis-Harness/catalog.toml:22)). The generic parser nevertheless permits undated values.

**Fix:** require freshness metadata wherever USD comparison depends on mutable prices/FX; absent or expired freshness must produce “unpriced/unavailable,” never an estimate.

#### M-03 — CONFIRMED: final answers are silently truncated per line

The shared display sanitizer caps lines at 500 characters ([crates/nh-vault/src/lib.rs:81–82](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-vault/src/lib.rs:81), [crates/nh-vault/src/lib.rs:239–257](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-vault/src/lib.rs:239)). CLI answer rendering applies it line by line ([crates/nh-cli/src/cmd_run.rs:383–395](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_run.rs:383), [crates/nh-cli/src/cmd_chat.rs:609–614](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_chat.rs:609)); TUI applies it before wrapping ([crates/nh-tui/src/lib.rs:365–378](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:365)).

**Fix:** use unbounded redaction/control escaping for substantive output and let the terminal renderer wrap. Retain bounded sanitization only for short status fields.

#### M-04 — CONFIRMED: `EditFile` has unbounded reads, a path-check/use race, and non-atomic replacement

Path containment is checked through canonicalization, then the path is reopened later ([crates/nh-tools/src/lib.rs:214–249](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/lib.rs:214)). Editing reads the entire file and rewrites it with `fs::write`, which truncates before successful completion ([crates/nh-tools/src/lib.rs:331–360](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/lib.rs:331)).

**Fix:** enforce a byte cap, use no-follow/capability-style handles, and write a temporary sibling followed by synchronization and atomic replacement.

#### M-05 — CONFIRMED: fleet failure bookkeeping can disappear, and an existing run ID can be reused as a new run

`RunFailed` append results are discarded ([crates/nh-fleet/src/lib.rs:861–868](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:861)); call sites return only the initiating failure ([crates/nh-fleet/src/lib.rs:402–404](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:402), [crates/nh-fleet/src/lib.rs:550–552](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:550)). `run_with_id` repairs an existing ledger and then appends a new run without asserting that the ledger is empty ([crates/nh-fleet/src/lib.rs:385–405](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:385), [crates/nh-fleet/src/lib.rs:442–458](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:442)).

**Fix:** compose bookkeeping failure into the returned error, and reject non-empty run IDs from the new-run path; require the explicit resume path.

#### M-06 — CONFIRMED: MCP server lifecycle and resource quotas are incomplete

`McpServer` has no `Drop` implementation joining or shutting down its accept thread, and accept-loop failure is reduced to diagnostic output rather than a server error ([crates/nh-mcp/src/lib.rs:43–90](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-mcp/src/lib.rs:43), [crates/nh-mcp/src/lib.rs:137–155](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-mcp/src/lib.rs:137)). Caller-supplied bearer tokens receive no minimum entropy validation, although generated tokens correctly use 32 random bytes ([crates/nh-mcp/src/lib.rs:93–120](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-mcp/src/lib.rs:93)). `fleet_run` accepts any nonzero `max_workers` and each request spawns a background run without a global active-run/rate cap ([crates/nh-mcp/src/lib.rs:858–922](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-mcp/src/lib.rs:858)).

**Fix:** RAII shutdown/join, error propagation, minimum caller-token strength or always-generated tokens, configured worker/task limits, and per-token active-run/rate quotas.

#### M-07 — CONFIRMED: the installed pre-commit secret guard is incomplete and silently yields to existing hooks

The generated hook scans only a subset of the vault scrubber’s documented key shapes ([crates/nh-cli/src/cmd_init.rs:15–25](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_init.rs:15), [crates/nh-vault/src/lib.rs:86–98](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-vault/src/lib.rs:86)). If a pre-commit hook already exists, installation silently returns ([crates/nh-cli/src/cmd_init.rs:97–110](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_init.rs:97)).

**Fix:** share one canonical detector, scan staged content, and explicitly warn or install a safely chained hook.

#### M-08 — CONFIRMED: hazardous-command blocking is bypassed by common executable wrappers

The command classifier peels only selected shell wrappers and otherwise decides mainly from the first token ([crates/nh-law/src/lib.rs:420–457](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-law/src/lib.rs:420)). Wrapper forms such as environment launchers can turn a configured `Block` into `Ask`.

**Fix:** represent executable and arguments structurally, unwrap recognized launchers recursively with a depth cap, and fail closed on ambiguous shell constructs.

#### M-09 — CONFIRMED: repository notification configuration can direct a user’s Telegram bot to an arbitrary chat

TUI configuration reads `.nosis/notify.toml` from the repository ([crates/nh-cli/src/cmd_tui.rs:106–129](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_tui.rs:106)). The TUI then retrieves the user’s Telegram credential and sends status messages to the repository-selected chat without the normal send-policy/approval path ([crates/nh-tui/src/lib.rs:975–1049](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:975)).

**Fix:** make destination configuration user-global/trusted or require first-use approval; repository policy may disable or restrict notification, not introduce an arbitrary destination.

#### M-10 — CONFIRMED: external code can mint `ResolvedRoute`

`ResolvedRoute` exposes public fields despite the hard rule that only `RouteResolver` may create resolved routes ([crates/nh-routes/src/lib.rs:266–289](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-routes/src/lib.rs:266)).

**Fix:** make fields private, expose read-only accessors, and restrict construction to the resolver module.

### LOW

#### L-01 — CONFIRMED: a caught TUI panic leaves the global panic hook installed

`PanicHookGuard::drop` returns immediately while its thread is panicking, so an outer `catch_unwind` can retain the TUI hook after the session has ended ([crates/nh-tui/src/lib.rs:3089–3104](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:3089)).

**Fix:** own the entire TUI session inside `catch_unwind`, restore the previous hook after the catch, then resume unwinding.

### NIT

#### N-01 — CONFIRMED: CLI help understates the MCP surface

The command description still advertises three MCP tools although the server now exposes fleet operations as well ([crates/nh-cli/src/main.rs:138](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/main.rs:138), [crates/nh-mcp/src/lib.rs:575–653](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-mcp/src/lib.rs:575)).

### Confirmed controls that should be preserved

- Core HTTP redirects are disabled ([crates/nh-core/src/lib.rs:19–31](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:19)).
- MCP’s default bearer token is CSPRNG-generated, comparison is constant-time, binding is loopback-only, unauthorized requests receive 401, and request bodies are capped ([crates/nh-mcp/src/lib.rs:93–120](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-mcp/src/lib.rs:93), [crates/nh-mcp/src/lib.rs:158–204](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-mcp/src/lib.rs:158), [crates/nh-mcp/src/lib.rs:230–279](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-mcp/src/lib.rs:230)).
- Shell stdin is null, and Windows command construction uses verbatim/raw argument handling ([crates/nh-tools/src/lib.rs:491–508](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/lib.rs:491)).
- Fleet uses a filesystem run lock, validates run IDs, and its read-only ledger readers do not repair state ([crates/nh-fleet/src/lib.rs:761–799](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:761), [crates/nh-fleet/src/lib.rs:1720–1739](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:1720), [crates/nh-fleet/src/lib.rs:1802–1827](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:1802)).

## 4. PART 3 — THE LAW scorecard

Legend: **P** = PASS, **W** = WEAK, **F** = FAIL.

| Crate | Small | Simple | Secure | Safe | Lightweight | Readable | Auditable | Congruent | Harmonic | Modular | Worst area |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `nh-law` | W — large single module | W — stringly policy parsing | F — instruction symlink exfiltration | W — unbounded file ingestion | P — lean dependencies | P — generally direct helpers | W — host/URL contract is implicit | F — send API and callers disagree | W — strong intent, porous source trust | W — loading and policy coupled | [constitution loading and command parsing](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-law/src/lib.rs:203) |
| `nh-core` | F — 2K+ line central module | W — wire, loop and receipts intertwined | F — unsafe receipt path/body bounds | F — overflow and finish misclassification | W — HTTP weight is justified but centralized poorly | W — readable locally, difficult globally | F — receipt failure is invisible | F — parsed state is ignored downstream | F — “Pass” can contradict reality | F — multiple subsystems in `lib.rs` | [agent termination and receipts](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-core/src/lib.rs:1519) |
| `nh-routes` | F — large parser/resolver/meter module | W — multiple policy layers mixed | W — URL origin invariant absent | F — invalid metering accepted | P — lean data-oriented dependencies | W — clear helpers, oversized whole | W — optional freshness weakens audit | F — public routes defeat sole-minter rule | F — catalog truth is not fail-closed | W — profiles split, core remains monolithic | [costing, freshness and route construction](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-routes/src/lib.rs:170) |
| `nh-vault` | P — compact | P — cohesive API | F — host-only audience and overlap leak | W — secret copies outlive zeroizing values | P — dependencies serve keyring/security | P — straightforward implementation | W — ordering-sensitive redaction | F — behavior conflicts with least privilege | F — secure storage is weakened at retrieval | P — cohesive responsibility | [audience and scrubber implementation](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-vault/src/lib.rs:119) |
| `nh-tools` | F — roughly 3K lines | F — filesystem, shell, MCP and OAuth complexity | F — approval/SSRF/process containment gaps | F — timeout and edit guarantees fail | W — networking is needed, process control is incomplete | W — functions are readable but modules are oversized | F — approval display differs from execution | F — hard exec/send laws are not enforced at boundary | F — controls compose inconsistently | W — two modules, each too broad | [shell lifecycle and MCP policy](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tools/src/lib.rs:394) |
| `nh-mcp` | F — server and all tools in one large file | F — transport, auth, fleet and receipts coupled | F — unbounded authorized work/direct vault use | F — detached lifecycle and background errors | P — chosen server/auth dependencies are modest | W — clear sections, difficult whole | F — background failure visibility is weak | W — previews and execution mostly align | F — local security is undercut by resource behavior | F — no internal subsystem boundaries | [server lifecycle and fleet spawning](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-mcp/src/lib.rs:43) |
| `nh-fleet` | F — large scheduler/ledger module | F — orchestration state machine is dense | F — credential scoping and receipt paths fail | F — workers detach on error | P — dependency footprint is restrained | W — well-named, but oversized flows | F — receipt/RunFailed truth can disappear | W — ledger is strong, side channels are not | W — good durability intent, incomplete lifecycle | F — scheduler, worker, storage and resume coupled | [coordinator and worker lifecycle](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:892) |
| `nh-cli` | W — split by command but chat is oversized | W — substantial duplicated surface wiring | F — init can escape repository boundaries | W — output truncation/unchecked totals | P — primarily orchestration dependencies | W — command modules help; chat remains dense | W — sanitization behavior is dispersed | W — run/chat are scoped while delegated surfaces differ | W — UX surfaces do not share one security path | W — command split is useful but incomplete | [init path resolution and chat wiring](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_init.rs:68) |
| `nh-tui` | F — 5K+ line `lib.rs` | F — UI, worker, networking, metering and notifications combined | F — raw credentials and stale scrubber | F — teardown and shutdown are not guaranteed | W — UI stack is expected; direct networking duplicates layers | F — audit requires navigating one giant module | F — global hook and callback lifetimes are subtle | F — diverges from CLI credential behavior | F — lifecycle/security pieces interfere | F — clear internal extraction points remain unextracted | [session lifecycle and terminal restoration](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:1063) |
| **Workspace** | **F** | **F** | **F** | **F** | **W** | **W** | **F** | **F** | **F** | **W** | Credential logic is duplicated across [CLI](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-cli/src/cmd_run.rs:110), [TUI](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-tui/src/lib.rs:1063), [fleet](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-fleet/src/lib.rs:1382), and [MCP](C:/Users/capv2/Desktop/nosis-Harness/crates/nh-mcp/src/lib.rs:931). |

### Workspace verdict

The crate boundaries are directionally good, and dependency choices are mostly restrained. Internally, however, several crates have become god modules, while the most security-sensitive invariants remain duplicated or stringly typed.

Top LAW violations:

1. Credential authorization is not a single congruent abstraction.
2. Controls are enforced above the dangerous operation rather than at the operation boundary.
3. Receipt and metering failures can be silently converted into apparent success.
4. Large `lib.rs` modules make lifecycle and cross-path auditing unnecessarily difficult.
5. Cleanup, cancellation, and background-thread ownership are inconsistent across TUI, fleet, and MCP.

## 5. Prioritized action list

1. **[BLOCKER]** Centralize credentialed client creation; enforce exact HTTPS origin/port and scoped retrieval on every surface. Make `ResolvedRoute` non-forgeable. Addresses C-01 and M-10.
2. **[BLOCKER]** Make terminal restoration independently best-effort and failure-tested; replace unconditional worker joining with cancellation-aware bounded shutdown. Addresses H-01 and H-02.
3. **[BLOCKER]** Close filesystem trust-boundary escapes for constitution files, Git hook discovery, receipts, and edit paths. Addresses C-02, H-08, H-13, and M-04.
4. **[BLOCKER]** Make receipt and fleet-worker cleanup mandatory on every outcome; never return `Pass` when required audit persistence failed. Addresses H-07, H-08, and M-05.
5. **[BLOCKER]** Make metering fail closed: checked counters, validated cached usage, finite cost only, mandatory freshness, and typed finish-reason handling. Addresses H-09, H-10, and M-02.
6. **[BLOCKER]** Put MCP discovery and execution behind normalized egress policy; remove repository-controlled auto trust and add response/concurrency limits. Addresses H-04, H-12, and M-06.
7. **[BLOCKER]** Enforce approval inside `ExecShell` and implement verified whole-process-tree termination with bounded output draining. Addresses H-05 and H-06.
8. **[BLOCKER]** Fix overlapping-secret redaction, refresh the active tool scrubber on route changes, and ensure approval displays cover the exact executed bytes. Addresses H-03, H-11, and H-14.
9. **[SHOULD]** Reduce plaintext credential lifetime, complete pre-commit detection, move notification destinations out of repository-controlled trust, and remove final-answer truncation. Addresses M-01, M-03, M-07, and M-09.
10. **[NICE]** Extract TUI session/terminal/worker/notification modules and split the large core, routes, tools, MCP, and fleet modules along existing responsibility boundaries; repair panic-hook ownership and stale help text.