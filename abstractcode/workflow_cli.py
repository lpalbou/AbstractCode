from __future__ import annotations

import json
from typing import Any, Optional


def _gateway_api(*, gateway_url: Optional[str], gateway_token: Optional[str]):
    from .gateway_cli import GatewayApi, default_gateway_token, default_gateway_url

    url = str(gateway_url or "").strip() or default_gateway_url()
    token_raw = str(gateway_token or "").strip()
    token = token_raw if token_raw else default_gateway_token()
    return GatewayApi(base_url=url, token=token)


def install_workflow_bundle_command(
    *,
    source: str,
    gateway_url: Optional[str] = None,
    gateway_token: Optional[str] = None,
    overwrite: bool = False,
    output_json: bool = False,
) -> dict[str, Any]:
    api = _gateway_api(gateway_url=gateway_url, gateway_token=gateway_token)
    try:
        resp = api.upload_bundle(path=source, overwrite=bool(overwrite), reload=True)
    except Exception as e:
        return {"ok": False, "error": str(e)}

    out = dict(resp or {})
    out.setdefault("gateway_url", str(getattr(api, "base_url", "") or ""))
    if output_json:
        print(json.dumps(out, indent=2, ensure_ascii=False))
    else:
        bref = str(out.get("bundle_ref") or "").strip() or str(out.get("bundle_id") or "").strip() or "?"
        print(f"Installed on gateway: {bref}")
    return out


def list_workflow_bundles_command(
    *,
    gateway_url: Optional[str] = None,
    gateway_token: Optional[str] = None,
    interface: Optional[str] = None,
    all_versions: bool = False,
    include_deprecated: bool = False,
    output_json: bool = False,
) -> dict[str, Any]:
    api = _gateway_api(gateway_url=gateway_url, gateway_token=gateway_token)
    try:
        resp = api.list_bundles(all_versions=bool(all_versions), include_deprecated=bool(include_deprecated))
    except Exception as e:
        return {"ok": False, "error": str(e)}

    bundles_raw = resp.get("items") if isinstance(resp, dict) and isinstance(resp.get("items"), list) else []
    items: list[dict[str, Any]] = []
    for b in bundles_raw:
        if not isinstance(b, dict):
            continue
        bid = str(b.get("bundle_id") or "").strip()
        bver = str(b.get("bundle_version") or "").strip()
        bref = str(b.get("bundle_ref") or "").strip() or (f"{bid}@{bver}" if bid and bver else bid)
        eps_raw = b.get("entrypoints") if isinstance(b.get("entrypoints"), list) else []
        default_fid = str(b.get("default_entrypoint") or "").strip()
        implied_default = len(eps_raw) == 1
        for ep in eps_raw:
            if not isinstance(ep, dict):
                continue
            fid = str(ep.get("flow_id") or "").strip()
            if not fid:
                continue
            deprecated = bool(ep.get("deprecated") is True)
            interfaces = [str(x).strip() for x in list(ep.get("interfaces") or []) if isinstance(x, str) and x.strip()]
            if interface and interface not in interfaces:
                continue
            items.append(
                {
                    "bundle_id": bid,
                    "bundle_version": bver,
                    "bundle_ref": bref,
                    "workflow_id": f"{bref}:{fid}" if bref and fid else None,
                    "flow_id": fid,
                    "name": str(ep.get("name") or "") or fid,
                    "description": str(ep.get("description") or "") or "",
                    "interfaces": interfaces,
                    "default": bool(implied_default or (default_fid and fid == default_fid)),
                    "deprecated": bool(deprecated),
                    "deprecated_reason": str(ep.get("deprecated_reason") or "") or "",
                }
            )

    items.sort(key=lambda x: (str(x.get("bundle_id") or ""), str(x.get("bundle_version") or ""), str(x.get("flow_id") or "")))
    out = {"ok": True, "gateway_url": str(getattr(api, "base_url", "") or ""), "count": len(items), "entrypoints": items}
    if output_json:
        print(json.dumps(out, indent=2, ensure_ascii=False))
    else:
        if not items:
            print("No workflows found on the gateway.")
        for it in items:
            name = it.get("name") or it.get("workflow_id") or ""
            iface = ""
            interfaces = it.get("interfaces") or []
            if isinstance(interfaces, list) and interfaces:
                iface = f" [{', '.join(interfaces)}]"
            default = " *" if it.get("default") else ""
            dep = " (deprecated)" if it.get("deprecated") else ""
            wid = str(it.get("workflow_id") or it.get("bundle_ref") or "")
            print(f"{wid}{default}{dep}  {name}{iface}")
    return out


