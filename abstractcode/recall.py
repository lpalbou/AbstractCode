"""AbstractCode recall helpers (no LLM required).

AbstractCode is a host UX; recall should stay consistent with runtime-owned
contracts. This module provides:
- lightweight argument parsing for `/recall`
- a thin execution helper that uses AbstractRuntime's ActiveContextPolicy

The goal is testability without requiring an LLM provider to be reachable.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from abstractruntime.memory import ActiveContextPolicy, TimeRange


@dataclass(frozen=True)
class RecallRequest:
    since: Optional[str] = None
    until: Optional[str] = None
    tags: Dict[str, Any] = None  # type: ignore[assignment]
    tags_mode: str = "all"  # all|any
    users: List[str] = None  # type: ignore[assignment]
    locations: List[str] = None  # type: ignore[assignment]
    query: Optional[str] = None
    limit: int = 10
    into_context: bool = False
    placement: str = "after_summary"
    show: bool = False
    scope: str = "run"  # run|session|global|all

    def __post_init__(self) -> None:
        object.__setattr__(self, "tags", dict(self.tags or {}))

        raw_mode = str(getattr(self, "tags_mode", "") or "").strip().lower() or "all"
        if raw_mode in ("and",):
            raw_mode = "all"
        if raw_mode in ("or",):
            raw_mode = "any"
        if raw_mode not in ("all", "any"):
            raw_mode = "all"
        object.__setattr__(self, "tags_mode", raw_mode)

        def _norm_list(values: Any) -> List[str]:
            if not isinstance(values, list):
                return []
            out: List[str] = []
            for v in values:
                if isinstance(v, str) and v.strip():
                    out.append(v.strip())
            # preserve order but dedup (case-insensitive)
            seen: set[str] = set()
            deduped: List[str] = []
            for s in out:
                key = s.lower()
                if key in seen:
                    continue
                seen.add(key)
                deduped.append(s)
            return deduped

        object.__setattr__(self, "users", _norm_list(getattr(self, "users", None)))
        object.__setattr__(self, "locations", _norm_list(getattr(self, "locations", None)))

        raw_scope = str(getattr(self, "scope", "") or "").strip().lower() or "run"
        if raw_scope not in ("run", "session", "global", "all"):
            raw_scope = "run"
        object.__setattr__(self, "scope", raw_scope)


def parse_recall_args(raw: str) -> RecallRequest:
    """Parse `/recall` arguments.

    Supported flags:
      - `--since ISO`
      - `--until ISO`
      - `--tag k=v` (repeatable)
      - `--tags-mode all|any` (default all; AND/OR across tag keys)
      - `--user NAME` (repeatable; alias: --users)
      - `--location LOC` (repeatable; alias: --locations)
      - `--q text`  (if absent, remaining args become query)
      - `--limit N`
      - `--into-context`
      - `--placement after_summary|after_system|end`
      - `--show` (show full note content for memory_note matches)
      - `--scope run|session|global|all`
    """
    import shlex

    try:
        parts = shlex.split(raw) if raw else []
    except ValueError:
        parts = raw.split() if raw else []

    since: Optional[str] = None
    until: Optional[str] = None
    tags: Dict[str, Any] = {}
    tags_mode = "all"
    users: List[str] = []
    locations: List[str] = []
    query: Optional[str] = None
    limit = 10
    into_context = False
    placement = "after_summary"
    show = False
    scope = "run"

    leftovers: list[str] = []
    i = 0
    while i < len(parts):
        p = parts[i]
        if p in ("--since", "--from"):
            if i + 1 >= len(parts):
                raise ValueError("--since requires an ISO timestamp")
            since = str(parts[i + 1])
            i += 2
            continue
        if p in ("--until", "--to"):
            if i + 1 >= len(parts):
                raise ValueError("--until requires an ISO timestamp")
            until = str(parts[i + 1])
            i += 2
            continue
        if p in ("--tag", "--tags"):
            if i + 1 >= len(parts):
                raise ValueError("--tag requires k=v")
            kv = str(parts[i + 1])
            if "=" not in kv:
                raise ValueError("--tag requires k=v")
            k, v = kv.split("=", 1)
            k = k.strip()
            v = v.strip()
            if not k or not v:
                raise ValueError("--tag requires non-empty k=v")
            if k != "kind":
                prev = tags.get(k)
                if prev is None:
                    tags[k] = v
                elif isinstance(prev, str):
                    if prev != v:
                        tags[k] = [prev, v]
                elif isinstance(prev, list):
                    if v not in prev:
                        prev.append(v)
            i += 2
            continue
        if p in ("--tags-mode", "--tag-mode", "--tags-op", "--tag-op"):
            if i + 1 >= len(parts):
                raise ValueError("--tags-mode requires all|any")
            mode = str(parts[i + 1]).strip().lower()
            if mode in ("and",):
                mode = "all"
            if mode in ("or",):
                mode = "any"
            if mode not in ("all", "any"):
                raise ValueError("--tags-mode must be all|any")
            tags_mode = mode
            i += 2
            continue
        if p in ("--user", "--users"):
            if i + 1 >= len(parts):
                raise ValueError("--user requires a name")
            raw_user = str(parts[i + 1]).strip()
            if raw_user:
                for seg in raw_user.split(","):
                    s = seg.strip()
                    if s:
                        users.append(s)
            i += 2
            continue
        if p in ("--location", "--locations"):
            if i + 1 >= len(parts):
                raise ValueError("--location requires a value")
            raw_loc = str(parts[i + 1]).strip()
            if raw_loc:
                for seg in raw_loc.split(","):
                    s = seg.strip()
                    if s:
                        locations.append(s)
            i += 2
            continue
        if p in ("--q", "--query"):
            if i + 1 >= len(parts):
                raise ValueError("--q requires a query string")

            # Consume tokens until the next flag, so `--q player dies` works
            # without requiring quotes.
            j = i + 1
            buf: list[str] = []
            while j < len(parts) and not str(parts[j]).startswith("--"):
                buf.append(str(parts[j]))
                j += 1
            query = " ".join([x for x in buf if x]).strip() or None
            i = j
            continue
        if p == "--limit":
            if i + 1 >= len(parts):
                raise ValueError("--limit requires a number")
            try:
                limit = int(parts[i + 1])
            except Exception:
                raise ValueError("--limit requires a number") from None
            if limit < 1:
                limit = 1
            i += 2
            continue
        if p == "--into-context":
            into_context = True
            i += 1
            continue
        if p == "--show":
            show = True
            i += 1
            continue
        if p == "--scope":
            if i + 1 >= len(parts):
                raise ValueError("--scope requires a value")
            scope = str(parts[i + 1]).strip().lower() or "run"
            if scope not in ("run", "session", "global", "all"):
                raise ValueError("--scope must be run|session|global|all")
            i += 2
            continue
        if p == "--placement":
            if i + 1 >= len(parts):
                raise ValueError("--placement requires a value")
            placement = str(parts[i + 1]).strip()
            if placement not in ("after_summary", "after_system", "end"):
                raise ValueError("--placement must be after_summary|after_system|end")
            i += 2
            continue
        if p.startswith("--"):
            raise ValueError(f"Unknown flag: {p}")

        leftovers.append(str(p))
        i += 1

    if query is None and leftovers:
        query = " ".join([p for p in leftovers if p]).strip() or None

    def _validate_iso(value: Optional[str], *, flag: str) -> None:
        if value is None:
            return
        import datetime as _dt

        v = str(value).strip()
        if not v:
            return
        if v.endswith("Z"):
            v = v[:-1] + "+00:00"
        try:
            _dt.datetime.fromisoformat(v)
        except Exception as e:
            raise ValueError(f"{flag} must be ISO8601 (got: {value})") from e

    _validate_iso(since, flag="--since")
    _validate_iso(until, flag="--until")

    return RecallRequest(
        since=since,
        until=until,
        tags=tags,
        tags_mode=tags_mode,
        users=users,
        locations=locations,
        query=query,
        limit=limit,
        into_context=into_context,
        placement=placement,
        show=show,
        scope=scope,
    )


def execute_recall(
    *,
    run_id: str,
    run_store: Any,
    artifact_store: Any,
    request: RecallRequest,
) -> Dict[str, Any]:
    """Execute a recall request against a run.

    Returns:
      dict with keys:
        - matches: list[dict]
        - rehydration: dict | None
    """
    policy = ActiveContextPolicy(run_store=run_store, artifact_store=artifact_store)

    def _resolve_session_root_id(start_run_id: str) -> str:
        cur_id = str(start_run_id or "").strip()
        seen: set[str] = set()
        while cur_id and cur_id not in seen:
            seen.add(cur_id)
            st = run_store.load(cur_id)
            if st is None:
                return cur_id
            parent = getattr(st, "parent_run_id", None)
            if not isinstance(parent, str) or not parent.strip():
                return cur_id
            cur_id = parent.strip()
        return str(start_run_id or "").strip()

    def _global_memory_run_id() -> str:
        import os
        import re

        rid = str(os.environ.get("ABSTRACTRUNTIME_GLOBAL_MEMORY_RUN_ID") or "").strip()
        if rid and re.match(r"^[a-zA-Z0-9_-]+$", rid):
            return rid
        return "global_memory"

    time_range: Optional[TimeRange] = None
    if request.since or request.until:
        time_range = TimeRange(start=request.since, end=request.until)

    scope = str(request.scope or "run").strip().lower() or "run"
    run_ids: list[str] = []
    if scope == "run":
        run_ids = [run_id]
    elif scope == "session":
        run_ids = [_resolve_session_root_id(run_id)]
    elif scope == "global":
        run_ids = [_global_memory_run_id()]
    else:  # all
        root_id = _resolve_session_root_id(run_id)
        global_id = _global_memory_run_id()
        # Deterministic order; dedup.
        seen_ids: set[str] = set()
        for rid in (run_id, root_id, global_id):
            if rid and rid not in seen_ids:
                seen_ids.add(rid)
                run_ids.append(rid)

    matches: list[dict[str, Any]] = []
    seen_artifacts: set[str] = set()
    for rid in run_ids:
        # Skip missing runs (e.g. global memory not created yet).
        st = run_store.load(rid)
        if st is None:
            continue
        part = policy.filter_spans(
            rid,
            time_range=time_range,
            tags=(request.tags or None),
            tags_mode=request.tags_mode,
            authors=(request.users or None),
            locations=(request.locations or None),
            query=request.query,
            limit=int(request.limit),
        )
        for s in part:
            if not isinstance(s, dict):
                continue
            aid = str(s.get("artifact_id") or "").strip()
            if not aid or aid in seen_artifacts:
                continue
            seen_artifacts.add(aid)
            annotated = dict(s)
            annotated["owner_run_id"] = rid
            matches.append(annotated)

    rehydration: Optional[Dict[str, Any]] = None
    if request.into_context:
        # Rehydrate the selected span(s) into active context. This is deterministic and persists
        # the updated run state. Notes are rehydrated as a synthetic message.
        span_ids: list[str] = []
        for s in matches:
            if not isinstance(s, dict):
                continue
            aid = s.get("artifact_id")
            if isinstance(aid, str) and aid:
                span_ids.append(aid)
        if span_ids:
            rehydration = policy.rehydrate_into_context(
                run_id,
                span_ids=span_ids,
                placement=request.placement,
                dedup_by="message_id",
            )

    return {"matches": matches, "rehydration": rehydration}
