"""Exercise real request assembly against a synthetic loopback provider.

No external provider, real key, paid call or model-quality claim. The executable
is supplied explicitly. All project and home files belong to a temporary fixture.
Usage: python -B scripts/efficiency-smoke.py --binary target/release/nh.exe
"""

import argparse
import hashlib
import importlib.util
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import threading
import uuid


MARKER = "EVIDENCE_MIDDLE_1000"
PRIVATE_FIXTURE_TEXT = (MARKER, "Read the middle evidence in fixture.txt.", "fixture.txt",
                        "sample sample", "local-fixture-placeholder", "CONTEXT_EVIDENCE_PUBLIC")


def check_report_contract(path, task_id):
    module_spec = importlib.util.spec_from_file_location("efficiency_report", Path(__file__).with_name("efficiency-report.py"))
    reporter = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(reporter)
    report = reporter.summarize(reporter.read_records([path]), {"schema_version": 1, "trials": [{
        "trial_id": "synthetic", "pair_id": "synthetic", "case_id": "synthetic",
        "variant": "baseline", "task_ids": [task_id], "judgment": "unjudged", "evidence": "",
        "settings": {"model": "fixture-model", "thinking": "none",
                     "cache_condition": "synthetic", "fixture_revision": "synthetic"}}]})
    if report["cohorts"][0]["total_cost_estimate"] is not None or report["automatic_promotion"]:
        raise AssertionError("report invented a billed cost or promoted an unjudged synthetic run")


