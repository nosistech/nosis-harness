# Measure efficiency without lowering quality

These are unreleased source features. The goal is lower cost per correctly
completed task, including failed attempts and repairs. No overall cost reduction
or unchanged model quality has been established yet. Experimental options default
to off. Provider choice and reasoning defaults are unchanged.

## Local measurement

Add `--measure-efficiency` to an `nh run` command. For example, from an initialized
project, use a model whose key you have already stored:

```powershell
nh run "Explain this project's README. Do not edit files." --model deepseek-v4-flash --read-only --measure-efficiency
```

This is a real provider request and normal provider charges apply. Measurement
does not send telemetry to Nosis or change the model request. It appends local
metadata to `.nosis/efficiency-v1.jsonl`, capped at 8 MiB. A measurement write
failure produces a warning and does not change the task result. If the log fills,
archive it locally before starting another evaluation. Do not delete normal
receipts to make room for measurements.
A recording warning means the affected run's evidence is incomplete. Keep that
attempt in your study notes; do not discard it to improve the reported result.
If no record was written, include the printed task ID in the judgment's attempt
list anyway. The report marks its billing as unknown instead of counting it as free.

Records include request section byte counts, reported token/cache usage, aggregate
retries, tool names and observed outcomes, timing, compaction, and task completion.
Task-start records make runs with no final summary visible. Summaries count prior
dropped records; a complete receipt's usage can survive even when request/tool
composition is incomplete. Keep the printed task ID with your study notes.
Request records include schema count/bytes and a noncryptographic schema-only
identity to detect changes across requests. It excludes message content and is
not proof of provider token order, a cache hit, or cryptographic integrity.
They exclude task text, file contents, tool arguments/results, reasoning text,
headers, credentials and raw errors. They are still local usage metadata; review
them before sharing. This measurement option currently applies to `run`, not chat,
TUI or Fleet. Task duration includes measurement overhead, including local log
flushes, as well as any enabled observation storage work. Enable measurement on
both sides of a timing comparison; per-request and
per-tool elapsed times exclude their subsequent measurement write.

Byte counts describe the request before provider encoding. They are not exact
token counts. Providers report usage for the whole request; those totals do not
reveal which individual source was cached. Cost by source and billing type is
therefore unavailable. The captured catalog quote is taken at task start and may
not match a task that crosses a pricing window. Missing prices or usage remain
unknown. Retry usage is aggregate; attempt-level billing is not reconstructed.

## Ranged reads

`nh run --enable-ranged-reads` lets the model request a 1-based `start_line` and
`line_count` from `read_file`. Ordinary path-only reads and policy boundaries are
preserved. This can avoid sending a whole file when only a section is needed, but
extra retrieval turns can erase that saving. Keep it experimental until paired
task comparisons establish the tradeoff.
The range must specify both arguments. Limits are 1,000 returned lines, a starting
line no greater than 100,000, 64 KiB per scanned line, 8 MiB scanned/file size and
2 MiB selected text before normal result rendering. Oversized requests are refused;
ask for a smaller range. A range extending past EOF identifies the shorter result.

## Recover large observations

`nh run --retain-observations` stores large, scrubbed tool results temporarily for
the current run. The model receives a short excerpt and a session-specific handle.
It can call `read_observation` with that handle, a zero-based `char_offset`, and a
`char_count` of 1 to 6,000. Character ranges preserve UTF-8 text and can reach the
middle of a long single line. This also works with read-only runs.

Unlike metadata measurement, this option stores scrubbed project/tool text locally.
It is not an encrypted document store. Active credentials and recognized secret
patterns pass through the existing scrubber before storage and again on retrieval.
Unix session directories and files use owner-only permissions; Windows uses the
project directory's inherited permissions. Use a private project directory when
its contents should not be readable by other accounts on the computer.
The reader accepts only handles registered in this run and verifies captured bytes
before returning them. Ordinary reads still cannot access `.nosis` runtime files.
Retrieving an observation cannot execute its contents or approve another action.

