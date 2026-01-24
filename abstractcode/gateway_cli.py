from __future__ import annotations

import json
import os
import time
import uuid
from dataclasses import dataclass
from pathlib import Path
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


def _request_multipart(
    *,
    url: str,
    token: Optional[str],
    fields: Dict[str, str],
    file_field: str,
    filename: str,
    content: bytes,
    content_type: str = "application/octet-stream",
    timeout_s: float = 60.0,
) -> Dict[str, Any]:
    boundary = uuid.uuid4().hex
    crlf = b"\r\n"
    body = bytearray()

    for k, v in (fields or {}).items():
        body.extend(b"--" + boundary.encode("ascii") + crlf)
        body.extend(f'Content-Disposition: form-data; name="{k}"'.encode("utf-8"))
        body.extend(crlf + crlf)
        body.extend(str(v).encode("utf-8"))
        body.extend(crlf)

    body.extend(b"--" + boundary.encode("ascii") + crlf)
    body.extend(f'Content-Disposition: form-data; name="{file_field}"; filename="{filename}"'.encode("utf-8"))
    body.extend(crlf)
    body.extend(f"Content-Type: {content_type}".encode("utf-8"))
    body.extend(crlf + crlf)
    body.extend(bytes(content or b""))
    body.extend(crlf)
    body.extend(b"--" + boundary.encode("ascii") + b"--" + crlf)

    headers = {
        "Accept": "application/json",
        "Content-Type": f"multipart/form-data; boundary={boundary}",
    }
    if token:
        headers["Authorization"] = f"Bearer {token}"

    req = Request(url=url, data=bytes(body), headers=headers, method="POST")
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


