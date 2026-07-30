# Continue Here — authoritative checkpoint

**Updated:** 2026-07-30 America/Guatemala
**Resume phrase:** `continue`
**Works for:** Codex or Claude in `C:\Users\capv2\Desktop\nosis-Harness`

This file supersedes every older checkpoint and "next task" in `CURRENT_TASK.md`. Historical records
remain useful for provenance, but do not execute their stale instructions.

## THE HEADLINE: the product work is DONE through wave 4. The repo is public. CI is 3/4 green.

All planned feature work for v0.1.0 has shipped and is gated. The source is public at
`github.com/nosistech/nosis-harness`, HEAD `5b4cefb`, working tree **CLEAN**, local and remote **in
sync**. Gate: **`GATE: PASS` — 579 passed / 0 failed / 1 ignored `--release`**, with `fmt --check`,
`clippy -D warnings`, `rustdoc -D warnings` and `cargo deny --locked check` all green.

**What is left is not feature work.** It is: the owner's Linux/macOS verification, three small
decisions, and the tag.

## ⚠ EVERY COMMIT SHA CHANGED ON 2026-07-30

History was rewritten to remove the owner's personal email from the author and committer fields of
all 74 commits. **No file content changed** — the tree object was byte-identical (`eac1553`) before
and after. The rewrite was force-pushed.

**To resolve ANY old SHA quoted in an older note, an article draft, or a commit message, use the full
74-entry map in `08-decisions-and-risk/HISTORY_REWRITE_2026-07-30.md`.** 315 stale references across
20 documents were repaired (zero residual). **Commit messages still quote old SHAs and were left
alone deliberately** — repairing them needs a second rewrite for a cosmetic gain.

`git config --local user.email` is now `98294098+arparvar@users.noreply.github.com`. **Do not reset
it.** **Do not rewrite history again** without an explicit owner decision; a second rewrite would
invalidate the published map.

## First action on `continue`

Read this file in full. Then verify with read-only commands:

```powershell
git status --short                  # expect CLEAN (this is the real signal)
git log -1 --oneline                # expect the newest docs(checkpoint) commit, or later
git log origin/main..HEAD --oneline # expect empty (in sync)
git config --local user.email       # expect 98294098+arparvar@users.noreply.github.com
gh run list --repo nosistech/nosis-harness --limit 3
Get-Process -Name nh,codex -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,StartTime,Path
```

Then ask the owner which of the three open decisions below to take, or proceed with the Linux work if
the owner is ready for it.

## Hard constraints

- **Do not commit, push, or tag without owner authorization.** The orchestrator commits gated work;
  pushing and tagging are outward-facing.
- **Do not run write-mode `cargo fmt`.** `cargo fmt --all --check` is a gate step.
- **Do not rewrite git history again.** See the warning above.
- Do not expose MCP publicly. Loopback-only, bearer-guarded, preview.
- Do not delete or truncate `.nosis/` receipts or Fleet state.
- Do not add dependencies or broaden scope to finish the release.
- Never print, persist, or upload credentials. `nh key` has no read subcommand by design.
- **Never run two nosis codexes.** The owner runs his own; check `--working-dir` before assuming a
  codex process is yours. On 2026-07-29 three unrelated codexes were running on a different project.
- Roles: **Sol max = executor** (all code, one wave at a time, via `codex exec`), **Opus 5 =
  orchestrator** (briefs, gates, review, commits; writes no feature code), **Fable 5 = research**.
- **Sol launch shape** (PS 5.1 mangles big multiline args, so the prompt goes via STDIN):
  `Get-Content -Raw <brief>.txt | codex exec -m gpt-5.6-sol -c 'model_reasoning_effort=max' -s workspace-write --color never -C <repo> -o <out>.md *> <run>.log`
- **The background-command timeout ceiling is 10 minutes.** A 7-item wave exceeded it and Sol was
  killed mid-verification (no work was lost, but no self-report either). For long waves, launch codex
  detached rather than as a timed background command, or split the wave.
- **Write commit messages to a file and use `git commit -F`.** A here-string containing `|` or `%`
  broke PowerShell parsing. Use `[System.IO.File]::WriteAllText` with `UTF8Encoding($false)` —
  `Set-Content -Encoding utf8` writes a BOM that lands in the commit subject.
- Follow root `AGENTS.md`, `05-ai-collaboration/AGENTS.md`, and THE LAW.

## Exact state

- Branch `main`, HEAD **`5b4cefb`**, tree **CLEAN**, `origin/main` identical.
- Remote `github.com/nosistech/nosis-harness` — **PUBLIC**, one branch (`main`), no tags.
- Gate **PASS 579/0/1 `--release`**; all five steps green (`gate.ps1` is five steps: fmt, clippy,
  rustdoc, deny, test — not four).
