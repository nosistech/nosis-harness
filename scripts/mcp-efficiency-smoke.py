"""Check real chat MCP discovery against synthetic loopback endpoints only.

No real credentials, external provider, billed usage or model-quality assertion.
Usage: python -B scripts/mcp-efficiency-smoke.py --binary target/release/nh.exe
"""

import argparse
import hashlib
import importlib.util
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import time
import uuid


DISCOVER = "mcp_discover"
INVOKE = "mcp_invoke"
EVIDENCE = "SYNTHETIC_MCP_EVIDENCE"
CORE_TOOLS = {"read_file", "glob_files", "grep_files", "write_file", "edit_file", "exec_shell"}


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def request_identity(requests, root, origin, entry):
    """Compare payloads after replacing only controlled ephemeral fixture values."""
    def normalized(value):
        if isinstance(value, str):
            return (value.replace(str(root), "<fixture-root>")
                    .replace(root.as_posix(), "<fixture-root>")
                    .replace(origin, "<fixture-origin>").replace(entry, "<fixture-entry>"))
        if isinstance(value, list):
            return [normalized(item) for item in value]
        if isinstance(value, dict):
            return {key: normalized(item) for key, item in value.items()}
        return value
    return hashlib.sha256(encoded(normalized(requests))).hexdigest()


def fingerprint(value):
    """Match the review store's canonical object ordering for public fixtures."""
    canonical = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def write_review(home, origin, schemas, enabled=True):
    # This trusted test fixture supplies the operator's saved review. Production
    # reviews come from the terminal; repository settings cannot authorize tools.
    connection = {"url": origin + "/mcp", "protocol": "2026-07-28",
                  "auth": {"kind": "none"}, "scopes": []}
    snapshot = [{"descriptor": schema, "fingerprint": fingerprint(schema)}
                for schema in sorted(schemas, key=lambda item: item["name"])]
    state = {"version": 1, "server": "fixture", "connection": connection,
             "connection_fingerprint": fingerprint(connection), "snapshot": snapshot,
             "enabled": [item["descriptor"]["name"] for item in snapshot] if enabled else []}
    directory = home / ".nosis" / "mcp-reviews"
    directory.mkdir()
    (directory / "fixture.json").write_bytes(encoded(state))


