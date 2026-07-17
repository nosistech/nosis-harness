# NOSIS HARNESS — Lens M: Agentic Surface & Extensibility (2026-07)

Research pass, 2026-07-17. Repo grounding: `crates/nh-tools/src/lib.rs` (read/edit/exec + Guard),
`crates/nh-tools/src/mcp.rs` (MCP client + OAuth2), `crates/nh-law/src/lib.rs` (Policy/Verdict),
`crates/nh-tui/src/lib.rs` (slash palette, Slice E), `crates/nh-fleet/src/lib.rs` (ledger, workers,
`status_from_ledger`), `crates/nh-mcp/src/lib.rs` (stateless server), `catalog.toml` (routes = data),
`05-ai-collaboration/AGENTS.md` (THE LAW), `00-start-here/CURRENT_TASK.md` (binding UX decisions).

The organizing thesis for this lens: **nosis already made its extensibility bet — "new capability =
new data file, never new code" (catalog.toml, law.toml, AGENTS.md). Every finding below extends that
one bet to tools, skills, agents, and provenance, so the product stays ONE idea instead of a feature
pile.** In 2026 the industry converged on exactly this shape (SKILL.md open standard, markdown
subagents, config-declared tools), which means nosis can adopt ecosystem-compatible formats instead
of inventing anything.

---

## Finding 1 — Adopt the Agent Skills open standard (SKILL.md) as the nosis skills + custom-command system

