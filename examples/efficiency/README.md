# Evaluate cost per correct task

This is a development protocol for the unreleased efficiency experiments. It is
not a benchmark result. Python is needed for the report script, not for Nosis.

Keep the model, reasoning setting, output limit, task, starting files and permitted
tools identical between baseline and candidate. Change one flag at a time. Run in
fresh disposable project folders. Keep acceptance checks outside the editable
folder, inspect generated code before executing it, and record human corrections.
Use the same attempt and time limits for both variants. Alternate run order and
record cache conditions; deleting local files does not make a provider cache cold.

## Development tasks

Use these twelve cases first. A separate, unseen set is required before changing
defaults. The three practice edits use [the practice kit](../practice-tasks/README.md).

| ID | Task and setup | Independent acceptance |
| --- | --- | --- |
| explain | Explain the practice project's three functions in read-only mode | Correct signatures and behavior; no claimed test run or changed files |
| delivery | Fix only the practice delivery calculation | Four delivery checks and unchanged other functions |
| names | Fix only the practice name cleanup | Four name checks, input preserved, unchanged other functions |
| quantities | Implement only the practice quantity parser | Four quantity checks and unchanged other functions |
| multi-file | In a small two-module copy, add a configurable delivery threshold and update its caller | Default behavior preserved; custom boundary works; both files consistent |
| middle-file | Locate a named marker and its two neighboring lines in a 2,000-line UTF-8 text file | Exact middle evidence, no invented lines; compare path-only and ranged reads |
| middle-log | Find the failing assertion buried in a long synthetic test log | Correct failing test and assertion, recovered from stored observation when enabled |
| cancel | Cancel after a read, then start a new run to finish the same task | Cancellation is acknowledged; no action after cancellation; no false completion |
| long-task | A single request requires many reads before a small edit | Original constraints and required middle evidence survive; final edit passes checks |
| resume | Several chat turns refine a task before enough history accumulates to compact | Regression check of existing chat: latest constraints and earlier unresolved requirements honored; no claim of new archive retrieval |
| mcp | Ask for a specific tool on a local synthetic MCP server with many tools | Correct schema discovered and invoked; no unexpected call; same approval boundary |
| refusal | Decline a requested shell command, then ask for a read-only alternative | No process was spawned; model acknowledges refusal and does not claim test success |

Build the large fixtures from public synthetic text. Do not put private transcripts
or credentials in the evaluation kit. Mock-provider tests can prove request shapes,
retrieval and accounting. They cannot prove model quality or real token savings.
The context/archive experiment and JSONL measurement currently apply to `run`.
The resume case checks existing chat behavior, and MCP discovery is chat-only;
those two cases need separate manual/provider evidence rather than the run-only
measurement report. Do not present them as instrumented cost comparisons.

## Judging and measurement

Enable measurement in both variants. A normal receipt outcome records loop
completion; it does not certify correctness. Assign each attempt a task ID from
its measurement file. Group repair attempts under the same trial ID so their cost
and latency remain in the denominator. Do not quietly discard failed requests.

For each trial record case ID, repeated pair ID, variant, chosen model and reasoning
setting, cache condition, relevant configuration, and an independent judgment:
`success`, `partial`, `failure`, or `unjudged`. Success requires passing checks and
human confirmation that the requested scope was respected. Record that evidence
separately; the measurement file never contains prompts or file contents.

Report all observed spend divided by correctly completed trials, alongside success
counts, unknown billing, turns, retries, tool outcomes, latency and cache usage.
Compare matched pairs and the whole set. A lower cost per success does not excuse
more wrong answers. Missing cache splits, missing usage, interrupted logs and
unjudged tasks must remain visible. Source byte counts are estimates of request
composition, not provider-token counts or cost attribution by source.

No paid runs are performed by this kit. Set a separate budget and choose a small
paired pilot before making real API calls. A pilot helps estimate the budget for
repeated runs; it cannot establish that quality is unchanged. Leave experiments
off by default when evidence is absent or inconclusive.
