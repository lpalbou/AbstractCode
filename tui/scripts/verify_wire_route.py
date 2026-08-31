#!/usr/bin/env python3
"""Verify a benchmark's route from the RELAY'S OWN REQUEST LOG, not from the client.

WHY THIS EXISTS. A 24-cell cross-client benchmark was invalidated because five of
eight arms ran with reasoning OFF while every layer above them reported medium.
The gateway stored `_runtime.thinking = "medium"`, its run store reported the
route as verified, and the harness copied that into the provenance as
"CONFIRMED gpt-5.4/medium (run store)". All of it was true and none of it
reached the wire: abstractcore's `openai-compatible` provider dropped the field
before the request was built, and the relay's own default for gpt-5.4 is `none`.

The lesson is narrow and worth stating: **a client's record of what it intended
to send is not evidence of what it sent.** Route verification has to read the
receiving end. This script reads the relay's `inbound_request` log — the verbatim
bytes the client put on the socket — and the `upstream_request` it forwarded.

It answers three questions per window:
  1. Did every request carry the reasoning effort we believe we ran at?
  2. Did the wire model ever drift (a previous campaign silently ran gpt-5.5)?
  3. Which client sent each request (user-agent), so an arm that routes through
     a different HTTP stack cannot hide inside another arm's totals.

Usage:
  python3 scripts/verify_wire_route.py --since 2026-08-03T20:05:08Z
  python3 scripts/verify_wire_route.py --since ... --until ... --expect-effort medium
"""
from __future__ import annotations

import argparse
import collections
import glob
import json
import sys
from pathlib import Path

# The relay writes one file per UTC hour: ~/.airelays/logs/YYYY/MM/DD-HH.log.
# NOTE the timestamps inside are UTC too. This bit me once already: `ps` reports
# local time, the logs report UTC, and comparing the two directly made a live,
# correctly-routed run look like it had produced no traffic at all.
LOG_GLOB = str(Path.home() / ".airelays" / "logs" / "*" / "*" / "*.log")


def candidate_files(since: str | None, until: str | None) -> list[str]:
    """Only the hour-files the window can touch.

    The relay keeps months of history and individual hour-files reach 128 MB, so
    scanning all of them costs minutes per invocation — enough that the check
    stops being run, which is how the original route bug survived. The filename
    itself carries YYYY/MM/DD-HH, so the window can be applied before opening
    anything. Bounds are compared as plain ISO strings, which sorts correctly
    for a fixed-width UTC format.
    """
    files = sorted(glob.glob(LOG_GLOB))
    if not (since or until):
        return files
    keep = []
    for f in files:
        p = Path(f)
        try:
            day, hour = p.stem.split("-")
            stamp = f"{p.parent.parent.name}-{p.parent.name}-{day}T{hour}"
        except (ValueError, IndexError):
            keep.append(f)      # unparseable name: scan it rather than skip it
            continue
        # Compare on the hour prefix. `since[:13]` is "YYYY-MM-DDTHH", so an
        # hour-file is kept when it could contain any second of the window.
        if since and stamp < since[:13]:
            continue
        if until and stamp > until[:13]:
            continue
        keep.append(f)
    return keep


def load(since: str | None, until: str | None):
    inbound, upstream = [], {}
    for f in candidate_files(since, until):
        try:
            fh = open(f, errors="replace")
        except OSError:
            continue
        with fh:
            for line in fh:
                if '"inbound_request"' not in line and '"upstream_request"' not in line:
                    continue
                try:
                    d = json.loads(line)
                except Exception:  # noqa: BLE001
                    continue
                ts = d.get("logged_at") or ""
                if since and ts < since:
                    continue
                if until and ts > until:
                    continue
                body = (d.get("body") or {}).get("json") or {}
                if d.get("phase") == "upstream_request":
                    upstream[d.get("request_id")] = body
                    continue
                # Only real generation calls. /models probes and health checks
                # carry no messages and would dilute every ratio below.
                if not isinstance(body, dict) or "messages" not in body:
                    continue
                ua = ((d.get("headers") or {}).get("user-agent") or "?").split()[0]
                inbound.append({
                    "id": d.get("request_id"), "ts": ts, "ua": ua,
                    "model": body.get("model"),
                    "effort": body.get("reasoning_effort", None),
                })
    return inbound, upstream


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--since", help="ISO8601 UTC lower bound (inclusive)")
    ap.add_argument("--until", help="ISO8601 UTC upper bound (inclusive)")
    ap.add_argument("--expect-effort", default="medium")
    ap.add_argument("--expect-model", default="gpt-5.4")
    a = ap.parse_args()

    inbound, upstream = load(a.since, a.until)
    if not inbound:
        # Absence of traffic is UNKNOWN, never "the route was wrong". An empty
        # window most often means the bound was wrong (see the UTC note above).
        print("NO generation requests in window — UNKNOWN, not a failure.")
        print("Check the bounds are UTC; the relay logs and `ps` disagree by your tz offset.")
        return 2

    by_ua = collections.defaultdict(lambda: collections.Counter())
    models = collections.defaultdict(collections.Counter)
    for r in inbound:
        by_ua[r["ua"]][r["effort"] if r["effort"] is not None else "<ABSENT>"] += 1
        models[r["ua"]][r["model"]] += 1

    print(f"window: {inbound[0]['ts']} .. {inbound[-1]['ts']}   ({len(inbound)} generation requests)\n")
    print(f"{'client (user-agent)':32}{'reqs':>6}{'at ' + a.expect_effort:>12}{'other':>8}  models")
    ok = True
    for ua in sorted(by_ua):
        c = by_ua[ua]
        n = sum(c.values())
        good = c.get(a.expect_effort, 0)
        other = n - good
        drift = [m for m in models[ua] if m != a.expect_model]
        if other or drift:
            ok = False
        print(f"{ua:32}{n:>6}{good:>12}{other:>8}  {dict(models[ua])}")
        if other:
            bad = {k: v for k, v in c.items() if k != a.expect_effort}
            print(f"{'':32}  ^ NOT at {a.expect_effort}: {bad}")

    # Upstream leg: what the relay actually forwarded. A client can send the
    # field and a relay can still drop or override it; that would be invisible
    # from the inbound side alone.
    up_eff = collections.Counter()
    for r in inbound:
        b = upstream.get(r["id"])
        if b is None:
            up_eff["<no upstream row>"] += 1
            continue
        reasoning = b.get("reasoning")
        up_eff[json.dumps(reasoning) if reasoning is not None else "null"] += 1
    print(f"\nupstream reasoning payload: {dict(up_eff)}")

    print("\nVERDICT:", "PASS — every request on the expected model and effort" if ok
          else "FAIL — at least one request was off-route (see rows above)")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
