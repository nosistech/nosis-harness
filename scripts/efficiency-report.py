"""Report local v1 efficiency records with separate trial correctness judgments.

No API calls. Run with --measurements FILE [FILE ...] --judgments FILE.
Amounts are catalog-quote estimates, not invoices. All attempts must be assigned
to a trial; incomplete billing is never treated as zero. See docs/EFFICIENCY.md.
"""

import argparse
from collections import Counter, defaultdict
from decimal import Decimal, InvalidOperation
import json
from pathlib import Path
import statistics


SOURCES = ("system", "user", "assistant", "tool", "tool_schema", "reasoning", "image_base64")
JUDGMENTS = {"success", "partial", "failure", "unjudged"}


def integer(value):
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def decimal(value):
    try:
        result = Decimal(str(value))
    except InvalidOperation:
        return None
    return result if result.is_finite() and result >= 0 else None


def priced_usage(record):
    """Return complete split-priced catalog estimate or an explicit gap."""
    usage = record.get("usage", {})
    if set(usage) - {"usage_object_reported", "evidence", "prompt_tokens", "completion_tokens", "cached_tokens"}:
        return None, "unknown_usage_bucket"
    if not usage.get("usage_object_reported") or usage.get("evidence") != "measured":
        return None, "usage_missing_or_partial"
    prompt, output, cached = (usage.get(name) for name in
                              ("prompt_tokens", "completion_tokens", "cached_tokens"))
    if not all(integer(value) for value in (prompt, output)):
        return None, "invalid_usage"
    if not integer(cached) or cached > prompt:
        return None, "cache_split_missing_or_invalid"
    price = record.get("route", {}).get("price", {})
    if price.get("status") != "captured" or price.get("unit") != "per_million_tokens":
        return None, "price_unavailable"
    if set(price) - {"status", "quote_at_utc", "unit", "currency", "input_cache_hit",
                     "input_cache_miss", "output", "confidence", "peak", "limitation"}:
        return None, "unknown_price_bucket"
    # Rust PriceQuote already selects the rates at quote_at_utc. Its peak field
    # is a boolean annotation, not another multiplier or a rate schedule.
    if not isinstance(price.get("peak"), bool):
        return None, "unsupported_peak_quote"
    currency = price.get("currency")
    rates = [decimal(price.get(name)) for name in ("input_cache_miss", "input_cache_hit", "output")]
    if currency not in {"USD", "CNY"}:
        return None, "unsupported_currency"
    if any(rate is None for rate in rates):
        return None, "price_invalid"
    costs = [Decimal(tokens) * rate / Decimal(1000000)
             for tokens, rate in zip((prompt - cached, cached, output), rates)]
    return {"currency": currency, "uncached_input": costs[0], "cached_input": costs[1],
            "output": costs[2], "total": sum(costs)}, None


def read_records(paths):
    records = []
    seen = set()
    for path in paths:
        for line_number, line in enumerate(path.read_text(encoding="utf-8-sig").splitlines(), 1):
            if not line.strip():
                continue
            try:
                row = json.loads(line)
                kind, task_id = row["record_type"], row["task_id"]
                if row["schema_version"] != 1 or kind not in {"task_start", "request", "tool", "task", "context"}:
                    raise ValueError("unsupported schema or record type")
                if not isinstance(task_id, str) or not task_id:
                    raise ValueError("invalid task ID")
                sequence = row.get(f"{kind}_seq", 0)
                if kind == "context":
                    sequence = row.get("request_seq", 0)
                if kind in {"request", "tool", "context"} and (not integer(sequence) or sequence == 0):
                    raise ValueError("invalid sequence")
                key = (task_id, kind, sequence)
                if key in seen:
                    raise ValueError("duplicate record; do not pass overlapping logs")
                seen.add(key)
                records.append(row)
            except (KeyError, TypeError, ValueError) as error:
                # No raw line content echoed: logs may have been manually altered.
                raise ValueError(f"invalid measurement at input line {line_number}: {type(error).__name__}") from None
    return records


