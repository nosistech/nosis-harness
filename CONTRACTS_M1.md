# CONTRACTS_M1.md — Locked public API for Milestone M1

**Status: LOCKED.** Owner: M1 architect. Builders implement EXACTLY these surfaces;
private helpers are free, public deviations are not. Spec source:
`NOSIS_HARNESS_Master_Plan.md` §6 (M1), §3 (routing brain), §4.5 (MCP 2026-07-28),
Appendix A (providers), Appendix B (catalog — supersedes earlier tables).
Amendments to this file go through the architect only.

---

## 0. Ground rules (bind every builder)

- **M0 stays green.** All M0 public APIs of nh-vault / nh-tools / nh-routes /
  nh-core / nh-cli remain source-compatible except the explicit amendments in §5.2.
- **Banned model strings** (never emitted anywhere, incl. tests/docs):
  `deepseek-chat`, `deepseek-reasoner`, `mimo-v2-<x>` prefix (`mimo-v2.5*` is fine),
  `gpt-5.2*`, `gpt-5.3-codex`, `moonshot-v1-*`. Enforcement lives in nh-routes.
- **Catalog/pricing is DATA** in `catalog.toml`. Honest-cost rule: stale/uncertain
  prices are flagged (`price_confidence`, `valid_until`), never guessed.
- **No plaintext secrets**; every output path passes `nh_vault::Scrubber`;
  exec and state-mutating MCP calls go through the approval gate — no exceptions.
- **UX IS THE PRODUCT**: every user-facing line short, concrete, actionable;
  errors say what to do next; no debug dumps.
- Verification before handoff, per crate you touched:
  `cargo test -p <crate>` and `cargo clippy -p <crate> --all-targets -- -D warnings`,
  then `cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings`.

**M1 exit criteria (plan §6, §4.5):** `/model` and `/provider` switch mid-session;
peak/off-peak price shown correctly; one MCP tool call against a stateless
2026-07-28 server with a returned handle passed back on the next call and **no
session header anywhere on the wire**.

---

## 1. nh-routes — DONE by the architect (reference surface, do not modify)

Already implemented and green. Builders consume this; they do not change it.

```rust
pub enum Wire { OpenAi, AnthropicMessages }
pub enum RouteClass { Api, Delegate }
pub enum ThinkingDialect { DeepseekNhm, AlwaysThinking, GlmHm, None } // .as_str()
pub enum Currency { Cny, Usd }                    // .as_str(), Display: "CNY"/"USD"
pub enum PriceConfidence { Confirmed, Reported, VerifyLive } // Display: catalog words

pub struct PeakWindows { pub multiplier: f64, pub timezone: String,
                         pub utc_offset_secs: i32, pub windows: Vec<(NaiveTime, NaiveTime)> }
pub struct RoutePrice  { pub currency: Currency, pub cache_hit: f64, pub cache_miss: f64,
                         pub output: f64, pub confidence: PriceConfidence,
                         pub valid_until: Option<NaiveDate>, pub peak: Option<PeakWindows> }
pub struct PriceQuote  { pub cache_hit: f64, pub cache_miss: f64, pub output: f64,
                         pub currency: Currency, pub peak: bool,
                         pub confidence: PriceConfidence, pub stale: bool }

pub struct ResolvedRoute {
    pub id: String,            // catalog key, may differ from model_id (…-anthropic)
    pub provider: String,
    pub model_id: String,
    pub base_url: String,
    pub wire: Wire,
    pub vault_entry: String,
    pub class: RouteClass,
    pub modality: Vec<String>, // validated subset of text|image|video|audio
    pub context: Option<u64>,
    pub max_out: Option<u64>,
    pub thinking_dialect: ThinkingDialect,
    pub preserve_reasoning: bool,
    pub quirks: Vec<String>,
    pub price: Option<RoutePrice>, // None = no token price (delegate routes)
}
impl ResolvedRoute {
    pub fn price_at(&self, at: DateTime<Utc>) -> Option<PriceQuote>;
    pub fn has_quirk(&self, name: &str) -> bool;
}
impl RouteResolver {
    pub fn from_toml(toml_str: &str) -> anyhow::Result<Self>;
    pub fn resolve(&self, model_id: &str) -> anyhow::Result<ResolvedRoute>;
    pub fn provider_default(&self, provider: &str) -> anyhow::Result<ResolvedRoute>;
    pub fn available(&self) -> Vec<String>;
    pub fn available_by_provider(&self) -> BTreeMap<String, Vec<String>>;
}
pub const BANNED_EXACT: &[&str];
pub const BANNED_PREFIXES: &[&str];
pub fn is_banned(model_id: &str) -> bool;
```