Limits are 4 MiB per observation, 16 MiB and 32 observations per session, and up to
6,000 characters/24 KiB per retrieval. Results above the existing 32,000-character
inline threshold are candidates for retention. Short outputs keep their original
format. If capture fails or a quota is reached, the old bounded excerpt remains
with an unavailable notice. A file read or command may already have reached its
own capture limit; retained observations do not recover data never captured.

Normal exit attempts to remove the owned observation files and empty session
directory. Crashes, filesystem errors or unexpected files can leave scrubbed text
under `.nosis/observations`. Handles expire with the session and are not usable by
another run. This is a temporary retrieval aid, not persistent cross-task memory.
Extra retrieval turns and storage work can offset savings; evaluate whole tasks.
Run `nh init` with this source version in existing projects to add the observation
directory and efficiency log to `.nosis/.gitignore`, preserving custom entries.
Git ignore rules prevent accidental addition, not intentional forced staging.

## Context and identity experiments

For a long single-task run, `--context-experiment extractive-v1` together with
`--retain-observations` can archive older completed assistant/tool exchanges.
Original system and user messages stay intact, as do the two most recent complete
tool exchanges. Reasoning attached to retained messages stays attached. Removed
messages are scrubbed and retrievable through `read_observation` during the run.
Archived image payloads and malformed tool arguments are replaced with redaction
markers; the archive is not an exact backup of the original conversation.
This is not a generated summary or persistent memory, and it does not change
chat/resume behavior.

At moderate context pressure, the experiment requires usable price/cache evidence
and a favorable cost estimate. The estimate assumes two future requests and one
retrieval with a 512-output-token allowance; it cannot know the actual remaining
work. Unknown or unfavorable economics defer elective compaction. At high context
pressure it may archive without that estimate to make room. Archive failures keep
unarchived history; item/session limits still apply, with at most eight context
archives per run. Measurement records decisions and estimated sizes separately
from ordinary compaction. Estimates are not billable token counts or savings.
Superseded archives remain readable until the session ends and share the same
16 MiB/32-item budget with tool observations; that quota may stop archiving before
eight archives. Irreducible task/policy content can still exceed the model window.

`--identity-prompt compact-v1` tests a shorter identity sentence. It preserves the
policy, route/provider details, available tools and read-only constraints. The
existing identity is already short, so expect a small ceiling for this experiment.
Neither experiment changes the model or reasoning effort. Leave both off until
matched tasks demonstrate acceptable outcomes, including successful retrieval of
details removed from the inline history.

## MCP schema discovery

For chat sessions with many configured MCP tools, `--mcp-discovery` tests exposing
small discovery/invocation tools instead of sending every remote schema on every
request. Local coding tools remain available immediately. Discovery is a separate
model tool call, so its extra turn can outweigh reduced schema size. Ordinary
sessions without MCP tools have no target saving from this option.

Only existing operator-configured servers are eligible. Invocation uses the original
adapter, preserving configured trust, send policy, credentials, approval and
cancellation. Finding a tool does not approve its execution. This option changes
schema exposure, not the MCP transport protocol. The remote registry is ordered
deterministically, and ambiguous duplicate names are refused.
The model uses `mcp_discover` to search/page through schemas, then `mcp_invoke`
with a returned name and argument object. Discovery returns at most eight tools
per page by default, with a 128-character query limit, 8 KiB per available entry
and 24 KiB per response. Oversized entries are marked unavailable and cannot be
invoked through discovery. Full schemas are not silently cut into invalid JSON.
The flag and discovered-name state are not persisted. `nh resume` uses the ordinary
full MCP registry; start a new `nh chat --mcp-discovery` for this experiment.

## Compare tasks

Use the [twelve-case development protocol](../examples/efficiency/README.md).
Measure baseline and candidate with the same model, reasoning, fixtures and task
limits. Record actual cache conditions; do not assume a local restart clears a
provider cache. Include every attempt, including failed or interrupted runs.

