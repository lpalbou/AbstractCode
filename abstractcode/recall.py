"""AbstractCode recall helpers (no LLM required).

AbstractCode is a host UX; recall should stay consistent with runtime-owned
contracts. This module provides:
- lightweight argument parsing for `/recall`
- a thin execution helper that uses AbstractRuntime's ActiveContextPolicy

The goal is testability without requiring an LLM provider to be reachable.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, Optional

from abstractruntime.memory import ActiveContextPolicy, TimeRange


@dataclass(frozen=True)
class RecallRequest:
    since: Optional[str] = None
    until: Optional[str] = None
    tags: Dict[str, str] = None  # type: ignore[assignment]
    query: Optional[str] = None
    limit: int = 10
    into_context: bool = False
    placement: str = "after_summary"
    show: bool = False

    def __post_init__(self) -> None:
        object.__setattr__(self, "tags", dict(self.tags or {}))


def parse_recall_args(raw: str) -> RecallRequest:
    """Parse `/recall` arguments.

    Supported flags:
      - `--since ISO`
      - `--until ISO`
      - `--tag k=v` (repeatable)
      - `--q text`  (if absent, remaining args become query)
      - `--limit N`
      - `--into-context`
      - `--placement after_summary|after_system|end`
      - `--show` (show full note content for memory_note matches)
    """
    import shlex

    try:
        parts = shlex.split(raw) if raw else []
    except ValueError:
        parts = raw.split() if raw else []

    since: Optional[str] = None
    until: Optional[str] = None
    tags: Dict[str, str] = {}
    query: Optional[str] = None
    limit = 10
    into_context = False
    placement = "after_summary"
    show = False

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
                tags[k] = v
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
        query=query,
        limit=limit,
        into_context=into_context,
        placement=placement,
        show=show,
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

    time_range: Optional[TimeRange] = None
    if request.since or request.until:
        time_range = TimeRange(start=request.since, end=request.until)

    matches = policy.filter_spans(
        run_id,
        time_range=time_range,
        tags=(request.tags or None),
        query=request.query,
        limit=int(request.limit),
    )

    rehydration: Optional[Dict[str, Any]] = None
    if request.into_context:
        # Only rehydrate spans that have archived messages (notes are not message spans).
        span_ids: list[str] = []
        for s in matches:
            if not isinstance(s, dict):
                continue
            if str(s.get("kind") or "") == "memory_note":
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
