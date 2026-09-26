# Security Model

This describes the current source implementation, including changes after the
published v0.2.2 release. Check the [changelog](../CHANGELOG.md) for which fixes and
features your version contains. Historical or aspirational plans do not override
this file, `SECURITY.md`, or executable tests.

## Security Goals

- Never assemble the Lethal Trifecta: external input + secrets + state mutation without gates.
- Provider credentials use the OS vault and are redacted from application output.
  Cloud authentication headers are transmitted over HTTPS to the authorized origin;
  credentials must reach that provider. Explicit environment fallbacks and loopback
  HTTP have the boundaries described below.
- Every accepted model turn auditable through a typed local receipt.
- Constitution, policy, and approval boundaries are enforced in code and cannot be changed
  by model or tool text.
- No OS-level sandbox is claimed.

The unreleased `nh run --read-only` mode additionally restricts agent tools to guarded
local reads and searches. It disables editing, shell execution and remote tools for
that task. Normal provider requests and local receipt writes still occur. See
[read-only tasks](READ_ONLY.md) for its exact boundary.

## Access Control

Roles:

- Layered bundled/user/repository law. Repository policy can only tighten trust.
- Protected read/write paths hard-block at every autonomy level.
- On Windows, protective path rules also check ASCII case variants of the path
  and pattern. The more restrictive result wins. This covers existing uppercase
  files as well as ordinary lowercase names; it is not a claim to normalize every
  Unicode name or filesystem alias. Unix policy matching remains case-sensitive.
- Every non-blocked shell command requires explicit approval at the execution operation.
- Outbound MCP discovery/calls require `[send]` permission; repository MCP configuration
  cannot create a trusted destination or auto-trust a server.

Permissions:

- Tool outputs are data; they cannot approve their own shell or MCP action.
- Unreleased TUI approval uses dedicated F2/F3/F4 keys. Ordinary typing, pasted
  text and Enter stay in the composer rather than becoming permission choices
  when a request arrives. This does not authenticate injected terminal events or
  prevent a held function key from being reported as multiple fresh presses.
  Unreleased plain run/chat require an exact fresh eight-character code generated
  from OS randomness for each approval. Pretyped `y/yes`, stale codes, nonterminal
  input and randomness failure decline. This reduces accidental typeahead approval;
  the 32-bit code is not user authentication or protection against terminal control.
  A nonmatching input line is consumed as a declined response.
- TUI session-repeat rules require identical, untransformed shell approval text
  explicitly labeled by the shell tool. Scrubbed/escaped prompts, file actions and
  all MCP prompts are ineligible both when saving and matching a rule. MCP argument
  summaries and text from another tool are not treated as shell identities.
- Tool-context construction requires an explicit policy guard and output scrubber.
  Production callers supply their session policy; this prevents an implicit
  permissive default, but does not make caller-provided policy inherently safe.
- MCP responses, provider responses, task fields, tool results, and receipt reads are bounded.
- `nh-mcp` accepts at most four active Fleet runs, clamps workers to the configured ceiling,
  requires a bounded positive token budget, and accepts at most 256 tasks per run.
- MCP Apps and the MCP Tasks extension are not implemented.

## Cancellation, retries and budgets

- Cancellation is checked after a provider response, before each tool, and again before
  file mutations, shell launch or an MCP tool request. A response that arrives after
  cancellation cannot authorize another tool action. Work already completed is not rolled back.
- An in-flight provider request can still finish after cancellation. Its reported usage is
  retained because the provider may bill it. The answer is retained in the conversation,
  with cancelled results for tool calls that were skipped. Cancellation does not promise an immediate
  network abort or a refund.
- The first provider attempt retains its 600-second timeout for long reasoning requests.
  Later attempts share the remainder of a 45-second window measured from the start of the
  call. A slow first attempt can exhaust that window without being cut short by it.
- Retry waits do not shorten a valid provider `Retry-After` delay. If the wait cannot fit in
  the remaining retry window, the call stops. Request timeouts and incomplete response bodies
  are not retried because their billing outcome may be unknown.
- The TUI token budget is an observed-usage stop for new tasks. The active task can exceed
  it. A budgeted session stops new dispatch when usage is unknown or incomplete. It is not
  a guaranteed token ceiling or a monetary cap.
- New TUI sessions save whether a budget was set and preserve that choice on resume. Older
  TUI ledgers without this setting cannot be resumed; start a new session and choose a budget.
  An incomplete final record makes restored usage uncertain. A crash can lose in-flight
  usage before any record is written, so saved totals are not a provider billing statement.