**What.** In 2026, skills consolidated into a single open standard: a folder with a `SKILL.md`
(YAML frontmatter `name` + `description` required; optional `license`, `compatibility`, `metadata`,
experimental `allowed-tools`), loaded via three-stage progressive disclosure — (1) name+description
only at startup (~100 tokens), (2) full body on activation, (3) bundled `references/`/`scripts/`
on demand ([agentskills.io/specification](https://agentskills.io/specification)). It was released
by Anthropic as an open standard and is now supported by ~40 tools including Claude Code, Codex,
Gemini CLI, Cursor, Copilot, OpenCode, Goose, and — directly relevant — Rust-first harnesses like
ZeroClaw and DeepSeek-focused "Deep Code" ([agentskills.io](https://agentskills.io)). Claude Code
**merged custom slash commands into skills** — `.claude/commands/deploy.md` and
`.claude/skills/deploy/SKILL.md` both create `/deploy`
([code.claude.com/docs/en/skills](https://code.claude.com/docs/en/skills)) — and OpenAI **deprecated
Codex custom prompts in favor of skills**
([developers.openai.com/codex/custom-prompts](https://developers.openai.com/codex/custom-prompts)).

**Why it matters for nosis.** (a) Interop for free: a user's existing Claude Code / Codex skills
drop into `.nosis/skills/` unchanged — huge adoption lever for a new harness. (b) Progressive
disclosure is *literally the KV-cache-first design*: the tiny name+description index lives in the
stable prefix (cache-warm across all turns); the body arrives late in context as an on-demand load,
so 30 skills cost ~3k prefix tokens, not 150k. No incumbent markets that connection; nosis can put
"skills are cache-native" in the brand story. (c) It replaces the need to build any bespoke
command-authoring system — the TUI slash menu (nh-tui/src/lib.rs:509-543, Slice E, Carlos-approved
2026-07-14/15) just gains entries from `.nosis/skills/*/SKILL.md` and `~/.nosis/skills/`.

**Smallest MVP.** New tiny module `nh_tools::skills` (or `nh-core`): walk two dirs, parse only
`name:`/`description:` frontmatter lines (hand-rolled, ~40 lines — no serde_yaml dep; spec fields
are flat strings), inject the index as one `## Skills` section of the constitution (nh-law already
assembles labeled sections — nh-law/src/lib.rs:12-16), and expose one built-in read-only tool
`use_skill {name}` that returns the SKILL.md body **as a tool result (data)**. `/name` in the TUI
= sugar that dispatches "use skill <name>: <args>". Ignore `allowed-tools` in v1 (spec marks it
experimental).

**Security / LAW.** A skill body is model-visible instruction text from a repo file — same trust
class as AGENTS.md, which nosis already loads; but unlike AGENTS.md it loads lazily, so pin it
(see Finding 9). `scripts/` never auto-execute: the model must call `exec_shell`, which always
passes the approval gate (nh-tools/src/lib.rs:263-274). Fits small/simple/modular/congruent —
skills are data files; harmonic — one slash surface for built-ins and user skills.

Sources: https://agentskills.io/specification · https://agentskills.io ·
https://code.claude.com/docs/en/skills · https://developers.openai.com/codex/custom-prompts ·
https://github.com/agentskills/agentskills

---

## Finding 2 — Built-in `grep_files` + `glob_files` (ripgrep-engine) read-only search tools

**What.** Nosis's tool surface is read/edit/exec only (nh-tools/src/lib.rs:310-312). Every 2026
agent standardizes on ripgrep-backed search: Claude Code's Grep tool is built on ripgrep, and
Codex's system prompt tells it to prefer `rg`
([codeant.ai](https://www.codeant.ai/blogs/why-coding-agents-should-use-ripgrep)). Warp's
Codex-integration writeup showed tool-name/dialect alignment with what models were trained on
measurably improves tool selection
([warp.dev](https://www.warp.dev/blog/codex-models-in-warp-apply-patch-and-prompting-changes)).

**Why it matters.** Today a nosis agent must either read whole files (token waste + KV-cache churn
— directly against differentiator 4) or shell out to `findstr`/`grep` via `exec_shell`, which (a)
triggers an approval per search — the exact approval fatigue nosis exists to fix, and (b) is
miserable on Windows `cmd /C` (nh-tools/src/lib.rs:276-279) — against the Windows-first wedge. A
read-only search tool is `Guard::Allow` by definition (no `Access::Write`/`Exec`), so it removes a
whole class of prompts while *reducing* spend: targeted matches instead of full-file reads keep the
prefix stable and the marginal context small. Cheapest-capable routing gets better because DeepSeek
Flash-class models do fine when handed precise snippets.

**Smallest MVP.** Two `Tool` impls in nh-tools using BurntSushi's `grep-searcher` + `ignore`
crates (the actual ripgrep engine, pure Rust, no child process — Windows-clean): `grep_files
{pattern, glob?, max_results}` returning `path:line: text` lines (hard-capped, e.g. 200 lines /
16KB), and `glob_files {pattern}`. Reuse `resolve_in_workdir` for containment; respect
`.gitignore` via `ignore`. Frozen-crate note: nh-tools is frozen — needs a CONTRACTS amendment
(additive, like A-M4-1) or land the tools in a new `nh-tools-ext` module re-exported at
registration in nh-tui/nh-cli.

**LAW.** Small (two ~80-line tools), lightweight (library, no subprocess), safe (read-only,
workdir-contained), congruent (fixes documented pain #1: approval fatigue). Tension: two new deps —
justified because they are the least-code path to a Windows-correct search.

Sources: https://www.codeant.ai/blogs/why-coding-agents-should-use-ripgrep ·
https://www.warp.dev/blog/codex-models-in-warp-apply-patch-and-prompting-changes ·
https://github.com/cortexkit/aft (2026 tool-harness survey: fast grep/glob as core agent moves)

---

## Finding 3 — `write_file` (file creation) — the gap the code already warns about

**What.** `EditFile` only mutates existing files; the agent cannot create a file except via an
approved shell redirect. nh-tools/src/lib.rs:128-130 already contains the load-bearing warning:
"any future file-creation tool must canonicalize or case-fold its guard path… or variants such as
`.GIT/x` and `.ENV` could bypass `.git/**` and `**/.env*`."

**Why it matters.** File creation is table stakes for a coding agent (scaffolding a test, a new
module). Doing it through `cmd /C echo … > file` is the worst path on Windows (quoting, encoding,
UTF-16 pitfalls — see the repo's own mojibake war stories in CURRENT_TASK.md) and burns an exec
approval each time. A first-class `write_file` goes through the same `Access::Write` guard, so
nh-law path policy (`write_block`/`write_ask`/`write_auto`, nh-law/src/lib.rs:88-104) governs it
uniformly — *more* auditable than shell redirects the law can't see into.

**Smallest MVP.** One `Tool` in nh-tools: `write_file {path, content}` — refuse if the file
exists (creation-only keeps `edit_file` the single mutation path; no overwrite ambiguity),
case-fold the guard-relative path per the existing warning, parent-dir creation contained by
`resolve_in_workdir`. ~60 lines + tests. Defer `apply_patch` (V4A/unified diff) to a later slice:
valuable for big refactors, but str-replace + create covers the 95% case and stays small — revisit
when a delegate route (Codex) lands, since Codex models are V4A-trained
([warp.dev](https://www.warp.dev/blog/codex-models-in-warp-apply-patch-and-prompting-changes)).

**LAW.** Small, secure (closes a documented future-bypass before it exists), congruent (one guard
for all writes). Frozen nh-tools → same amendment path as Finding 2.

Sources: https://www.warp.dev/blog/codex-models-in-warp-apply-patch-and-prompting-changes ·
repo: crates/nh-tools/src/lib.rs:128-130, 228-241

---

## Finding 4 — A `plan` tool that feeds three existing differentiators (not just UX)

**What.** Claude Code (TodoWrite) and Codex (update_plan) both give the model a structured plan
tool; the 2026 pattern is plan → validate → implement → review loops with the plan as the visible
spine ([developersdigest.tech loop-engineering guide](https://www.developersdigest.tech/blog/loop-engineering-definitive-guide)).

**Why it matters — the cohesion play.** In nosis, a plan is not decoration; it is the missing
*shared signal* for three shipped systems: (a) **thinking-budget governor** — step count/step type
is a better complexity prior than message length (plan with 8 steps → schedule `/effort high` on
the hard step only); (b) **KV-cache progressive compaction** — step boundaries are the natural,
semantically safe compaction points (compact everything before "step 3 done" into one receipt
line; the prefix stays stable); (c) **fleet** — plan steps marked independent are exactly what
`fleet_run` takes today (nh-mcp already exposes `fleet_run` with an echo-set of tasks). The TUI
gets the "ambiguous status" fix for free: render current step in the header/status strip.

**Smallest MVP.** One built-in tool `plan {steps: [{text, status}]}` (idempotent full-replace, like
TodoWrite), state held in the session struct, rendered by nh-tui as a 1-3 line strip; every update
appended to `.nosis/receipts.jsonl` (auditable). No persistence beyond receipts in v1; no
enforcement — it is a coordination artifact, and the LAW guard still governs each real action.
~100 lines core + render.

**LAW.** Small, simple, auditable (plan history is receipts), harmonic — one artifact that the
governor, compactor, fleet, and status line all read. Avoid the bloat trap: no plan "modes", no
plan file format, no separate planner model in v1.

Sources: https://www.developersdigest.tech/blog/loop-engineering-definitive-guide ·
https://aseemshrey.in/blog/claude-codex-iterative-plan-review/ ·
https://developers.openai.com/codex/cli/slash-commands

---

## Finding 5 — Surface the fleet interactively: `/fleet` in the TUI (sub-agents you can see)

**What.** As of July 2026, Claude Code runs subagents in the background *by default*, with
kill-keys and status surfaced in-session, and agent teams share a task list
([code.claude.com/docs/en/agent-teams](https://code.claude.com/docs/en/agent-teams),
[gradually.ai Claude Code changelog July 2026](https://www.gradually.ai/en/changelogs/claude-code/)).
Users now *expect* delegation to be a keystroke, not a separate CLI ceremony.

**Why it matters.** Nosis already has the hard 90%: nh-fleet's fsync-durable append-only ledger,
std-thread workers, idempotent resume, budget stop, escalation ladder, off-peak parking, plus
`run_with_id` / `read_run_ledger` / `status_from_ledger` seams added in M4 Slice C. But it is
reachable only via `nh fleet run` and MCP. Surfacing it as `/fleet` in the chat TUI turns nosis's
most differentiated machinery into a *felt* feature: "3 tasks parked until 18:00 CST (off-peak 2×
saving), 2 running on flash, 1 done — ¥0.14" in the status strip is the cost-transparency +
status-clarity demo in one line. No incumbent shows *why* a background task is waiting (clock
economics); nosis can.

**Smallest MVP.** Three slash entries reusing the existing palette (nh-tui/src/lib.rs:509-543):
`/fleet run <task>…` (spawn via existing `run_with_id` on a background thread), `/fleet status`
(render `status_from_ledger` + parked-until times from the scheduler), `/fleet stop <id>` (budget
stop already exists). One status-strip line while a run is live. No inter-agent chat, no shared
task list, no teams — the ledger stays the only coordination primitive (that's the LAW edge over
Claude Code's experimental teams).

**LAW.** Congruent (fleet, scheduler, budget already exist — this adds zero new machinery, only a
surface), auditable (everything already lands in the ledger), harmonic (one status language across
CLI, MCP, and TUI).

Sources: https://code.claude.com/docs/en/agent-teams ·
https://www.gradually.ai/en/changelogs/claude-code/ ·
https://www.developersdigest.tech/blog/claude-code-subagents-vs-agent-teams-vs-workflows ·
repo: 00-start-here/CURRENT_TASK.md (Slice A/C seams)

---

## Finding 6 — Agent definitions as data (`.nosis/agents/*.md`) with a nosis twist: declare a price ceiling, not a model

**What.** Claude Code subagents are markdown files with YAML frontmatter (`description`, `tools`
allowlist, `model`, `permissionMode`…) in `.claude/agents/`
([code.claude.com/docs/en/sub-agents](https://code.claude.com/docs/en/sub-agents);
[mindstudio.ai guide](https://www.mindstudio.ai/blog/build-custom-sub-agents-claude-code-yaml)).
The 2026 best practice is role-scoped minimal tools (reviewers = read-only, researchers = +web).

**Why it matters.** For nosis, copying `model:` frontmatter would *undermine* the product: routing
is the differentiator. The nosis-native version declares **constraints, not models**: `route_class:
cheapest-capable`, `max_price: 1.0 CNY/M`, `modality: text`, `effort: low`, `tools: read-only` —
and the RouteResolver picks the actual route at spawn time (off-peak-aware, cache-aware). That
makes user-authored agents portable across catalog updates and keeps "cheapest capable route" true
even inside user extensions — the definition of congruent. Tools-allowlist per agent maps cleanly
onto the existing `GuardFn` seam (nh-tools/src/lib.rs:29): a fleet worker for a read-only agent
gets a guard that blocks `Access::Write`/`Exec` outright.

**Smallest MVP.** Parse `.nosis/agents/<name>.md` (same tiny frontmatter parser as Finding 1);
fields: `description`, `tools` (`read-only` | `default`), `max_price_per_mtok`, `effort`. Fleet
`Backend::Native` accepts an optional agent profile; `/fleet run reviewer: <task>` uses it. The
agent body = the worker's system-prompt suffix (after the identity constitution — the 7faf44b
lesson says identity wrapping must apply at EVERY agent surface, including fleet workers).

**LAW.** Modular (agents are files), simple (4 fields), secure (least-privilege guards per agent),
harmonic with catalog.toml (constraints resolved by the same resolver).

Sources: https://code.claude.com/docs/en/sub-agents ·
https://www.mindstudio.ai/blog/build-custom-sub-agents-claude-code-yaml ·
https://www.tembo.io/blog/claude-code-subagents

---

## Finding 7 — User-defined command tools in TOML (`.nosis/tools.toml`) — extend the toolset without a plugin runtime

**What.** OpenCode lets users add custom tools, but as TypeScript files executed by its JS runtime
([opencode.ai/docs/custom-tools](https://opencode.ai/docs/custom-tools/)) — a whole language
runtime as plugin surface. The LAW-simple equivalent for a Rust harness: a **declared command
tool** — TOML entry with `name`, `description`, JSON-schema params, and a command template —
exactly how catalog.toml already declares routes ("catalog is data" is an AGENTS.md hard rule).

**Why it matters.** This closes the loop on "users extend nosis via data, not forks": routes =
TOML (shipped), skills = SKILL.md (Finding 1), agents = markdown (Finding 6), tools = TOML (this).
A user wires `cargo_test`, `gh_pr_view`, or a KORVIN script as a first-class tool the model can
call with typed args — no MCP server to run, no fork. Every such tool still executes through the
`Access::Exec` guard, so nh-law `exec_block` patterns and the always-Ask exec posture apply
unchanged (nh-law/src/lib.rs:107-117).

**Smallest MVP + the one security rule.** `[[tool]]` entries in `.nosis/tools.toml`; args are
passed **as argv items or `NH_ARG_*` env vars — never string-interpolated into a shell line**
(template injection is the classic failure; the prompt-injection design-patterns paper calls out
keeping the executed artifact fixed while data flows through parameters —
[arxiv.org/pdf/2506.08837](https://arxiv.org/pdf/2506.08837)). The approval prompt shows the exact
resolved argv. Optional `readonly = true` downgrades to `Guard::Allow` ONLY if the user set it and
the tool is hash-pinned (Finding 9). ~150 lines in nh-tools + registration.

**LAW.** Small (no runtime, no ABI, no dynamic loading), modular, congruent (TOML like the
catalog), auditable (declared surface, receipts log every call). Explicitly rejected alternative:
WASM/scripting plugins — bloat, new attack surface, against small/lightweight.

Sources: https://opencode.ai/docs/custom-tools/ · https://opencode.ai/docs/config/ ·
https://arxiv.org/pdf/2506.08837 · repo: 05-ai-collaboration/AGENTS.md ("catalog is data" rule)

---

## Finding 8 — `web_fetch` (keyless, guarded by a new `Access::Net` law seam)

**What.** Research agents need to read docs/issues; every 2026 harness ships web fetch. The 2026
security literature is equally clear on the failure mode: agent HTTP tools + prompt injection =
SSRF against metadata endpoints/localhost, and sloppy allowlists have been weaponized
(CVE-2026-22708 against Cursor; [futureagi.com SSRF/excessive-agency](https://futureagi.com/glossary/ssrf-excessive-agency-attack/);
[jsmon.sh prompt-injection→SSRF](https://blogs.jsmon.sh/prompt-injection-to-ssrf-exploiting-ai-agents-and-tool-calling/);
OWASP MCP cheat sheet: never fetch arbitrary URLs without strict validation —
[cheatsheetseries.owasp.org](https://cheatsheetseries.owasp.org/cheatsheets/MCP_Security_Cheat_Sheet.html)).

**Why it matters.** Without fetch, users leave nosis mid-task (context loss — documented pain #4).
With a *guarded* fetch, nosis extends its trust model instead of bolting on a hole: add
`Access::Net(&str)` as the third guard variant next to Write/Exec (nh-tools/src/lib.rs:16-19) so
network becomes a first-class law category — `net_block` (RFC1918/link-local/localhost/file: —
hard, non-negotiable), `net_ask` default, per-domain sticky session approval ("allow docs.rs this
session"). nh-core precedent already exists: the wire client refuses redirects to avoid key
exfiltration (nh-core/src/lib.rs:17-23) — same paranoia, same style. Fetched bytes are tool-result
DATA under the existing constitution rule, and the "Lethal Trifecta never assembled" posture
(MASTER_CONTEXT) gets a concrete enforcement point: web content in context is exactly when exec
approval must never be auto.

**Smallest MVP.** `web_fetch {url}` → text (HTML tag-stripped, 50KB cap), reqwest is already a
dependency; hard-coded private-range/scheme blocks + `Guard::Ask` per new domain. **No web_search
in v1** — search requires a provider key (Brave/Tavily/Exa; flag: keyRequired) and fetch alone
covers "read this URL/docs page," which is most of the value.

**LAW.** Secure (guarded, deny-by-default egress), small (one tool + one enum variant), congruent
(third access class completes the Write/Exec/Net triad — harmonic symmetry in the law itself).

Sources: https://cheatsheetseries.owasp.org/cheatsheets/MCP_Security_Cheat_Sheet.html ·
https://futureagi.com/glossary/ssrf-excessive-agency-attack/ ·
https://blogs.jsmon.sh/prompt-injection-to-ssrf-exploiting-ai-agents-and-tool-calling/ ·
https://arxiv.org/pdf/2506.08837

---

## Finding 9 — One provenance model for ALL extensions: `.nosis/extensions.lock` (hash-pinned skills, tools, MCP descriptions)

**What.** The defining MCP attack class of 2025-26 is tool poisoning / rug-pulls: a server ships a
benign tool description, then swaps in injected instructions after approval (Invariant Labs
disclosure — [invariantlabs.ai](https://invariantlabs.ai/blog/mcp-security-notification-tool-poisoning-attacks);
CVE-2025-54136 analysis — [truefoundry.com](https://www.truefoundry.com/blog/blog-mcp-tool-poisoning-gateway-defense)).
The standard mitigation: pin tool descriptions with a hash and re-approve on change. The same
applies to skills and command tools once Findings 1/7 land — a SKILL.md edited by a malicious PR
is instruction-injection with a delay fuse.

**Why it matters — cohesion.** Instead of three ad-hoc trust mechanisms, nosis gets ONE rule:
*anything that can inject instructions or run commands is content-addressed; first sight = show +
approve; any byte change = diff + re-approve.* This is THE LAW's "auditable" made mechanical, and
it becomes a marketable line: "nosis never lets an extension change behind your back." MCP trust
already exists (`McpTrust` in nh-tools/src/mcp.rs); this extends the concept to a persisted,
inspectable lockfile — same mental model as Cargo.lock, which every Rust user already trusts.

**Smallest MVP.** `.nosis/extensions.lock` (TOML): `sha256` per skill body, per tools.toml entry,
per MCP tool name+description+schema. On session start / tool-list refresh: compare, and route
mismatches through the existing approval UI with a short diff. SHA-256 via the `sha2` crate (tiny,
audited) or `ring` if already in-tree. ~120 lines.

**LAW.** Secure, auditable, small; harmonic — one trust story across every extensibility surface
in this report. This is the finding that keeps Findings 1, 6, 7 safe to ship.

Sources: https://invariantlabs.ai/blog/mcp-security-notification-tool-poisoning-attacks ·
https://www.truefoundry.com/blog/blog-mcp-tool-poisoning-gateway-defense ·
https://cheatsheetseries.owasp.org/cheatsheets/MCP_Security_Cheat_Sheet.html

---

## Deliberately rejected (LAW screen)

- **JS/WASM plugin runtime** (OpenCode-style TypeScript tools): a language runtime as attack+bloat
  surface; TOML command tools + MCP cover the need.
- **Agent teams / inter-agent messaging**: Claude Code's own version is experimental and
  flag-gated; nh-fleet's ledger is the simpler, auditable coordination primitive. Revisit post-M6.
- **Hooks system** (pre/post-tool shell hooks): powerful but a config-driven code-execution channel
  that fights the "exec always gated" invariant; skills + command tools deliver most of the value.
- **web_search with a paid provider**: real value, but needs a new key (Brave/Tavily) and fetch
  covers the core; park in IDEA_BANK with keyRequired flag.
- **apply_patch (V4A) now**: adopt only when a Codex delegate route lands; str-replace + write_file
  is in-distribution for the open-weight fleet nosis actually routes to.

## Sequencing sketch (smallest coherent slices)

1. Slice: `write_file` + `grep_files`/`glob_files` (Findings 3, 2) — one CONTRACTS amendment for
   frozen nh-tools, immediate daily-driver value.
2. Slice: skills standard + slash integration (Finding 1) + extensions.lock (Finding 9) shipped
   together — extensibility arrives WITH its trust model, never before it.
3. Slice: `plan` tool (Finding 4), then `/fleet` surfacing (Finding 5) — plan feeds fleet.
4. Slice: agents-as-data (Finding 6) + tools.toml (Finding 7).
5. Later: `Access::Net` + web_fetch (Finding 8) once the trifecta-interaction rule is written into
   bundled_law.toml.