def run_case(binary, discovery, reverse=False, deny=False, changed=False, review_state="enabled",
             measured=False, task_count=1):
    requests, bodies, remote_calls, remote_lists, errors = [], [], [], [], []
    remote_name = "mutate_record" if deny else "lookup_17"
    exposed = "mcp__fixture__" + remote_name
    enabled = review_state == "enabled"
    requests_per_task = (3 if discovery else 2) if enabled else 1
    expected_requests = requests_per_task * task_count
    schemas = [{"name": f"lookup_{index:02}",
                "description": f"Synthetic lookup {index:02}. " + "Public fixture documentation. " * 12,
                "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}},
                                "required": ["query"]},
                "annotations": {"readOnlyHint": True}}
               for index in range(20)]
    schemas.append({**schemas[0], "name": "mutate_record", "annotations": {"readOnlyHint": False}})
    if reverse:
        schemas.reverse()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= 8 * 1024 * 1024:
                    raise ValueError("invalid synthetic request size")
                body = self.rfile.read(length)
                request = json.loads(body)
                if self.path == "/mcp":
                    method = request["method"]
                    if method == "tools/list":
                        remote_lists.append(method)
                        result = {"tools": schemas}
                        if changed and len(remote_lists) > 1:
                            result = {"tools": [dict(schema, description="Changed after review")
                                                if schema["name"] == remote_name else schema
                                                for schema in schemas]}
                    elif method == "tools/call":
                        remote_calls.append(request["params"])
                        result = {"content": [{"type": "text", "text": EVIDENCE}]}
                    else:
                        raise ValueError("unexpected MCP method")
                    response = {"jsonrpc": "2.0", "id": request["id"], "result": result}
                else:
                    requests.append(request)
                    bodies.append(body)
                    absolute_step = len(requests)
                    step = (absolute_step - 1) % requests_per_task + 1
                    if absolute_step > expected_requests:
                        raise ValueError("unexpected provider request")
                    if enabled and discovery and step == 1:
                        name, arguments = DISCOVER, {"query": remote_name, "offset": 0, "limit": 1}
                    elif step < requests_per_task:
                        name = INVOKE if discovery else exposed
                        arguments = {"query": "public fixture"}
                        if discovery:
                            arguments = {"name": exposed, "arguments": arguments}
                    else:
                        name, arguments = None, None
                    if name:
                        message = {"role": "assistant", "content": None, "tool_calls": [{
                            "id": f"fixture_{absolute_step}", "type": "function",
                            "function": {"name": name, "arguments": json.dumps(arguments)}}]}
                    else:
                        message = {"role": "assistant", "content": "Synthetic chat fixture complete."}
                    response = {"choices": [{"index": 0, "message": message,
                                              "finish_reason": "tool_calls" if name else "stop"}],
                                "usage": {"prompt_tokens": 100, "completion_tokens": 10,
                                          "prompt_tokens_details": {"cached_tokens": 20}}}
                payload = encoded(response)
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(payload)))
                self.end_headers()
                self.wfile.write(payload)
            except (KeyError, ValueError, OSError) as error:
                errors.append(type(error).__name__)
                self.send_error(400, "Synthetic fixture rejected request")

    with HTTPServer(("127.0.0.1", 0), Handler) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory(prefix="nh-mcp-efficiency-") as temporary:
                root = Path(temporary)
                project, home = root / "project", root / "home"
                project.mkdir()
                (home / ".nosis").mkdir(parents=True)
                origin = f"http://127.0.0.1:{server.server_port}"
                entry = "mcp-efficiency-fixture-" + uuid.uuid4().hex
                catalog = f'''[routes.fixture]
provider = "fixture"
model_id = "fixture-model"
base_url = "{origin}"
wire = "openai"
vault_entry = "{entry}"
class = "local"
modality = ["text"]
context = 128000
max_out = 256
thinking_dialect = "none"
'''
                (project / "catalog.toml").write_text(catalog, encoding="utf-8")
                (home / ".nosis" / "catalog.toml").write_text(catalog, encoding="utf-8")
                (home / ".nosis" / "law.toml").write_text(
                    f'[credential.{entry}]\naudience = ["{origin}"]\n', encoding="utf-8")
                (home / ".nosis" / "mcp.toml").write_text(
                    f'[servers.fixture]\nurl = "{origin}/mcp"\nauth = "none"\ntrust = "auto"\n',
                    encoding="utf-8")
                if review_state != "missing":
                    write_review(home, origin, schemas, enabled=enabled)
                essentials = {"path", "systemroot", "windir", "temp", "tmp", "comspec", "pathext"}
                env = {name: value for name, value in os.environ.items() if name.lower() in essentials}
                env.update(HOME=str(home), USERPROFILE=str(home), NO_PROXY="127.0.0.1",
                           APPDATA=str(home / "AppData"), LOCALAPPDATA=str(home / "LocalAppData"),
                           XDG_CONFIG_HOME=str(home / "config"), XDG_DATA_HOME=str(home / "data"))
                env["NH_" + entry.upper().replace("-", "_") + "_KEY"] = "local-fixture-placeholder"
                command = [str(binary), "chat", "--model", "fixture"]
                if discovery:
                    command.append("--mcp-discovery")
                if measured:
                    command.append("--measure-efficiency")
                # Non-terminal stdin cannot grant approval. Do not send a fake
                # approval answer: it would become a second chat task instead.
                user_input = (b"Use the synthetic MCP fixture.\n/tools\n" * task_count) + b"/quit\n"
                started = time.perf_counter()
                completed = subprocess.run(command, cwd=project, env=env, input=user_input,
                                           stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=90)
                local_duration_ms = (time.perf_counter() - started) * 1000
                if completed.returncode or errors or len(requests) != expected_requests:
                    raise RuntimeError(completed.stderr.decode("utf-8", errors="replace")
                                       + f"\nfixture errors={errors}; requests={len(requests)}")
                tools = requests[0]["tools"]
                names = {tool["function"]["name"] for tool in tools}
                if not CORE_TOOLS <= names:
                    raise AssertionError("core tools missing from first request")
                if not enabled:
                    if names != CORE_TOOLS or remote_calls or remote_lists:
                        raise AssertionError("disabled or unreviewed server was contacted/offered tools")
                elif discovery:
                    if not {DISCOVER, INVOKE} <= names or exposed in names:
                        raise AssertionError("discovery did not replace eager MCP schemas")
                    discovery_output = requests[1]["messages"][-1]["content"]
                    if exposed not in discovery_output or "query" not in discovery_output:
                        raise AssertionError("discovery did not return the requested schema")
                elif exposed not in names:
                    raise AssertionError("eager tools unexpectedly absent")
                if any(request["tools"] != tools for request in requests):
                    raise AssertionError("schema registry changed mid-task")
                tool_output = requests[-1]["messages"][-1]["content"]
                if not enabled:
                    pass
                elif changed:
                    if remote_calls or EVIDENCE in tool_output or "review" not in tool_output.lower():
                        raise AssertionError("changed definition was not refused before tools/call")
                    if len(remote_lists) < 2:
                        raise AssertionError("definition was not fetched again before invocation")
                elif deny:
                    if remote_calls or EVIDENCE in tool_output:
                        raise AssertionError("unapproved mutation reached remote tool")
                    if b"approval refused" not in completed.stderr:
                        raise AssertionError("mutation fixture did not reach the approval boundary")
                elif (len(remote_calls) != task_count
                      or any(call.get("name") != remote_name
                             or call.get("arguments") != {"query": "public fixture"}
                             for call in remote_calls)
                      or EVIDENCE not in tool_output):
                    raise AssertionError("MCP invocation/result did not match the discovered adapter")
                measurement = check_measurement(project, measured, task_count, requests_per_task,
                                                len(remote_calls), discovery, enabled)
                return {"discovery": discovery, "reverse_server_order": reverse, "approval_refused_mutation": deny,
                        "changed_definition_refused": changed,
                        "review_state": review_state,
                        "request_bytes": [len(body) for body in bodies],
                        "normalized_request_sha256": request_identity(requests, root, origin, entry),
                        "tool_schema_bytes": len(encoded(tools)), "tools": len(tools),
                        "schema_sha256": hashlib.sha256(encoded(tools)).hexdigest(),
                        "remote_calls": len(remote_calls), "remote_lists": len(remote_lists),
                        "measurement_enabled": measured, "task_count": task_count,
                        "observed_local_duration_ms": local_duration_ms,
                        "measurement": measurement}
        finally:
            server.shutdown()
            thread.join(timeout=5)


