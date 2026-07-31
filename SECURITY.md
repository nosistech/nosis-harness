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

Do not open a public issue for a security problem. Do not put security data in a public discussion or a pull request.

**Use GitHub private vulnerability reporting. It is the primary channel.** The feature is enabled on this repository. Do one of these:

- Go to <https://github.com/nosistech/nosis-harness/security/advisories/new>.
- Or open the **Security** tab of the repository, then select **Report a vulnerability**.

A GitHub account is necessary. Your report stays private. Only the maintainers can read it until we publish an advisory.

If you cannot use GitHub, send an email to `info@nosistech.com`. **This mailbox is best-effort.** We do not monitor it continuously, and a reply can be slow or can fail to arrive. Use GitHub private reporting for all urgent problems.

We try to send a first reply in 5 business days. A GitHub report gets a reply more quickly than an email.

Include this data in your report:

- The steps that cause the problem.
- The version or the commit that shows the problem.
- The effect of the problem on users or on data.

## Security Model (Summary)

nosis is a local terminal agent. The operator selects the execution route. The separate `nh why` command estimates the cheapest capable route from trusted catalog data. Accepted model turns produce local receipts. The file `02-architecture/SECURITY_MODEL.md` contains the detailed security model.

### The law and verdict classes

A layered law controls read, write, send, credential, and shell actions. A verdict is Allow, Ask, or Block. Repository policy can restrict trusted user policy but cannot widen it. Every non-blocked shell command still requires explicit human approval at the execution boundary. Command-block patterns unwrap common shell and environment launchers as a defense in depth; they are not an operating-system sandbox.

### Secret audience binding

The `[credential]` law binds each vault entry to approved origins. Before reading a key, the shared connection boundary compares the exact scheme, host, and effective port. Remote origins require HTTPS; literal loopback addresses have a narrow HTTP exception. An empty, malformed, downgraded, or unapproved origin is refused before the key is materialized. After authorization, the key exists in process memory and is sent in the provider authorization header to that approved origin.

### Secret storage

The primary store is the operating-system credential store. The optional `NH_<ENTRY>_KEY` environment fallback is for CI and headless use. Active secrets use zeroizing application-owned buffers. Interactive sessions retain only credentials that have been active so later output can redact them, then zeroize those buffers when the session ends. Third-party HTTP and operating-system libraries can make transient internal copies that `nh` cannot prove were zeroized.

### The Scrubber

The `Scrubber` covers application-controlled terminal, receipt, tool-result, and MCP-result paths. It matches known key shapes and the literal values active in the current session. It does not bulk-read every catalog credential. Literal values are stored in zeroizing buffers and matched longest-first. Tool and remote response paths have explicit size limits.

### Fail-closed defaults

When a safety check cannot complete, nosis refuses the action. nosis does not continue with a default permission when a check fails.

### The local MCP preview server

The MCP preview server binds only to the local address 127.0.0.1. The server makes a bearer token from a secure random source (CSPRNG). The server compares the bearer token in constant time. The server examines the Host header and the Origin header. These checks stop DNS-rebind attacks. The server refuses a request that has no valid bearer token. The server refuses a request that has an incorrect Host or Origin.

### Audits

Two internal reviews are part of the public record. The first full audit on 2026-07-20 reported no critical findings. A stricter pre-release audit on 2026-07-21 then found **2 critical and 14 high** findings and correctly judged that tree not releasable. The current release candidate includes the Slice G remediations, including exact-origin credential authorization for C-01 and symlink-rejecting, contained, 64 KiB-bounded constitution reads for C-02, with regression tests. The original report remains available at `04-research/AUDIT_2026-07-21_sol-max_pre-release.md`; the release checklist still requires the complete gate and manual release checks before a tag.

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