- `target\release\nh.exe` is current with the source.
- Identity on every commit: `Carlos Paredes Vargas <98294098+arparvar@users.noreply.github.com>`.

## CI — the first honest cross-platform answer, now reproduced twice

| Job | Result |
|---|---|
| Checks (windows-latest) | **success** |
| Checks (macos-latest) | **success** — passed clean on the very first attempt, never built before |
| Supply chain | **success** |
| Checks (ubuntu-latest) | **HANGS** — `Test` step never finishes, hits the 30-minute ceiling |

**Linux compiles and lints fine.** `fmt`, `clippy` and `rustdoc` all pass on Ubuntu. The failure is a
**runtime hang in the test suite**, not a portability problem. The hanging test is **unnamed**:
GitHub does not archive logs for a timed-out job (`gh run view --log` → `log not found`).

Two things already ruled out, do not re-investigate:

- **Keyring / D-Bus is NOT the cause.** The only keyring-touching test is `#[ignore]`d (it is the
  "1 ignored"); every other vault test uses trait fakes.
- **It is Linux-specific, not Unix-generic.** macOS passed, so every `#[cfg(unix)]` path — including
  `exec.rs`'s `kill -KILL -pgid` process-group teardown — executes correctly. Prime suspects:
  `nh-fleet`'s std `File::try_lock`/flock semantics, or Linux process-group timing.

**Owner decision 2026-07-29: Linux and Apple verification is THE LAST THING before the tag.** The
owner will run the Ubuntu VM himself. Leave the red badge (it is honest). Do not burn CI cycles on
it. Note `RELEASE_CHECKLIST.md` line 37 requires green Windows + Ubuntu + macOS, so **the tag is
gated behind this.**

## What shipped this session (2026-07-29 → 30), all gated

- **`0c838d5` wave 3b** — dropped both `deepseek-*-anthropic` catalog routes; catalog 14 → 12 routes.
  `wire/anthropic.rs`, the `Wire` enum, the `deepseek-nhm` dialect and `effort_for`'s wire-awareness
  are **retained deliberately as unused capability** (no dead-code warning: they are `pub` in a lib
  crate). The anthropic route test was rebased onto an inline test-local fixture keeping all five
  assertions.
- **`cf6569e` wave A** — `NH_DEBUG_USAGE=1` prints a provider's **verbatim** `usage` value to stderr,
  scrubbed and route-tagged, on both wires. Uses `serde_json`'s `RawValue` so unknown field names
  survive. Display-only. Documented in `06-operations/ENVIRONMENT.md`.
- **`d80b115` wave 4 — all 7 items.** W4-1 tolerant edit cascade (exact → whitespace-normalised →
  indentation-flexible, ambiguity is failure, nearest-candidate on total failure); W4-2 bounded
  mechanical tool-call repair + closed alias table through the identical guard path; **W4-3 the
  meter-honesty fix (below)**; W4-4 `--jinja`/`--cache-reuse` docs; W4-5 factual route-change note in
  history; W4-6 absolute prices everywhere; W4-7 `WAITING <route> · <elapsed>s` indicator.
- **`10d3c57`** — corrected three stale `RELEASE_CHECKLIST` boxes against live state.
- **`5b4cefb`** — the history rewrite record, the SHA repair, and the README safety section.

## The two findings that mattered most

**1. P-4 was a FALSE ALARM and needs no fix. Do not "fix" it.** The fear was that Moonshot documents
a top-level `usage.cached_tokens` the harness does not read, causing ~5x Kimi input overstatement.
Measured live: **Kimi sends BOTH shapes**, and the harness already reads the nested
`prompt_tokens_details.cached_tokens` correctly:

    kimi-k2.6 hit → {"prompt_tokens":1053,...,"cached_tokens":1053,
                     "prompt_tokens_details":{"cached_tokens":1053}}
    meter said    → 1053 cached | cache 100% | cost $0.0002 — saved 82% vs no-cache

MiMo sends only the nested form, also parsed correctly. This is exactly why the rule is *never
implement a wire fix from documentation alone*.

**2. The probes exposed a REAL meter-honesty bug, now fixed in `d80b115`.** On a cache **miss**,
Kimi and K3 send **no cached-token field at all** — yet the meter printed `0 cached | cache 0%`,
asserting a measured zero the provider never reported. Fixed and verified live:

    call 1 (miss, no field in wire) → tokens 1076 in / 11 out          (no cache segment)
    call 2 (hit,  field present)    → ... / 1076 cached | cache 100%

Receipts gained `cache_hit_pct: Option<f64>`, serialised only when measured.

**Also settled live, make no change:** MiMo accepts `max_tokens` (no `max_completion_tokens`
migration). `kimi-k3` accepted `max_tokens = 1048576` with no error, so its catalog `max_out` stands
— verified only at a ~1,200-token prompt; the large-prompt interaction is untested and was judged not
worth real money to probe.

