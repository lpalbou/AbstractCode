#!/usr/bin/env python3
"""Adversarial regression suite for `scripts/rtype_review_score.py`.

WHY THIS EXISTS. Every gameability defence in the R-Type rubric was built in
response to a fixture that had already beaten it — an attract loop whose keydown
handler was literally empty scored 0.790 and beat 13 of 23 real products; genre
nouns in a COMMENT took the whole content tier; 13 stub files took 4.75/5 on
modularity. Those fixtures lived in a scratch directory that was deleted, so
every one of those defences was load-bearing and untested. A refactor could
reopen any of them silently.

So the attacks live HERE, committed, next to the control they must lose to.

WHAT IT ASSERTS. Not just the ORDER — a suite that only checks "control wins"
passes just as happily when the margin has collapsed from 0.40 to 0.01. It
asserts:

  1. every control outscores every attack;
  2. the margin (worst control minus best attack) is at least MIN_MARGIN;
  3. no attack clears ATTACK_CEILING in absolute terms;
  4. per-fixture, the specific checks each attack was built to steal are still
     denied to it (an attack that loses on TOTAL while stealing R4 is a defect
     that will resurface the moment the weights change);
  5. nothing drifted from the recorded baseline by more than BASELINE_TOL.

Usage:
  python3 tests/rtype_fixtures/run_fixture_suite.py            # score + assert
  python3 tests/rtype_fixtures/run_fixture_suite.py --jobs 2
  python3 tests/rtype_fixtures/run_fixture_suite.py --only control-playable
  python3 tests/rtype_fixtures/run_fixture_suite.py --isolation  # session reuse
  python3 tests/rtype_fixtures/run_fixture_suite.py --update-baseline
"""
from __future__ import annotations

import argparse
import concurrent.futures
import json
import statistics
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(REPO / "scripts"))

from rtype_review_score import (  # noqa: E402
    GRID_H,
    GRID_W,
    _Session,
    _branch,
    _cells,
    _grid_of,
    score_one,
    sha_of,
)
from zelda_review_score import resolve_entry  # noqa: E402

BASELINE = HERE / "baseline.json"

# The gap the suite defends. Set BELOW the measured margin, not at it, so normal
# instrument jitter does not fail the suite — but far enough above zero that a
# defence collapsing is unmissable. The measured margin is recorded in
# baseline.json; this is the floor under it.
MIN_MARGIN = 0.25
# No attack may clear this in absolute terms, whatever the control scores. A
# page that cannot be played is not half a game.
ATTACK_CEILING = 0.50
# Per-fixture drift allowed against the recorded baseline before it is reported
# as a regression. Grid cell counts are integers over a deterministic replay, so
# a real product's score is reproducible to the point; this is slack for a
# Chrome version change, not for a defence quietly failing.
BASELINE_TOL = 0.06
# Gap allowed between two fixtures that are the SAME GAME at different canvas
# resolutions. Anything above this is the rubric scoring pixels, not play.
SCALE_TOL = 0.02

# Behavioural checks an attack must never be credited with. `None` (UNKNOWN) is
# acceptable — it means the instrument could not measure, which is not a claim
# that the page responded. Only True is a violation.
#
# The spec tier's play-dependent checks joined the list: an attract loop that
# cannot be started can neither kill enemies, grow a weapon, nor pace a level,
# and crediting any of those to it would be the same class of defect the
# original four guard against.
INPUT_CHECKS = ["R2_ship_moves_vertically", "R3_ship_moves_horizontally",
                "R4_weapon_fires", "R7_input_persists",
                "S1_fire_direction", "S2_enemies_killable",
                "S3_weapon_progression", "S4_charge_shot", "S5_enemy_pacing",
                "S8b_boss_encounter", "S10_impact_vfx"]

