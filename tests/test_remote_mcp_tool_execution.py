from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Dict, List

import httpx

from abstractcode.react_shell import ReactShell


def _mcp_wsgi_app(environ: Dict[str, Any], start_response) -> List[bytes]:
    accept = str(environ.get("HTTP_ACCEPT") or "")
    if "application/json" not in accept or "text/event-stream" not in accept:
        start_response("406 Not Acceptable", [("Content-Type", "application/json")])
        return [
            json.dumps(
                {
                    "jsonrpc": "2.0",
                    "error": {
                        "code": -32000,
                        "message": "Not Acceptable: Client must accept both application/json and text/event-stream",
                    },
                    "id": None,
                }
            ).encode("utf-8")
        ]

    try:
        length = int(environ.get("CONTENT_LENGTH") or 0)
    except Exception:
        length = 0
    body = environ.get("wsgi.input").read(length) if environ.get("wsgi.input") else b""
    req = json.loads(body.decode("utf-8"))

    req_id = req.get("id") if isinstance(req, dict) else None
    method = req.get("method") if isinstance(req, dict) else None
    params = req.get("params") if isinstance(req, dict) and isinstance(req.get("params"), dict) else {}

    if method == "tools/list":
        long_desc = "Add two integers. " + ("More details. " * 80)
        resp = {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "tools": [
                    {
                        "name": "add",
                        "description": long_desc,
                        "inputSchema": {
                            "type": "object",
                            "properties": {"a": {"type": "integer"}, "b": {"type": "integer"}},
                            "required": ["a", "b"],
                        },
                    },
                    {
                        "name": "soft_error",
                        "description": "Always returns an error string with isError=false.",
                        "inputSchema": {"type": "object", "properties": {}},
                    }
                ]
            },
        }
    elif method == "tools/call":
        name = str(params.get("name") or "")
        arguments = params.get("arguments") if isinstance(params.get("arguments"), dict) else {}
        if name == "add":
            a = int(arguments.get("a") or 0)
            b = int(arguments.get("b") or 0)
            resp = {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"content": [{"type": "text", "text": json.dumps(a + b)}], "isError": False},
            }
        elif name == "soft_error":
            resp = {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"content": [{"type": "text", "text": "Error: soft failure"}], "isError": False},
            }
        else:
            resp = {
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"content": [{"type": "text", "text": "Unknown tool"}], "isError": True},
            }
    else:
        resp = {"jsonrpc": "2.0", "id": req_id, "error": {"code": -32601, "message": "Method not found"}}

    start_response("200 OK", [("Content-Type", "application/json")])
    return [json.dumps(resp).encode("utf-8")]


class _FakeUI:
    def append_output(self, text: str) -> None:
        _ = text


class _FakeAgent:
    def __init__(self) -> None:
        self.logic = type("_Logic", (), {"tools": []})()


class _FailingToolRunner:
    def execute(self, *, tool_calls: List[Dict[str, Any]]) -> Dict[str, Any]:  # pragma: no cover
        raise AssertionError(f"Local tool runner should not be called: {tool_calls}")


def _minimal_shell(tmp_path: Path, *, http_client: httpx.Client) -> ReactShell:
    from abstractcore.mcp import McpClient

    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _FakeUI()

    shell._auto_approve = True
    shell._approve_all_session = False

    shell._mcp_servers = {"srv": {"url": "http://mcp.test/", "headers": {}}}
    shell._mcp_client_factory = lambda server_id, entry: McpClient(url="http://mcp.test/", client=http_client)

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


def test_approve_and_execute_routes_mcp_tool_calls(tmp_path: Path) -> None:
    transport = httpx.WSGITransport(app=_mcp_wsgi_app)
    with httpx.Client(transport=transport, base_url="http://mcp.test") as http_client:
        shell = _minimal_shell(tmp_path, http_client=http_client)
        payload = shell._approve_and_execute(
            [{"name": "mcp::srv::add", "arguments": {"a": 2, "b": 3}, "call_id": "c1"}]
        )

    assert payload is not None
    assert payload["mode"] == "executed"
    assert payload["results"][0]["call_id"] == "c1"
    assert payload["results"][0]["success"] is True
    assert payload["results"][0]["output"] == 5


def test_mcp_sync_adds_tools_to_agent_catalog(tmp_path: Path) -> None:
    transport = httpx.WSGITransport(app=_mcp_wsgi_app)
    with httpx.Client(transport=transport, base_url="http://mcp.test") as http_client:
        shell = _minimal_shell(tmp_path, http_client=http_client)
        shell._sync_mcp_tools(server_id="srv")

    assert "mcp::srv::add" in shell._tool_specs
    tool_names = [getattr(t, "name", "") for t in shell._agent.logic.tools]
    assert "mcp::srv::add" in tool_names


def test_mcp_soft_error_strings_are_treated_as_failures(tmp_path: Path) -> None:
    transport = httpx.WSGITransport(app=_mcp_wsgi_app)
    with httpx.Client(transport=transport, base_url="http://mcp.test") as http_client:
        shell = _minimal_shell(tmp_path, http_client=http_client)
        payload = shell._approve_and_execute([{"name": "mcp::srv::soft_error", "arguments": {}, "call_id": "c2"}])

    assert payload is not None
    assert payload["mode"] == "executed"
    assert payload["results"][0]["call_id"] == "c2"
    assert payload["results"][0]["success"] is False
    assert "soft failure" in str(payload["results"][0].get("error") or "")
