from __future__ import annotations

import json
import os
import time
import uuid
from dataclasses import dataclass
from typing import Any, Dict, Optional
from urllib.error import HTTPError
from urllib.request import Request, urlopen


def _env(name: str, fallback: Optional[str] = None) -> Optional[str]:
    v = os.getenv(name)
    if v is not None and str(v).strip():
        return v
    if fallback:
        v2 = os.getenv(fallback)
        if v2 is not None and str(v2).strip():
            return v2
    return None


def default_gateway_url() -> str:
    # Canonical env vars:
    # - ABSTRACTGATEWAY_URL (gateway)
    # - ABSTRACTFLOW_GATEWAY_URL (legacy compatibility)
    # AbstractCode convention:
    # - ABSTRACTCODE_GATEWAY_URL
    candidates = [
        "ABSTRACTCODE_GATEWAY_URL",
        "ABSTRACTFLOW_GATEWAY_URL",
        "ABSTRACTGATEWAY_URL",
    ]
    for name in candidates:
        v = os.getenv(name)
        if isinstance(v, str) and v.strip():
            return v.strip().rstrip("/")
    # AbstractGateway docs default to 8081.
    return "http://127.0.0.1:8081"


def default_gateway_token() -> Optional[str]:
    # Canonical env vars:
    # - ABSTRACTGATEWAY_AUTH_TOKEN
    # - ABSTRACTFLOW_GATEWAY_AUTH_TOKEN (legacy compatibility)
    # AbstractCode convention:
    # - ABSTRACTCODE_GATEWAY_TOKEN
    candidates = [
        "ABSTRACTCODE_GATEWAY_TOKEN",
        "ABSTRACTGATEWAY_AUTH_TOKEN",
        "ABSTRACTFLOW_GATEWAY_AUTH_TOKEN",
    ]
    for name in candidates:
        v = os.getenv(name)
        if isinstance(v, str) and v.strip():
            return v.strip()

    token_lists = [
        "ABSTRACTGATEWAY_AUTH_TOKENS",
        "ABSTRACTFLOW_GATEWAY_AUTH_TOKENS",
    ]
    for name in token_lists:
        raw = os.getenv(name)
        if not isinstance(raw, str) or not raw.strip():
            continue
        first = raw.split(",", 1)[0].strip()
        if first:
            return first

    return None


def _join_url(base_url: str, path: str) -> str:
    b = str(base_url or "").rstrip("/")
    p = str(path or "")
    if not p.startswith("/"):
        p = "/" + p
    return b + p