FIXTURES = {
    "control-playable": {
        "kind": "control",
        "what": "genuinely playable: ship flies, fires, stage scrolls",
        "require": ["R0_loads_without_exception", "R1_renders_and_animates",
                    "R2_ship_moves_vertically", "R3_ship_moves_horizontally",
                    "R4_weapon_fires", "R5_world_scrolls", "R7_input_persists"],
        "forbid": [],
    },
    "control-scaled-large": {
        "kind": "control",
        "what": "control-playable rendered at 1200x900 — same picture, 6.25x the "
                "pixels. A scale-invariant instrument must score it the same",
        "require": ["R0_loads_without_exception", "R2_ship_moves_vertically",
                    "R3_ship_moves_horizontally", "R4_weapon_fires"],
        "forbid": [],
        "same_score_as": "control-playable",
    },
    "control-scaled-small": {
        "kind": "control",
        "what": "control-playable rendered at 240x180 — same picture, 1/4 the "
                "pixels",
        "require": ["R0_loads_without_exception", "R2_ship_moves_vertically",
                    "R3_ship_moves_horizontally", "R4_weapon_fires"],
        "forbid": [],
        "same_score_as": "control-playable",
    },
    "control-batched-fire": {
        "kind": "control",
        "what": "playable, but every projectile is rendered in ONE batched draw "
                "call — the case a draw-rate-only weapon test false-negatives",
        "require": ["R0_loads_without_exception", "R2_ship_moves_vertically",
                    "R3_ship_moves_horizontally", "R4_weapon_fires",
                    "R7_input_persists"],
        "forbid": [],
    },
    # SOURCE-ONLY: scored from the files alone, no browser. `control-onefile` is
    # character-for-character the same game as control-playable with the three
    # modules concatenated into one inline script, so any Tier-2 difference
    # between them is a packaging choice being scored as quality.
    "control-onefile": {
        "kind": "source",
        "what": "control-playable in a single file — packaging control",
        "require": [], "forbid": [],
        # KNOWN DEFECT UNDER GUARD, not a target. Tier-2 modularity awards 3.0
        # of its 5 points for having two or more substantial files, so the same
        # game loses them by being shipped in one. Measured gap: 3.38 points.
        # Across the corpus Tier-2 modularity correlates with file count at
        # r = +0.940 and three single-file products score 0.00/5. This assertion
        # stops the gap GROWING; closing it is a rubric change that needs its own
        # pass and a corpus re-run.
        "max_t2_gap_vs": ("control-playable", 3.5),
    },
    "attack-a-attract-empty": {
        "kind": "attack",
        "what": "attract loop, keydown handler literally empty",
        "require": [],
        "forbid": INPUT_CHECKS,
    },
    "attack-a2-cosmetic": {
        "kind": "attack",
        "what": "attract loop + HUD lamp that lights on any key for 12 frames",
        "require": [],
        "forbid": INPUT_CHECKS,
    },
    "attack-b-comment-nouns": {
        "kind": "attack",
        "what": "genre vocabulary present only as prose (markup, comments, "
                "single-line strings, multi-line template literal)",
        "require": [],
        "forbid": INPUT_CHECKS,
        # The point of this fixture is the SOURCE tier, so it is also asserted
        # directly: prose must not buy content points.
        "max_t2_parts": {"weapons": 0.6, "powerups": 0.6, "enemies": 0.5,
                         "stages": 0.3},
    },
    "attack-c-split-stubs": {
        "kind": "attack",
        "what": "13 two-line modules to buy the modularity metric",
        "require": [],
        "forbid": INPUT_CHECKS,
        "max_t2_parts": {"modularity": 2.5},
    },
    # PROBES: playable fixtures that exist to pin ONE spec-tier check to a
    # known value, positive or negative. They are excluded from the
    # control/attack margin arithmetic — a probe is deliberately defective (or
    # deliberately generous) in exactly one dimension, so its TOTAL is not a
    # statement the margin should rest on. `expect_checks` is exact-value
    # assertion; `expect_min` a floor.
    "probe-backward-fire": {
        "kind": "probe",
        "what": "fully playable EXCEPT the gun fires leftward, away from the "
                "enemies — the operator-reported defect S1 exists to catch",
        "require": ["R2_ship_moves_vertically", "R3_ship_moves_horizontally",
                    "R4_weapon_fires"],
        "forbid": [],
        "expect_checks": {"S1_fire_direction": 0.0},
    },
    "probe-powerup-drop": {
        "kind": "probe",
        "what": "the corrected spec's core loop working: kills burst into "
                "particles, every 5th kill drops a magnet capsule, each "
                "capsule adds a parallel gun",
        "require": ["R2_ship_moves_vertically", "R3_ship_moves_horizontally",
                    "R4_weapon_fires"],
        "forbid": [],
        "expect_min": {"S1_fire_direction": 1.0, "S2_enemies_killable": 0.5,
                       "S3_weapon_progression": 0.5,
                       "S11_delivery_selfsufficient": 1.0},
    },
    "attack-d-combined": {
        "kind": "attack",
        "what": "attract loop + prose vocabulary + 13 padded stub modules",
        "require": [],
        "forbid": INPUT_CHECKS,
        # NOT per-part caps here. The prose vector is asserted on attack-b,
        # which is pure prose and must score 0.00 on all four content parts.
        # What is left in this fixture's content score comes from REAL
        # identifiers in real (if vacuous) code — `SECTION_0`, `layers`,
        # `scroll` — and a vocabulary count openly cannot tell vacuous code from
        # working code; that is why the whole tier is capped at 20 and why the
        # runtime content check R8 carries the weight. The property that must
        # hold is the comparative one: gaming the source tier must not beat a
        # real game ON the source tier.
        "max_t2_points_vs": ("control-playable", 0.0),
    },
}


