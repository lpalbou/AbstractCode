#!/usr/bin/env python3
"""Zero-LLM capture of the client's /runs/start POST body.

A local HTTP capture server stands in for the gateway: it answers the
boot reads (catalog/tools/skills/entities/capability-defaults/probe) with
minimal valid JSON so the TUI reaches Idle, records the FIRST
/runs/start body, writes it to $OUT, and returns 503 so NO run is ever
created on the real gateway (0 LLM budget). Proves exactly what the
client serializes for workspace_allowed_paths + _runtime.tool_policy at a
given tier — the fields the gateway's /input_data echo normalizes away.

Usage: capture_start_body.py <out.json>  (driven by the pty harness).
"""

import json
import os
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

OUT = sys.argv[1] if len(sys.argv) > 1 else "/tmp/start-body.json"
captured = threading.Event()


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def _json(self, code, obj):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        p = self.path
        if "/runs/start" in p:
            return self._json(200, {})
        if p.endswith("/ping") or "/health" in p:
            return self._json(200, {"status": "ok"})
        if "capability-defaults" in p:
            return self._json(200, {"routes": []})
        if "/bundles" in p:
            return self._json(
                200,
                {
                    "items": [
                        {
                            "bundle_id": "basic-agent",
                            "entrypoints": [
                                {
                                    "flow_id": "81795ea9",
                                    "name": "basic-agent",
                                    "interfaces": ["abstractcode.agent.v1"],
                                }
                            ],
                        }
                    ]
                },
            )
        if "discovery/tools" in p:
            return self._json(
                200,
                {
                    "items": [
                        {"name": "read_file"},
                        {"name": "list_files"},
                        {"name": "write_file"},
                        {"name": "edit_file"},
                        {"name": "execute_command"},
                        {"name": "fetch_url"},
                    ]
                },
            )
        if "/skills" in p:
            return self._json(200, {"items": []})
        if "/entities" in p:
            return self._json(200, {"entities": []})
        if "/runs" in p:
            return self._json(200, {"items": []})
        return self._json(200, {})

    def do_POST(self):
        n = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(n) if n else b"{}"
        if "/runs/start" in self.path and not captured.is_set():
            try:
                with open(OUT, "wb") as f:
                    f.write(raw)
            finally:
                captured.set()
        # 503 so the client shows an error and NO run is created anywhere.
        return self._json(503, {"detail": "capture-proxy: no run started"})


def main():
    port = int(os.environ.get("CAPTURE_PORT", "8899"))
    srv = ThreadingHTTPServer(("127.0.0.1", port), H)
    srv.timeout = 1
    print(f"capture proxy on {port} -> {OUT}", flush=True)
    # Serve until the start body is captured, then a short grace period.
    while not captured.is_set():
        srv.handle_request()
    for _ in range(3):
        srv.handle_request()
    print("captured", flush=True)


if __name__ == "__main__":
    main()