Pinned semantics:
- `price_at`: peak windows evaluated in the route's **fixed-offset** timezone
  (Asia/Shanghai = UTC+8, no DST); window **start inclusive, end exclusive**; when
  peak, all three rates scale by the multiplier. `stale` = the instant falls after
  `valid_until` (prices valid through that whole UTC day). `None` = route has no
  price table — display "no price data", never invent numbers.
- `provider_default` rule (data-driven): **the provider's cheapest `class = "api"`
  route by off-peak output price; ties break alphabetically by route id; routes
  without a price table are skipped.** All of one provider's routes share a currency.
- Known quirk string: `"empty-reasoning-content-on-tool-replay"` (all deepseek routes).

---

## 2. nh-core contract

All items live in `nh_core::wire` unless stated. M0 surface stays intact; §5.2
lists the two struct amendments.

### 2.1 Factory

```rust
pub fn make_client(route: &nh_routes::ResolvedRoute,
                   api_key: zeroize::Zeroizing<String>) -> Box<dyn ChatClient>
```

- `Wire::OpenAi` → `OpenAiCompatClient` (existing), `Wire::AnthropicMessages` →
  `AnthropicMessagesClient` (new). Total over `Wire` — never fails.
- The factory captures per-route wire policy at construction: `thinking_dialect`,
  `preserve_reasoning`, and `has_quirk("empty-reasoning-content-on-tool-replay")`.
  `AgentLoop` stays policy-free.
- Callers must NOT call `make_client` for `RouteClass::Delegate` routes; they check
  `route.class` first and print: `delegate routes arrive in M4 — pick an api route`.
  (The active catalog contains no delegate routes; examples are commented out.)

### 2.2 AnthropicMessagesClient (new)

```rust
pub struct AnthropicMessagesClient { /* fields private except: */ pub base_url: String }
impl AnthropicMessagesClient {
    pub fn new(base_url: String, api_key: zeroize::Zeroizing<String>, max_tokens: u64) -> Self;
}
impl ChatClient for AnthropicMessagesClient { fn complete(&self, req: &ChatRequest) -> anyhow::Result<ChatResponse>; }
```

- **Endpoint:** `POST {base_url trimmed of trailing '/'}/v1/messages`
  (e.g. `https://api.deepseek.com/anthropic/v1/messages` — the deepclaude-proven path).
- **Headers:** `x-api-key: <key>`, `anthropic-version: 2023-06-01`, JSON body.
  Key held `Zeroizing`, injected per call, never logged.
- **`max_tokens` is REQUIRED** on this wire, always sent. `make_client` sets it to
  `min(route.max_out.unwrap_or(8192), 8192)`.
- **Request mapping** from `ChatRequest`:
  - First `role == "system"` message → top-level `system` string field (not a message).
  - `user`/`assistant` text → messages with `content: [{"type":"text","text":…}]`.
  - Assistant `tool_calls` → content blocks: optional leading text block, then one
    `{"type":"tool_use","id","name","input"}` per call; `input` is the parsed
    `arguments` JSON (unparseable arguments → `{}`).
  - `role == "tool"` messages → user message with
    `{"type":"tool_result","tool_use_id": tool_call_id, "content": text}` blocks;
    **consecutive tool messages merge into ONE user message** (roles must alternate).
  - `tools` → `[{"name","description","input_schema": ToolSpec.parameters}]`.
  - M1 sends **no thinking toggle on this wire** (default model thinking; see §2.3).
  - `reasoning_content` is never serialized on this wire in M1 (thinking blocks are M2).