- Process termination is best effort. If process-tree termination fails but the shell exits,
  the result warns that descendants may survive. Reaping the shell alone does not prove that
  all child processes stopped.

## File publication and concurrent changes

`write_file` stages and syncs the complete content, then publishes it without replacing an
existing destination. If another process creates that destination first, its file is preserved.
The filesystem must support hard links; otherwise creation stops with an error. On Windows,
use an NTFS project directory. There is no fallback to an operation that can overwrite a file.

`edit_file` compares the current content and metadata with the file it read before publishing
its replacement. A detected change stops the edit so you can read the current file and retry.
This is a best-effort conflict check: another writer can still change the file between that
check and replacement. Avoid editing the same file simultaneously in different programs.
Edits replace the file with staged content. Ordinary Unix read/write/execute permissions are
preserved; special mode bits are not guaranteed. The replacement does not preserve arbitrary
filesystem metadata: Windows explicit access-control entries and alternate data
streams are not copied to the replacement.

Path checks are not an operating-system sandbox. A process able to replace parent directories,
symlinks, or junctions can race path-based operations. Handle-relative protection against those
races is not implemented. File syncing also does not guarantee recovery from every power loss
or storage failure.

## Secrets

Where secrets live:

- nh-vault → OS-native store (Windows Credential Manager, macOS Keychain, Linux Secret
  Service) via `keyring`.
- `NH_<ENTRY>_KEY` is an explicit CI/headless fallback and remains outside the OS store.
- Outbound MCP OAuth/API credentials use the same zeroizing secret type.

How secrets are accessed:

- One shared connection boundary checks HTTPS plus exact scheme/host/effective port before
  reading only the active route's key. Literal loopback HTTP is the only transport exception.
- Active and previously active session credentials stay in a zeroizing registry only as long
  as needed to redact later output. They are zeroized when their owners drop.
- No delegate child-CLI credential path ships.

What must never be logged:

- Known key shapes and active literal values. The canonical shape registry is shared by the
  runtime scrubber and generated Git hook.
- Application-controlled terminal, receipt, tool-result, and MCP-result paths pass through
  redaction and control-character escaping.
- Outbound MCP headers are linted for secret shapes; credentials are limited to the
  authorization mechanism selected by trusted configuration.

## Audit Logs

Log:

- JSONL receipt per accepted turn: route/model, turns, tool calls, outcome, validated usage,
  and effective profile.
- Fleet ledger: append-only, typed receipts (pass/fail/partial/skip/timeout).
- There is no Nosis-operated telemetry or OpenTelemetry exporter.

A normal receipt outcome records completed execution, not independently checked
correctness. Source-build progress can show actual tool outcomes, but those facts
are not added to historical receipt schemas. Do not infer passed checks from an
assistant's answer or from a receipt's `pass` value.

## Security Checklist

- Can this leak data? (scrubber on every output path?)
- Can this expose secrets? (exact origin before materialization, zeroizing ownership, header lint?)
- Can this action be abused via tool output? (data, never instructions?)
- Is the action logged? (receipt with handles?)
- Can a required receipt fail without changing the reported outcome?
- Git guard: `nh init` writes `.nosis/.gitignore` covering the runtime artifacts (`receipts.jsonl`, `fleet/`, `sessions/`, `observations/`, `efficiency-v1.jsonl`, `*.log`, `auth*`). Rerunning it adds missing entries while preserving custom ones. The directory itself is not ignored, so a repository `law.toml` or `mcp.toml` can still be committed and reviewed. A pre-commit hook blocks files matching secret patterns (installed by `nh init`).

Opt-in [efficiency experiments](EFFICIENCY.md) can retain scrubbed tool output and
extractive history temporarily under `.nosis/observations`. Handles are bound to
one running session; retrieval verifies stored bytes and cannot authorize another
action. This is not encryption: Windows inherits directory permissions, while
Unix session files use owner-only modes. Normal exit attempts narrow cleanup;
crashes or filesystem errors can leave scrubbed project text behind. Measurement
JSONL contains usage metadata rather than task text. These features default off.

- Existing user hooks are preserved and produce an actionable manual-chaining warning.
- The MCP preview stays bearer-guarded and loopback-only; Host and Origin checks fail closed.

Unreleased outbound MCP clients reject configured literal link-local destinations
before startup discovery or OAuth token exchange: IPv4 `169.254.0.0/16` (including
the common `169.254.169.254` metadata endpoint), IPv6 `fe80::/10`, and IPv4-mapped
IPv6 forms of the blocked IPv4 range. Loopback and ordinary LAN destinations remain
available. This is not a complete inventory of cloud metadata endpoints, DNS
filtering, rebinding protection or operating-system network isolation.