def compact(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def run_case(binary, ranged, measured, retained=False, context=False, identity=False):
    requests = []
    request_bodies = []
    errors = []
    expected_requests = 10 if context else 3 if retained else 2
    marker_offset = 0
    task_text = "Read the middle evidence in fixture.txt."

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= 8 * 1024 * 1024:
                    raise ValueError("fixture request size refused")
                body = self.rfile.read(length)
                request_bodies.append(body)
                request = json.loads(body)
                requests.append(request)
                if len(requests) > expected_requests:
                    raise ValueError("unexpected extra fixture request")
                if context and len(requests) <= 8:
                    message = {
                        "role": "assistant", "content": None,
                        "tool_calls": [{"id": f"fixture_read_{len(requests)}", "type": "function",
                                        "function": {"name": "read_file",
                                                     "arguments": json.dumps({"path": "context.txt"})}}],
                    }
                    finish = "tool_calls"
                elif context and len(requests) == 9:
                    notices = "\n".join(message.get("content") or "" for message in request["messages"]
                                        if message["role"] == "system")
                    handles = re.findall(r"\bobs_[0-9a-f]{32}\b", notices)
                    if not handles:
                        raise ValueError("context archive notice missing")
                    message = {
                        "role": "assistant", "content": None,
                        "tool_calls": [{"id": "fixture_context_retrieve", "type": "function",
                                        "function": {"name": "read_observation", "arguments": json.dumps({
                                            "handle": handles[-1], "char_offset": 0, "char_count": 1500})}}],
                    }
                    finish = "tool_calls"
                elif not context and len(requests) == 1:
                    arguments = {"path": "fixture.txt"}
                    if ranged:
                        arguments.update(start_line=999, line_count=3)
                    message = {
                        "role": "assistant", "content": None,
                        "tool_calls": [{"id": "fixture_read", "type": "function",
                                        "function": {"name": "read_file",
                                                     "arguments": json.dumps(arguments)}}],
                    }
                    finish = "tool_calls"
                elif not context and retained and len(requests) == 2:
                    observation = next(message["content"] for message in reversed(request["messages"])
                                       if message["role"] == "tool")
                    match = re.search(r"handle=(obs_[0-9a-f]{32})", observation)
                    if match is None or MARKER in observation:
                        raise ValueError("expected a compact observation handle without middle evidence")
                    message = {
                        "role": "assistant", "content": None,
                        "tool_calls": [{"id": "fixture_retrieve", "type": "function",
                                        "function": {"name": "read_observation", "arguments": json.dumps({
                                            "handle": match.group(1), "char_offset": marker_offset - 10,
                                            "char_count": len(MARKER) + 20})}}],
                    }
                    finish = "tool_calls"
                else:
                    message = {"role": "assistant", "content": "Synthetic fixture complete."}
                    finish = "stop"
                response = compact({
                    "choices": [{"index": 0, "message": message, "finish_reason": finish}],
                    # Deliberately synthetic counters. Never report as paid usage.
                    "usage": {"prompt_tokens": 100, "completion_tokens": 10,
                              "prompt_tokens_details": {"cached_tokens": 20}},
                })
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(response)))
                self.end_headers()
                self.wfile.write(response)
            except (ValueError, KeyError, OSError) as error:
                errors.append(type(error).__name__)
                self.send_error(400, "Synthetic fixture rejected request")

    with HTTPServer(("127.0.0.1", 0), Handler) as server:
        server.timeout = 1
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory(prefix="nh-efficiency-") as temporary:
                root = Path(temporary)
                project, home = root / "project", root / "home"
                project.mkdir()
                (home / ".nosis").mkdir(parents=True)
                origin = f"http://127.0.0.1:{server.server_port}"
                vault_entry = "efficiency-fixture-" + uuid.uuid4().hex
                catalog = f'''[routes.fixture]
provider = "fixture"
model_id = "fixture-model"
base_url = "{origin}"
wire = "openai"
vault_entry = "{vault_entry}"
class = "local"
modality = ["text"]
context = {12000 if context else 128000}
max_out = 256
thinking_dialect = "none"
'''
                (project / "catalog.toml").write_text(catalog, encoding="utf-8")
                (home / ".nosis" / "catalog.toml").write_text(catalog, encoding="utf-8")
                (home / ".nosis" / "law.toml").write_text(
                    '[credential.' + vault_entry + ']\naudience = ["' + origin + '"]\n',
                    encoding="utf-8")
                fixture = "\n".join(
                    MARKER if line == 1000 else f"line {line:04}: " + "sample " * 10
                    for line in range(1, 2001)) + "\n"
                (project / "fixture.txt").write_text(fixture, encoding="utf-8")
                marker_offset = (project / "fixture.txt").read_bytes().decode("utf-8").index(MARKER)
                (project / "context.txt").write_text("CONTEXT_EVIDENCE_PUBLIC\n" + "synthetic context line\n" * 500,
                                                     encoding="utf-8")
                # Keep only OS process essentials. The unique vault entry cannot
                # accidentally select a developer's usual stored provider key.
                essentials = {"path", "systemroot", "windir", "temp", "tmp", "comspec", "pathext"}
                env = {name: value for name, value in os.environ.items() if name.lower() in essentials}
                env.update(HOME=str(home), USERPROFILE=str(home), NO_PROXY="127.0.0.1",
                           APPDATA=str(home / "AppData"), LOCALAPPDATA=str(home / "LocalAppData"),
                           XDG_CONFIG_HOME=str(home / "config"), XDG_DATA_HOME=str(home / "data"))
                env["NH_" + vault_entry.upper().replace("-", "_") + "_KEY"] = "local-fixture-placeholder"
                command = [str(binary), "run", task_text,
                           "--model", "fixture", "--read-only", "--max-turns", str(expected_requests + 1)]
                if ranged:
                    command.append("--enable-ranged-reads")
                if measured:
                    command.append("--measure-efficiency")
                if retained:
                    command.append("--retain-observations")
                if context:
                    command.extend(["--context-experiment", "extractive-v1"])
                if identity:
                    command.extend(["--identity-prompt", "compact-v1"])
                completed = subprocess.run(command, cwd=project, env=env, stdin=subprocess.DEVNULL,
                                           stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=90)
                if completed.returncode or errors or len(requests) != expected_requests:
                    # Fixture-only diagnostics; no real home, keys or provider traffic.
                    raise RuntimeError(completed.stderr.decode("utf-8", errors="replace")
                                       + f"\nfixture errors={errors}; requests={len(requests)}")
                tool_messages = [message for message in requests[-1]["messages"]
                                 if message["role"] == "tool"]
                observation = "\n".join(message.get("content") or "" for message in tool_messages)
                telemetry = project / ".nosis" / "efficiency-v1.jsonl"
                telemetry_text = telemetry.read_text(encoding="utf-8") if telemetry.exists() else ""
                assert_private_fixture_text_absent(telemetry_text)
                records = [json.loads(line) for line in telemetry_text.splitlines() if line.strip()]
                if measured != bool(records):
                    raise AssertionError("measurement activation differs from requested flag")
                if measured:
                    request_records = [row for row in records if row.get("record_type") == "request"]
                    schema_ids = []
                    for index, (row, request) in enumerate(zip(request_records, requests, strict=True)):
                        schema = row["tool_schema"]
                        if schema["tool_count"] != len(request.get("tools", [])) or schema["canonical_json_bytes"] <= 0:
                            raise AssertionError("schema diagnostics do not describe the outgoing tools")
                        if schema["same_as_previous_request"] is not (None if index == 0 else True):
                            raise AssertionError("stable schema incorrectly reported as changed")
                        if not re.fullmatch(r"fnv1a64:[0-9a-f]{16}", schema["identity"]):
                            raise AssertionError("unexpected schema identity format")
                        schema_ids.append(schema["identity"])
                    if len(set(schema_ids)) != 1:
                        raise AssertionError("stable schemas have inconsistent diagnostic identity")
                    tasks = [row for row in records if row.get("record_type") == "task"]
                    starts = [row for row in records if row.get("record_type") == "task_start"]
                    if len(tasks) != 1 or any(row.get("schema_version") != 1 for row in records):
                        raise AssertionError("unexpected measurement schema/task summaries")
                    task = tasks[0]
                    if len(starts) != 1 or starts[0]["task_id"] != task["task_id"]:
                        raise AssertionError("task start evidence is missing or mismatched")
                    if task["recorder"]["records_dropped_before_summary"] != 0:
                        raise AssertionError("fixture unexpectedly dropped measurement records")
                    if task["correctness_assessed"] or task["turns"] != expected_requests or task["retries"]["attempts"] != expected_requests:
                        raise AssertionError("task measurement misrepresents fixture execution")
                    if task["usage"] != {"usage_object_reported": True, "evidence": "measured",
                                         "prompt_tokens": 100 * expected_requests,
                                         "completion_tokens": 10 * expected_requests,
                                         "cached_tokens": 20 * expected_requests}:
                        raise AssertionError("task usage differs from synthetic provider usage")
                    check_report_contract(telemetry, task["task_id"])
                if not context and (ranged or retained) and MARKER not in observation:
                    raise AssertionError("retrieval lost middle evidence")
                if not context and not (ranged or retained) and MARKER in observation:
                    raise AssertionError("fixture no longer exercises legacy middle omission")
                if retained and list((project / ".nosis" / "observations").glob("session-*")):
                    raise AssertionError("normal exit left an owned observation session behind")
                context_records = [row for row in records if row.get("record_type") == "context"]
                if context:
                    request_sequences = [row["request_seq"] for row in records if row.get("record_type") == "request"]
                    if [row["request_seq"] for row in context_records] != request_sequences:
                        raise AssertionError("context decisions do not align with measured provider requests")
                    if not any(row["decision"] == "archive_created" for row in context_records):
                        raise AssertionError("context experiment did not create an archive")
                    if not any(row["sent_estimated_tokens"] < row["original_estimated_tokens"]
                               for row in context_records):
                        raise AssertionError("context experiment did not reduce the sent history")
                    if "CONTEXT_EVIDENCE_PUBLIC" not in tool_messages[-1]["content"]:
                        raise AssertionError("archived context was not retrievable")
                    original_system = requests[0]["messages"][0]
                    for request in requests:
                        if request["messages"][0] != original_system:
                            raise AssertionError("context experiment changed original system instructions")
                        users = [message for message in request["messages"] if message["role"] == "user"]
                        if len(users) != 1 or users[0].get("content") != task_text:
                            raise AssertionError("context experiment changed the task")
                        assert_complete_tool_pairs(request["messages"])
                return {
                    "ranged": ranged, "measured": measured, "retained": retained,
                    "context": context, "identity": identity,
                    "request_bytes": [len(body) for body in request_bodies],
                    "schemas_sha256": [hashlib.sha256(compact(request.get("tools", []))).hexdigest()
                                       for request in requests],
                    "observation_bytes": len(observation.encode("utf-8")),
                    "middle_evidence_present": MARKER in observation,
                    "telemetry_records": len(records),
                    "context_decisions": [row["decision"] for row in context_records],
                    "requests": requests,
                    "request_bodies": request_bodies,
                }
        finally:
            server.shutdown()
            thread.join(timeout=5)


