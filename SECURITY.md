# Security Policy

This document tells you how to report a security problem in nosis and shows the security model of the `nh` tool.

## Supported Versions

nosis is in development and is before version 1.0. We apply security fixes to the latest release and to the `main` branch only. Older releases do not get security fixes.

| Version            | Security fixes |
| ------------------ | -------------- |
| Latest release     | Yes            |
| `main` branch      | Yes            |
| All older releases | No             |

## How to Report a Vulnerability

Do not open a public issue for a security problem. Do not put security data in a public discussion or a pull request. Send a private email to `info@nosistech.com`.

We send a first reply in 5 business days or less. If you do not get a reply, send the email again after 5 business days.

Include this data in your report:

- The steps that cause the problem.
- The version or the commit that shows the problem.
- The effect of the problem on users or on data.

## Security Model (Summary)

nosis is a local agent harness for Windows. It routes each task to a capable model with the lowest cost. It gives you a receipt for each turn. The file `02-architecture/SECURITY_MODEL.md` in this repository contains the full security model.

### The law and verdict classes

A local law file controls each tool action. The law gives a verdict for each action. The verdict classes are `[read]`, `[send]`, and `[credential]`. A verdict is Allow, Ask, or Block. This defense prevents the "Lethal Trifecta": external input, plus secrets, plus a state change, together without a gate. Shell commands stay behind an approval gate.

### Secret audience binding

The law class `[credential]` binds each secret to a list of approved hosts. The broker compares the host of each request with this list. If the host is not in the list, the broker refuses the request. The secret does not leave the vault. Thus a repo configuration cannot send a secret to a different host. If the list of approved hosts is empty, the broker also refuses the request.

### Secret storage

The vault keeps secrets in the store of the operating system. On Windows, this store is the Windows Credential Manager. nosis reads a secret only at request time. The secret stays in memory only. nosis sets the memory of the secret to zero after use.

### The Scrubber

The `Scrubber` examines each output path. Output paths include the terminal, the logs, the receipts, and all outbound tool results. The `Scrubber` finds text that has the shape of a secret. It also finds the secret values that are in the local vault. It replaces each match with the text `[REDACTED]`. Each tool result also has a size limit.

### Fail-closed defaults

When a safety check cannot complete, nosis refuses the action. nosis does not continue with a default permission when a check fails.

### The local MCP preview server

The MCP preview server binds only to the local address 127.0.0.1. The server makes a bearer token from a secure random source (CSPRNG). The server compares the bearer token in constant time. The server examines the Host header and the Origin header. These checks stop DNS-rebind attacks. The server refuses a request that has no valid bearer token. The server refuses a request that has an incorrect Host or Origin.

### Audits

An internal security audit examined the full codebase in July 2026. The audit found no critical problems. The project repairs the other findings in planned waves.

## Scope

In scope:

- The `nh` binary and all crates in this repository.
- The local MCP preview server.

Out of scope:

- The third-party model providers: DeepSeek, Kimi (Moonshot), MiMo (Xiaomi), and GLM (Z.ai).
- Your procedures for your secrets outside the vault.
- The content of the prompts that you send.

nosis sends your prompts to the model provider that you select. The provider processes your prompts on its servers. Read the privacy policy of each provider. Report a problem in a provider's service to that provider.

## Coordinated Disclosure

We welcome good-faith security research. Obey these rules when you do research on nosis:

- Do not read, copy, or remove data that is not yours.
- Do not decrease the availability of a service.
- If you find a secret, report it and do not use it.
- Give the project a reasonable time for a repair before a public disclosure.

If you obey these rules, we do not start legal action against your research. When the repair is available, you can publish your findings. We coordinate the time of the public disclosure with you.