def summarize(records, judgments):
    if not isinstance(judgments, dict) or judgments.get("schema_version") != 1:
        raise ValueError("unsupported judgment schema")
    by_task = defaultdict(list)
    for row in records:
        by_task[row["task_id"]].append(row)
    trials = judgments.get("trials", [])
    if not trials:
        raise ValueError("at least one independently judged or unjudged trial is required")
    assigned, trial_ids, paired = set(), set(), set()
    results = []
    for trial in trials:
        for key in ("trial_id", "pair_id", "case_id", "variant"):
            if not isinstance(trial.get(key), str) or not trial[key]:
                raise ValueError(f"trial requires {key}")
        identity = (trial["pair_id"], trial["variant"])
        if trial["trial_id"] in trial_ids or identity in paired:
            raise ValueError("duplicate trial or pair/variant")
        trial_ids.add(trial["trial_id"])
        paired.add(identity)
        judgment = trial.get("judgment")
        if judgment not in JUDGMENTS:
            raise ValueError("invalid independent judgment")
        if judgment != "unjudged" and not trial.get("evidence"):
            raise ValueError("judged trials require an evidence reference")
        settings = trial.get("settings", {})
        for key in ("model", "thinking", "cache_condition", "fixture_revision"):
            if not isinstance(settings.get(key), str) or not settings[key]:
                raise ValueError(f"settings require {key}")
        task_ids = trial.get("task_ids")
        if not isinstance(task_ids, list) or not task_ids:
            raise ValueError("trial requires every attempt's task ID")
        costs, gaps = [], Counter()
        duration, turns, retries, missing_duration = 0, 0, 0, 0
        missing_turns, retry_count_attempts, quote_metadata = 0, 0, []
        interrupted_requests, cache_attempts = 0, 0
        dropped_records, drop_coverage = 0, 0
        usage_counts = Counter()
        for task_id in task_ids:
            if not isinstance(task_id, str) or not task_id or task_id in assigned:
                raise ValueError("invalid task ID or attempt assigned more than once")
            assigned.add(task_id)
            if task_id not in by_task:
                gaps["missing_attempt_records"] += 1
                missing_duration += 1
                continue
            task_records = by_task[task_id]
            task = next((row for row in task_records if row["record_type"] == "task"), None)
            models = {row.get("route", {}).get("model_id") for row in task_records
                      if row["record_type"] in {"task_start", "request", "task"}}
            if models and models != {settings["model"]}:
                raise ValueError("judgment model differs from measured model")
            if task is None:
                gaps["missing_task_summary"] += 1
                missing_duration += 1
                interrupted_requests += sum(row["record_type"] == "request" for row in task_records)
                continue
            cost, gap = priced_usage(task)
            price = task.get("route", {}).get("price", {})
            quote_metadata.append({"task_id": task_id, **{name: price.get(name) for name in
                                   ("status", "quote_at_utc", "confidence", "peak", "limitation")}})
            dropped = task.get("recorder", {}).get("records_dropped_before_summary")
            if integer(dropped):
                dropped_records += dropped
                drop_coverage += 1
            if gap:
                gaps[gap] += 1
            else:
                costs.append(cost)
            if integer(task.get("duration_ms")):
                duration += task["duration_ms"]
            else:
                missing_duration += 1
            if integer(task.get("turns")):
                turns += task["turns"]
            else:
                missing_turns += 1
            reported_retries = task.get("retries", {}).get("retries")
            if integer(reported_retries):
                retries += reported_retries
                retry_count_attempts += 1
            usage = task.get("usage", {})
            if usage.get("evidence") == "measured" and integer(usage.get("cached_tokens")):
                if integer(usage.get("prompt_tokens")) and usage["cached_tokens"] <= usage["prompt_tokens"]:
                    usage_counts.update(prompt=usage["prompt_tokens"], cached=usage["cached_tokens"])
                    cache_attempts += 1
        currencies = {cost["currency"] for cost in costs}
        if len(currencies) > 1:
            gaps["mixed_currencies"] += 1
        currency = next(iter(currencies)) if len(currencies) == 1 else None
        subtotals = {name: sum(cost[name] for cost in costs if cost["currency"] == currency)
                     for name in ("uncached_input", "cached_input", "output", "total")}
        results.append({
            **{key: trial[key] for key in ("trial_id", "pair_id", "case_id", "variant")},
            "judgment": judgment, "settings": settings, "attempts": len(task_ids),
            "task_ids": task_ids,
            "currency": currency, "priced_attempts": len(costs), "billing_gaps": dict(gaps),
            "quote_metadata": quote_metadata,
            "priced_attempt_subtotal": subtotals if currency else None,
            "total_cost_estimate": subtotals["total"] if not gaps and currency else None,
            "duration_ms": duration if not missing_duration else None,
            "turns": turns if not (missing_turns or gaps["missing_task_summary"] or gaps["missing_attempt_records"]) else None,
            "interrupted_request_records": interrupted_requests,
            "reported_retries": retries if retry_count_attempts == len(task_ids) else None,
            "known_retry_subtotal": retries, "retry_count_attempts": retry_count_attempts,
            "measurement_records_dropped": dropped_records,
            "drop_count_attempts": drop_coverage,
            "cache_fraction_attempts": cache_attempts,
            "cache_fraction_prompt_tokens": usage_counts["prompt"],
            "reported_cache_hit_fraction": (usage_counts["cached"] / usage_counts["prompt"]
                                             if usage_counts["prompt"] else None),
        })
    if set(by_task) - assigned:
        raise ValueError("unassigned attempts remain; assign failed and interrupted runs too")

    cohorts = []
    for variant in sorted({trial["variant"] for trial in results}):
        selected = [trial for trial in results if trial["variant"] == variant]
        outcomes = Counter(trial["judgment"] for trial in selected)
        currencies = {trial["currency"] for trial in selected}
        complete = all(trial["total_cost_estimate"] is not None for trial in selected) and len(currencies) == 1
        total = sum(trial["total_cost_estimate"] for trial in selected) if complete else None
        settings = [json.loads(value) for value in sorted({json.dumps(trial["settings"], sort_keys=True)
                                                          for trial in selected})]
        cohort_ids = {task_id for trial in trials if trial["variant"] == variant for task_id in trial["task_ids"]}
        cohort_requests = [row for row in records if row["task_id"] in cohort_ids and row["record_type"] == "request"]
        contexts = [row for row in records if row["task_id"] in cohort_ids and row["record_type"] == "context"]
        schemas = [row["tool_schema"] for row in cohort_requests
                   if isinstance(row.get("tool_schema"), dict)]
        cohorts.append({"variant": variant, "trials": len(selected), "outcomes": dict(outcomes),
                        "settings": settings, "settings_homogeneous": len(settings) == 1,
                        "attempts": sum(trial["attempts"] for trial in selected),
                        "currency": next(iter(currencies)) if len(currencies) == 1 else None,
                        "total_cost_estimate": total,
                        "cost_per_correct_task": total / outcomes["success"]
                        if total is not None and outcomes["success"] else None,
                        "median_turns": statistics.median(trial["turns"] for trial in selected)
                        if all(trial["turns"] is not None for trial in selected) else None,
                        "request_records": len(cohort_requests),
                        "tool_schema_diagnostics": {
                            "records": len(schemas),
                            "distinct_observed_identities": len({row["identity"] for row in schemas
                                                                 if row.get("identity")}),
                            "comparable_to_previous": sum(isinstance(row.get("same_as_previous_request"), bool)
                                                          for row in schemas),
                            "observed_changes": sum(row.get("same_as_previous_request") is False
                                                    for row in schemas),
                            "basis": "noncryptographic schema-only drift indicators; not provider cache hits"},
                        "source_bytes_before_provider_encoding": {
                            source: sum(row.get("sizes", {}).get(f"{source}_bytes") or 0 for row in cohort_requests)
                            for source in SOURCES},
                        "experimental_context": {
                            "records": len(contexts),
                            "decisions": dict(Counter(row["decision"] for row in contexts)),
                            "reasons": dict(Counter(row["reason"] for row in contexts)),
                            "original_estimated_tokens": sum(row["original_estimated_tokens"] for row in contexts),
                            "sent_estimated_tokens": sum(row["sent_estimated_tokens"] for row in contexts),
                            "basis": "request-size estimates; separate from receipt.compaction and measured billing"},
                        "billing_complete": complete})

    requests = [row for row in records if row["record_type"] == "request"]
    source_bytes = {source: sum(row.get("sizes", {}).get(f"{source}_bytes") or 0 for row in requests)
                    for source in SOURCES}
    tools = defaultdict(lambda: {"calls": 0, "attempt_ids": set(), "outcomes": Counter()})
    for row in records:
        if row["record_type"] == "tool":
            tool = tools[row["tool_name"]]
            tool["calls"] += 1
            tool["attempt_ids"].add(row["task_id"])
            tool["outcomes"][row["outcome"]] += 1
    tool_report = {name: {"calls": value["calls"], "observed_attempt_share": len(value["attempt_ids"]) / len(assigned),
                          "outcomes": dict(value["outcomes"]),
                          "returned_error_fraction": value["outcomes"]["error"] / value["calls"],
                          "unclassified_fraction": value["outcomes"]["returned_unclassified"] / value["calls"]}
                   for name, value in sorted(tools.items())}
    pairs = []
    for pair_id in sorted({trial["pair_id"] for trial in results}):
        group = [trial for trial in results if trial["pair_id"] == pair_id]
        baseline = next((trial for trial in group if trial["variant"] == "baseline"), None)
        for candidate in (trial for trial in group if trial["variant"] != "baseline"):
            comparable = baseline is not None and baseline["settings"] == candidate["settings"] and baseline["case_id"] == candidate["case_id"]
            priced = comparable and baseline["currency"] == candidate["currency"] and all(
                trial["total_cost_estimate"] is not None for trial in (baseline, candidate))
            pairs.append({"pair_id": pair_id, "candidate": candidate["variant"], "settings_match": comparable,
                          "baseline_judgment": baseline["judgment"] if baseline else None,
                          "candidate_judgment": candidate["judgment"],
                          "cost_delta": candidate["total_cost_estimate"] - baseline["total_cost_estimate"] if priced else None})
    return {"schema_version": 1, "automatic_promotion": False,
            "top_level_composition_scope": "pooled across all supplied variants; cohort composition is separate",
            "cost_basis": "reported complete usage times task-start catalog quote; not an invoice",
            "source_billing_attribution": "unavailable; bytes cannot identify cached token locations",
            "source_bytes_before_provider_encoding": source_bytes,
            "mean_system_and_schema_bytes_per_request":
                (source_bytes["system"] + source_bytes["tool_schema"]) / len(requests) if requests else None,
            "request_records": len(requests), "tools": tool_report,
            "trials": results, "cohorts": cohorts, "paired_comparisons": pairs,
            "limitations": ["Receipt completion is not correctness; judgments are supplied independently.",
                            "Partial/missing billing never counts as zero; priced subsets omit unknown spend.",
                            "Attempt-level usage inside provider retries is unavailable.",
                            "Returned-error rates exclude failures hidden in unclassified tool text.",
                            "Dropped records make request/tool composition incomplete even when a task total survives.",
                            "A task-start quote may differ from rates across a pricing window.",
                            "This report does not establish statistical quality equivalence."]}


def encode_decimal(value):
    if isinstance(value, Decimal):
        return str(value)
    raise TypeError("unsupported report value")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--measurements", nargs="+", required=True, type=Path)
    parser.add_argument("--judgments", required=True, type=Path)
    args = parser.parse_args()
    try:
        report = summarize(read_records(args.measurements), json.loads(args.judgments.read_text(encoding="utf-8-sig")))
        print(json.dumps(report, indent=2, default=encode_decimal))
    except (ValueError, OSError, TypeError, KeyError, AttributeError) as error:
        parser.exit(2, f"Cannot compare: {error}\n")


if __name__ == "__main__":
    main()
