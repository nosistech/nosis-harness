# Architecture Overview

## Current Architecture

A nine-crate Rust workspace. The operator selects one direct API route or one explicitly
configured local route for execution. Only `nh-routes::RouteResolver` can mint a
`ResolvedRoute`; it carries the trusted endpoint, wire protocol, model ID, limits, modality,
thinking dialect, and catalog price data. Local routes are OpenAI-compatible and loopback-only;
they are excluded from cheapest-capable advice and cost anchors. The schema can parse delegate
routes, but no delegate execution backend ships in v0.1.

## Main Components

- `nh-cli`: command parsing and the `init`, `key`, `run`, `chat`, `why`, `profile`, `tui`,
  `fleet`, and loopback `mcp serve` surfaces.
- `nh-tui`: ratatui frontend, with terminal lifecycle and worker ownership extracted into
  dedicated modules. Its timeline is view/inspect only; snapshot restore is not implemented.
- `nh-core`: OpenAI/Anthropic wire clients, turn loop, context compaction, receipts, the
  shared credentialed-connection boundary, and contained runtime paths.
- `nh-routes`: trusted catalog parsing, non-forgeable routes, profiles, capability checks,
  price calculation, and cheapest-capable advice.
- `nh-law`: layered constitution and policy loading. Repository inputs are symlink-rejected,
  contained, and size-bounded.
- `nh-tools`: bounded read/edit/exec tools plus the outbound MCP client. Shell execution
  requires explicit approval and receives a minimal environment.
- `nh-vault`: OS-native storage, exact-origin credential authorization, zeroizing secret
  ownership, and output redaction.
- `nh-fleet`: bounded task validation, worker ownership, append-only JSONL ledger, locking,
  budgets, escalation, and idempotent resume.
- `nh-mcp`: stateless, bearer-guarded, loopback-only preview server with bounded request and
  Fleet concurrency limits.

## Internal Responsibility Boundaries

Crate roots expose the existing public API and retain only top-level orchestration or a small
shared import boundary. Production responsibilities live in named modules:

- `nh-cli`: one command module per user-facing command; `cmd_run::{config,meter}` isolate trusted
  configuration assembly and receipt projection, chat startup is isolated from its REPL, and
  large command test suites live beside, but outside, their production modules.
- `nh-tui`: `input` owns event reduction while `input::commands` owns slash commands; `render`
  owns frame components while `render::transcript` owns projection and wrapping; `session`,
  `state`, `terminal`, `timeline`, and `worker` each retain one lifecycle concern.
- `nh-core`: `agent`, `credential`, `receipt`, `runtime_path`, and `wire`. `agent::context` owns
  cache-safe prefix sealing, estimation, and compaction. The wire facade owns shared types and
  route-to-client construction; `wire::{http,openai,anthropic}` isolate common HTTP safety limits
  and provider-specific encoding.
- `nh-routes`: `pricing`, `profiles`, `resolver`, and `route`. `ResolvedRoute` is defined inside
  `resolver`, keeping its private construction boundary colocated with `RouteResolver`;
  `resolver::catalog` owns untrusted TOML parsing and validation.
- `nh-law`: `load` for layered parsing/compilation, `matcher` for pure matching, and `model` for
  policy types and read-only views.
- `nh-tools`: `exec` for process execution and `mcp::{adapter,client,config}` for outbound MCP;
  `mcp::client::oauth` owns token lifetime and refresh state, while the crate root owns the
  bounded built-in read/edit/tool facade.
- `nh-fleet`: `engine` owns workers and durable I/O, `scheduler` owns task state transitions,
  and `ledger`, `model`, and `prepare` own persistence, public types, and validated setup; the
  crate root owns run/resume orchestration.
- `nh-mcp`: `protocol`, `route_tools`, `receipts`, `fleet_tools`, and `response`; the crate root
  owns authenticated loopback transport and shutdown.
- `nh-vault`: one cohesive production module for the OS key store, secret ownership, audience
  checks, and scrubber; its test suite is isolated from production code.

## Data Flow

1. The CLI loads bundled/user/repository policy. Repository policy may only tighten trust.
2. A trusted catalog is accepted only when it matches the bundled catalog or an exact
   operator-reviewed user-global copy.
3. The operator's route is resolved. The shared credential boundary validates its exact
   origin before materializing only that route's key.
4. `nh-core` sends the stable constitution plus dynamic conversation data to the selected
   provider. Read, edit, shell, and MCP tools pass through policy and approval boundaries.
5. Provider-reported usage is validated and written to a local typed receipt. Fleet adds a
   locked append-only run ledger. Required-receipt failure cannot be reported as success.

## Deployment Shape

`nh` is a local CLI, not a hosted multi-user service. Public v0.1 is a source-install release.
Windows is the only platform executed locally so far. CI is configured for Windows, Linux,
and macOS, but those remote jobs cannot run until the repository has a public remote.

There is no OS-level sandbox in v0.1. Containment is policy-level: workspace path checks,
protected-file holds, exact-origin credential audiences, minimal child environments, explicit
shell approval, time/output bounds, and verified best-effort process-tree termination. The
MCP server cannot bind outside `127.0.0.1` and is not a public network service.
