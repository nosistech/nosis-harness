# History rewrite — 2026-07-30

**Every commit SHA in this repository changed on 2026-07-30.** This document records why, what
changed, what did not, and how to resolve any older SHA you find quoted elsewhere.

## What happened

The repository was made public on 2026-07-30. Every one of its 74 commits carried the maintainer's
personal email address in both the author and committer fields, because that address was configured
in `git config` from the first commit. Publishing the repository published the address on every
commit page and through the GitHub API.

The author and committer email was rewritten across all 74 commits, from a personal address to the
maintainer's GitHub `users.noreply.github.com` address. The author **name** was deliberately kept:
an open-source tool that asks for trust with credentials and shell access should say who maintains
it.

## What did NOT change

**No file content changed.** This is verifiable rather than asserted: the Git tree object of the
rewritten head is `eac15539c24b7ea268efac0f162cd8648462549d`, byte-identical to the tree of the head
before the rewrite. Only commit metadata differs, so every commit's snapshot of the working tree is
unchanged.

The gate was re-run after the rewrite and after the reference repair below.

## Why this was done now rather than later, or not at all

Rewriting published history is disruptive and normally the wrong answer. Three facts made it correct
here, and all three were time-limited:

- The repository had been public for hours, carried no tag, and had no forks or downstream clones.
- The cost only ever rises. After a release tag and real users, rewriting breaks other people's
  clones, not just this one.
- Launch traffic is expected. There is a real difference between an address sitting in a scraper's
  database and an address displayed on the commit list of a repository people are being invited to
  read.

The argument against was genuine and is recorded honestly: this project's central claim is
auditability, and its decision log cites commit SHAs as evidence. Breaking those references would
damage the asset the project is built on. That objection was answered by repairing the references
rather than accepting their loss — see below — not by dismissing it.

It is also worth stating plainly what this does **not** achieve. The address was public for several
hours on a public repository, and public repository events are mirrored and scraped within minutes.
This rewrite removes the address from the canonical history and from casual discovery. It does not,
and cannot, retract a disclosure that already happened.

## Reference repair

315 commit-SHA references across 20 documents pointed at commits that no longer exist. Every one was
mechanically rewritten to its new SHA using the map below, matching each old SHA prefix together
with any longer hex form so that abbreviated and full SHAs were both handled at their original
length. After the repair, zero stale references remained. Only Markdown files were affected; no
source file, manifest, workflow or script contained a commit reference.

The map is included in full so that any SHA quoted in an older note, an external document, or a
draft article can still be resolved. Old SHAs are on the left.

## A known, deliberate inconsistency

**Commit messages still quote old SHAs.** Many messages refer to the commit they close, for example
"docs: close M5 Slice A (TRUTH, committed `9c96259`)". Those SHAs are part of the message text, so
repairing them would require rewriting the messages, which is a second rewrite for a cosmetic gain.
They were left alone.

The consequence: a SHA quoted inside a commit message will not resolve with `git show`. Look it up in
the map below instead. This was preferred over either a second rewrite or a silent inconsistency.

## How to resolve an old SHA

Find the old short SHA in the left column. The right column is the same commit, with the same tree
and the same message, under its new identity.

## Map — old SHA to new SHA (74 commits, oldest first)

