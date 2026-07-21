# Privacy

Nosis Harness (`nh`) is a local terminal agent. This document states exactly what data
leaves your machine, what stays local, and what `nh` does not do.

## What leaves your machine

Your prompts and task text are sent **only** to the model provider you explicitly select -
for example DeepSeek (`api.deepseek.com`), Moonshot/Kimi (`api.moonshot.ai`), Xiaomi/MiMo
(`api.xiaomimimo.com`), or Z.ai/GLM (`api.z.ai`) - over TLS. `nh` adds no intermediary:
requests go directly from your machine to the provider's API.

## Telemetry

`nh` does **not** phone home. There are no analytics, no usage beacons, and no crash
reporting to nosis. The only network egress is your chosen provider's API.

## Local data

- **API keys** are stored in the OS-native vault via `nh key add <entry>` - Windows
  Credential Manager, macOS Keychain, or Linux Secret Service. They are never echoed and
  never written to files. For CI or headless use, the env fallback is `NH_<ENTRY>_KEY`
  (uppercased entry).
- **Receipts** - the cost/usage ledger - are written locally to `.nosis/receipts.jsonl`
  and are never uploaded.
- **Fleet state** lives under `.nosis/fleet/`.
- **Secrets** are scrubbed from all logs and tool output.

## The MCP preview

`nh mcp serve` binds **loopback (`127.0.0.1`) only** and is guarded by a bearer token.
The MCP server is a **preview** and must not be exposed publicly before the MCP final
spec lands on **2026-07-28**.

## Provider policies

Each provider has its own data-retention policy. `nh` does not control provider-side
retention - consult your provider's policy before sending sensitive material.

## Contact

Privacy questions: **info@nosistech.com**. For security vulnerabilities, see
[SECURITY.md](SECURITY.md).
