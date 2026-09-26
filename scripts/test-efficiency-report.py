"""Accounting regressions: unknown costs and failed attempts must not disappear."""

from copy import deepcopy
from decimal import Decimal
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("efficiency_report", Path(__file__).with_name("efficiency-report.py"))
report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(report)


def task(task_id="a"):
    return {"schema_version": 1, "record_type": "task", "task_id": task_id,
            "route": {"model_id": "fixture", "price": {
                "status": "captured", "unit": "per_million_tokens", "currency": "USD",
                "input_cache_miss": 2, "input_cache_hit": 0.5, "output": 4, "peak": False}},
            "usage": {"usage_object_reported": True, "evidence": "measured",
                      "prompt_tokens": 1000, "cached_tokens": 400, "completion_tokens": 100},
            "receipt_outcome": "pass", "correctness_assessed": False,
            "duration_ms": 3000, "turns": 2, "retries": {"retries": 1}}


def trial(task_ids=None, **overrides):
    value = {"trial_id": "case-1-base", "pair_id": "case-1", "case_id": "delivery",
             "variant": "baseline", "task_ids": task_ids or ["a"], "judgment": "success",
             "evidence": "independent-checks.txt",
             "settings": {"model": "fixture", "thinking": "normal",
                          "cache_condition": "uncontrolled", "fixture_revision": "fixture-v1"}}
    value.update(overrides)
    return value


def summarize(records, trials=None):
    return report.summarize(records, {"schema_version": 1, "trials": trials or [trial()]})