def _split_bundle_ref(raw: str) -> tuple[str, Optional[str]]:
    s = str(raw or "").strip()
    if not s:
        return ("", None)
    if "@" not in s:
        return (s, None)
    a, b = s.split("@", 1)
    a = a.strip()
    b = b.strip() if b.strip() else None
    if not a:
        return ("", None)
    return (a, b)


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

    def kg_query(
        self,
        *,
        run_id: Optional[str] = None,
        session_id: Optional[str] = None,
        scope: str = "session",
        owner_id: Optional[str] = None,
        all_owners: bool = False,
        subject: Optional[str] = None,
        predicate: Optional[str] = None,
        object_value: Optional[str] = None,
        since: Optional[str] = None,
        until: Optional[str] = None,
        active_at: Optional[str] = None,
        query_text: Optional[str] = None,
        min_score: Optional[float] = None,
        limit: int = 500,
        order: str = "desc",
        timeout_s: float = 60.0,
    ) -> Dict[str, Any]:
        body: Dict[str, Any] = {
            "scope": str(scope or "session").strip().lower() or "session",
            "limit": int(limit),
            "order": str(order or "desc").strip().lower() or "desc",
        }
        if bool(all_owners):
            body["all_owners"] = True
        if run_id:
            body["run_id"] = str(run_id or "").strip()
        if session_id:
            body["session_id"] = str(session_id or "").strip()
        if owner_id:
            body["owner_id"] = owner_id
        if subject:
            body["subject"] = subject
        if predicate:
            body["predicate"] = predicate
        if object_value:
            body["object"] = object_value
        if since:
            body["since"] = since
        if until:
            body["until"] = until
        if active_at:
            body["active_at"] = active_at
        if query_text:
            body["query_text"] = query_text
        if min_score is not None:
            body["min_score"] = float(min_score)
        return _request_json(
            method="POST",
            url=_join_url(self.base_url, "/api/gateway/kg/query"),
            token=self.token,
            payload=body,
            timeout_s=float(timeout_s),
        )

    def list_bundles(self, *, all_versions: bool = False) -> Dict[str, Any]:
        qs = "all_versions=true" if bool(all_versions) else "all_versions=false"
        return _request_json(
            method="GET",
            url=_join_url(self.base_url, f"/api/gateway/bundles?{qs}"),
            token=self.token,
            payload=None,
        )

    def get_bundle(self, *, bundle_id: str, bundle_version: Optional[str] = None) -> Dict[str, Any]:
        bid = str(bundle_id or "").strip()
        if not bid:
            raise ValueError("bundle_id is required")
        qs = f"?bundle_version={bundle_version}" if isinstance(bundle_version, str) and bundle_version.strip() else ""
        return _request_json(
            method="GET",
            url=_join_url(self.base_url, f"/api/gateway/bundles/{bid}{qs}"),
            token=self.token,
            payload=None,
        )

    def upload_bundle(self, *, path: str, overwrite: bool = False, reload: bool = True) -> Dict[str, Any]:
        src = Path(str(path or "").strip()).expanduser().resolve()
        if not src.exists() or not src.is_file():
            raise FileNotFoundError(f"Bundle not found: {src}")
        content = src.read_bytes()
        return _request_multipart(
            url=_join_url(self.base_url, "/api/gateway/bundles/upload"),
            token=self.token,
            fields={"overwrite": "true" if overwrite else "false", "reload": "true" if reload else "false"},
            file_field="file",
            filename=src.name,
            content=content,
            content_type="application/octet-stream",
            timeout_s=60.0,
        )

    def remove_bundle(self, *, bundle_ref: str, reload: bool = True) -> Dict[str, Any]:
        bid, ver = _split_bundle_ref(bundle_ref)
        if not bid:
            raise ValueError("bundle_ref must be 'bundle_id' or 'bundle_id@version'")
        qs = []
        if ver:
            qs.append(f"bundle_version={ver}")
        if bool(reload):
            qs.append("reload=true")
        else:
            qs.append("reload=false")
        q = ("?" + "&".join(qs)) if qs else ""
        return _request_json(
            method="DELETE",
            url=_join_url(self.base_url, f"/api/gateway/bundles/{bid}{q}"),
            token=self.token,
            payload=None,
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


def query_gateway_kg_command(
    *,
    gateway_url: Optional[str],
    gateway_token: Optional[str],
    run_id: Optional[str],
    scope: str = "session",
    owner_id: Optional[str] = None,
    all_owners: bool = False,
    subject: Optional[str] = None,
    predicate: Optional[str] = None,
    object_value: Optional[str] = None,
    since: Optional[str] = None,
    until: Optional[str] = None,
    active_at: Optional[str] = None,
    query_text: Optional[str] = None,
    min_score: Optional[float] = None,
    limit: int = 0,
    order: str = "desc",
    fmt: str = "triples",
    pretty: bool = False,
) -> None:
    api = GatewayApi(base_url=str(gateway_url or default_gateway_url()), token=gateway_token or default_gateway_token())
    id_value = str(run_id or "").strip() if isinstance(run_id, str) else ""
    scope_norm = str(scope or "").strip().lower() or "session"
    if not id_value and not all_owners and scope_norm not in {"global"} and not owner_id:
        raise SystemExit("abstractcode gateway kg: id is required unless using --scope global or --all-owners (or provide --owner-id)")

    run_id_arg: Optional[str] = None
    session_id_arg: Optional[str] = None
    if id_value and scope_norm != "global" and not all_owners and not owner_id:
        # Prefer session_id for session scope (common id shape overlaps with run_ids).
        if scope_norm == "session":
            session_id_arg = id_value
        else:
            run_id_arg = id_value
    try:
        resp = api.kg_query(
            run_id=run_id_arg,
            session_id=session_id_arg,
            scope=str(scope_norm),
            owner_id=owner_id,
            all_owners=bool(all_owners),
            subject=subject,
            predicate=predicate,
            object_value=object_value,
            since=since,
            until=until,
            active_at=active_at,
            query_text=query_text,
            min_score=min_score,
            limit=int(limit),
            order=str(order),
        )
    except RuntimeError as e:
        # Convenience: when the user passes a session id (e.g. AbstractCode Web session_id),
        # the gateway won't find a RunState by that id. Retry as `session_id` for session scope.
        msg = str(e)
        is_run_not_found = "Gateway HTTP 404:" in msg and "not found" in msg and "Run '" in msg
        if (
            is_run_not_found
            and scope_norm in {"session", "all"}
            and not owner_id
            and not all_owners
            and id_value
        ):
            resp = api.kg_query(
                run_id=None,
                session_id=str(id_value),
                scope=str(scope_norm),
                owner_id=owner_id,
                all_owners=bool(all_owners),
                subject=subject,
                predicate=predicate,
                object_value=object_value,
                since=since,
                until=until,
                active_at=active_at,
                query_text=query_text,
                min_score=min_score,
                limit=int(limit),
                order=str(order),
            )
        else:
            raise

    warnings = resp.get("warnings")
    if isinstance(warnings, list) and warnings:
        for w in warnings:
            if isinstance(w, str) and w.strip():
                print(f"warning: {w.strip()}", file=os.sys.stderr)

    items = resp.get("items")
    if not isinstance(items, list):
        items = []

    fmt2 = str(fmt or "triples").strip().lower() or "triples"
    if fmt2 == "json":
        indent = 2 if bool(pretty) else None
        print(json.dumps(resp, indent=indent, ensure_ascii=False))
        return

    if fmt2 == "jsonl":
        for item in items:
            if isinstance(item, dict):
                print(json.dumps(item, ensure_ascii=False))
        return

    # Default: human-readable triples.
    for item in items:
        if not isinstance(item, dict):
            continue
        observed_at = str(item.get("observed_at") or "").strip()
        subj = str(item.get("subject") or "").strip()
        pred = str(item.get("predicate") or "").strip()
        obj = str(item.get("object") or "").strip()
        scope_v = str(item.get("scope") or "").strip()
        owner_v = str(item.get("owner_id") or "").strip()

        suffix_parts: list[str] = []
        if scope_v:
            suffix_parts.append(f"scope={scope_v}")
        if owner_v:
            suffix_parts.append(f"owner_id={owner_v}")
        conf = item.get("confidence")
        if isinstance(conf, (int, float)):
            suffix_parts.append(f"confidence={float(conf):.3f}")
        attrs = item.get("attributes")
        if isinstance(attrs, dict):
            ret = attrs.get("_retrieval")
            if isinstance(ret, dict) and isinstance(ret.get("score"), (int, float)):
                suffix_parts.append(f"score={float(ret['score']):.3f}")

        suffix = f" ({', '.join(suffix_parts)})" if suffix_parts else ""
        ts = f"[{observed_at}] " if observed_at else ""
        print(f"{ts}{subj} --{pred}--> {obj}{suffix}")


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
