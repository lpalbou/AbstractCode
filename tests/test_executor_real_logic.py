from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Dict, List

import httpx
import pytest

from abstractcode.react_shell import ReactShell


def _mcp_wsgi_app(environ: Dict[str, Any], start_response) -> List[bytes]:
    try:
        length = int(environ.get("CONTENT_LENGTH") or 0)
    except Exception:
        length = 0
    body = environ.get("wsgi.input").read(length) if environ.get("wsgi.input") else b""
    req = json.loads(body.decode("utf-8")) if body else {}

    req_id = req.get("id") if isinstance(req, dict) else None
    method = req.get("method") if isinstance(req, dict) else None

    if method == "tools/list":
        resp = {
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "tools": [
                    {
                        "name": "read_file",
                        "description": "Read a file from the remote machine.",
                        "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]},
                    },
                    {
                        "name": "list_files",
                        "description": "List files on the remote machine.",
                        "inputSchema": {"type": "object", "properties": {"directory_path": {"type": "string"}}},
                    },
                ]
            },
        }
    else:
        resp = {"jsonrpc": "2.0", "id": req_id, "error": {"code": -32601, "message": "Method not found"}}

    start_response("200 OK", [("Content-Type", "application/json")])
    return [json.dumps(resp).encode("utf-8")]


class _FakeUI:
    def append_output(self, text: str) -> None:
        _ = text


class _FakeAgent:
    def __init__(self, *, logic: Any) -> None:
        self.logic = logic


def _minimal_shell(tmp_path: Path, *, http_client: httpx.Client) -> ReactShell:
    from abstractagent.logic.react import ReActLogic
    from abstractcore.mcp import McpClient
    from abstractcore.tools import ToolDefinition

    shell = ReactShell.__new__(ReactShell)
    shell._color = False
    shell._output_lines = []
    shell._ui = _FakeUI()

    shell._auto_approve = True
    shell._approve_all_session = False

    shell._mcp_servers = {"remote": {"url": "http://mcp.test/", "headers": {}}}
    shell._mcp_client_factory = lambda server_id, entry: McpClient(url="http://mcp.test/", client=http_client)

    shell._tool_specs = {}
    shell._agent = _FakeAgent(logic=ReActLogic(tools=[ToolDefinition(name="local", description="L", parameters={})]))

    shell._tool_executor_server_id = None
    shell._executor_synced_server_ids = set()

    shell._config_file = tmp_path / "cfg.json"
    shell._max_tokens = None
    shell._max_history_messages = -1
    shell._allowed_tools = ["local"]
    shell._tool_prompt_examples = True
    shell._plan_mode = False
    shell._review_mode = False
    shell._review_max_rounds = 1

    # Avoid touching runtime/run store in this unit test.
    shell._safe_get_state = lambda: None
    shell._status_cache_key = None
    shell._status_cache_text = ""

    return shell


def test_executor_use_works_with_real_react_logic(tmp_path: Path) -> None:
    pytest.importorskip("abstractcore")
    pytest.importorskip("abstractagent")

    transport = httpx.WSGITransport(app=_mcp_wsgi_app)
    with httpx.Client(transport=transport, base_url="http://mcp.test") as http_client:
        shell = _minimal_shell(tmp_path, http_client=http_client)

        shell._sync_mcp_tools(server_id="remote")
        tool_names = [getattr(t, "name", "") for t in shell._agent.logic.tools]
        assert "mcp::remote::read_file" in tool_names

        shell._handle_executor("use remote")
        assert shell._tool_executor_server_id == "remote"
        assert any(str(n).startswith("mcp::remote::") for n in (shell._allowed_tools or []))

