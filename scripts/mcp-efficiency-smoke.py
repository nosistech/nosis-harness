"""Check real chat MCP discovery against synthetic loopback endpoints only.

No real credentials, external provider, billed usage or model-quality assertion.
Usage: python -B scripts/mcp-efficiency-smoke.py --binary target/release/nh.exe
"""

import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import uuid


DISCOVER = "mcp_discover"
INVOKE = "mcp_invoke"
EVIDENCE = "SYNTHETIC_MCP_EVIDENCE"
CORE_TOOLS = {"read_file", "glob_files", "grep_files", "write_file", "edit_file", "exec_shell"}


def encoded(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def run_case(binary, discovery, reverse=False, deny=False):
    requests, bodies, remote_calls, errors = [], [], [], []
    remote_name = "mutate_record" if deny else "lookup_17"
    exposed = "mcp__fixture__" + remote_name
    expected_requests = 3 if discovery else 2
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
                        result = {"tools": schemas}
                    elif method == "tools/call":
                        remote_calls.append(request["params"])
                        result = {"content": [{"type": "text", "text": EVIDENCE}]}
                    else:
                        raise ValueError("unexpected MCP method")
                    response = {"jsonrpc": "2.0", "id": request["id"], "result": result}
                else:
                    requests.append(request)
                    bodies.append(body)
                    step = len(requests)
                    if step > expected_requests:
                        raise ValueError("unexpected provider request")
                    if discovery and step == 1:
                        name, arguments = DISCOVER, {"query": remote_name, "offset": 0, "limit": 1}
                    elif step < expected_requests:
                        name = INVOKE if discovery else exposed
                        arguments = {"query": "public fixture"}
                        if discovery:
                            arguments = {"name": exposed, "arguments": arguments}
                    else:
                        name, arguments = None, None
                    if name:
                        message = {"role": "assistant", "content": None, "tool_calls": [{
                            "id": f"fixture_{step}", "type": "function",
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
                essentials = {"path", "systemroot", "windir", "temp", "tmp", "comspec", "pathext"}
                env = {name: value for name, value in os.environ.items() if name.lower() in essentials}
                env.update(HOME=str(home), USERPROFILE=str(home), NO_PROXY="127.0.0.1",
                           APPDATA=str(home / "AppData"), LOCALAPPDATA=str(home / "LocalAppData"),
                           XDG_CONFIG_HOME=str(home / "config"), XDG_DATA_HOME=str(home / "data"))
                env["NH_" + entry.upper().replace("-", "_") + "_KEY"] = "local-fixture-placeholder"
                command = [str(binary), "chat", "--model", "fixture"]
                if discovery:
                    command.append("--mcp-discovery")
                # Non-terminal stdin cannot grant approval. Do not send a fake
                # approval answer: it would become a second chat task instead.
                user_input = b"Use the synthetic MCP fixture.\n/quit\n"
                completed = subprocess.run(command, cwd=project, env=env, input=user_input,
                                           stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=90)
                if completed.returncode or errors or len(requests) != expected_requests:
                    raise RuntimeError(completed.stderr.decode("utf-8", errors="replace")
                                       + f"\nfixture errors={errors}; requests={len(requests)}")
                tools = requests[0]["tools"]
                names = {tool["function"]["name"] for tool in tools}
                if not CORE_TOOLS <= names:
                    raise AssertionError("core tools missing from first request")
                if discovery:
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
                if deny:
                    if remote_calls or EVIDENCE in tool_output:
                        raise AssertionError("unapproved mutation reached remote tool")
                    if b"approval refused" not in completed.stderr:
                        raise AssertionError("mutation fixture did not reach the approval boundary")
                elif (len(remote_calls) != 1
                      or remote_calls[0].get("name") != remote_name
                      or remote_calls[0].get("arguments") != {"query": "public fixture"}
                      or EVIDENCE not in tool_output):
                    raise AssertionError("MCP invocation/result did not match the discovered adapter")
                return {"discovery": discovery, "reverse_server_order": reverse, "approval_refused_mutation": deny,
                        "request_bytes": [len(body) for body in bodies],
                        "tool_schema_bytes": len(encoded(tools)), "tools": len(tools),
                        "schema_sha256": hashlib.sha256(encoded(tools)).hexdigest(),
                        "remote_calls": len(remote_calls)}
        finally:
            server.shutdown()
            thread.join(timeout=5)


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
        cases.extend([run_case(binary, True), run_case(binary, True, deny=True)])
    print(json.dumps({"synthetic_only": True, "model_quality_assessed": False,
                      "real_token_savings_assessed": False, "cases": cases}, indent=2))


if __name__ == "__main__":
    main()