def workflow_bundle_info_command(
    *,
    bundle_ref: str,
    gateway_url: Optional[str] = None,
    gateway_token: Optional[str] = None,
    output_json: bool = False,
) -> dict[str, Any]:
    api = _gateway_api(gateway_url=gateway_url, gateway_token=gateway_token)
    ref = str(bundle_ref or "").strip()
    if not ref:
        return {"ok": False, "error": "bundle_ref is required"}
    bid, ver = (ref.split("@", 1) + [""])[:2] if "@" in ref else (ref, "")
    ver2 = ver.strip() or None
    try:
        resp = api.get_bundle(bundle_id=str(bid).strip(), bundle_version=ver2)
    except Exception as e:
        return {"ok": False, "error": str(e)}

    out = dict(resp or {})
    out.setdefault("ok", True)
    out.setdefault("gateway_url", str(getattr(api, "base_url", "") or ""))
    if output_json:
        print(json.dumps(out, indent=2, ensure_ascii=False))
    else:
        bref = str(out.get("bundle_ref") or out.get("bundle_id") or "").strip()
        print(bref or ref)
        de = out.get("default_entrypoint")
        if isinstance(de, str) and de.strip():
            print(f"default: {de.strip()}")
        eps = out.get("entrypoints") if isinstance(out.get("entrypoints"), list) else []
        for ep in eps:
            if not isinstance(ep, dict):
                continue
            fid = str(ep.get("flow_id") or "").strip()
            name = str(ep.get("name") or "").strip() or fid
            if fid:
                print(f"- {fid}  {name}")
    return out


def remove_workflow_bundle_command(
    *,
    bundle_ref: str,
    gateway_url: Optional[str] = None,
    gateway_token: Optional[str] = None,
    output_json: bool = False,
) -> dict[str, Any]:
    api = _gateway_api(gateway_url=gateway_url, gateway_token=gateway_token)
    try:
        resp = api.remove_bundle(bundle_ref=str(bundle_ref or "").strip(), reload=True)
    except Exception as e:
        return {"ok": False, "error": str(e)}

    out = dict(resp or {})
    out.setdefault("gateway_url", str(getattr(api, "base_url", "") or ""))
    if output_json:
        print(json.dumps(out, indent=2, ensure_ascii=False))
    else:
        removed = out.get("removed")
        bref = str(out.get("bundle_ref") or bundle_ref or "").strip() or "?"
        print(f"Removed {removed} bundle(s) from gateway for {bref}")
    return out


def deprecate_workflow_bundle_command(
    *,
    bundle_id: str,
    flow_id: Optional[str] = None,
    reason: Optional[str] = None,
    gateway_url: Optional[str] = None,
    gateway_token: Optional[str] = None,
    output_json: bool = False,
) -> dict[str, Any]:
    api = _gateway_api(gateway_url=gateway_url, gateway_token=gateway_token)
    bid = str(bundle_id or "").strip()
    if not bid:
        return {"ok": False, "error": "bundle_id is required"}
    try:
        resp = api.deprecate_bundle(bundle_id=bid, flow_id=flow_id, reason=reason)
    except Exception as e:
        return {"ok": False, "error": str(e)}

    out = dict(resp or {})
    out.setdefault("gateway_url", str(getattr(api, "base_url", "") or ""))
    if output_json:
        print(json.dumps(out, indent=2, ensure_ascii=False))
    else:
        fid = str(out.get("flow_id") or "").strip() or "*"
        print(f"Deprecated on gateway: {bid}:{fid}")
    return out


def undeprecate_workflow_bundle_command(
    *,
    bundle_id: str,
    flow_id: Optional[str] = None,
    gateway_url: Optional[str] = None,
    gateway_token: Optional[str] = None,
    output_json: bool = False,
) -> dict[str, Any]:
    api = _gateway_api(gateway_url=gateway_url, gateway_token=gateway_token)
    bid = str(bundle_id or "").strip()
    if not bid:
        return {"ok": False, "error": "bundle_id is required"}
    try:
        resp = api.undeprecate_bundle(bundle_id=bid, flow_id=flow_id)
    except Exception as e:
        return {"ok": False, "error": str(e)}

    out = dict(resp or {})
    out.setdefault("gateway_url", str(getattr(api, "base_url", "") or ""))
    if output_json:
        print(json.dumps(out, indent=2, ensure_ascii=False))
    else:
        fid = str(out.get("flow_id") or "").strip() or "*"
        removed = bool(out.get("removed") is True)
        print(f"Undeprecated on gateway: {bid}:{fid} ({'changed' if removed else 'no-op'})")
    return out