| old | new | subject |
|---|---|---|
| `dfbbf2699a` | `e2762da283` | Pre-M0: project OS, workspace scaffold with locked API contracts |
| `51aef977e4` | `c7614739e3` | M0: implement workspace (turn loop, tools, vault, routes, CLI) |
| `6663d9c2e3` | `829eaf7fd2` | docs: correct M0 test count in build log |
| `86b6402468` | `92c65a2c30` | M0: hardening fixes from adversarial review |
| `1a0eaa5118` | `c3f24041c2` | docs: M0 build log, current task, quickstart |
| `0ed3d6d9c8` | `fa5e986dfd` | M1: full catalog, clock pricing, Anthropic wire, thinking dialects, MCP client, chat session |
| `bfdfc598e0` | `29ec70fd2f` | M1: hardening fixes from adversarial review |
| `96c46b13ae` | `588e297550` | docs: M1 build log, milestones, quickstart |
| `31559491f8` | `c62be7af16` | M2: context engine + nested law + mechanical write-holds |
| `f45fb02ae3` | `28708cc2fc` | M3 Slice A: nh-tui shell + semáforo + cost HUD + `nh tui` |
| `13c36c9c79` | `f1acaaa22d` | M3 Slice B: trust-dial view + `?` discoverability palette |
| `40f4180954` | `e24ef4af54` | M1 live-fix: DeepSeek reasoning_effort omits the field for non-thinking |
| `6f9524ea36` | `2ec8fa14a4` | docs: add M6 multimodal orchestration milestone |
| `21b92e40e3` | `66f5628963` | M3 Slice C: timeline view + Telegram notify hook (M3 content-complete) |
| `28e8cf6d72` | `b3503d96ad` | docs: set CURRENT_TASK resume anchor (M3 content-complete) |
| `3fcd00ed7f` | `44f9809488` | M3 TUI UX overhaul: framed chat, slash commands, native copy + bracketed paste (Slices D+E+F, M3 closed) |
| `d5143c5368` | `27a6a172ec` | docs: close M3 (UX-approved), set CURRENT_TASK anchor to M4 |
| `9b0a8ad747` | `b79c65dde4` | fix(tui): case-fold /effort argument so /effort High works |
| `347bce693b` | `96db4f7c92` | feat(fleet): M4 Slice A — nh-fleet append-only ledger + workers + idempotent resume |
| `5889fb78e1` | `a4dd1989be` | docs: set M4 resume anchor (Slice A committed 347bce6, next = Slice B) |
| `ecadc0aaa6` | `25bd5b3865` | feat(fleet): M4 Slice B — off-peak scheduler + escalation ladder + Kimi swarm seam |
| `4caad2cb88` | `f439e1752d` | docs: close M4 Slice B (committed ecadc0a), set resume anchor to Slice C (nh-mcp, E3) |
| `26c6a22e07` | `ece6bb0d8f` | feat(mcp): M4 Slice C — nh-mcp stateless server + fleet handle seams (E3) |
| `7680bca140` | `80bf2ac26f` | docs: close M4 Slice C (committed 26c6a22), set resume anchor to Slice D (OAuth2, E4) |
| `7faf44b804` | `d100d0d943` | fix(identity): apply the honest-identity prompt in nh run + nh chat, not just the TUI |
| `bd35b4d1ba` | `ebea70915d` | docs: record identity bugfix (7faf44b), bump resume anchor |
| `aa751f4e3b` | `9344251530` | feat(mcp): M4 Slice D — OAuth2 MCP client with refresh + 401-retry (E4) |
| `d3cac390f3` | `a2c2b83246` | docs(research): July-2026 deep improvement research (2 models, 13 lenses) |
| `6de331aeec` | `0039cc4862` | docs: close M4; set M5 direction "The Honest Meter" + resume anchor |
| `e2b2f02fa9` | `9e36a94e33` | docs: lock CONTRACTS_M5 "The Honest Meter" + M5 best-in-category positioning |
| `fe04ce55dd` | `88b84e8974` | docs: update CURRENT_TASK resume anchor — M5 contract locked, next = brief Sol for Slice A |
| `0bd1d7f75f` | `a126ee2cb2` | docs: log CONTRACTS_M5 §8 amendment A-M5-1 + Slice A briefed to Sol |
| `9c962592e7` | `68f91e6f97` | feat(nh-core,nh-routes): M5 Slice A — the honest meter (TRUTH; E1) |
| `70a2f9d14b` | `740487845a` | docs: close M5 Slice A (TRUTH, committed 9c96259) — next = brief Sol for Slice B |
| `1a9d92ae91` | `edfcd62f4d` | feat(nh-tools,nh-law,nh-vault,nh-mcp): M5 Slice B — the floor (FLOOR; E2) |
| `b777290d75` | `918989acf7` | docs: close M5 Slice B (FLOOR, committed 1a9d92a) — next = brief Sol for Slice C (VISIBLE, the FEEL gate) |
| `e97ec1f35c` | `1fb0861582` | docs: update CURRENT_TASK ON-RESUME anchor to Slice C — bare "continue" now drives the FEEL gate |
| `a0a4036a83` | `a0f77be7ef` | feat(nh-routes,nh-tui,nh-cli): M5 Slice C — the felt meter (VISIBLE; E3) |
| `3a5df91766` | `213ed0a0b5` | docs: close M5 Slice C (VISIBLE, committed a0a4036) — next = brief Sol for Slice D (LEVER, profiles) |
| `d6e2c7f35d` | `256447610e` | feat(nh-routes,nh-core,nh-cli,nh-tui): M5 Slice D — LEVER (profiles; E4) |
| `bc2a1b1cb1` | `68f71cd69e` | style/build: rustfmt-normalize workspace + fmt --check gate |
| `a71eb23072` | `059a00ecf8` | build: pin toolchain (1.96.0) + EOL policy + cargo-deny supply-chain policy |
| `0c14743232` | `6a11f32c51` | docs: close M5 Slice D + Slice E hygiene (fmt/gate/toolchain) + audit in flight |
| `d868f16953` | `c0ceaefad4` | docs(audit): Fable 5 high full workspace audit report (75 confirmed findings) |
| `d1c7733dcd` | `ed262517b7` | docs: M5 "Slice F: HARDEN" plan — audit remediation in 5 waves |
| `497f43ca39` | `86a53ab598` | docs: ratify Slice F wave order W1->W3->W2->W5->W4 (owner-approved) |
| `d04460fc2d` | `1750249592` | docs: W1 (Slice F HARDEN) briefed + launch-ready — contract §0.1-F seams + url §0.4 exception |
| `d95a8d609f` | `6cefd56438` | feat(nh-vault,nh-law,nh-cli): M5 Slice F W1 — SECURITY FLOOR (audit W1-1..W1-13) |
| `d0972bc521` | `be45e34b77` | docs: close Slice F W1 (SECURITY FLOOR, d95a8d6) — BUILD_LOG + CURRENT_TASK; W3 next |
| `591544cae3` | `f514975f65` | docs: W3 (Slice F HARDEN) briefed + launch-ready — CONTRACTS §0.1-F W3 seam table + A-M5-9 |
| `2b681631f0` | `73d278bbce` | feat(nh-core,nh-routes,nh-cli,nh-tui): M5 Slice F W3 — METER TRUTH (audit W3-1..W3-14 + A-M5-9) |
| `bfaa8c9a66` | `1db67f9894` | docs: close Slice F W3 (METER TRUTH, 2b68163) — BUILD_LOG + CURRENT_TASK + CONTRACTS; W2 next |
| `4d26e36467` | `3aa34815d3` | docs: W2 (Slice F HARDEN) briefed + launch-ready — CONTRACTS §0.1-F W2 seam table + §0.4 getrandom/subtle |
| `e903ef0706` | `2e09513ab6` | feat(nh-tools,nh-mcp,nh-cli,nh-tui): M5 Slice F W2 — TOOL EGRESS + EXEC (audit W2-1..W2-18) |
| `de15a4ed50` | `380e57b920` | docs: close Slice F W2 (TOOL EGRESS + EXEC, e903ef0) — BUILD_LOG + CURRENT_TASK + CONTRACTS; W5 next |
| `fe54e48006` | `2aded2d1d3` | docs: W5 (Slice F HARDEN) briefed + launch-ready — CONTRACTS §8 A-M5-8 + §0.1-F W5 seam table |
| `eaadfb27b5` | `441727beda` | feat(nh-fleet,nh-mcp,nh-cli): M5 Slice F W5 — FLEET RELIABILITY (audit W5-1..W5-11) |
| `974d9942b8` | `30f6760c58` | docs: close Slice F W5 (FLEET RELIABILITY, eaadfb2) — BUILD_LOG + CURRENT_TASK + CONTRACTS §0.1-F SHIPPED; Release Slice next |
| `b43b0238a0` | `7f4add6b87` | docs: add LICENSE (MIT © nosistech LLC) + SECURITY.md (ASD-STE100) — Release Slice items 1-2 |
| `1d04871dac` | `7c2b2c4d22` | feat(nh-mcp): Release Slice — MCP metered-service expansion (why/route_cost/receipts + structuredContent) |
| `202bdca94a` | `b708b8c69f` | docs: Release Slice sections C+D — CHANGELOG, README quickstart, PRIVACY, CONTRIBUTING, real RELEASE_CHECKLIST |
| `2ba8f6c115` | `ec0a92fb96` | docs: close Release Slice MCP wave (1d04871) + docs C+D (202bdca) — BUILD_LOG + CURRENT_TASK |
| `d1f9ad0f3c` | `cccb2dc25e` | chore(release): Section B — engineering tail (forbid-unsafe + workspace lints + MIT license + cargo-deny gate + keyless CI) |
| `85e9374306` | `dc227f2e60` | docs: close Release Slice Section B (d1f9ad0) — BUILD_LOG + CURRENT_TASK + RELEASE_CHECKLIST |
| `28cfee7896` | `6637b45baf` | docs(release): record LIVE provider tests — launch evidence (4 providers, ~$0.0014 total) |
| `5496b44230` | `c9863d1e31` | docs(contract): pre-authorize M5 Slice F W4 seam table (surfaces, FEEL) — the last wave |
| `9856ebfa9b` | `e42a5bcdd4` | feat(release): harden and modularize public v0.1 |
| `6b05688f94` | `0056a072fa` | chore(release): prepare v0.1.0 candidate |
| `91e4f5431e` | `cba2444f4a` | feat: complete the v0.1.0 release candidate |
| `8d245af350` | `514d8636a7` | docs(checkpoint): record commit 91e4f54; next gate is the push |
| `3587b48939` | `0c838d51a9` | feat(routes): drop the two DeepSeek Anthropic-wire routes |
| `b9720b82dc` | `cf6569e0f3` | feat(core): opt-in raw provider-usage diagnostic |
| `90a7ce7bcc` | `d80b115e9a` | feat: wave 4 - failure-path repair and meter honesty |
| `dfe4739af9` | `10d3c5763f` | docs(release): correct three stale checklist boxes against live state |