def check_measurement(project, measured, task_count, requests_per_task, remote_calls, discovery, enabled):
    path = project / ".nosis" / "efficiency-v1.jsonl"
    if not measured:
        if path.exists():
            raise AssertionError("chat measurement wrote a log with the flag disabled")
        return None
    text = path.read_text(encoding="utf-8")
    for private in ("Use the synthetic MCP fixture.", "public fixture", EVIDENCE,
                    "local-fixture-placeholder", "Synthetic chat fixture complete."):
        if private in text:
            raise AssertionError("chat measurement retained private fixture content")
    rows = [json.loads(line) for line in text.splitlines() if line.strip()]
    starts = [row for row in rows if row["record_type"] == "task_start"]
    tasks = [row for row in rows if row["record_type"] == "task"]
    ids = {row["task_id"] for row in starts}
    if len(starts) != task_count or len(ids) != task_count or len(tasks) != task_count:
        raise AssertionError("chat tasks/slash commands did not have separate measurement lifetimes")
    if {row["task_id"] for row in rows} != ids or {row["task_id"] for row in tasks} != ids:
        raise AssertionError("request/tool/task measurement identities differ")
    for task in tasks:
        task_rows = [row for row in rows if row["task_id"] == task["task_id"]]
        requests = [row for row in task_rows if row["record_type"] == "request"]
        tools = [row for row in task_rows if row["record_type"] == "tool"]
        if len(requests) != requests_per_task or task["turns"] != requests_per_task:
            raise AssertionError("chat task measurement has wrong request/turn counts")
        if [row["request_seq"] for row in requests] != list(range(1, requests_per_task + 1)):
            raise AssertionError("chat request sequence did not reset at the task boundary")
        expected_tools = (2 if discovery else 1) if enabled else 0
        if len(tools) != expected_tools:
            raise AssertionError("chat measurement skipped or duplicated a tool")
        if [row["tool_seq"] for row in tools] != list(range(1, expected_tools + 1)):
            raise AssertionError("chat tool sequence did not reset at the task boundary")
        if tools and tools[0]["repetition"]["comparison_status"] != "first_call":
            raise AssertionError("chat repetition state leaked across tasks")
        if task["correctness_assessed"] or task["recorder"]["records_dropped_before_summary"]:
            raise AssertionError("synthetic chat invented correctness or dropped measurements")
        if task["usage"] != {"usage_object_reported": True, "evidence": "measured",
                             "prompt_tokens": 100 * requests_per_task,
                             "completion_tokens": 10 * requests_per_task,
                             "cached_tokens": 20 * requests_per_task}:
            raise AssertionError("chat measurement differs from synthetic provider usage")
    module_spec = importlib.util.spec_from_file_location(
        "efficiency_report", Path(__file__).with_name("efficiency-report.py"))
    reporter = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(reporter)
    report = reporter.summarize(reporter.read_records([path]), {"schema_version": 1, "trials": [
        {"trial_id": f"synthetic-chat-{index}", "pair_id": f"synthetic-chat-{index}",
         "case_id": "synthetic-chat", "variant": "discovery" if discovery else "eager",
         "task_ids": [task["task_id"]], "judgment": "unjudged", "evidence": "",
         "settings": {"model": "fixture-model", "thinking": "none",
                      "cache_condition": "synthetic", "fixture_revision": "synthetic-chat"}}
        for index, task in enumerate(tasks)]})
    if report["automatic_promotion"] or any(cohort["total_cost_estimate"] is not None
                                            for cohort in report["cohorts"]):
        raise AssertionError("chat report invented billed costs or promoted synthetic runs")
    return {"tasks": len(tasks), "requests": sum(row["record_type"] == "request" for row in rows),
            "tools": sum(row["record_type"] == "tool" for row in rows),
            "provider_usage_is_synthetic": True, "remote_actions": remote_calls}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--baseline-only", action="store_true")
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    cases = [run_case(binary, False), run_case(binary, False, reverse=True)]
    if cases[0]["schema_sha256"] != cases[1]["schema_sha256"]:
        raise AssertionError("server response order changed eager model schema order")
    if not args.baseline_only:
        cases.extend([run_case(binary, True), run_case(binary, True, deny=True),
                      run_case(binary, False, changed=True), run_case(binary, True, changed=True),
                      run_case(binary, False, review_state="none"),
                      run_case(binary, True, review_state="none"),
                      run_case(binary, False, review_state="missing")])
        for discovery in (False, True):
            enabled_measurement = run_case(binary, discovery, measured=True, task_count=2)
            disabled_measurement = run_case(binary, discovery, task_count=2)
            for key in ("request_bytes", "normalized_request_sha256", "schema_sha256",
                        "remote_calls", "remote_lists"):
                if enabled_measurement[key] != disabled_measurement[key]:
                    raise AssertionError("measurement changed rendered chat requests or tool execution")
            cases.extend([disabled_measurement, enabled_measurement])
    print(json.dumps({"synthetic_only": True, "model_quality_assessed": False,
                      "real_token_savings_assessed": False, "cases": cases}, indent=2))


if __name__ == "__main__":
    main()