Create a judgments JSON file with this structure. Replace the example IDs and
settings with your actual records. Group repair attempts in the same `task_ids`
array. `evidence` points to your independently checked result; the report does not
run or certify that check for you. Keep private evidence local.

```json
{
  "schema_version": 1,
  "trials": [
    {
      "trial_id": "delivery-01-baseline",
      "pair_id": "delivery-01",
      "case_id": "delivery",
      "variant": "baseline",
      "task_ids": ["REPLACE_WITH_MEASUREMENT_TASK_ID"],
      "judgment": "unjudged",
      "evidence": "",
      "settings": {
        "model": "REPLACE_WITH_MEASURED_MODEL_ID",
        "thinking": "REPLACE_WITH_ACTUAL_SETTING",
        "cache_condition": "uncontrolled",
        "fixture_revision": "practice-kit-source-revision"
      }
    }
  ]
}
```

Use `success` only after checking correctness and requested scope. Other values
are `partial`, `failure`, and `unjudged`. Candidate rows share the baseline's pair
ID, with a different variant and trial ID. The script rejects unassigned attempts,
duplicate records and mismatched model metadata. Supply only the intended study
logs, including all its failures; unrelated runs belong in a separate study.

From the Nosis source directory, run:

```powershell
python -B scripts/efficiency-report.py --measurements path/to/efficiency-v1.jsonl --judgments path/to/judgments.json
```

The JSON report contains trial and cohort costs, unknown billing, outcomes, paired
differences, request composition and tool usage. Decimal costs are strings to
preserve precision. `null` means unavailable, not free. Costs are catalog-based
estimates, not invoices. Different currencies are never added. A lower cost per
success cannot justify more failures; no result automatically promotes a flag.
Trials retain quote time, confidence, peak annotation and limitations. The peak
annotation describes the already-selected task-start rates; it is not another
multiplier. Missing turn/retry totals remain unknown, with known retry subtotals
and coverage shown separately.
Request composition is separated by variant in the cohort rows; top-level
composition and tool usage pool all supplied runs. Cache ratios show the number
of attempts and prompt tokens with a usable cache split. Unclassified tool returns
can include refusals or errors, so the returned-error fraction is not a complete
error rate. Cohorts list their settings and flag mixed configurations.

## Offline checks

These development scripts use Python 3.10 or newer. Installing or running Nosis
itself does not require Python.

The report's regression checks make no network requests:

```powershell
python -B scripts/test-efficiency-report.py
```

After building the current source, exercise the actual binary against a temporary
loopback fixture:

```powershell
python -B scripts/efficiency-smoke.py --binary target/release/nh.exe
```

This uses synthetic text, a fake local credential and an isolated temporary home.
It compares rendered requests with measurement off/on and verifies middle-file
retrieval. It does not call an external provider, measure real token savings or
judge model quality.
Add the smoke script's `--observations` option to check retained-output retrieval
and normal-exit cleanup as well. It invokes the product's `--retain-observations`
flag against the same synthetic local provider.
Add `--context` to exercise long-run archive creation, retrieval, preservation of
the task/system messages, complete tool-call pairs and normal-exit cleanup.
Add `--identity` to check that the compact prompt changes only its first clause.

For the actual chat discovery/invocation path, run:

```powershell
python -B scripts/mcp-efficiency-smoke.py --binary target/release/nh.exe
```

This fixture checks stable schemas when a synthetic MCP server reverses tool order,
discovery followed by invocation, and refusal of a mutation because piped input
cannot grant approval. Both endpoints are temporary loopback fixtures. This does
not substitute for observing a person decline an interactive approval prompt.

Before changing defaults, use repeated matched tasks and a separate held-out set.
Report task success, turns, latency, errors and cache usage alongside total cost.
Keep experiments off when quality evidence is absent or inconclusive.
