from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Optional


def _default_registry_dir() -> Path:
    from abstractruntime.workflow_bundle import default_workflow_bundles_dir

    return default_workflow_bundles_dir()


def install_workflow_bundle_command(
    *,
    source: str,
    registry_dir: Optional[str] = None,
    overwrite: bool = False,
    output_json: bool = False,
) -> dict[str, Any]:
    from abstractruntime.workflow_bundle import WorkflowBundleRegistry, WorkflowBundleRegistryError

    reg = WorkflowBundleRegistry(registry_dir or _default_registry_dir())
    try:
        installed = reg.install(source, overwrite=bool(overwrite))
    except WorkflowBundleRegistryError as e:
        return {"ok": False, "error": str(e)}

    out = {
        "ok": True,
        "bundle_id": installed.bundle_id,
        "bundle_version": installed.bundle_version,
        "bundle_ref": installed.bundle_ref,
        "bundle_path": str(installed.path),
        "sha256": installed.sha256,
        "registry_dir": str(reg.bundles_dir),
    }
    if output_json:
        print(json.dumps(out, indent=2, ensure_ascii=False))
    else:
        print(f"Installed: {installed.bundle_ref} -> {installed.path}")
    return out


def list_workflow_bundles_command(
    *,
    registry_dir: Optional[str] = None,
    interface: Optional[str] = None,
    all_versions: bool = False,
    output_json: bool = False,
) -> dict[str, Any]:
    from abstractruntime.workflow_bundle import WorkflowBundleRegistry

    reg = WorkflowBundleRegistry(registry_dir or _default_registry_dir())
    eps = reg.list_entrypoints(interface=interface, latest_only=not bool(all_versions))
    items = [
        {
            "bundle_id": e.bundle_id,
            "bundle_version": e.bundle_version,
            "bundle_ref": e.bundle_ref,
            "workflow_id": e.workflow_id,
            "flow_id": e.flow_id,
            "name": e.name,
            "description": e.description,
            "interfaces": list(e.interfaces),
            "default": bool(e.is_default),
        }
        for e in eps
    ]
    out = {"ok": True, "registry_dir": str(reg.bundles_dir), "count": len(items), "entrypoints": items}
    if output_json:
        print(json.dumps(out, indent=2, ensure_ascii=False))
    else:
        if not items:
            print(f"No workflows found in {reg.bundles_dir}")
        for it in items:
            name = it.get("name") or it.get("bundle_id") or ""
            iface = ""
            interfaces = it.get("interfaces") or []
            if isinstance(interfaces, list) and interfaces:
                iface = f" [{', '.join(interfaces)}]"
            default = " *" if it.get("default") else ""
            print(f"{it['bundle_ref']}{default}  {name}{iface}")
    return out


def workflow_bundle_info_command(
    *,
    bundle_ref: str,
    registry_dir: Optional[str] = None,
    output_json: bool = False,
) -> dict[str, Any]:
    from abstractruntime.workflow_bundle import WorkflowBundleRegistry, WorkflowBundleRegistryError

    reg = WorkflowBundleRegistry(registry_dir or _default_registry_dir())
    try:
        b = reg.resolve_bundle(bundle_ref)
    except WorkflowBundleRegistryError as e:
        return {"ok": False, "error": str(e)}

    man = b.manifest
    entrypoints = [
        {
            "flow_id": str(getattr(ep, "flow_id", "") or ""),
            "name": str(getattr(ep, "name", "") or ""),
            "description": str(getattr(ep, "description", "") or ""),
            "interfaces": list(getattr(ep, "interfaces", None) or []),
        }
        for ep in list(getattr(man, "entrypoints", None) or [])
    ]
    out = {
        "ok": True,
        "registry_dir": str(reg.bundles_dir),
        "bundle_id": b.bundle_id,
        "bundle_version": b.bundle_version,
        "bundle_ref": b.bundle_ref,
        "bundle_path": str(b.path),
        "created_at": str(getattr(man, "created_at", "") or ""),
        "default_entrypoint": str(getattr(man, "default_entrypoint", "") or "") or None,
        "entrypoints": entrypoints,
        "flows": dict(getattr(man, "flows", None) or {}),
        "assets": dict(getattr(man, "assets", None) or {}),
        "metadata": dict(getattr(man, "metadata", None) or {}),
    }
    if output_json:
        print(json.dumps(out, indent=2, ensure_ascii=False))
    else:
        print(f"{b.bundle_ref} ({b.path})")
        if out.get("default_entrypoint"):
            print(f"default: {out['default_entrypoint']}")
        for ep in entrypoints:
            name = ep.get("name") or ep.get("flow_id") or ""
            print(f"- {ep.get('flow_id')}  {name}")
    return out


def remove_workflow_bundle_command(
    *,
    bundle_ref: str,
    registry_dir: Optional[str] = None,
    output_json: bool = False,
) -> dict[str, Any]:
    from abstractruntime.workflow_bundle import WorkflowBundleRegistry, WorkflowBundleRegistryError

    reg = WorkflowBundleRegistry(registry_dir or _default_registry_dir())
    try:
        removed = int(reg.remove(bundle_ref))
    except WorkflowBundleRegistryError as e:
        return {"ok": False, "error": str(e)}

    out = {"ok": True, "removed": removed, "registry_dir": str(reg.bundles_dir)}
    if output_json:
        print(json.dumps(out, indent=2, ensure_ascii=False))
    else:
        print(f"Removed {removed} bundle(s) from {reg.bundles_dir}")
    return out