def assert_private_fixture_text_absent(text):
    for private in PRIVATE_FIXTURE_TEXT:
        if private in text:
            raise AssertionError("measurement contains private fixture content")


def assert_complete_tool_pairs(messages):
    pending = set()
    for message in messages:
        if message["role"] == "assistant":
            if pending:
                raise AssertionError("assistant followed an incomplete tool group")
            pending = {call["id"] for call in (message.get("tool_calls") or [])}
        elif message["role"] == "tool":
            if message["tool_call_id"] not in pending:
                raise AssertionError("orphan tool response in provider request")
            pending.remove(message["tool_call_id"])
    if pending:
        raise AssertionError("provider request contains unanswered tool calls")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--observations", action="store_true", help="Also exercise retained observations")
    parser.add_argument("--context", action="store_true", help="Also exercise extractive context and archive retrieval")
    parser.add_argument("--identity", action="store_true", help="Also check the compact identity changes only its first clause")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    cases = [run_case(binary, ranged, measured)
             for ranged, measured in [(False, False), (False, True), (True, True)]]
    if args.observations:
        cases.append(run_case(binary, False, True, retained=True))
    if args.context:
        cases.append(run_case(binary, False, True, retained=True, context=True))
    if args.identity:
        candidate = run_case(binary, False, True, identity=True)
        for baseline, changed in zip(cases[1]["requests"], candidate["requests"], strict=True):
            original = baseline["messages"][0]["content"]
            shorter = changed["messages"][0]["content"]
            if len(shorter) >= len(original) or original.partition("\n\n")[2] != shorter.partition("\n\n")[2]:
                raise AssertionError("identity experiment did not preserve the complete policy suffix")
            # Only the first system clause may differ, even after a tool round trip.
            normalized = json.loads(json.dumps(changed))
            normalized["messages"][0]["content"] = original
            if normalized != baseline:
                raise AssertionError("identity experiment changed more than its first system clause")
        cases.append(candidate)
    # Measurement must not mutate the provider request, including instruction order.
    if cases[0]["request_bodies"] != cases[1]["request_bodies"]:
        raise AssertionError("measurement changed rendered provider requests")
    if cases[2]["request_bodies"] == cases[1]["request_bodies"]:
        raise AssertionError("request-difference control did not fire for ranged reads")
    for private in PRIVATE_FIXTURE_TEXT:
        try:
            assert_private_fixture_text_absent('"content":"' + private + '"')
        except AssertionError:
            pass
        else:
            raise AssertionError("privacy negative control did not fire")
    for case in cases:
        case.pop("requests")
        case.pop("request_bodies")
        if len(set(case["schemas_sha256"])) != 1:
            raise AssertionError("tool schemas changed between turns")
    print(json.dumps({"synthetic_only": True, "model_quality_assessed": False,
                      "real_token_savings_assessed": False, "cases": cases}, indent=2))


if __name__ == "__main__":
    main()