- **Response mapping** to `ChatResponse`:
  - Concatenate `text` blocks → `message.content` (None when there are none).
  - `tool_use` blocks → `ToolCallReq { id, name, arguments: serde_json::to_string(input) }`.
  - `stop_reason` string verbatim → `finish_reason` (`end_turn`, `tool_use`, `max_tokens`…).
  - `usage.input_tokens` → `prompt_tokens`, `usage.output_tokens` → `completion_tokens`,
    `usage.cache_read_input_tokens` (when present) → `cached_tokens`.
- **Errors:** same UX as `OpenAiCompatClient`: friendly one-liner, status hints
  (401/403 → `run \`nh key add <provider>\``, 429 → retry later), body snippet
  scrubbed and truncated. Mock-server unit tests required; no live calls in tests.

### 2.3 ThinkingEffort + dialect mapping

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThinkingEffort { #[default] None, Low, High, Max }
```

`ChatRequest` gains `pub thinking: ThinkingEffort` (amendment §5.2). Clients map
`(dialect, effort)` → wire params in ONE function each:

| dialect | OpenAI wire behavior |
|---|---|
| `deepseek-nhm` | body param `"reasoning_effort"`: None→`"none"`, Low→`"none"` (DeepSeek has no low tier), High→`"high"`, Max→`"max"`. **ASSUMPTION — VERIFY LIVE**: param name and values are unconfirmed; keep the mapping in one function with a `// verify at live test` comment. |
| `always-thinking` | never send any toggle, whatever the requested effort (Kimi K2.7 has no non-thinking mode). |
| `glm-hm` | M1 sends no toggle (GLM thinking is High/Max server-side; mapping verified live in M2). |
| `none` | omit entirely. |

Anthropic wire: M1 sends no thinking parameter for any dialect.

### 2.4 reasoning_content handling

`ChatMessage` gains `pub reasoning_content: Option<String>` with
`#[serde(default, skip_serializing_if = "Option::is_none")]` (amendment §5.2).

- `OpenAiCompatClient` parses `reasoning_content` from responses into the field.
- On send (OpenAI wire), serialization policy lives in the client (captured from
  the route at `make_client` time):
  1. `preserve_reasoning == true` → assistant history messages keep and send their
     stored `reasoning_content` (Kimi K2.7*, MiMo routes — plan A.10.5; stripping
     it degrades the model).
  2. `preserve_reasoning == false` → `reasoning_content` is never serialized.
  3. Quirk `"empty-reasoning-content-on-tool-replay"` (deepseek routes): assistant
     replay messages that carry ONLY `tool_calls` (content `None` or empty) get
     `"reasoning_content": ""` — **empty string, not null** — even under rule 2.
     A stored value under rule 1 wins over the empty string.
- Unit tests required for all three rules (body-building tests, mirroring the
  existing `body_nests_tools_and_tool_calls` style).

### 2.5 Session history (for `nh chat`)

```rust
impl AgentLoop {
    pub fn run_with_history(&mut self, history: &mut Vec<ChatMessage>, task: &str)
        -> anyhow::Result<(String, Receipt)>;
}
```

- Empty `history` → push the system message first (same text `run` uses today).
- Pushes the user task, runs the existing turn loop; **all** produced messages
  (assistant + tool) are appended to `history`, which holds the full session on
  return — even on the timeout path.
- Exactly one receipt per call, semantics unchanged from `run`.
- `run(&mut self, task)` keeps its exact M0 signature/behavior and becomes a thin
  wrapper over a fresh history.
- Mid-session route switch needs no new API: `AgentLoop.client` and
  `AgentLoop.model_id` are already `pub`; nh-cli replaces them.

---

## 3. nh-tools contract — `pub mod mcp`

New module targeting **MCP 2026-07-28 stateless core** (plan §4.5). New deps
allowed for nh-tools: `reqwest`, `toml`, `nh-vault` (workspace versions).

### 3.1 `.nosis/mcp.toml` schema

```toml
[servers.playwright]
url = "http://localhost:8931/mcp"   # stateless Streamable HTTP
spec = "2026-07-28"                  # default; "2025-11-25" accepted as fallback
auth = "none"                        # none | apikey | oauth2
vault_entry = "playwright"           # REQUIRED when auth = "apikey" — key via nh-vault
scopes = ["browse"]                  # optional, default []
default_mode = "snapshot"            # optional (token-bomb guard, plan §5.8)
trust = "ask"                        # auto | ask | block — default "ask"
```

```rust
pub enum McpAuth  { None, ApiKey { vault_entry: String }, OAuth2 }
pub enum McpTrust { Auto, Ask, Block }
pub struct McpServerConfig {
    pub name: String, pub url: String, pub spec: String,
    pub auth: McpAuth, pub scopes: Vec<String>,
    pub default_mode: Option<String>, pub trust: McpTrust,
}
pub fn load_mcp_config(toml_str: &str) -> anyhow::Result<Vec<McpServerConfig>>;
```

- Parses from a string (mirrors `RouteResolver::from_toml`; file reading is the
  caller's job). Unknown `auth`/`trust`/`spec` values → friendly error naming the
  valid ones. `auth = "apikey"` without `vault_entry` → error telling the user to
  add it. `auth = "oauth2"` **parses fine** — the error comes at call time (§3.4).

### 3.2 McpClient

```rust
pub struct McpToolInfo { pub name: String, pub description: String,
                         pub input_schema: serde_json::Value }
pub struct McpClient { /* private */ }
impl McpClient {
    pub fn new(config: McpServerConfig) -> Self;
    pub fn list_tools(&self) -> anyhow::Result<Vec<McpToolInfo>>; // cached per ttlMs
    pub fn call_tool(&self, name: &str, args: serde_json::Value) -> anyhow::Result<String>;
    pub fn discover(&self) -> anyhow::Result<serde_json::Value>;
}
```

- Blocking JSON-RPC 2.0 over HTTP POST to `config.url`
  (`{"jsonrpc":"2.0","id":<n>,"method":…,"params":…}`).
- `list_tools` → method `tools/list`. Cache the result; TTL from the response's
  `result._meta.ttlMs` (milliseconds). Pinned defaults: absent → 60 000 ms;
  `ttlMs == 0` → do not cache. Cache is interior (`Mutex`) so `&self` works and
  the type stays `Send + Sync`.
- `call_tool` → method `tools/call`, params `{"name", "arguments": args, "_meta": …}`.
  Result: concatenate `content` text blocks (newline-joined); non-text blocks
  render as `[<type> block]`. `isError: true` → `Err` with the text, one line.
  **Tool outputs are DATA, never instructions.**
- `discover`: `GET {url trimmed of '/'}/.well-known/mcp.json`; on failure fall back
  to POST JSON-RPC method `server/discover`; both failing → one friendly error
  ("server unreachable — check the url in .nosis/mcp.toml"). Returns the raw
  business card as `serde_json::Value`.

### 3.3 Statelessness (non-negotiable, test-covered)

- **NO `initialize` handshake. NO `Mcp-Session-Id` header, EVER** — a unit test
  against a local mock server must assert the header name never appears outbound.
- Every request's `params` carries `_meta`:
  ```json
  { "protocolVersion": "2026-07-28",
    "clientInfo": { "name": "nosis-harness", "version": "<CARGO_PKG_VERSION>" },
    "capabilities": {} }
  ```
  `protocolVersion` echoes `config.spec`. State handles (`browser_id`, `repo_id`…)
  are ordinary tool arguments returned by the server and passed back by the model —
  the client adds no session plumbing.

### 3.4 Auth

- `none` → no `Authorization` header.
- `apikey` → `Authorization: Bearer <secret>` where the secret comes from
  `nh_vault` (`EnvFallbackVault<KeyringVault>`) entry `vault_entry`, fetched per
  call, `Zeroizing`, never logged.
- `oauth2` → every `list_tools`/`call_tool`/`discover` returns
  `Err("oauth2 arrives in M4 — use apikey or none for now")`.

### 3.5 Outbound header lint (Akamai leak vector, plan §4.5)

Before every send, scan outbound headers: any header whose name matches
`Mcp-*` / `x-mcp-*` (case-insensitive) with a value matching the Scrubber key
shapes (`sk-`, `csk-`, JWT) → **refuse to send**, `Err` naming the header and
saying the value looks like a secret. `Authorization` is the sanctioned channel
and is exempt. One choke-point function, unit-tested with a fake `x-mcp-token`
header carrying an `sk-…` value.

### 3.6 Tool adapters

```rust
pub struct McpToolset { pub tools: Vec<Box<dyn Tool>>, pub warnings: Vec<String> }
pub fn mcp_tools(configs: &[McpServerConfig]) -> McpToolset
```

- One adapter per server tool, named **`mcp__<server>__<tool>`**; `ToolSpec`
  description prefixed `[MCP <server>] `; `parameters` = the server's input schema.
- A server whose `list_tools` fails contributes zero tools plus one friendly
  warning line in `warnings` (nh-cli prints them; never a hard failure).
- `execute` routing by trust:
  - `Block` → `Ok("blocked by .nosis/mcp.toml (trust = \"block\") — set trust = \"ask\" to enable")`.
  - `Ask` → `ctx.approve("mcp <server> <tool> <args on one line, truncated>")`;
    denial → `Ok("user denied: mcp <server> <tool>")` (Ok-shaped, model-readable).
  - `Auto` → only tools annotated read-only by the server (`annotations.readOnlyHint
    == true`) skip the gate; **everything else still asks** — state-mutating MCP
    calls pass the approval gate at every autonomy level (non-negotiable).
- Results and warnings pass through the caller's Scrubber before display.

---

## 4. nh-cli contract — `nh chat`

New subcommand: `nh chat [--model <id>]` (default `deepseek-v4-flash`).
Line REPL that works with piped stdin. Per-task turn cap: 20 (like `nh run`).

- **Prompt `nh> ` goes to stderr**; stdout carries only answers and command output.
  EOF or `/quit` → exit 0. Blank lines re-prompt.
- **Plain line** = a task, run via `AgentLoop::run_with_history` with ONE
  persistent session history — context carries across turns. Answer (scrubbed) to
  stdout; each task writes a receipt exactly like `nh run`.
- **Footer after each answer**, one line to stderr:
  `<route id> | peak` / `| off-peak` / `| no price data` `| session tokens <in> in / <out> out / <cached> cached`
  — peak from `route.price_at(Utc::now())`; tokens are session-cumulative.
- **`/model <id>`** — `resolver.resolve(id)`; on success keep the history, build a
  new client via `make_client` (fetch the new route's vault key; scrub literals on
  all output paths), confirm with one line `switched to <id>`. Delegate class →
  `delegate routes arrive in M4 — pick an api route`. Failure (unknown/banned id,
  missing key) → print the friendly error, keep the current route.
- **`/provider <name>`** — `resolver.provider_default(name)`, then the same switch
  semantics as `/model`.
- **`/price`** — quote NOW (`price_at(Utc::now())`), one line:
  `<route id> | peak|off-peak | in <hit> hit / <miss> miss | out <output> | <CUR>/M tokens | confidence <confidence>`
  (rates printed with `{:.4}`). If `stale`, add:
  `warning: price data past valid_until — verify before trusting these numbers`.
  No price table → `no price data for <id> — add a [routes.<id>.price] table to catalog.toml`.
- **`/tools`** — builtin tools then MCP tools (from `.nosis/mcp.toml` when present,
  via `mcp_tools`), one per line `name — <first line of description>`; MCP warnings
  printed after, to stderr.
- **Unknown `/command`** → exactly one line:
  `unknown command — try /model <id>, /provider <name>, /price, /tools, /quit`.
- Every printed line passes the Scrubber (all session key literals registered,
  including keys of routes switched away from). Errors: one friendly line, never
  a stack trace.

---

## 5. What is frozen

### 5.1 Frozen surfaces

- Everything public in M0: `nh-vault` (Vault, KeyringVault, EnvFallbackVault,
  Scrubber, SERVICE), `nh-tools` (ToolSpec, ToolCtx, Tool, ReadFile, EditFile,
  ExecShell, builtin_tools), `nh-routes` (§1 above), `nh-core`
  (wire/receipt/agent modules), `nh-cli` commands `init` / `key` / `run` and their
  flags and messages.
- Everything specified in §§1–4 of this file.
- Builders may add private helpers and private modules freely; new PUBLIC items
  beyond this file need an architect amendment.

### 5.2 Explicit M0 amendments (the only source-compat changes allowed)

1. `nh_core::wire::ChatMessage` gains
   `pub reasoning_content: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`).
2. `nh_core::wire::ChatRequest` gains `pub thinking: ThinkingEffort`.
3. *(ratified at integration, 2026-07-13)* `nh_core::agent::AgentLoop` gains
   `pub thinking: ThinkingEffort` — the loop forwards it into every `ChatRequest`
   and stays policy-free. The AgentLoop struct literals in nh-cli
   (`cmd_run.rs`, `cmd_chat.rs`) were updated in the same change.

`ChatMessage`/`ChatRequest` are constructed by literal only inside nh-core and
its tests (verified by grep) — the builder updates those literals in the same PR.

### 5.3 Dependency additions allowed

- nh-routes: `chrono` (done).
- nh-tools: `reqwest`, `toml`, `nh-vault`.
- nh-cli: `chrono`; plus `serde_json` as **dev-dependency** only *(ratified at
  integration, 2026-07-13)* — nh-cli tests build `ChatMessage` via
  `serde_json::from_value`, preserving the §5.2 literal-only-in-nh-core rule.
All workspace-version (`.workspace = true`); nothing new enters
`[workspace.dependencies]` without an architect amendment.

---

## 6. Verify-live ledger (assumptions a live key must confirm)

| item | where | note |
|---|---|---|
| `reasoning_effort` param name/values | §2.3, nh-core | deepseek-nhm mapping — still open |
| Kimi/MiMo/GLM base URLs | catalog.toml | **RESOLVED 2026-07-13** — confirmed first-party; MiMo host is `api.xiaomimimo.com/v1` |
| MiMo prices | catalog.toml | **RESOLVED 2026-07-13** — plan B.3 conflict settled first-party (`mimo.mi.com/docs/pricing`); now `confirmed` |
| Kimi K2.6 cache-hit rate | catalog.toml | **RESOLVED 2026-07-13** — published first-party: $0.16/M |
| `ttlMs` location in tools/list response | §3.2, nh-tools | pinned default 60 s — still open |
| GLM free-tier rate limits | catalog.toml | zeros now `confirmed` free tier; rate limits still open |
| DeepSeek peak windows / prices | catalog.toml | re-verify on/around `valid_until` 2026-07-24 |

Ledger resolutions above were live-verified by the catalog builder on 2026-07-13
(first-party source comments sit next to each value in catalog.toml). The three
nh-routes tests that asserted the pre-verification values were reconciled with
the confirmed catalog data at integration — catalog is data-of-record
(honest-cost rule); tests follow data, never the other way around.

---

## 7. Integration amendments (2026-07-13, ratified with orchestrator authority)

1. **§4 — keyless startup.** `nh chat` starts without any key configured: one
   `warning: <friendly vault error>` line to stderr, then the REPL. Commands
   (`/model`, `/provider`, `/price`, `/tools`, `/quit`) all work keyless; running
   a task re-surfaces the vault error as one friendly line and the session keeps
   going. `echo /quit | nh chat` exits 0 on a fresh machine (§4's "EOF or /quit
   → exit 0" holds unconditionally).
2. **§4 — peak field format.** The footer and `/price` peak field is
   `peak <multiplier>x until HH:MM` (window boundary rendered in the user's
   local time); `off-peak` and `no price data` stay verbatim. Prefix-compatible
   with the original bare `peak` wording; blessed as the canonical format.
3. **§3.6 — crate-root re-exports.** nh-tools additionally re-exports the §3
   public items at crate root (`pub use mcp::{…}`). Purely additive.
4. **§5.1 — `nh run --think` (ratified at hardening, 2026-07-13, orchestrator
   authority).** `nh run` gains one optional flag,
   `--think <none|low|high|max>`, mapping 1:1 onto `ThinkingEffort`. Flag
   absent → default per route dialect: `always-thinking` / `glm-hm` → High,
   `deepseek-nhm` / `none` → None (cheap by default on effort-toggle routes).
   `nh chat` applies the same per-dialect default on start and on every
   `/model` / `/provider` switch. Additive only; every other `run` flag and
   message is unchanged, and the mapping lives in one function
   (`cmd_run::effort_for`).
