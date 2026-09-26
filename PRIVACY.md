# Privacy

Nosis Harness (`nh`) is a local terminal agent. This document states exactly what data
leaves your machine, what stays local, and what `nh` does not do.

## What leaves your machine

For a model turn, `nh` sends the selected provider the system instructions, conversation
history, task text, tool definitions, and tool results needed for that request. Provider
routes use TLS and go directly from your machine to the exact catalog origin approved for
that credential; `nh` adds no Nosis-operated intermediary.

A configured local route connects to a loopback endpoint. That server can forward requests
to a cloud service. `nh` cannot establish the final execution location or billing policy from
the loopback address. Check the server's model and cloud settings before sending private data.

Two other features can create network traffic:

- MCP discovery and tool calls go to endpoints that the operator placed in user-global
  configuration and that the local `[send]` policy permits. Repository configuration can
  restrict those endpoints but cannot introduce a trusted destination.
- A shell command that you explicitly approve can perform any network activity that the
  command itself implements. `nh` does not proxy or inspect that traffic.

## Telemetry

`nh` does **not** phone home. There are no analytics, no usage beacons, and no crash
reporting to NosisTech. Provider and MCP services can keep their own server-side logs under
their own policies.

## Local data

- **API keys** are stored in the OS-native vault via `nh key add <entry>` - Windows
  Credential Manager, macOS Keychain, or Linux Secret Service. They are never echoed and
  are not written to files by `nh`. For CI or headless use, the env fallback is
  `NH_<ENTRY>_KEY` (uppercased entry). Active credentials are held in zeroizing
  application-owned buffers for the request or session so later output can still be
  redacted.
- **Receipts** - the cost/usage ledger - are written locally to `.nosis/receipts.jsonl`
  and are not automatically uploaded by `nh`.
- **Fleet state** lives under `.nosis/fleet/`.
- **Remembered model (unreleased)** is an opt-in route ID in `~/.nosis/model`,
  shared across projects. It contains no credential, endpoint or permission grant.
  `nh model clear` removes the preference; explicit `--model` overrides it.
- **Catalog migration backups (unreleased)** stay beside the project catalog as
  `catalog.toml.nh-backup-*`. They contain the recognized older bundled catalog,
  not API keys. Keep them until you have verified the upgrade.
- **Session transcripts** for `nh chat` and `nh tui` are written locally to
  `.nosis/sessions/<id>.jsonl` so that an interrupted session can be resumed. They contain
  the conversation itself, not only its cost. The same redaction is applied before each
  append. They are not uploaded by `nh`.
- **Retention** is operator-controlled. Receipt, session and Fleet records are append-only
  and are not automatically pruned; they grow until the operator deletes them. Other than the
  configuration and Git hook created by an explicit `nh init`, `nh` creates no source-code
  or cache artifacts of its own.
- **Redaction** is applied to application-controlled terminal, receipt, tool-result, and
  MCP-result paths using known key shapes plus active literal credentials. Redaction lowers
  risk but is not a reason to put secrets in prompts or task text.
  It does not remove all confidential code, personal information or other sensitive content
  from saved conversations. Review transcripts before sharing them.

## Delete local data

- Run `nh key remove <entry>` to remove an OS-vault entry. If you used
  `NH_<ENTRY>_KEY`, unset it in the environment or CI secret store separately.
- Delete the repository's `.nosis/receipts.jsonl`, `.nosis/sessions/` and `.nosis/fleet/`
  to remove local run history and saved conversations. Deleting only `receipts.jsonl`
  leaves your `nh chat` and `nh tui` transcripts in `.nosis/sessions/`. These are ordinary
  local files; NosisTech has no server-side copy.

## The MCP preview

`nh mcp serve` binds **loopback (`127.0.0.1`) only** and is guarded by a bearer token.
The MCP server is a **preview** and **must not be exposed on a public interface**. This is
not a restriction that lapses on a date.

When the server generates a token, it prints that token for the local client to
use. Treat it as a live credential: omit it from screenshots, shared terminal
logs and support reports. A token loaded from a vault entry is not printed.

## Provider policies

Each provider has its own data-retention policy. `nh` does not control provider-side
retention. The same applies to MCP services and programs launched through approved shell
commands. Consult each service's policy before sending sensitive material.

## Contact

Privacy questions: **info@nosistech.com**. For security vulnerabilities, see
[SECURITY.md](SECURITY.md).