def _num(v, places: int) -> str:
    """Format a number, or '-' when there is none. A source-only fixture has no
    SCORE and no T1; printing 'FAIL' or 0 there would be a claim, not a blank."""
    return "-" if v is None else f"%.{places}f" % v


def run_one(name: str) -> dict:
    """Score one fixture. `source` fixtures skip the browser entirely."""
    if FIXTURES[name]["kind"] == "source":
        from rtype_review_score import t2_score, tier2
        from zelda_review_score import tier0
        r = {}
        r.update(tier0(HERE / name))
        t2 = tier2(HERE / name)
        r.update(t2)
        pts, parts = t2_score(t2)
        r.update({"T2_POINTS": pts, "T2_PARTS": parts, "T1_POINTS": None,
                  "SCORE": None, "TIER0_PASS": bool(
                      r.get("G0_single_entry") and r.get("G0_refs_resolve")
                      and r.get("G1_js_parses"))})
    else:
        r = score_one(HERE / name)
    r["fixture"] = name
    return r


def isolation_probe(jobs_note: str = "") -> dict:
    """Prove that reusing one browser+server across branches keeps branches
    independent.

    `_Session` holds ONE Chrome and ONE static server per product and gives each
    branch a fresh PAGE. That is cheaper by ~20x, and it is also the exact shape
    of change that can silently couple measurements: same browser process, same
    origin, same HTTP cache. Three things are checked, because they fail
    differently:

      STORAGE   a page writes to localStorage / sessionStorage / cookie /
                window, and the NEXT page in the same session looks for it. A
                game that persisted a high score would otherwise carry state
                from one branch into the next.
      ORDERING  a no-input branch is run after an ArrowUp branch inside one
                session, and compared with a no-input branch from a VIRGIN
                session. Any difference is an ordering effect.
      REUSE     the same branch run twice in the same session, and once in a
                fresh session, must land on the identical world.
      CACHE     every request the shared server receives is counted. If later
                branches skipped the socket, their scripts came from a cache the
                first branch warmed — a timing difference, and timing
                differences are the class of bug that made this instrument
                non-deterministic in the first place.
    """
    import zelda_review_score as Z

    seen: list = []
    orig_get = Z.QuietHandler.do_GET

    def counting_get(self):
        seen.append(self.path)
        return orig_get(self)

    Z.QuietHandler.do_GET = counting_get
    entry, root = resolve_entry(HERE / "control-playable")
    out: dict = {"note": jobs_note}
    with _Session(entry, root) as sess:
        p1 = sess.browser.new_page()
        p1.goto(f"http://127.0.0.1:{sess.port}/{entry.name}", wait_until="load")
        p1.evaluate("""() => {
            localStorage.setItem('rtype_probe', 'leaked');
            sessionStorage.setItem('rtype_probe', 'leaked');
            document.cookie = 'rtype_probe=leaked; path=/';
            window.__rtype_probe = 'leaked';
        }""")
        p1.close()
        p2 = sess.browser.new_page()
        p2.goto(f"http://127.0.0.1:{sess.port}/{entry.name}", wait_until="load")
        out["storage"] = p2.evaluate("""() => ({
            local: localStorage.getItem('rtype_probe'),
            session: sessionStorage.getItem('rtype_probe'),
            cookie: document.cookie.indexOf('rtype_probe') >= 0,
            global_: typeof window.__rtype_probe,
        })""")
        p2.close()

        # ORDERING: does a preceding input branch change a later no-input one?
        seen.clear()
        _branch(entry, root, ["ArrowUp"], "none", sess)
        out["requests_branch_1"] = list(seen)
        seen.clear()
        after_input = _branch(entry, root, None, "none", sess)
        out["requests_branch_2"] = list(seen)
        # REUSE: the same branch twice inside one session.
        again = _branch(entry, root, None, "none", sess)

    with _Session(entry, root) as fresh:
        virgin = _branch(entry, root, None, "none", fresh)
    Z.QuietHandler.do_GET = orig_get

    def cells(a, b, slot="grid_post"):
        return _cells(_grid_of(a, slot), _grid_of(b, slot))["cells"]

    out["ordering_cells"] = cells(after_input, virgin)
    out["reuse_cells"] = cells(again, virgin)
    out["within_session_cells"] = cells(after_input, again)
    out["hashes_equal"] = (after_input.get("hash") == virgin.get("hash")
                           == again.get("hash"))
    out["draws_equal"] = (after_input.get("draws_per_frame")
                          == virgin.get("draws_per_frame")
                          == again.get("draws_per_frame"))
    st = out["storage"]
    # A later branch must still fetch the same files as the first one: same set
    # of paths, from the socket, not from a cache the previous branch warmed.
    out["requests_identical"] = (sorted(out.get("requests_branch_1") or [])
                                 == sorted(out.get("requests_branch_2") or []))
    out["PASS"] = bool(
        st["local"] is None and st["session"] is None and not st["cookie"]
        and st["global_"] == "undefined"
        and out["ordering_cells"] == 0 and out["reuse_cells"] == 0
        and out["within_session_cells"] == 0
        and out["hashes_equal"] and out["draws_equal"]
        and out["requests_identical"])
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--jobs", type=int, default=2,
                    help="fixtures scored in parallel; keep low, each one "
                         "drives a real Chrome")
    ap.add_argument("--only", action="append", default=None)
    ap.add_argument("--isolation", action="store_true",
                    help="run the session-reuse isolation probe instead")
    ap.add_argument("--update-baseline", action="store_true")
    ap.add_argument("--out", default=None)
    a = ap.parse_args()

    if a.isolation:
        iso = isolation_probe()
        print(json.dumps(iso, indent=1))
        print("\nISOLATION: " + ("PASS — branches are independent under session "
                                 "reuse" if iso["PASS"] else "FAIL"))
        return 0 if iso["PASS"] else 1

    names = a.only or sorted(FIXTURES)
    for n in names:
        if n not in FIXTURES:
            print(f"unknown fixture {n}", file=sys.stderr)
            return 2
        if not (HERE / n / "index.html").is_file():
            print(f"fixture {n} has no index.html", file=sys.stderr)
            return 2

    rubric = sha_of(REPO / "scripts" / "rtype_review_score.py",
                    REPO / "scripts" / "zelda_review_score.py",
                    REPO / "scripts" / "rtype_spec_tier.py")
    print(f"rubric sha256 {rubric[:16]}  ({len(names)} fixtures, jobs={a.jobs})\n")

    rows: dict = {}
    with concurrent.futures.ProcessPoolExecutor(max_workers=a.jobs) as ex:
        futs = {ex.submit(run_one, n): n for n in names}
        for f in concurrent.futures.as_completed(futs):
            n = futs[f]
            try:
                rows[n] = f.result()
            except Exception as e:  # noqa: BLE001
                rows[n] = {"fixture": n, "SCORE": None,
                           "notes": f"scorer crashed: {str(e)[:200]}"}
            r = rows[n]
            print(f"  {n:26} {_num(r.get('SCORE'), 4):>7}"
                  f"  T1={_num(r.get('T1_POINTS'), 1):>5}"
                  f"  T2={_num(r.get('T2_POINTS'), 2):>6}"
                  f"  act={str(r.get('activated_by')):<12} det={r.get('deterministic')}")

    print(f"\n{'fixture':26}{'kind':9}{'score':>8}{'T1':>7}{'T2':>7}   flags")
    for n in names:
        r = rows[n]
        # UNKNOWN prints '?', not '.': a check the instrument could not measure
        # is not a check the fixture failed, and the table must not blur them.
        flags = ""
        for k in ["R0_loads_without_exception", "R1_renders_and_animates",
                  "R2_ship_moves_vertically", "R3_ship_moves_horizontally",
                  "R4_weapon_fires", "R5_world_scrolls", "R6_audio_scheduled",
                  "R7_input_persists", "R8_scene_populated"]:
            v = r.get(k)
            flags += "?" if v is None else (k[1] if v else ".")
        sflags = ""
        for k in ["S1_fire_direction", "S2_enemies_killable",
                  "S3_weapon_progression", "S4_charge_shot", "S5_enemy_pacing",
                  "S6_orb_companion", "S7_arcade_mode", "S8a_level_intro",
                  "S8b_boss_encounter", "S9_music_layer", "S10_impact_vfx",
                  "S11_delivery_selfsufficient"]:
            v = r.get(k)
            sflags += "?" if v is None else ("x" if v else ".")
        print(f"{n:26}{FIXTURES[n]['kind']:9}{_num(r.get('SCORE'), 4):>8}"
              f"{_num(r.get('T1_POINTS'), 1):>7}{_num(r.get('T2_POINTS'), 2):>7}"
              f"   R{flags} S{sflags}")
    print("   flags: R0..R8 then S1,S2,S3,S4,S5,S6,S7,S8a,S8b,S9,S10,S11; "
          "digit/x = credited, '.' = not credited, '?' = UNKNOWN (not measured)")

    # ---- assertions --------------------------------------------------------
    fails: list[str] = []
    controls = {n: rows[n]["SCORE"] for n in names
                if FIXTURES[n]["kind"] == "control" and rows[n].get("SCORE") is not None}
    attacks = {n: rows[n]["SCORE"] for n in names
               if FIXTURES[n]["kind"] == "attack" and rows[n].get("SCORE") is not None}
    for n in names:
        if rows[n].get("SCORE") is None and FIXTURES[n]["kind"] != "source":
            fails.append(f"{n}: did not score ({rows[n].get('notes')})")

    margin = None
    if controls and attacks:
        worst_control = min(controls.values())
        best_attack = max(attacks.values())
        margin = round(worst_control - best_attack, 4)
        print(f"\nMARGIN  worst control {worst_control:.4f} - best attack "
              f"{best_attack:.4f} = {margin:+.4f}   (floor {MIN_MARGIN})")
        if margin < MIN_MARGIN:
            fails.append(f"margin {margin:+.4f} below floor {MIN_MARGIN}")
        if best_attack > ATTACK_CEILING:
            top = max(attacks, key=lambda k: attacks[k])
            fails.append(f"attack {top} scored {best_attack:.4f} > ceiling "
                         f"{ATTACK_CEILING}")

    for n in names:
        r, spec = rows[n], FIXTURES[n]
        for k in spec.get("require", []):
            if not r.get(k):
                fails.append(f"{n}: control lost {k} (= {r.get(k)!r})")
        for k in spec.get("forbid", []):
            if r.get(k) is True or (isinstance(r.get(k), (int, float))
                                    and r.get(k)):
                fails.append(f"{n}: attack was CREDITED with {k} (= {r.get(k)!r})")
        for k, want in (spec.get("expect_checks") or {}).items():
            got = r.get(k)
            if got is None or abs(float(got) - want) > 1e-6:
                fails.append(f"{n}: probe expected {k} == {want}, measured {got!r}")
        for k, floor in (spec.get("expect_min") or {}).items():
            got = r.get(k)
            if got is None or float(got) < floor:
                fails.append(f"{n}: probe expected {k} >= {floor}, measured {got!r}")
        for k, cap in (spec.get("max_t2_parts") or {}).items():
            got = (r.get("T2_PARTS") or {}).get(k)
            if got is not None and got > cap:
                fails.append(f"{n}: T2 part {k} = {got} > cap {cap}")
        # SCALE INVARIANCE. Two fixtures that are the same game at different
        # resolutions must score the same; any gap is the instrument reading
        # canvas size as quality.
        vs = spec.get("max_t2_points_vs")
        if vs and vs[0] in rows:
            other, slack = vs
            mine, theirs = r.get("T2_POINTS"), rows[other].get("T2_POINTS")
            if mine is not None and theirs is not None and mine > theirs + slack:
                fails.append(f"{n}: T2 {mine:.2f} beats {other}'s {theirs:.2f} "
                             f"— an attack out-scored a real game on the source "
                             f"tier")
        gapspec = spec.get("max_t2_gap_vs")
        if gapspec and gapspec[0] in rows:
            other, cap = gapspec
            mine = r.get("T2_POINTS")
            theirs = rows[other].get("T2_POINTS")
            if mine is not None and theirs is not None:
                print(f"\nPACKAGING GAP  {n} T2={mine:.2f} vs {other} "
                      f"T2={theirs:.2f} = {theirs - mine:+.2f} for the same game "
                      f"in one file (guard {cap})")
                if abs(theirs - mine) > cap:
                    fails.append(f"{n}: T2 gap {theirs - mine:+.2f} vs {other} "
                                 f"exceeds {cap} — packaging scored as quality")
        twin = spec.get("same_score_as")
        if twin and twin in rows:
            mine, theirs = r.get("SCORE"), rows[twin].get("SCORE")
            if mine is not None and theirs is not None and abs(mine - theirs) > SCALE_TOL:
                fails.append(f"{n}: scores {mine:.4f} against its {twin} twin's "
                             f"{theirs:.4f} — same game, gap {mine - theirs:+.4f} "
                             f"is resolution, not quality")

    # ---- baseline drift ----------------------------------------------------
    base = json.loads(BASELINE.read_text()) if BASELINE.is_file() else None
    if base:
        print("\nBASELINE DRIFT")
        for n in names:
            was = (base.get("rows") or {}).get(n, {}).get("SCORE")
            now = rows[n].get("SCORE")
            if was is None or now is None:
                print(f"  {n:26} baseline {was} now {now}")
                continue
            d = now - was
            mark = "  <-- DRIFT" if abs(d) > BASELINE_TOL else ""
            print(f"  {n:26} {was:.4f} -> {now:.4f}  {d:+.4f}{mark}")
            if abs(d) > BASELINE_TOL:
                fails.append(f"{n}: drifted {d:+.4f} from baseline (tol "
                             f"{BASELINE_TOL})")

    payload = {
        "rubric_sha256": rubric,
        "margin": margin,
        "min_margin": MIN_MARGIN,
        "attack_ceiling": ATTACK_CEILING,
        "rows": {n: {k: v for k, v in rows[n].items()
                     if k not in ("activation_trials", "fire_rejected")}
                 for n in names},
    }
    out = Path(a.out) if a.out else None
    if a.update_baseline:
        BASELINE.write_text(json.dumps(payload, indent=1) + "\n")
        print(f"\nwrote baseline {BASELINE}")
    if out:
        out.write_text(json.dumps(payload, indent=1) + "\n")
        print(f"wrote {out}")

    print("")
    if fails:
        print(f"SUITE FAIL ({len(fails)})")
        for f in fails:
            print(f"  - {f}")
        return 1
    print("SUITE PASS — every attack scored below every control by at least "
          f"{MIN_MARGIN}, and no attack cleared {ATTACK_CEILING}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
