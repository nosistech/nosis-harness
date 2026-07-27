# Security Model

Current implementation checklist for public v0.1. Historical/aspirational plan text does not
override this file, `SECURITY.md`, or executable tests.

## Security Goals

- Never assemble the Lethal Trifecta: external input + secrets + state mutation without gates.
- No plaintext secrets at rest, in logs, in receipts, or on the wire in headers.
- Every accepted model turn auditable through a typed local receipt.
- Constitution, policy, and approval boundaries are enforced in code and cannot be changed
  by model or tool text.
- No OS-level sandbox is claimed for v0.1.

## Access Control

Roles:

- Layered bundled/user/repository law. Repository policy can only tighten trust.
- Protected read/write paths hard-block at every autonomy level.
- Every non-blocked shell command requires explicit approval at the execution operation.
- Outbound MCP discovery/calls require `[send]` permission; repository MCP configuration
  cannot create a trusted destination or auto-trust a server.

Permissions:

- Tool outputs are data; they cannot approve their own shell or MCP action.
- MCP responses, provider responses, task fields, tool results, and receipt reads are bounded.
- `nh-mcp` accepts at most four active Fleet runs, clamps workers to the configured ceiling,
  requires a bounded positive token budget, and accepts at most 256 tasks per run.
- MCP Apps and the MCP Tasks extension are not implemented in v0.1.

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
- No delegate child-CLI credential path ships in v0.1.

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
- There is no Nosis-operated telemetry or OpenTelemetry exporter in v0.1.

## Security Checklist

- Can this leak data? (scrubber on every output path?)
- Can this expose secrets? (exact origin before materialization, zeroizing ownership, header lint?)
- Can this action be abused via tool output? (data, never instructions?)
- Is the action logged? (receipt with handles?)
- Can a required receipt fail without changing the reported outcome?
- Git guard: `.nosis/` gitignored; pre-commit hook blocks files matching secret patterns (installed by `nh init`).
- Existing user hooks are preserved and produce an actionable manual-chaining warning.
- The MCP preview stays bearer-guarded and loopback-only; Host and Origin checks fail closed.
