from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Dict, List

import httpx

from abstractcode.react_shell import ReactShell


class _SpinnerUI:
    def __init__(self) -> None:
        self.spinner: str | None = None
        self.set_calls: list[str] = []
        self.cleared = 0

    def append_output(self, text: str) -> None:
        _ = text

    def set_spinner(self, text: str) -> None:
        self.spinner = str(text)
        self.set_calls.append(self.spinner)

    def clear_spinner(self) -> None:
        self.cleared += 1
        self.spinner = None


def _mcp_wsgi_app(environ: Dict[str, Any], start_response) -> List[bytes]:
    try:
        length = int(environ.get("CONTENT_LENGTH") or 0)
    except Exception:
        length = 0
    body = environ.get("wsgi.input").read(length) if environ.get("wsgi.input") else b""
    req = json.loads(body.decode("utf-8"))

    req_id = req.get("id") if isinstance(req, dict) else None
    method = req.get("method") if isinstance(req, dict) else None

    if method == "tools/list":
        resp = {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {"tools": [{"name": "add", "description": "add", "inputSchema": {"type": "object"}}]},
        }
    else:
        resp = {"jsonrpc": "2.0", "id": req_id, "error": {"code": -32601, "message": "Method not found"}}

    start_response("200 OK", [("Content-Type", "application/json")])
    return [json.dumps(resp).encode("utf-8")]


def test_on_step_act_sets_mcp_spinner_word() -> None:
    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _SpinnerUI()
    shell._turn_trace = []
    shell._pending_tool_markers = []
    shell._pending_tool_metas = []

    ReactShell._on_step(shell, "act", {"tool": "mcp::srv::add", "args": {"a": 1}, "call_id": "c1"})

    assert shell._ui.spinner == "MCP"


def test_mcp_sync_sets_and_clears_spinner(tmp_path: Path) -> None:
    from abstractcore.mcp import McpClient

    transport = httpx.WSGITransport(app=_mcp_wsgi_app)
    with httpx.Client(transport=transport, base_url="http://mcp.test") as http_client:
        shell = ReactShell.__new__(ReactShell)
        shell._color = False
        shell._output_lines = []
        shell._ui = _SpinnerUI()
        shell._auto_approve = True
        shell._approve_all_session = False
        shell._mcp_servers = {"srv": {"url": "http://mcp.test/", "headers": {}}}
        shell._mcp_client_factory = lambda server_id, entry: McpClient(url="http://mcp.test/", client=http_client)
        shell._tool_runner = None
        shell._tool_specs = {}
        shell._agent = type("_Agent", (), {"logic": type("_Logic", (), {"tools": []})()})()
        shell._config_file = tmp_path / "cfg.json"

        shell._sync_mcp_tools(server_id="srv")

    assert "MCP" in shell._ui.set_calls
    assert shell._ui.cleared >= 1
    assert shell._ui.spinner is None