def _request_json(
    *,
    method: str,
    url: str,
    token: Optional[str],
    payload: Optional[Dict[str, Any]] = None,
    timeout_s: float = 30.0,
) -> Dict[str, Any]:
    body: Optional[bytes]
    headers = {"Accept": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    if payload is None:
        body = None
    else:
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        headers["Content-Type"] = "application/json"

    req = Request(url=url, data=body, headers=headers, method=str(method).upper())
    try:
        with urlopen(req, timeout=float(timeout_s)) as resp:
            raw = resp.read().decode("utf-8")
            return json.loads(raw) if raw else {}
    except HTTPError as e:
        try:
            raw = e.read().decode("utf-8")
        except Exception:
            raw = ""
        detail = raw.strip() or str(e)
        raise RuntimeError(f"Gateway HTTP {e.code}: {detail}") from e


@dataclass(frozen=True)
class GatewayApi:
    base_url: str
    token: Optional[str] = None

    def start_run(self, *, flow_id: str, input_data: Dict[str, Any], bundle_id: Optional[str] = None) -> str:
        body: Dict[str, Any] = {"flow_id": flow_id, "input_data": dict(input_data or {})}
        if bundle_id:
            body["bundle_id"] = bundle_id
        resp = _request_json(
            method="POST",
            url=_join_url(self.base_url, "/api/gateway/runs/start"),
            token=self.token,
            payload=body,
        )
        run_id = resp.get("run_id")
        if not isinstance(run_id, str) or not run_id.strip():
            raise RuntimeError(f"Invalid gateway response: {resp}")
        return run_id.strip()

    def get_run(self, run_id: str) -> Dict[str, Any]:
        return _request_json(
            method="GET",
            url=_join_url(self.base_url, f"/api/gateway/runs/{run_id}"),
            token=self.token,
            payload=None,
        )

    def get_ledger(self, run_id: str, *, after: int, limit: int = 200) -> Dict[str, Any]:
        return _request_json(
            method="GET",
            url=_join_url(self.base_url, f"/api/gateway/runs/{run_id}/ledger?after={int(after)}&limit={int(limit)}"),
            token=self.token,
            payload=None,
        )

    def submit_command(
        self,
        *,
        run_id: str,
        typ: str,
        payload: Dict[str, Any],
        command_id: Optional[str] = None,
        client_id: Optional[str] = None,
    ) -> Dict[str, Any]:
        body = {
            "command_id": command_id or f"cmd_{uuid.uuid4().hex}",
            "run_id": run_id,
            "type": typ,
            "payload": dict(payload or {}),
            "client_id": client_id,
        }
        return _request_json(
            method="POST",
            url=_join_url(self.base_url, "/api/gateway/commands"),
            token=self.token,
            payload=body,
        )


def _extract_sub_run_id_from_step(record: Dict[str, Any]) -> Optional[str]:
    if not isinstance(record, dict):
        return None
    if record.get("status") != "waiting":
        return None
    result = record.get("result")
    if not isinstance(result, dict):
        return None
    wait = result.get("wait")
    if not isinstance(wait, dict):
        return None
    if wait.get("reason") != "subworkflow":
        return None
    details = wait.get("details")
    if not isinstance(details, dict):
        return None
    sub_run_id = details.get("sub_run_id")
    return sub_run_id.strip() if isinstance(sub_run_id, str) and sub_run_id.strip() else None


def _print_step(*, run_id: str, rec: Dict[str, Any]) -> None:
    node_id = rec.get("node_id") or rec.get("nodeId") or ""
    status = rec.get("status") or ""
    effect = rec.get("effect") if isinstance(rec.get("effect"), dict) else None
    effect_type = effect.get("type") if isinstance(effect, dict) else None
    prefix = run_id[:8]

    line = f"[{prefix}] {status} {node_id}"
    if effect_type:
        line += f"  ({effect_type})"
    print(line)

    sub = _extract_sub_run_id_from_step(rec)
    if sub:
        print(f"[{prefix}] ↳ sub_run_id={sub}")


def _prompt_user(waiting: Dict[str, Any]) -> str:
    prompt = waiting.get("prompt") or "Please respond:"
    choices = waiting.get("choices")
    if isinstance(choices, list) and choices:
        print(str(prompt))
        for i, c in enumerate(choices, start=1):
            print(f"  {i}. {c}")
        raw = input("> ").strip()
        if raw.isdigit():
            idx = int(raw)
            if 1 <= idx <= len(choices):
                return str(choices[idx - 1])
        return raw
    return input(f"{prompt}\n> ").strip()


def run_gateway_flow_command(
    *,
    gateway_url: Optional[str],
    gateway_token: Optional[str],
    flow_id: str,
    bundle_id: Optional[str],
    input_data: Dict[str, Any],
    follow: bool,
    poll_s: float = 0.25,
) -> str:
    api = GatewayApi(base_url=str(gateway_url or default_gateway_url()), token=gateway_token or default_gateway_token())

    run_id = api.start_run(flow_id=flow_id, bundle_id=bundle_id, input_data=input_data)
    print(f"run_id={run_id}")
    if not follow:
        return run_id
    _follow_runs(api=api, root_run_id=run_id, poll_s=poll_s)
    return run_id


def attach_gateway_run_command(
    *,
    gateway_url: Optional[str],
    gateway_token: Optional[str],
    run_id: str,
    follow: bool,
    poll_s: float = 0.25,
) -> None:
    api = GatewayApi(base_url=str(gateway_url or default_gateway_url()), token=gateway_token or default_gateway_token())
    if not follow:
        state = api.get_run(run_id)
        print(json.dumps(state, indent=2, ensure_ascii=False))
        return

    _follow_runs(api=api, root_run_id=str(run_id), poll_s=poll_s)


def _follow_runs(*, api: GatewayApi, root_run_id: str, poll_s: float) -> None:
    cursors: Dict[str, int] = {root_run_id: 0}
    active: Dict[str, bool] = {root_run_id: True}

    while True:
        # 1) Replay ledgers for all known runs (root + discovered subruns).
        for rid in list(cursors.keys()):
            cur = int(cursors.get(rid, 0))
            page = api.get_ledger(rid, after=cur, limit=200)
            items = page.get("items")
            if not isinstance(items, list):
                items = []
            for rec_any in items:
                rec = rec_any if isinstance(rec_any, dict) else None
                if rec is None:
                    continue
                _print_step(run_id=rid, rec=rec)
                sub = _extract_sub_run_id_from_step(rec)
                if sub and sub not in cursors:
                    cursors[sub] = 0
                    active[sub] = True

            cursors[rid] = int(page.get("next_after") or (cur + len(items)))

        # 2) Handle waiting/user prompts and stop conditions.
        any_active = False
        root_status: Optional[str] = None
        root_waiting: Optional[Dict[str, Any]] = None
        tool_blocked: list[tuple[str, str, Optional[str]]] = []
        for rid in list(active.keys()):
            if not active.get(rid):
                continue
            any_active = True
            state = api.get_run(rid)
            status = state.get("status")
            if rid == root_run_id:
                root_status = status if isinstance(status, str) else None
                root_waiting = state.get("waiting") if isinstance(state.get("waiting"), dict) else None
            if status in {"completed", "failed", "cancelled"}:
                active[rid] = False
                continue

            if status != "waiting":
                continue
            waiting = state.get("waiting")
            if not isinstance(waiting, dict):
                continue
            reason = waiting.get("reason")
            details = waiting.get("details") if isinstance(waiting.get("details"), dict) else {}
            mode = details.get("mode") if isinstance(details, dict) else None
            if reason in {"event", "job"} and isinstance(details, dict) and ("tool_calls" in details or "mode" in details):
                tool_blocked.append((rid, str(reason), str(mode) if mode is not None else None))

            if reason != "user":
                continue

            wait_key = waiting.get("wait_key")
            wait_key = wait_key.strip() if isinstance(wait_key, str) and wait_key.strip() else None
            if not wait_key:
                continue

            response = _prompt_user(waiting)
            api.submit_command(
                run_id=rid,
                typ="resume",
                payload={"wait_key": wait_key, "payload": {"response": response}},
            )

        # Root completion is the natural stop condition for "run".
        if isinstance(root_status, str) and root_status in {"completed", "failed", "cancelled"}:
            return

        # If the root is blocked waiting on a subworkflow, but a child is blocked on a tool wait,
        # stop to avoid hanging indefinitely (manual resume is required).
        if (
            isinstance(root_status, str)
            and root_status == "waiting"
            and isinstance(root_waiting, dict)
            and root_waiting.get("reason") == "subworkflow"
            and tool_blocked
        ):
            rid, reason, mode = tool_blocked[0]
            print(f"[{root_run_id[:8]}] blocked: subworkflow waiting on run={rid} reason={reason} mode={mode}")
            print(f"[{root_run_id[:8]}] resume tools via gateway, then re-attach")
            return

        # If the root is waiting on a non-user input, stop (manual resume required).
        if isinstance(root_status, str) and root_status == "waiting" and isinstance(root_waiting, dict):
            reason = root_waiting.get("reason")
            if reason not in {"user", "subworkflow", "until"}:
                details = root_waiting.get("details") if isinstance(root_waiting.get("details"), dict) else {}
                mode = details.get("mode") if isinstance(details, dict) else None
                print(f"[{root_run_id[:8]}] waiting reason={reason} mode={mode} (manual resume required)")
                return

        if not any_active:
            return

        time.sleep(float(poll_s))
