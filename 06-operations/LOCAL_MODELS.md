# Local models

Nosis can use an explicitly configured Ollama or llama.cpp server through the existing
OpenAI-compatible wire. Local routes are a degraded, operator-selected tier: `--model` and `/model`
can select them, but `nh why`, provider defaults, cheapest-capable selection, escalation ladders,
and top-tier cost comparisons exclude them.

The meter says exactly:

> Local: no billed tokens; hardware and power are not metered.

## Reference path and context honesty

llama.cpp is the reference path because its default context-overflow behavior fails closed with
HTTP 400 and token counts instead of silently dropping conversation history.

Do not enable llama.cpp `--context-shift` when that fail-closed property matters. It is opt-in and
changes the overflow behavior.

Ollama has a serious honesty hazard: when a conversation exceeds its configured context, it can
silently discard the oldest messages server-side. The OpenAI-compatible response contains no
signal, and `/v1` has no opt-out. Nosis therefore cannot detect, prevent, or report the lost
history. This is documented rather than hidden or worked around.

## Configure a route

The commented templates are in the local-routes section of `catalog.toml`. llama.cpp defaults to
`http://127.0.0.1:8080/v1`; Ollama defaults to `http://127.0.0.1:11434/v1`.

1. Uncomment one template and replace its three symbolic values:

   - `model_id`: the exact Ollama tag or llama.cpp server ID/`--alias`.
   - `context`: the context actually configured in this runtime session, not a model-card maximum.
   - `max_out`: a safe output cap for that configured context.

2. Treat `max_out` as mandatory and keep it at or below `context`, with room for the prompt, tool
   definitions, reasoning, and replayed tool results. If it is absent, the OpenAI client fallback
   is 65,536 tokens, which is larger than most local windows.

3. Review the complete edited catalog, then copy that exact file to
   `~/.nosis/catalog.toml`. Repository catalog changes are trusted only when they are byte-identical
   to that operator-reviewed copy.

4. Add the exact loopback origin to the user-global `~/.nosis/law.toml`. A repository law file
   cannot grant a new credential audience.

For llama.cpp:

```toml
[credential.llama-cpp-local]
audience = ["http://127.0.0.1:8080"]
```

For Ollama:

```toml
[credential.ollama-local]
audience = ["http://127.0.0.1:11434"]
```

5. Use the normal vault flow; there is no credential bypass:

```text
nh key add llama-cpp-local
value: llama

nh key add ollama-local
value: ollama
```

Ollama ignores the Authorization header. llama.cpp checks it only when launched with
`--api-key`; in that case, store the configured value instead of the placeholder. The harness
still scopes either value to the exact loopback origin and scrubs it from output.

The harness does not discover servers, start runtimes, pull models, hold a Hugging Face token, or
fall back to a local route automatically.

## Verify the live wire before relying on it

Run these checks against the exact runtime, model, quantisation, context, and launch flags you will
use:

1. A tool call returns `arguments` as a JSON string and that string round-trips without mutation.
2. `usage` contains real `prompt_tokens` and `completion_tokens`.
3. The server honors the route's configured `max_out`.
4. A multi-turn tool replay survives intact.
5. llama.cpp returns HTTP 400 with token counts when context is exceeded, rather than truncating.

These are live checks. Catalog fields can describe limits, but cannot prove that a particular
server build populates its response correctly.

## Source and verify model files

Use GGUF or safetensors only. Never load pickle or pickle-backed `.pt`, `.pth`, or legacy model
`.bin` files.

For every download:

1. Start from the verified organization or a reputation-bearing quantiser.
2. Pin the repository revision by its full commit hash, not a branch, tag, or short hash.
3. Record the exact filename and expected SHA-256.
4. Verify the SHA-256 before the runtime parses the file.
5. Save the repository LICENSE and required notices with the provenance record.

Hub artifacts are not signed by default. A full commit hash plus a verified checksum is the
available trustless integrity check; a floating branch name is not.

Safetensors avoids pickle execution. GGUF is data rather than executable model code, but that does
not make an untrusted GGUF inert: GGUF parsers have a live CVE history, so a malicious file is
parser-exploit input. Keep the runtime patched and parse only artifacts whose provenance and hash
you verified.

Reputable starting points:

- Verified organizations: `Qwen`, `zai-org`, `moonshotai`, and `ggml-org`.
- Reputation-bearing quantisers: `unsloth`, `bartowski`, `lmstudio-community`, and
  `mradermacher`.
- TheBloke has been stale since 2024 and is not recommended for current artifacts.

## Licences in plain language

Always read the exact repository LICENSE. A Hub `license:` tag is an uploader claim, not a legal
determination.

- Apache-2.0 Qwen3.x weights are a straightforward redistribution choice when their notices are
  preserved.
- MIT DeepSeek V4 and GLM-5.x weights are similarly permissive.
- Gemma terms require use restrictions to flow down as an enforceable provision, and Google
  reserves a remote usage-restriction right. That is a poor fit for an independently distributed
  product unless those obligations are deliberately adopted.
- Llama 4 adds branding and agreement-copy obligations plus a 700-million-monthly-active-user
  gate.
- Kimi K3 weight terms include a MaaS clause requiring a separate agreement above a revenue
  threshold. That clause governs use of the weights; it does not govern ordinary calls to
  Moonshot's hosted API.

## Size for the reference machine

The reference machine has a 12 GB laptop Blackwell GPU and 32 GB system RAM. After the desktop,
driver, display buffers, and runtime overhead, plan around 10.5 GB of usable VRAM, not the label's
12 GB. Use AC power; laptop power limits materially change sustained inference.

- Dense approximately 14B at Q4 is the fully resident ceiling.
- Dense 20–24B is an offload cliff: approximately 5–10 tokens/second is not useful for agent loops.
- A roughly 30–35B-A3B MoE can work because only a small active parameter set runs per token. Use
  llama.cpp `--n-cpu-moe` to keep appropriate expert weights in system RAM and q8_0 KV cache to
  control context memory.

Size KV cache rather than trusting a table:

```text
KV bytes per token =
    layers × kv_heads × head_dimension × (bytes_per_K_value + bytes_per_V_value)

total KV bytes =
    KV bytes per token × context_tokens × parallel_sequences
```

For the same K and V type, this is often written as
`2 × layers × kv_heads × head_dimension × bytes_per_value`. Quantised KV formats add block
metadata, so use the formula for planning and the runtime's reported allocation for the final
check. Model weights, compute buffers, graph scratch space, and desktop overhead are separate.

## Choose by criteria, not a permanent winner

There is no durable “best local model.” Select for:

1. a permissive licence that fits distribution plans;
2. reliable OpenAI-format tool calling, including JSON-string arguments and multi-turn replay;
3. measured fit inside the real VRAM/RAM/context budget.

Examples to evaluate as of 2026-07-28—not defaults—include Qwen3-14B at Q4 for the dense,
fully-resident lane and Qwen3-30B-A3B for the CPU-expert-offloaded MoE lane. Recheck the exact
repository, licence, quantisation, tool template, and runtime behavior at selection time; the date
is part of the recommendation.