class AccountingTests(unittest.TestCase):
    def test_split_prices_and_failed_repair_attempts_are_in_total(self):
        failed = task("a")
        failed["receipt_outcome"] = "fail"
        result = summarize([failed, task("b")], [trial(["a", "b"])])
        self.assertEqual(result["cohorts"][0]["cost_per_correct_task"], Decimal("0.0036"))
        self.assertEqual(result["trials"][0]["reported_retries"], 2)
        failed["receipt_outcome"] = "pass"
        self.assertEqual(result, summarize([failed, task("b")], [trial(["a", "b"])]))

    def test_receipt_pass_is_not_correctness(self):
        result = summarize([task()], [trial(judgment="failure")])
        self.assertIsNone(result["cohorts"][0]["cost_per_correct_task"])
        self.assertEqual(result["cohorts"][0]["outcomes"], {"failure": 1})

    def test_unknown_failed_attempt_blocks_complete_cost(self):
        failed = task("a")
        failed["usage"]["evidence"] = "unknown"
        result = summarize([failed, task("b")], [trial(["a", "b"])])
        self.assertIsNone(result["cohorts"][0]["total_cost_estimate"])
        self.assertEqual(result["trials"][0]["priced_attempt_subtotal"]["total"], Decimal("0.0018"))

    def test_missing_cache_split_is_not_zero_hits(self):
        row = task()
        row["usage"]["cached_tokens"] = None
        self.assertIsNone(summarize([row])["trials"][0]["total_cost_estimate"])

    def test_invalid_or_partial_usage_cannot_be_priced(self):
        for field, value in [("cached_tokens", 1001), ("prompt_tokens", -1),
                             ("completion_tokens", True), ("evidence", "partial")]:
            with self.subTest(field=field):
                row = task()
                row["usage"][field] = value
                self.assertIsNone(report.priced_usage(row)[0])

    def test_bad_prices_cannot_be_priced(self):
        for value in [None, -1, "NaN", "Infinity"]:
            row = task()
            row["route"]["price"]["output"] = value
            self.assertIsNone(report.priced_usage(row)[0])

    def test_request_and_task_usage_are_not_double_counted(self):
        request = deepcopy(task())
        request.update(record_type="request", request_seq=1, sizes={"system_bytes": 80})
        result = summarize([request, task()])
        self.assertEqual(result["cohorts"][0]["total_cost_estimate"], Decimal("0.0018"))
        self.assertEqual(result["source_bytes_before_provider_encoding"]["system"], 80)

    def test_unassigned_attempt_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "unassigned"):
            summarize([task(), task("failure-that-must-not-be-hidden")])

    def test_interrupted_run_has_no_zero_cost(self):
        row = task()
        row.update(record_type="request", request_seq=1)
        result = summarize([row], [trial(judgment="failure")])
        self.assertEqual(result["trials"][0]["billing_gaps"], {"missing_task_summary": 1})
        self.assertIsNone(result["cohorts"][0]["total_cost_estimate"])

    def test_mixed_currency_is_never_added(self):
        other = task("b")
        other["route"]["price"]["currency"] = "CNY"
        result = summarize([task(), other], [trial(["a", "b"])])
        self.assertIsNone(result["cohorts"][0]["total_cost_estimate"])

    def test_settings_difference_blocks_pair_delta(self):
        candidate = trial(["b"], trial_id="case-1-candidate", variant="ranges")
        candidate["settings"]["thinking"] = "high"
        result = summarize([task(), task("b")], [trial(), candidate])
        self.assertFalse(result["paired_comparisons"][0]["settings_match"])
        self.assertIsNone(result["paired_comparisons"][0]["cost_delta"])

    def test_duplicate_log_and_corrupt_line_fail_closed(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "measurements.jsonl"
            path.write_text(json.dumps(task()) + "\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "invalid measurement"):
                report.read_records([path, path])
            path.write_text('{"private-content":', encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "invalid measurement") as error:
                report.read_records([path])
            self.assertNotIn("private-content", str(error.exception))

    def test_unknown_billing_buckets_require_new_accounting(self):
        row = task()
        row["usage"]["separately_billed_reasoning_tokens"] = 500
        self.assertEqual(report.priced_usage(row)[1], "unknown_usage_bucket")
        row = task()
        row["route"]["price"]["cache_write"] = 2
        self.assertEqual(report.priced_usage(row)[1], "unknown_price_bucket")

    def test_interrupted_model_mismatch_is_rejected(self):
        row = task()
        row.update(record_type="request", request_seq=1)
        row["route"]["model_id"] = "other-fixture"
        with self.assertRaisesRegex(ValueError, "model differs"):
            summarize([row])

    def test_cache_ratio_has_coverage_and_incomplete_turns_stay_unknown(self):
        missing = task("b")
        missing.update(record_type="request", request_seq=1)
        result = summarize([task(), missing], [trial(["a", "b"])])
        self.assertEqual(result["trials"][0]["cache_fraction_attempts"], 1)
        self.assertIsNone(result["cohorts"][0]["median_turns"])

    def test_composition_is_separate_by_variant(self):
        records = [task(), task("b")]
        for task_id, size in [("a", 200), ("b", 20)]:
            row = task(task_id)
            row.update(record_type="request", request_seq=1, sizes={"tool_bytes": size})
            records.append(row)
        candidate = trial(["b"], trial_id="candidate", variant="ranges")
        result = summarize(records, [trial(), candidate])
        amounts = {cohort["variant"]: cohort["source_bytes_before_provider_encoding"]["tool"]
                   for cohort in result["cohorts"]}
        self.assertEqual(amounts, {"baseline": 200, "ranges": 20})

    def test_start_without_any_response_is_retained_as_unknown(self):
        row = task()
        row["record_type"] = "task_start"
        result = summarize([row], [trial(judgment="failure")])
        self.assertEqual(result["trials"][0]["billing_gaps"], {"missing_task_summary": 1})
        self.assertIsNone(result["cohorts"][0]["total_cost_estimate"])

    def test_dropped_request_metadata_does_not_erase_complete_receipt_usage(self):
        row = task()
        row["recorder"] = {"records_dropped_before_summary": 1}
        result = summarize([row])
        self.assertEqual(result["trials"][0]["measurement_records_dropped"], 1)
        self.assertEqual(result["trials"][0]["drop_count_attempts"], 1)
        self.assertEqual(result["cohorts"][0]["total_cost_estimate"], Decimal("0.0018"))

    def test_fully_unrecorded_attempt_is_explicitly_unknown(self):
        result = summarize([task()], [trial(["a", "id-from-warning-with-no-records"])])
        self.assertEqual(result["trials"][0]["billing_gaps"], {"missing_attempt_records": 1})
        self.assertIsNone(result["cohorts"][0]["total_cost_estimate"])

    def test_schema_drift_is_separate_from_billing_and_reports_coverage(self):
        records = [task()]
        for index, (identity, same) in enumerate([("one", None), ("one", True), ("two", False)], 1):
            row = task()
            row.update(record_type="request", request_seq=index,
                       tool_schema={"identity": identity, "same_as_previous_request": same})
            records.append(row)
        result = summarize(records)
        diagnostic = result["cohorts"][0]["tool_schema_diagnostics"]
        self.assertEqual(diagnostic["records"], 3)
        self.assertEqual(diagnostic["distinct_observed_identities"], 2)
        self.assertEqual(diagnostic["comparable_to_previous"], 2)
        self.assertEqual(diagnostic["observed_changes"], 1)
        self.assertEqual(result["cohorts"][0]["total_cost_estimate"], Decimal("0.0018"))

    def test_missing_turns_and_retries_are_not_zero(self):
        row = task()
        del row["turns"]
        del row["retries"]
        result = summarize([row])
        self.assertIsNone(result["cohorts"][0]["median_turns"])
        self.assertIsNone(result["trials"][0]["reported_retries"])
        self.assertEqual(result["trials"][0]["retry_count_attempts"], 0)

    def test_peak_is_selected_quote_annotation_not_a_second_rate(self):
        row = task()
        row["route"]["price"].update(peak=True, confidence="estimate", quote_at_utc="synthetic-time",
                                    limitation="task-start quote")
        result = summarize([row])
        self.assertEqual(result["cohorts"][0]["total_cost_estimate"], Decimal("0.0018"))
        self.assertTrue(result["trials"][0]["quote_metadata"][0]["peak"])
        self.assertEqual(result["trials"][0]["quote_metadata"][0]["confidence"], "estimate")
        row["route"]["price"]["peak"] = {"output": 99}
        self.assertEqual(report.priced_usage(row)[1], "unsupported_peak_quote")
        del row["route"]["price"]["peak"]
        self.assertEqual(report.priced_usage(row)[1], "unsupported_peak_quote")


if __name__ == "__main__":
    unittest.main()