Total live spend this session ≈ **$0.007**.

## THREE OPEN DECISIONS — ask the owner, do not decide these alone

**1. Branch protection.** `main` is unprotected. Note the mechanics: **required status checks and
direct pushes are the same lever** — enabling required checks blocks direct pushes entirely, because
a new commit cannot have passing checks before it exists. Orchestrator recommendation: apply
**force-push and deletion protection now** (prevents the two irreversible accidents, costs no
velocity), and add **required checks at tag time** once Ubuntu is green and all four contexts can be
required together. Requiring a knowingly-red Ubuntu check would force routine admin overrides.

**2. Is `info@nosistech.com` monitored** for the five-business-day target stated in `SECURITY.md`?
This gates a checklist box. GitHub private vulnerability reporting is **already enabled**, so it is
the working intake path either way. Record an accurate gap rather than a response time nobody
watches.

**3. Commit-author email.** The owner asked that "only `info@nosistech.com` shows". That is already
true of every readable file — `SECURITY.md:17` and `PRIVACY.md:67` both point there, and the only
other addresses in the repo are a Xiaomi business contact, the Anthropic co-author trailer, and a
`.invalid` fixture. The remaining appearance is commit authorship. **Orchestrator recommendation:
leave it.** The noreply address is non-deliverable by design, creates no contact surface, and reveals
only a GitHub username that is already public as the repo's owner. Switching commits to
`info@nosistech.com` would (a) likely lose GitHub attribution unless that address is verified on the
account, (b) invalidate the published SHA map via a second rewrite, and (c) mean two public
force-pushes. **Zero-churn alternative if the owner still wants it: verify `info@nosistech.com` on
the GitHub account, then set it as the commit email going forward only.**

## Remaining path to v0.1.0

1. Resolve the three decisions above.
2. **Owner runs the Ubuntu VM** (VirtualBox Ubuntu 26.04 Desktop; `libdbus-1-dev` required) and finds
   the hanging test. Suspects and exclusions are listed in the CI section above.
3. Fix the Linux hang (Sol wave), gate, commit, push, confirm CI goes 4/4 green.
4. **A short confirming FEEL re-pass on the current binary.** The owner's 2026-07-29 pass was against
   the *old* binary; wave 4 changed the exact surfaces it exercised — `/why`, the model-picker rows,
   the in-flight indicator, the cache line. The checklist box is ticked with this caveat recorded.
5. Enable required status checks with all four contexts; verify the rule.
6. Final gate + secret guard, then **tag `v0.1.0`** with explicit owner approval, source-install only.
7. Publish release notes from the `[0.1.0]` CHANGELOG entry.

## DEADLINE

**All 14 price blocks carry `valid_until = "2026-08-02"`.** That is three days out and the Linux work
sits in front of it. **Expect to miss it and budget a day to re-verify 14 price blocks** — that is
the honest plan. Do not rush the Linux fix to beat the date.

## Settled — do not reopen

- Provider scope is **DeepSeek + Kimi + GLM + MiMo + local**. No Anthropic/OpenAI/Gemini API routes.
- Never add a frontier price row as a `top_tier` anchor — it inflates every savings line 3–6x.
- Zero-price routes are a selectable tier, not a routing winner.
- **The harness does not auto-route.** The "router inside the harness" moat claim is retired.
- Do not add a quality axis to routing.
- **The route-switch path is correct.** W4-5 added evidence for the model; it did not fix a bug.
- Do not change the edit format, add tools, restructure the prompt, lower temperature, or use
  grammar-constrained decoding — each is evidence-backed in the wave-4 brief's DO NOT list.
- **ASD-STE100 is scoped, not repo-wide** (owner discussion 2026-07-30): apply it to user-facing and
  safety-critical text — `SECURITY.md`, `PRIVACY.md`, the README safety section, install steps,
  `06-operations` runbooks, and CLI error strings. Keep `DECISION_LOG`, `ARCHITECTURE_DECISIONS`,
  research and articles in normal prose. Describe it as **"STE-informed", never claim conformance** —
  real conformance needs a commercial checker, and an unverifiable claim would violate THE LAW.

## Do not redo

- The five-lane audit, the responsibility refactor, the GitHub bootstrap, Slice G.
- Waves 1, 2, 3, 3b, A, or 4.
- The three live probes. Their results are recorded above.
- The history rewrite.
- Do not claim Linux support. macOS **is** now verified green, but no support claim ships without the
  owner's sign-off.

Newest-first detail is in `00-start-here/BUILD_LOG.md`; the release truth table is
`03-execution/RELEASE_CHECKLIST.md`; the SHA map is
`08-decisions-and-risk/HISTORY_REWRITE_2026-07-30.md`.
