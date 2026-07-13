# Security Model

Source of truth: plan §A.8 (nh-vault), §2 (prompt-injection posture), §4.5 (MCP security). This file is the checklist form.

## Security Goals

- Never assemble the Lethal Trifecta: external input + secrets + state mutation without gates.
- No plaintext secrets at rest, in logs, in receipts, or on the wire in headers.
- Every risky action auditable (receipts) and reversible (side-git snapshots).
- Constitution + approval + sandbox enforced in code - never overridable by model text.

## Access Control

Roles:

- Trust dial per-path/per-command, compiled from `.nosis/law.toml`: e.g. `src/**` auto-approve edits, `migrations/**` always ask, `rm|curl|ssh` always ask, protected paths hard-block even in max autonomy.

Permissions:

- Tool outputs are always data; a poisoned tool result can never auto-approve its own exec. State-mutating MCP calls route through the trust dial regardless of autonomy level.
- Task creation (MCP Tasks ext) is rate-limited - cheap for client, expensive for server = DoS vector.
- MCP App HTML is untrusted display-only in v1 (stored-XSS surface).

## Secrets

Where secrets live:

- nh-vault → OS-native store (Windows Credential Manager/DPAPI, macOS Keychain, Linux secret-service) via the `keyring` crate. Delegates hold OAuth tokens, not keys; claude/codex manage their own token files - harness never touches them.

How secrets are accessed:

- Read at request time, injected per-call into the provider client or child-CLI env, memory-only, zeroized after use. Per-route scoping: a DeepSeek call can never read the Kimi key.

What must never be logged:

- Key material (`sk-`, `csk-`, JWT shapes) and the literal values currently in vault - a redaction scrubber sits on every output path (TUI, logs, receipts, MCP responses). Leaked key in a stack trace = failing test.
- Secrets/PII in `Mcp-Method`/`Mcp-Name`/`x-mcp-*` headers - outbound header lint (Akamai leak vector).

## Audit Logs

Log:

- JSONL receipt per turn: route, cost, tool calls, MCP state handles, failure classification.
- Fleet ledger: append-only, typed receipts (pass/fail/partial/skip/timeout).
- W3C Trace Context through MCP calls → one OpenTelemetry span tree.

## Security Checklist

- Can this leak data? (scrubber on every output path?)
- Can this expose secrets? (vault-only, zeroized, header lint?)
- Can this action be abused via tool output? (data, never instructions?)
- Is the action logged? (receipt with handles?)
- Can it be rolled back? (side-git snapshot addressable from timeline?)
- Git guard: `.nosis/` gitignored; pre-commit hook blocks files matching secret patterns (installed by `nh init`).
- Strict MCP header validation; reject ambiguous framing (request-smuggling/desync).
