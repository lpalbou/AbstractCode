from __future__ import annotations

import sys
from pathlib import Path
from typing import Dict, List

from abstractcode.react_shell import ReactShell


def _write_stdio_stub(path: Path) -> None:
    path.write_text(
        """
import json
import sys

initialized = False

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\\n")
    sys.stdout.flush()

for line in sys.stdin:
    line = (line or "").strip()
    if not line:
        continue
    try:
        req = json.loads(line)
    except Exception:
        continue
    if not isinstance(req, dict):
        continue

    req_id = req.get("id")
    method = req.get("method")
    params = req.get("params") if isinstance(req.get("params"), dict) else {}

    if req.get("jsonrpc") != "2.0" or not method:
        send({"jsonrpc": "2.0", "id": req_id, "error": {"code": -32600, "message": "Invalid Request"}})
        continue

    if req_id is None:
        continue

    if method == "initialize":
        initialized = True
        send(
            {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "protocolVersion": params.get("protocolVersion") or "2025-11-25",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "stub"},
                },
            }
        )
        continue

    if not initialized:
        send({"jsonrpc": "2.0", "id": req_id, "error": {"code": -32002, "message": "Not initialized"}})
        continue

    if method == "tools/list":
        send(
            {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "tools": [
                        {
                            "name": "add",
                            "description": "Add two integers.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {"a": {"type": "integer"}, "b": {"type": "integer"}},
                                "required": ["a", "b"],
                            },
                        }
                    ]
                },
            }
        )
        continue

    if method == "tools/call":
        name = str(params.get("name") or "")
        args = params.get("arguments") if isinstance(params.get("arguments"), dict) else {}
        if name != "add":
            send(
                {
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {"content": [{"type": "text", "text": "Unknown tool"}], "isError": True},
                }
            )
            continue
        a = int(args.get("a") or 0)
        b = int(args.get("b") or 0)
        send(
            {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"content": [{"type": "text", "text": json.dumps(a + b)}], "isError": False},
            }
        )
        continue

    send({"jsonrpc": "2.0", "id": req_id, "error": {"code": -32601, "message": "Method not found"}})
""".lstrip()
    )


class _FakeUI:
    def append_output(self, text: str) -> None:
        _ = text


class _FakeAgent:
    def __init__(self) -> None:
        self.logic = type("_Logic", (), {"tools": []})()


class _FailingToolRunner:
    def execute(self, *, tool_calls: List[Dict[str, str]]):  # pragma: no cover
        raise AssertionError(f"Local tool runner should not be called: {tool_calls}")


def _minimal_shell(tmp_path: Path, *, command: List[str]) -> ReactShell:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _FakeUI()

    shell._auto_approve = True
    shell._approve_all_session = False

    shell._mcp_servers = {"srv": {"transport": "stdio", "command": command}}
    shell._mcp_client_factory = None

    shell._tool_runner = _FailingToolRunner()
    shell._tool_specs = {}
    shell._agent = _FakeAgent()

    shell._config_file = tmp_path / "cfg.json"
    shell._max_tokens = None
    shell._max_history_messages = -1
    shell._allowed_tools = None
    shell._tool_prompt_examples = True
    shell._plan_mode = False
    shell._review_mode = False
    shell._review_max_rounds = 1

    shell._last_execute_command = None
    shell._last_execute_command_result = None
    shell._last_mutating_tool_call_key = None
    shell._last_mutating_tool_call_result = None
    return shell


def test_approve_and_execute_routes_stdio_mcp_tool_calls(tmp_path: Path) -> None:
    server = tmp_path / "mcp_stdio_stub.py"
    _write_stdio_stub(server)

    shell = _minimal_shell(tmp_path, command=[sys.executable, "-u", str(server)])
    payload = shell._approve_and_execute(
        [{"name": "mcp::srv::add", "arguments": {"a": 2, "b": 3}, "call_id": "c1"}]
    )

    assert payload is not None
    assert payload["mode"] == "executed"
    assert payload["results"][0]["call_id"] == "c1"
    assert payload["results"][0]["success"] is True
    assert payload["results"][0]["output"] == 5


def test_mcp_sync_adds_stdio_tools_to_agent_catalog(tmp_path: Path) -> None:
    server = tmp_path / "mcp_stdio_stub.py"
    _write_stdio_stub(server)

    shell = _minimal_shell(tmp_path, command=[sys.executable, "-u", str(server)])
    shell._sync_mcp_tools(server_id="srv")

    assert "mcp::srv::add" in shell._tool_specs
    tool_names = [getattr(t, "name", "") for t in shell._agent.logic.tools]
    assert "mcp::srv::add" in tool_names

