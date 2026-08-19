#!/usr/bin/env python3
"""Tier 1b — SPEC + GENRE behavioural checks for the R-Type rubric.

WHY THIS FILE EXISTS. The operator PLAYED the products and the rubric could not
see what they saw: a product whose shots travel the wrong way, a product whose
enemies soak sustained fire without dying, a product whose field is empty for
the first minute, and exactly one product in thirty with a working power-up —
all of them scored within a few points of each other, because R2-R8 grade
GENERIC shmup mechanics (moves, fires, scrolls) and none of the things the
operator's own prompt demanded. The corrected operator spec (2026-08-04,
`bench_clients.RTYPE_PROMPT`) names the missing nouns directly: orbs, monsters
dropping powered items, multiple weapons GAINED FROM those drops, a campaign of
2-minute levels with intro cards and bosses, per-level aesthetics, a win
sequence, an arcade mode, procedural SFX AND a music layer.

EVERY CHECK HERE IS BEHAVIOURAL where behaviour can be observed: driven in the
page under the same deterministic replay instrument as R2-R8 (seeded RNG,
pinned clock, single-stepped frames), never grepped from source. The one
exception is the ES-module half of the delivery gate, which is a fact about the
FILES as shipped and is paired with a live file:// load to keep it honest.

THE INSTRUMENT ADDITION. Two long "campaign" branches per product:

  C  90 virtual seconds, no input beyond the frozen-screen confirm policy.
     Carries: enemy-presence pacing control, the killability denominator, and
     the idle audio accrual that separates a soundtrack from input blips.
  F  150 virtual seconds holding the credited fire key on a fixed segment
     cycle (fire / fire+sweep / rest), the sweep so the ship crosses drop
     trajectories. Carries: killability numerator, weapon-progression contrast,
     boss/intro/aesthetics structure, impact-VFX bursts.

Both branches are cut into 30-frame segments (0.5 s virtual); each segment
records the 64x48 block-mean grid, the cumulative draw/frame/audio counters,
and every DISTINCT string the page passes to fillText/strokeText (first-seen
frame, bounded). The segment series — not any single frame — is what the spec
checks read, so a reading is a property of a minute of play rather than of one
lucky checkpoint.

FROZEN-SCREEN CONFIRM POLICY. Campaign products interpose cards (mission
briefings, game over) that WAIT for a key. A parked branch would grade the
card, which is exactly the pre-activation failure R2-R8 already closed once.
So: when a segment changes fewer than FROZEN_CELLS cells (a live shmup field
never holds that still — its own scroll repaints hundreds), the branch taps the
confirm key once, inside the segment's frame budget so branch alignment is
preserved. Taps are recorded and capped; a page that freezes forever stops
earning play-dependent checks rather than being poked indefinitely.

EVIDENCE LAW, inherited unchanged: every check is a float in [0,1] or None.
None (UNKNOWN) means the measurement could not be made — never "the spec item
was watched and found absent". A check that DEPENDS on another measurement
inherits honestly: a product with no credited fire key gets S1 (direction) as
UNKNOWN — the direction of nonexistent shots is not a fact — but S3 (weapon
progression) as 0.0 when the fire sweep MEASURED that nothing fires, because
"multiple weapons" is then an observed absence.

WIN-SEQUENCE AND AESTHETICS run on a budget that may not reach them (a full
campaign is 8+ minutes; the branch is 2.5). Unreached is UNKNOWN by
construction. That makes S8d a pure bonus with a ceiling cost, stated rather
than hidden: it cannot punish, only credit, and its weight is sized for that.

NOT SHIPPED — BULLET SALIENCE. The operator's complaint that enemy fire is
sometimes indistinguishable from the starfield is real and this tier does not
grade it, deliberately. Distinguishing 'enemy bullet' from 'background star'
needs objects the grid can resolve AND attribute: at 800 px a 2-3 px bullet
does not cross a 12.5 px cell's content threshold at all, and at 160 px a
bullet and a star are the same 1-2 cells — the same resolution wall S10 hits,
plus an attribution problem S10 does not have (a star that scrolls and a slow
bullet differ only in trajectory, and trajectory tracking across 0.5 s
segments at these speeds aliases). A check that cannot beat those two walls
would be noise wearing a spec item's name; the finding is recorded here as
unmeasurable at this instrument's resolution instead.

VALIDATION GROUND TRUTH (operator play-through of the 30-product corpus,
2026-08-04, mechanics only — the old corpus was built from the OLD prompt, so
spec items like orbs/arcade are expected near-absent there and their checks are
validated for NON-firing plus the fixture suite):
  * codex-1 "firing in wrong direction"      -> S1 must flag or the
    disagreement must be adjudicated with the recorded footprints;
  * codex-3 "enemies nearly unkillable"      -> S2 low;
  * pi-2 the only real power-up (spread)     -> S3 positive there, quiet
    elsewhere except products that grant weapons per level;
  * pi-3 "60 s without enemies"              -> S5 pacing must pay ~zero;
  * pi-1 long-press charge shot              -> S4 credit;
  * tui-multi-1 the only impact particles    -> S10 credit;
  * opencode 1-3 unplayable as delivered     -> S11 delivery gate.
"""
from __future__ import annotations

import re
import statistics

# NOTE ON IMPORTS. This module is imported by rtype_review_score at load time,
# so at module level it may import only the stdlib and the genre-NEUTRAL zelda
# harness. Everything R-Type-specific (grid constants, cell algebra, ramps) is
# imported INSIDE functions, by which time rtype_review_score is fully loaded.
from zelda_review_score import advance_frames  # noqa: E402

# ---------------------------------------------------------------- constants

SEG_FRAMES = 30          # one segment = half a virtual second at 60 fps
F_SEGMENTS = 300         # fire branch: 150 virtual seconds
C_SEGMENTS = 180         # control branch: 90 virtual seconds

# A live shmup field repaints its own scroll every frame; measured idle
# self-motion across the corpus is hundreds of changed pixels, tens of cells.
# A briefing card with a blinking "PRESS START" line changes only that line's
# few cells. 12 cells (0.4% of the 3072-cell grid) sits between the two with
# room on both sides. MEASURED on the pilot: pi-3 parked on an expired intro
# card read cell-identical segments (0 changed) for 20+ consecutive seconds
# while every in-play product read 15-110.
FROZEN_CELLS = 12
CONFIRM_TAP_COOLDOWN = 4     # segments between confirm taps
CONFIRM_TAP_MAX = 24         # a branch is a probe, not a woodpecker

# Occupancy: a grid cell counts as CONTENT when it differs from the field's
# median cell by more than this (0..255 block means; 24 is ~9% of full scale).
# The median is the background because the background is most of the area.
OCC_THR = 24
# The enemy zone. The ship spawns in the left ~15% of every product measured
# (codex-1 x=22/160, control fixture x=96/480) and the F-branch plan never
# presses Right, so content in this window is enemies, enemy fire, drops and
# player projectiles in flight — everything EXCEPT the ship itself and the
# HUD strips top and bottom.
ZONE_X0, ZONE_X1 = 0.35, 0.97
ZONE_Y0, ZONE_Y1 = 0.10, 0.90
# Background floor percentile. p20, not min: min would let one anomalous
# empty segment (mid-repaint) define the floor for the whole series.
FLOOR_PCT = 0.20

# ---- component AREA bands, in CANVAS PIXELS SQUARED -------------------------
# The one honest way to tell a bullet from an enemy from a particle on a grid
# that covers every canvas size: convert each connected content component's
# cell count to px^2 with the product's own cell size. Sprites have physical
# sizes; grids do not. MEASURED confounds that forced this: the fire branch's
# own projectiles counted as 'enemy presence', so pi-2 read a killability
# ratio of 6.04 (its collected SPREAD weapon flooding the zone with bullets
# scored as 'enemies surviving'), and codex-3's kill-vs-bullet draw asymmetry
# scored 1.0 on weapon progression the operator says it does not have.
#   bullets in this corpus:   9x2 .. 10x3 px  = 18..30 px^2
#   pickups:                  ~10x10          = ~100 px^2
#   wave enemies:             16x16 .. 24x24  = 256..576 px^2
#   explosion particles:      2x2 .. 3x3      = 4..9 px^2
BIG_PX = 120.0        # at or above: enemy-scale (an 11x11 sprite)
PROJ_PX_LO = 6.0      # projectile band lower edge; 6 keeps a 1-cell bullet on
                      # a 160px canvas (7.5 px^2) inside the band
TINY_PX = 12.0        # at or below (and <= 2 cells): a particle bit
# Enemy presence thresholds, floor-subtracted per branch (p20 of in-play
# segments — scrolling terrain is a near-constant component and the floor
# absorbs it). TWO presence definitions, because the branches differ in what
# is provably enemy-side:
#   C branch: it NEVER fires, so every non-background zone object — big or
#   bullet-sized — is the game's own content. Band-SUM presence, threshold
#   100 px^2 (a 10x10 object, or a few enemy missiles). MEASURED need:
#   codex-2's enemies are ~6x6 px = 37 px^2, invisible to a big-only band,
#   and its field read 'empty' while the operator fought missile swarms.
#   F branch (fallback only): its zone is full of its own bullets, so only
#   big components are attributable to the enemy. Threshold 150 px^2.
PRESENT_PX = 150.0    # F-branch big-only sustained presence
PRESENT_C_PX = 100.0  # C-branch band-sum sustained presence
CONTINUITY_FRAC = 0.66  # continuity threshold as a fraction of the entry bar

# S5 pacing anchors, virtual seconds. A shmup that engages inside 5 s of play
# earns full time credit; one that leaves the field empty for 45 s earns none —
# both are claims about the genre (the operator's complaint was a 60 s void),
# not about this corpus. Continuity: fraction of in-play time with any enemy
# present after first contact; 0.3 is a field empty two-thirds of the time.
PACING_FULL_S = 5.0
PACING_ZERO_S = 45.0
CONTINUITY_LO = 0.30
CONTINUITY_HI = 0.80

# S2 killability anchors. The claim: sustained fire measurably thins the
# standing enemy population. Under fire the field holding >= 90% of its
# control population is fire that does nothing; <= 35% is a field being
# visibly cleared. Ratios, not counts, so canvas size and spawn rate cancel.
KILL_RATIO_FULL = 0.35
KILL_RATIO_ZERO = 0.90
KILL_MIN_SEGS = 12       # enemy-present control segments needed for a verdict

# S3 progression anchors. Fire output (draw-rate contrast or projectile
# footprint) in the LAST quarter of a 2.5-minute fire branch vs the FIRST.
# +60% output is a second weapon's worth and earns full credit; +15% is below
# what a noisy wave cycle produces. Requires the late window to clear the same
# absolute detection floor R4 uses, so a ratio of two noise readings is not a
# progression.
PROG_RATIO_LO = 1.15
PROG_RATIO_HI = 1.60

# S4 charge anchors: largest connected projectile blob after a 2.2 s hold-and-
# release, against the largest after discrete taps of the same key over the
# same frames. A charge shot is a visibly BIGGER single object; 1.4x is inside
# sprite-quantisation jitter, 2.2x is unmistakable.
CHARGE_HOLD_FRAMES = 130
CHARGE_RATIO_LO = 1.4
CHARGE_RATIO_HI = 2.2
CHARGE_MIN_BLOB = 4      # cells; anything smaller is a bullet, not a charge

# S6 orb: a second connected component moving WITH the ship — present in the
# exclusive footprint of both the up and the down branch, 2..12 cells (a pod
# is smaller than the ship, bigger than a bullet), within 14 cells of the main
# component. Sized in cells, so canvas-invariant.
ORB_MIN, ORB_MAX, ORB_RADIUS = 2, 12, 14.0

# S8b boss: a single connected content component of at least a 30x30-px
# sprite's area that persists 8 consecutive segments (4 s) WITHOUT touching
# the zone's top or bottom row. Ordinary enemies traverse or die inside 4 s;
# scrolling terrain is bigger but hugs the cropped edges, which is what the
# edge exclusion is for.
BOSS_MIN_PX = 900.0
BOSS_MIN_SEGS = 8

# S9 music: scheduled audio must keep ACCRUING while nothing is pressed.
# Measured on a DEDICATED pure-idle branch (MUSIC_SEGS long, no keys ever
# held), so a per-keypress blip cannot masquerade as a soundtrack. Sustained =
# at least 70% of the branch's 15 s windows schedule something and the overall
# rate clears 8 starts/minute (a bar of 8th notes at 60 bpm is 480; one
# ambient pad restart is ~2). A single looping source (node.loop, or a looping
# <audio> actually playing) is a soundtrack by construction.
MUSIC_SEGS = 60
MUSIC_WINDOW_SEGS = 30
MUSIC_WINDOW_SHARE = 0.70
MUSIC_MIN_PER_MIN = 8.0

# S10 impact VFX: a transient burst of SIMULTANEOUS TINY components — a
# particle fountain is many separate 2-3 px bits at once, which nothing else
# in the genre produces (bullets are bigger and fewer; stars are background
# and constant, so the local median subtracts them). The first version
# detected draw-rate spikes instead and was REFUTED outright: enemy waves
# entering the zone spike the draw rate identically, and all four validation
# products scored ~1.0 where the operator credited exactly one.
BURST_TINY = 6           # simultaneous tiny bits above the local median
BURST_RATE_FULL = 3.0    # bursts per minute of fire for full credit
BURST_RATE_LO = 0.5

# S7 arcade: two title-menu branches (confirm vs down-then-confirm) landing in
# different worlds at the same virtual frame = a selectable second entry.
MENU_DIFF_FRAC = 0.06    # of the grid; a different screen, not a highlight

TEXT_INTRO = re.compile(
    r"(?:\b(?:level|stage|mission|sector|zone|area|wave)\s*[-.:# ]?\s*"
    r"(?:[0-9]+|[ivx]+)\b)|briefing|mission\s*start|get\s*ready", re.I)
TEXT_BOSS = re.compile(r"\bboss\b|\bguardian\b|\bwarship\b", re.I)
TEXT_WIN = re.compile(r"you\s*win|victory|mission\s+accomplished|congratul|"
                      r"campaign\s+complete|the\s+end\b", re.I)
TEXT_ARCADE = re.compile(r"\barcade\b|\bendless\b|\bsurvival\b", re.I)
# Failure screens name themselves many ways; 'game over' alone missed pi-2's
# animated failure card entirely, and the branch then measured a corpse for
# its last 70 seconds (fire contrast 0.00 where the live early window read
# +4.78 draws/frame).
TEXT_GAMEOVER = re.compile(
    r"game\s*over|mission\s*fail|you\s*die|you\s*died|ship\s*lost|"
    r"try\s*again|continue\?|insert\s*coin|retry", re.I)

# Injected AFTER the shared INSTRUMENT and GRID_INSTRUMENT (both of which it
# leaves untouched). Chains onto the already-wrapped canvas text methods, so
# the draw counter keeps counting; records each DISTINCT string with its
# first-seen frame `f`, LAST-seen frame `l` and a count `n`. The last-seen
# frame is what makes screen-STATE marking possible: a string that the title
# screen draws has an `[f, l]` interval covering every frame the title was up,
# so "is the branch back on the title" is answerable per segment without any
# per-frame streaming. Also counts looping audio sources: a BufferSource
# started with .loop set is a soundtrack signature no start-count can fake
# downward.
SPEC_INSTRUMENT = r"""
(function () {
  const P = window.__probe || (window.__probe = {});
  P.textSeen = {};
  P.audioLoopStarts = 0;
  const proto = CanvasRenderingContext2D.prototype;
  for (const m of ['fillText', 'strokeText']) {
    const o = proto[m];
    if (!o) continue;
    proto[m] = function (s, ...a) {
      try {
        const t = String(s).slice(0, 64).trim();
        if (t) {
          const rec = P.textSeen[t];
          if (rec) { rec.n++; rec.l = P.frames | 0; }
          else if (Object.keys(P.textSeen).length < 600)
            P.textSeen[t] = {f: P.frames | 0, l: P.frames | 0, n: 1};
        }
      } catch (e) {}
      return o.apply(this, arguments);
    };
  }
  // Strings whose last draw is at or after a given frame — the per-segment
  // screen-state question, filtered in-page so the per-segment round trip
  // stays small on text-heavy pages.
  window.__recentTexts = function (since) {
    const out = {};
    for (const t in P.textSeen) if (P.textSeen[t].l >= since) out[t] = P.textSeen[t].l;
    return out;
  };
  if (window.AudioBufferSourceNode && AudioBufferSourceNode.prototype.start) {
    const os = AudioBufferSourceNode.prototype.start;
    AudioBufferSourceNode.prototype.start = function (...a) {
      try { if (this.loop) P.audioLoopStarts++; } catch (e) {}
      return os.apply(this, a);
    };
  }
})();
"""


# ---------------------------------------------------------------- helpers

def _pctl(vals, p):
    if not vals:
        return 0.0
    s = sorted(vals)
    i = max(0, min(len(s) - 1, int(p * (len(s) - 1))))
    return s[i]


def _components(cells: set, gw: int) -> list[list[int]]:
    """8-connected components of a set of grid indices, largest first."""
    seen: set = set()
    comps: list[list[int]] = []
    for start in cells:
        if start in seen:
            continue
        comp, stack = [], [start]
        seen.add(start)
        while stack:
            i = stack.pop()
            comp.append(i)
            x, y = i % gw, i // gw
            for dx in (-1, 0, 1):
                for dy in (-1, 0, 1):
                    j = (y + dy) * gw + (x + dx)
                    if (dx or dy) and 0 <= x + dx < gw and j in cells and j not in seen:
                        seen.add(j)
                        stack.append(j)
        comps.append(comp)
    comps.sort(key=len, reverse=True)
    return comps


def _centroid(comp: list[int], gw: int):
    xs = [i % gw for i in comp]
    ys = [i // gw for i in comp]
    return sum(xs) / len(xs), sum(ys) / len(ys)


def _content_cells(grid, zone=True):
    """Indices of cells whose block mean sits OCC_THR from the field median.

    The median cell IS the background: it dominates the area in every product
    measured, dark fields and light fields alike, so 'differs from the median'
    is 'is not background' without assuming which polarity the artist chose.
    Block means already suppress single-pixel stars on large canvases; on
    160 px canvases a cell is ~2.5 px wide, so the p20 floor subtraction in the
    callers is what absorbs the residual star field.
    """
    from rtype_review_score import GRID_H, GRID_W
    if not grid:
        return None
    med = statistics.median(grid)
    out = set()
    x0, x1 = ZONE_X0 * GRID_W, ZONE_X1 * GRID_W
    y0, y1 = ZONE_Y0 * GRID_H, ZONE_Y1 * GRID_H
    for i, v in enumerate(grid):
        if abs(v - med) <= OCC_THR:
            continue
        x, y = i % GRID_W, i // GRID_W
        if zone and not (x0 <= x < x1 and y0 <= y < y1):
            continue
        out.add(i)
    return out


def _seg_diff_cells(a, b) -> int | None:
    from rtype_review_score import _cells
    r = _cells(a, b)
    return r["cells"] if r["measured"] else None


# ---------------------------------------------------------------- drivers

def _new_page(sess):
    from rtype_review_score import CLOCK_PIN, GRID_INSTRUMENT
    from zelda_review_score import INSTRUMENT
    page = sess.browser.new_page(viewport={"width": 1000, "height": 900},
                                 device_scale_factor=1)
    page.add_init_script(CLOCK_PIN)
    page.add_init_script(INSTRUMENT)
    page.add_init_script(GRID_INSTRUMENT)
    page.add_init_script(SPEC_INSTRUMENT)
    return page


def _confirm_key(gesture: str) -> str:
    if gesture and gesture.startswith("key:"):
        return gesture.split(":", 1)[1].split("*", 1)[0]
    return "Enter"


_PLAN_KEYS = {"F": (), "": ()}


def _plan_keys(spec: str, fire_key):
    keys = []
    if "F" in spec and fire_key:
        keys.append(fire_key)
    if "U" in spec:
        keys.append("ArrowUp")
    if "D" in spec:
        keys.append("ArrowDown")
    return keys


def long_branch(sess, entry, gesture: str, fire_key, plan: list[str]) -> dict:
    """Drive one campaign branch; return the per-segment series.

    `plan` is one spec string per segment: '' rest, 'F' fire, 'FU'/'FD' fire
    plus a vertical sweep step (so the ship crosses drop trajectories), 'U'/'D'
    sweep alone. Rest segments inside a fire plan are what make the fire/rest
    draw-rate CONTRAST measurable inside a single branch, with no cross-branch
    clock to disagree with.

    OFF-GAME SCREENS ARE DETECTED LIVE, in two independent ways, because they
    fail differently:

      FROZEN   the last two segment grids differ by fewer than FROZEN_CELLS —
               a briefing card waiting for a key. A live field never holds
               that still; its own scroll repaints tens of cells per segment.
      MARKED   the segment re-drew an ARMED title-marker string (see below)
               or a game-over string. This is what catches the failure the
               frozen rule cannot: a product that dies back to an ANIMATED
               title/game-over screen. MEASURED on codex-1: the fire branch
               died at ~84 s and sat 66 segments on an animated end card that
               read as 'in play' (324 presence cells, constant), which
               manufactured a structural boss and inflated pacing continuity,
               and the parked control branch spent nearly its whole life
               there, which pushed its occupancy floor to 371 cells and
               zeroed the killability denominator.

    ARMING. A title marker is any string the page drew during the pre-gesture
    settle — but several products draw their HUD on the title too (codex-3's
    title shows 'HP ===' and 'L 4', pi-3's shows 'SCORE 000000'), and those
    strings then never stop being drawn, so marking on them masked BOTH
    products' entire runs (measured: 0 in-play segments of 300). A marker
    therefore only ARMS once it has vanished for a whole segment — the title
    left the screen, so this string's return means the title came back. A HUD
    string never vanishes, never arms, never masks.

    Either state triggers one confirm tap (inside the segment's frame budget,
    so segment k stays the same virtual instant in every branch), which
    dismisses cards and restarts from game-over screens; both flags are stored
    per segment so the analysis can mask exactly what the driver saw. A third
    trigger, the FIRE WATCHDOG, taps when a fire branch's own gun goes
    quiet — two consecutive plan cycles in which the fire segments add fewer
    than FIRE-DELTA draws over the rest segments. A dead ship on an unmarked,
    animated failure screen shows exactly that signature and nothing else
    does; measured on pi-2, whose failure card neither froze nor matched any
    marker and absorbed the branch's last 70 seconds.

    Title marks are only collected when a start gesture exists: on a product
    that boots straight into play the pre-gesture strings ARE the game's HUD,
    and marking those would mask the whole run.
    """
    from rtype_review_score import GRID_H, GRID_W, apply_gesture
    out: dict = {"ok": False, "plan": plan, "grids": [], "draws": [], "frames": [],
                 "audio": [], "taps": [], "nonplay": [], "frozen_flags": [],
                 "errors": [], "title_marks": []}
    page = _new_page(sess)
    page.on("pageerror", lambda e: out["errors"].append(str(e)[:160]))
    confirm = _confirm_key(gesture)
    segments = len(plan)
    try:
        page.goto(f"http://127.0.0.1:{sess.port}/{entry.name}",
                  wait_until="load", timeout=30000)
        stalls = [0]

        def adv(n, t=9000):
            tm = t if stalls[0] < 1 else (300 if stalls[0] < 4 else 0)
            if not advance_frames(page, n, tm):
                stalls[0] += 1

        adv(30, 12000)
        title_marks: set = set()
        if gesture != "none":
            title_marks = set((page.evaluate(
                "() => Object.keys(window.__probe.textSeen)") or []))
        out["title_marks"] = sorted(title_marks)[:40]
        box = None
        try:
            box = page.locator("canvas").first.bounding_box(timeout=3000)
        except Exception:  # noqa: BLE001
            box = None
        apply_gesture(page, adv, gesture, box)
        adv(60, 12000)

        last_grid = None
        prev_grid = None
        tap_cool = 0
        off_game = False
        watchdog = False
        seg_start_frame = 0
        armed: set = set()
        dpf_seg: list[float] = []
        quiet_cycles = 0
        cycle_len = 8
        for k in range(segments):
            budget = SEG_FRAMES
            frozen = (last_grid is not None and prev_grid is not None
                      and (_seg_diff_cells(prev_grid, last_grid) or 0) < FROZEN_CELLS)
            if ((frozen or off_game or watchdog) and tap_cool <= 0
                    and len(out["taps"]) < CONFIRM_TAP_MAX):
                page.keyboard.down(confirm)
                adv(2)
                page.keyboard.up(confirm)
                budget -= 2
                out["taps"].append(k)
                tap_cool = CONFIRM_TAP_COOLDOWN
                watchdog = False
            tap_cool -= 1
            keys = _plan_keys(plan[k], fire_key)
            for key in keys:
                page.keyboard.down(key)
            adv(budget, 9000)
            for key in keys:
                page.keyboard.up(key)
            g = page.evaluate("g => window.__grid(g[0], g[1])", [GRID_W, GRID_H])
            p = page.evaluate(
                "() => ({d: window.__probe.draws, f: window.__probe.frames,"
                " a: window.__probe.audioStarts})") or {}
            recent = page.evaluate("s => window.__recentTexts(s)",
                                   seg_start_frame) or {}
            # Arm every marker the page has STOPPED drawing; mask only on the
            # return of an armed one. See the docstring's ARMING note.
            for m in title_marks:
                if m not in recent:
                    armed.add(m)
            off_game = any((t in armed) or TEXT_GAMEOVER.search(t)
                           for t in recent)
            out["grids"].append(g)
            out["draws"].append(int(p.get("d") or 0))
            out["frames"].append(int(p.get("f") or 0))
            out["audio"].append(int(p.get("a") or 0))
            out["nonplay"].append(bool(off_game))
            out["frozen_flags"].append(bool(frozen))
            df = int(p.get("f") or 0) - (out["frames"][-2] if k else 0)
            dd = int(p.get("d") or 0) - (out["draws"][-2] if k else 0)
            dpf_seg.append(dd / max(1, df))
            # FIRE WATCHDOG: at each completed plan cycle, compare fire-segment
            # draw rates with rest-segment draw rates inside that cycle. Two
            # consecutive quiet cycles on a branch that HAS a fire key means
            # the gun stopped mattering — a dead ship on an unrecognised
            # screen — and one confirm tap is the remedy either way.
            if fire_key and (k + 1) % cycle_len == 0:
                idx = range(k + 1 - cycle_len, k + 1)
                fi = [dpf_seg[i] for i in idx if "F" in plan[i]]
                ri = [dpf_seg[i] for i in idx if plan[i] == ""]
                if fi and ri:
                    quiet = (statistics.mean(fi) - statistics.mean(ri)) < 0.5
                    quiet_cycles = quiet_cycles + 1 if quiet else 0
                    if quiet_cycles >= 2:
                        watchdog = True
                        out.setdefault("watchdog_at", []).append(k)
                        quiet_cycles = 0
            seg_start_frame = int(p.get("f") or 0)
            prev_grid, last_grid = last_grid, g
            if stalls[0] >= 6:
                break
        out["stalls"] = stalls[0]
        out["text_seen"] = page.evaluate("() => window.__probe.textSeen") or {}
        out["audio_loop_starts"] = int(page.evaluate(
            "() => window.__probe.audioLoopStarts") or 0)
        out["dom_text"] = (page.evaluate(
            "() => (document.body && document.body.innerText || '').slice(0, 4000)") or "")
        out["media_loop_playing"] = bool(page.evaluate(
            """() => [...document.querySelectorAll('audio')].some(
                   a => a.loop && !a.paused)"""))
        out["ok"] = True
    except Exception as e:  # noqa: BLE001
        out["error"] = str(e)[:200]
    finally:
        try:
            page.close()
        except Exception:  # noqa: BLE001
            pass
    return out


def title_probe(sess, entry, gesture: str) -> dict:
    """Arcade-mode probe: does the title screen hold a navigable menu?

    Branch M1 presses the INERT key then confirm; branch M2 presses ArrowDown
    then confirm. Same number of presses at the same frames, so the two
    branches' confirm lands at the identical virtual instant and the ONLY
    difference is whether a menu cursor moved. (The first version pressed
    confirm alone in M1, one press earlier than M2's — the two worlds were
    then compared at different phases of the same intro and 'a menu exists'
    was measured TRUE on a product with no menu at all: codex-1, 422 cells of
    pure phase difference.) Different terminal worlds now mean the down press
    SELECTED something else — a second mode exists. The drawn/DOM text is
    scanned for arcade vocabulary to say which; menu difference without the
    word is credited at half, because 'a second selectable entry' might be an
    options screen.
    """
    from rtype_review_score import GRID_H, GRID_W, MIN_CELLS, _cells

    def branch(pre_keys):
        page = _new_page(sess)
        try:
            page.goto(f"http://127.0.0.1:{sess.port}/{entry.name}",
                      wait_until="load", timeout=30000)

            def adv(n, t=9000):
                advance_frames(page, n, t)
            adv(45, 12000)
            for k in pre_keys:
                page.keyboard.down(k)
                adv(2)
                page.keyboard.up(k)
                adv(20)
            adv(90, 9000)
            g = page.evaluate("g => window.__grid(g[0], g[1])", [GRID_W, GRID_H])
            txt = page.evaluate("() => window.__probe.textSeen") or {}
            dom = page.evaluate(
                "() => (document.body && document.body.innerText || '').slice(0, 4000)") or ""
            return {"ok": True, "grid": g, "texts": list(txt.keys()), "dom": dom}
        except Exception as e:  # noqa: BLE001
            return {"ok": False, "error": str(e)[:160]}
        finally:
            try:
                page.close()
            except Exception:  # noqa: BLE001
                pass

    confirm = _confirm_key(gesture)
    m1 = branch(["F7", confirm])
    m2 = branch(["ArrowDown", confirm])
    if not (m1.get("ok") and m2.get("ok")):
        return {"value": None, "why": "menu branches failed"}
    d = _cells(m1.get("grid"), m2.get("grid"))
    if not d["measured"]:
        return {"value": None, "why": "menu grids unreadable"}
    blob = " ".join(m1.get("texts", []) + m2.get("texts", [])) + " " + \
        m1.get("dom", "") + " " + m2.get("dom", "")
    worded = bool(TEXT_ARCADE.search(blob))
    menued = d["cells"] >= max(4 * MIN_CELLS, MENU_DIFF_FRAC * len(m1["grid"] or []))
    value = 1.0 if (menued and worded) else (0.5 if (menued or worded) else 0.0)
    return {"value": value, "menu_cells": d["cells"], "arcade_word": worded}


def file_url_probe(sess, entry) -> dict:
    """Load the artifact from file:// — the delivery the operator actually
    received — and ask only whether it renders and animates at all.

    The harness's own HTTP server is a measurement convenience, and it has been
    silently repairing a class of delivery (ES-module entry points, which
    Chrome refuses under file:// as a CORS violation). The operator
    double-clicks index.html; three products in the ground-truth corpus were
    unplayable for exactly this reason while scoring as full games over HTTP.
    """
    page = _new_page(sess)
    errs: list[str] = []
    page.on("pageerror", lambda e: errs.append(str(e)[:120]))
    try:
        page.goto(entry.resolve().as_uri(), wait_until="load", timeout=20000)

        def adv(n, t=6000):
            advance_frames(page, n, t)
        adv(60, 8000)
        probe = page.evaluate(
            "() => ({f: window.__probe.frames, d: window.__probe.draws,"
            " t: window.__probe.ticks})") or {}
        canvas = page.evaluate(
            "() => {const c = document.querySelector('canvas');"
            " return !!(c && c.width && c.height);}")
        ok = bool(canvas and (int(probe.get("f") or 0) >= 30
                              or int(probe.get("t") or 0) >= 30)
                  and int(probe.get("d") or 0) > 0)
        return {"ok": ok, "frames": int(probe.get("f") or 0),
                "draws": int(probe.get("d") or 0), "errors": errs[:3]}
    except Exception as e:  # noqa: BLE001
        return {"ok": False, "errors": (errs + [str(e)[:120]])[:3]}
    finally:
        try:
            page.close()
        except Exception:  # noqa: BLE001
            pass


def charge_probe(sess, entry, gesture: str, fire_key: str) -> dict:
    """Tap-vs-hold differential for a charge shot.

    Three branches over identical frames: T taps the fire key 8 times, H holds
    it CHARGE_HOLD_FRAMES then releases (a charge fires ON release), K holds
    the inert control key. Six frames after release, the largest connected
    component of each branch's exclusive footprint against K is its biggest
    single projectile. A charge mechanic makes H's biggest blob a multiple of
    T's; an autofire stream leaves them the same size.
    """
    from rtype_review_score import GRID_H, GRID_W, apply_gesture

    def branch(mode):
        page = _new_page(sess)
        try:
            page.goto(f"http://127.0.0.1:{sess.port}/{entry.name}",
                      wait_until="load", timeout=30000)

            def adv(n, t=9000):
                advance_frames(page, n, t)
            adv(30, 12000)
            box = None
            try:
                box = page.locator("canvas").first.bounding_box(timeout=3000)
            except Exception:  # noqa: BLE001
                box = None
            apply_gesture(page, adv, gesture, box)
            adv(60, 12000)
            if mode == "tap":
                for _ in range(8):
                    page.keyboard.down(fire_key)
                    adv(2)
                    page.keyboard.up(fire_key)
                    adv(14)
                adv(CHARGE_HOLD_FRAMES - 8 * 16 if CHARGE_HOLD_FRAMES > 8 * 16 else 0)
            else:
                key = fire_key if mode == "hold" else "F7"
                page.keyboard.down(key)
                adv(CHARGE_HOLD_FRAMES, 9000)
                page.keyboard.up(key)
            adv(6)
            return page.evaluate("g => window.__grid(g[0], g[1])", [GRID_W, GRID_H])
        except Exception:  # noqa: BLE001
            return None
        finally:
            try:
                page.close()
            except Exception:  # noqa: BLE001
                pass

    from rtype_review_score import GRID_W as GW, _measurable, _ramp
    t, h, k = branch("tap"), branch("hold"), branch("inert")
    if not _measurable(t, h, k):
        return {"value": None, "why": "charge branches unreadable"}
    thr = 6

    def excl(g):
        return {i for i, (a, b) in enumerate(zip(g, k)) if abs(a - b) > thr}

    tc = _components(excl(t), GW)
    hc = _components(excl(h), GW)
    tap_blob = len(tc[0]) if tc else 0
    hold_blob = len(hc[0]) if hc else 0
    if hold_blob < CHARGE_MIN_BLOB:
        return {"value": 0.0, "tap_blob": tap_blob, "hold_blob": hold_blob,
                "why": "no charge-scale object after hold-release"}
    if tap_blob == 0:
        # The tap branch shows nothing at all: either taps are ignored (a
        # hold-to-fire game) or shots die between taps. A big hold blob alone
        # cannot be attributed to a CHARGE rather than to the stream, so the
        # comparison is unmade.
        return {"value": None, "tap_blob": 0, "hold_blob": hold_blob,
                "why": "tap branch left no footprint to compare against"}
    ratio = hold_blob / float(tap_blob)
    return {"value": round(_ramp(ratio, CHARGE_RATIO_LO, CHARGE_RATIO_HI), 4),
            "tap_blob": tap_blob, "hold_blob": hold_blob,
            "ratio": round(ratio, 2)}


# ---------------------------------------------------------------- series math

def _series(br: dict, cell_px2: float, zone_rows: tuple[int, int]) -> dict:
    """Per-segment derived series for one long branch.

    `inplay[i]` is the conjunction of every screen-state test the driver and
    the grids afford: not frozen (briefing card), not a whole-scene repaint
    (death/restart flash), not title/game-over MARKED (see long_branch), and
    actually measured. Everything downstream — presence floors, pacing,
    killability windows, boss scans — reads only in-play segments, because a
    statistic that averages a title screen into a firefight measures neither.

    Beyond raw occupancy, every segment's zone content is decomposed into
    connected components and banded by PHYSICAL AREA (`cell_px2` = one grid
    cell in canvas px^2):

      big_px    total area of enemy-scale components (>= BIG_PX px^2)
      boss_px   largest single component >= BIG_PX that touches neither zone
                edge row (terrain hugs the crop; a boss floats)
      proj_px   total area of projectile-band components [PROJ_PX_LO, BIG_PX)
      tiny_n    count of particle bits (<= 2 cells and <= TINY_PX px^2)

    The bands are what de-confound the fire branch from its own weapon: a
    bullet can never be an enemy by AREA, whatever it does to cell counts or
    draw rates — the two failure modes this replaced.
    """
    from rtype_review_score import GRID_H, GRID_W, SCENE_CHANGE_FRAC_AXIS
    grids = br.get("grids") or []
    nonplay = br.get("nonplay") or []
    n = len(grids)
    selfdiff, occ, dpf, med = [], [], [], []
    big_px, boss_px, proj_px, tiny_n = [], [], [], []
    y_lo, y_hi = zone_rows
    for i in range(n):
        g = grids[i]
        cc = _content_cells(g)
        occ.append(len(cc) if cc is not None else None)
        med.append(statistics.median(g) if g else None)
        if cc is None:
            big_px.append(None)
            boss_px.append(None)
            proj_px.append(None)
            tiny_n.append(None)
        else:
            comps = _components(cc, GRID_W)
            b = p = 0.0
            t = 0
            bosses = 0.0
            for comp in comps:
                area = len(comp) * cell_px2
                if area >= BIG_PX:
                    b += area
                    rows = {j // GRID_W for j in comp}
                    if y_lo not in rows and y_hi not in rows:
                        bosses = max(bosses, area)
                elif area >= PROJ_PX_LO:
                    p += area
                if len(comp) <= 2 and area <= TINY_PX:
                    t += 1
            big_px.append(round(b, 1))
            boss_px.append(round(bosses, 1))
            proj_px.append(round(p, 1))
            tiny_n.append(t)
        if i == 0:
            selfdiff.append(None)
        else:
            selfdiff.append(_seg_diff_cells(grids[i - 1], g))
        df = br["frames"][i] - (br["frames"][i - 1] if i else 0)
        dd = br["draws"][i] - (br["draws"][i - 1] if i else 0)
        dpf.append(round(dd / max(1, df), 2))
    repaint_cut = SCENE_CHANGE_FRAC_AXIS * GRID_W * GRID_H
    frozen = [s is not None and s < FROZEN_CELLS for s in selfdiff]
    repaint = [s is not None and s > repaint_cut for s in selfdiff]
    inplay = [not (frozen[i] or repaint[i]
                   or (i < len(nonplay) and nonplay[i]))
              and selfdiff[i] is not None and occ[i] is not None
              for i in range(n)]
    valid_big = [big_px[i] for i in range(n) if inplay[i] and big_px[i] is not None]
    floor = _pctl(valid_big, FLOOR_PCT) if valid_big else 0.0
    presence = [max(0.0, big_px[i] - floor) if (big_px[i] is not None) else None
                for i in range(n)]
    return {"n": n, "occ": occ, "presence": presence, "inplay": inplay,
            "frozen": frozen, "repaint": repaint, "dpf": dpf, "median": med,
            "floor": floor, "selfdiff": selfdiff, "big_px": big_px,
            "boss_px": boss_px, "proj_px": proj_px, "tiny_n": tiny_n}


def _seg_of(frame: int, frames: list[int]) -> int:
    """Segment index whose frame window contains `frame`."""
    for i, f in enumerate(frames):
        if frame <= f:
            return i
    return max(0, len(frames) - 1)


def _text_seg_intervals(br: dict, pattern) -> list[tuple[int, int]]:
    """[first_seg, last_seg] intervals of every drawn string matching pattern."""
    frames = br.get("frames") or []
    out = []
    for t, rec in (br.get("text_seen") or {}).items():
        if pattern.search(t):
            out.append((_seg_of(int(rec.get("f", 0)), frames),
                        _seg_of(int(rec.get("l", 0)), frames)))
    return out


# ---------------------------------------------------------------- the tier

S_WEIGHTS = {
    # Direction and killability are the two operator-observed mechanic
    # failures the old tier was blind to; progression is the corrected spec's
    # central loop (drops -> weapons), weighted heaviest in the tier.
    "S1_fire_direction": 3.0,
    "S2_enemies_killable": 3.0,
    "S3_weapon_progression": 4.0,
    "S4_charge_shot": 1.0,        # genre signature: credit, never require
    "S5_enemy_pacing": 3.0,
    "S6_orb_companion": 2.0,
    "S7_arcade_mode": 2.0,
    "S8a_level_intro": 1.5,
    "S8b_boss_encounter": 1.5,
    "S8c_level_aesthetics": 0.5,  # transition often out of probe reach
    "S8d_win_sequence": 0.5,      # pure bonus: UNKNOWN unless actually seen
    "S9_music_layer": 1.5,
    "S10_impact_vfx": 0.5,
    "S11_delivery_selfsufficient": 1.0,
}
assert abs(sum(S_WEIGHTS.values()) - 25.0) < 1e-9


def _blank_spec(note: str) -> dict:
    res = {k: None for k in S_WEIGHTS}
    res["spec_evidence"] = {}
    res["spec_notes"] = note
    return res


def spec_tier(sess, entry, root, ctx: dict) -> dict:
    """Run the spec/genre tier inside the product's existing session.

    `ctx` carries what the mechanics tier already measured, so nothing is
    re-derived: the accepted start gesture, whether activation succeeded, the
    credited fire key, the noise gates, the four arrow branches (for the ship
    column and the orb test) and the R4 fire-branch footprint data.
    """
    from rtype_review_score import (
        CENTROID_MARGIN, GRID_H, GRID_W, _cells, _grid_of, _measurable, _ramp,
    )
    res = _blank_spec("")
    ev: dict = {}
    res["spec_evidence"] = ev
    notes: list[str] = []

    # ---- S11 delivery: measurable whether or not the game ever activates ---
    try:
        html = entry.read_text(errors="replace")
    except Exception:  # noqa: BLE001
        html = ""
    es_modules = bool(re.search(r'<script[^>]*type=["\']module["\']', html, re.I))
    fp = file_url_probe(sess, entry)
    ev["delivery"] = {"es_modules": es_modules, "file_probe": fp}
    if fp.get("ok") is None:
        res["S11_delivery_selfsufficient"] = None
    else:
        # Served-from-disk is the delivery contract; a page that renders under
        # file:// passes however it is written. One that needs a server fails
        # regardless of which packaging feature caused it — but the check
        # reports the module flag so the cause is visible.
        res["S11_delivery_selfsufficient"] = 1.0 if fp["ok"] else 0.0
        if not fp["ok"]:
            notes.append("D_server_required: does not render from file:// as delivered"
                         + (" (ES-module entry)" if es_modules else ""))

    if not ctx.get("activated"):
        res["spec_notes"] = "; ".join(
            notes + ["spec tier: product never activated — play checks UNKNOWN"])
        return res

    gesture = ctx.get("gesture") or "none"
    fire_key = ctx.get("fire_key")
    fire_measured_absent = bool(ctx.get("fire_measured_absent"))

    # ---- ship column from the arrow branches (no new loads) ----------------
    ship_cx = None
    try:
        up, down, base = ctx.get("up"), ctx.get("down"), ctx.get("base")
        gu, gd, gb = (_grid_of(up, "grid_post"), _grid_of(down, "grid_post"),
                      _grid_of(base, "grid_post"))
        if _measurable(gu, gd, gb):
            thr = 6
            xs = []
            for i, (a, b, z) in enumerate(zip(gu, gd, gb)):
                da, db = abs(a - z) > thr, abs(b - z) > thr
                if da != db:
                    xs.append(i % GRID_W)
            if len(xs) >= 3:
                ship_cx = sum(xs) / len(xs)
    except Exception:  # noqa: BLE001
        ship_cx = None
    ev["ship_cx"] = None if ship_cx is None else round(ship_cx, 2)

    # ---- S1 fire direction -------------------------------------------------
    # Forward in a horizontal shmup is +x. Two independent signatures, tried
    # in order of strength:
    #   1. gated downrange TRAVEL of the credited key's footprint (R4's own
    #      measurement, already null-guarded): signed, so its sign IS the
    #      direction;
    #   2. the fire footprint's centroid against the ship column measured off
    #      the vertical arrow pair: bullets hang on whichever side they were
    #      fired at during the 30-frame hold, muzzle-only responses sit ON the
    #      column and stay UNKNOWN.
    if fire_key is None:
        res["S1_fire_direction"] = None
        if fire_measured_absent:
            notes.append("S1 UNKNOWN: no weapon was credited, so no direction exists")
    else:
        travel = ctx.get("fire_travel")
        travels = bool(ctx.get("fire_travels_gated"))
        post_cx = ctx.get("fire_post_cx")
        post_cells = ctx.get("fire_post_cells") or 0
        gate = ctx.get("gate") or 3
        verdict = None
        basis = None
        if travels and travel is not None:
            if travel >= CENTROID_MARGIN:
                verdict, basis = 1.0, f"travel {travel:+.2f} cells downrange"
            elif travel <= -CENTROID_MARGIN:
                verdict, basis = 0.0, f"travel {travel:+.2f} cells BACKWARD"
        if verdict is None and post_cx is not None and ship_cx is not None \
                and post_cells > gate:
            off = post_cx - ship_cx
            if off >= 2.0:
                verdict, basis = 1.0, f"fire footprint {off:+.1f} cols right of ship"
            elif off <= -2.0:
                verdict, basis = 0.0, f"fire footprint {off:+.1f} cols LEFT of ship"
            else:
                basis = f"fire footprint on the ship column ({off:+.1f}) — inconclusive"
        res["S1_fire_direction"] = verdict
        ev["fire_direction"] = {"travel": travel, "travel_gated": travels,
                                "post_cx": post_cx, "ship_cx": ev["ship_cx"],
                                "basis": basis}
        if verdict == 0.0:
            notes.append(f"S1: WRONG-DIRECTION fire ({basis})")
        elif verdict is None:
            notes.append("S1 UNKNOWN: no gated travel and no off-column footprint")

    # ---- S6 orb companion (from the same arrow branches) -------------------
    try:
        thr = 6
        verdicts = []
        for br, other in ((ctx.get("up"), ctx.get("down")),
                          (ctx.get("down"), ctx.get("up"))):
            ga, gb, gz = (_grid_of(br, "grid_post"), _grid_of(other, "grid_post"),
                          _grid_of(ctx.get("base"), "grid_post"))
            if not _measurable(ga, gb, gz):
                verdicts.append(None)
                continue
            own = {i for i, (a, b, z) in enumerate(zip(ga, gb, gz))
                   if abs(a - z) > thr and not abs(b - z) > thr}
            comps = _components(own, GRID_W)
            if not comps:
                verdicts.append(None)
                continue
            main_c = _centroid(comps[0], GRID_W)
            orb = False
            for c in comps[1:]:
                if not (ORB_MIN <= len(c) <= ORB_MAX):
                    continue
                cc = _centroid(c, GRID_W)
                if ((cc[0] - main_c[0]) ** 2 + (cc[1] - main_c[1]) ** 2) ** 0.5 <= ORB_RADIUS:
                    orb = True
            verdicts.append(orb)
        if any(v is None for v in verdicts):
            res["S6_orb_companion"] = None
        else:
            res["S6_orb_companion"] = 1.0 if all(verdicts) else 0.0
        ev["orb"] = verdicts
    except Exception as e:  # noqa: BLE001
        res["S6_orb_companion"] = None
        ev["orb"] = f"error: {e}"

    # ---- S7 arcade menu probe ---------------------------------------------
    men = title_probe(sess, entry, gesture)
    res["S7_arcade_mode"] = men.get("value")
    ev["arcade"] = {k: v for k, v in men.items() if k != "value"}

    # ---- S4 charge shot ----------------------------------------------------
    # Tried on up to two ACCEPTED fire keys, not just the credited one: a
    # product may put its charge on a secondary weapon key (codex-1 documents
    # 'Charge: X/K' beside 'Fire: Z/J'), and the R4 sweep has already measured
    # which keys demonstrably fire. Best credit wins — monotone in evidence.
    charge_keys = [k for k in (ctx.get("fire_keys_accepted") or [fire_key])
                   if k][:2]
    # 'x' is the corpus's second charge convention (documented in three
    # products' own help text as CHARGE while R4 cannot accept it — a charge
    # needs a hold longer than R4's 30 frames to show anything). One extra
    # candidate, only when not already tried.
    if fire_key and "x" not in charge_keys and len(charge_keys) < 3:
        charge_keys.append("x")
    if not charge_keys:
        res["S4_charge_shot"] = None
    else:
        best_ch: dict = {"value": None}
        for ck in charge_keys:
            ch = charge_probe(sess, entry, gesture, ck)
            ch["key"] = ck
            if (best_ch.get("value") is None
                    or (ch.get("value") is not None
                        and ch["value"] > best_ch["value"])):
                best_ch = ch
        res["S4_charge_shot"] = best_ch.get("value")
        ev["charge"] = {k: v for k, v in best_ch.items() if k != "value"}

    # ---- the three campaign branches --------------------------------------
    # F: the fire cycle for its whole 150 s. C: the SAME cycle from segment 0
    # WITHOUT the fire key — identical sweep, identical timing, so until one
    # of them leaves play the two worlds differ only in what fire does, and
    # the killability window is as long as the no-fire ship survives. M: 30 s
    # of pure idle for the music measurement — no keys held at all, so nothing
    # input-triggered can masquerade as a soundtrack.
    cycle = ["F", "FU", "F", "FD", "F", "F", "", ""]
    f_plan = [cycle[i % len(cycle)] for i in range(F_SEGMENTS)]
    c_plan = [cycle[i % len(cycle)].replace("F", "") for i in range(C_SEGMENTS)]
    fbr = long_branch(sess, entry, gesture, fire_key, f_plan)
    cbr = long_branch(sess, entry, gesture, None, c_plan)
    mbr = long_branch(sess, entry, gesture, None, [""] * MUSIC_SEGS)

    # One grid cell's area in canvas px^2, from the mechanics tier's own
    # canvas reading — the constant that turns component cell counts into
    # sprite-scale areas. Falls back to the corpus-typical 160x144 handheld
    # resolution ONLY to keep a missing reading from crashing the tier; the
    # fallback is recorded, because a guessed cell size weakens every band.
    import math as _m
    cv = (ctx.get("base") or {}).get("canvas") or ""
    m = re.match(r"^(\d+)x(\d+)$", cv)
    if m:
        cw, chh = int(m.group(1)), int(m.group(2))
    else:
        cw, chh = 160, 144
        notes.append(f"canvas size unreadable ({cv!r}) — px^2 bands on fallback cell size")
    cell_px2 = (cw / GRID_W) * (chh / GRID_H)
    zone_rows = (_m.ceil(ZONE_Y0 * GRID_H - 1e-9),
                 _m.floor(ZONE_Y1 * GRID_H - 1e-9))
    ev["cell_px2"] = round(cell_px2, 2)
    if not (fbr.get("ok") and cbr.get("ok")):
        notes.append("campaign branches failed: "
                     f"{fbr.get('error', 'ok')} / {cbr.get('error', 'ok')}")
        # The idle music branch is independent of the campaign pair; salvage
        # it rather than losing a measured check to an unrelated failure.
        if mbr.get("ok") and (mbr.get("audio_loop_starts", 0) >= 1
                              or mbr.get("media_loop_playing")):
            res["S9_music_layer"] = 1.0
            ev["music"] = {"loop_source": True}
        res["spec_notes"] = "; ".join(n for n in notes if n)
        return res
    fs, cs = (_series(fbr, cell_px2, zone_rows),
              _series(cbr, cell_px2, zone_rows))
    ev["campaign"] = {
        "f_taps": fbr["taps"], "c_taps": cbr["taps"],
        "f_watchdog": fbr.get("watchdog_at"),
        "f_inplay": sum(fs["inplay"]), "c_inplay": sum(cs["inplay"]),
        "f_floor": fs["floor"], "c_floor": cs["floor"],
        "f_stalls": fbr.get("stalls"), "c_stalls": cbr.get("stalls"),
        "f_presence": [p if p is None else int(p) for p in fs["presence"]],
        "c_presence": [p if p is None else int(p) for p in cs["presence"]],
        "f_proj_px": fs["proj_px"], "f_tiny": fs["tiny_n"],
        "f_dpf": fs["dpf"], "f_plan_note": "cycle F,FU,F,FD,F,F,rest,rest",
        "texts": {t: d for t, d in sorted((fbr.get("text_seen") or {}).items(),
                                          key=lambda kv: kv[1]["f"])[:60]},
    }

    # ---- S5 pacing ---------------------------------------------------------
    # Read off the CONTROL branch whenever it reached enough play: it never
    # fires, so EVERYTHING in its zone is the game's own content and even
    # bullet-sized enemies count (see PRESENT_C_PX). The fire branch is the
    # fallback, big-components only, because its zone is full of its own
    # projectiles. Time is counted over IN-PLAY segments, so briefing cards
    # and restarts the driver clicked through cost nothing; the check grades
    # the emptiness of the field, not the length of the menus.
    def _band_sum(sr, i):
        b, p = sr["big_px"][i], sr["proj_px"][i]
        if b is None:
            return None
        return b + (p or 0)

    c_play = [i for i in range(cs["n"]) if cs["inplay"][i]
              and _band_sum(cs, i) is not None]
    if len(c_play) >= 30:
        src, src_name, entry_bar = cs, "C", PRESENT_C_PX
        vals = {i: _band_sum(cs, i) for i in c_play}
        floor = _pctl([vals[i] for i in c_play], FLOOR_PCT)
        series = {i: max(0.0, vals[i] - floor) for i in c_play}
        play = c_play
    else:
        src, src_name, entry_bar = fs, "F", PRESENT_PX
        play = [i for i in range(fs["n"]) if fs["inplay"][i]
                and fs["presence"][i] is not None]
        series = {i: fs["presence"][i] for i in play}
    if len(play) < 30:
        res["S5_enemy_pacing"] = None
        notes.append("S5 UNKNOWN: fewer than 15 s of measurable play")
    else:
        first_pos = None
        run = 0
        for pos, i in enumerate(play):
            run = run + 1 if series[i] >= entry_bar else 0
            if run >= 2:
                first_pos = pos - 1
                break
        if first_pos is None:
            # 90-150 s of measured play and never two consecutive half-seconds
            # with an enemy-scale object: the strongest pacing failure there is.
            res["S5_enemy_pacing"] = 0.0
            ev["pacing"] = {"first_enemy_s": None, "branch": src_name,
                            "max_presence": max((series[i] for i in play),
                                                default=0)}
            notes.append("S5: no enemy-scale content in the measured window")
        else:
            tfirst = first_pos * SEG_FRAMES / 60.0
            after = play[first_pos:]
            cont = (sum(1 for i in after
                        if series[i] >= CONTINUITY_FRAC * entry_bar)
                    / max(1, len(after)))
            t_credit = 1.0 - _ramp(tfirst, PACING_FULL_S, PACING_ZERO_S)
            c_credit = _ramp(cont, CONTINUITY_LO, CONTINUITY_HI)
            res["S5_enemy_pacing"] = round(min(t_credit, c_credit), 4)
            ev["pacing"] = {"first_enemy_s": round(tfirst, 1),
                            "continuity": round(cont, 3), "branch": src_name}

    # ---- S2 killability ----------------------------------------------------
    # Sustained fire must thin the field. The comparison window is PRISTINE
    # play: segments before EITHER branch first leaves play (death, card,
    # repaint), because after a death-restart the two branches can be on
    # different levels and their populations stop being comparable. Within it,
    # only segments where the control branch holds an enemy-scale population
    # prove anything. Too few provable segments is UNKNOWN, not a pass — and
    # the note says which branch cut the window short.
    if fire_key is None:
        res["S2_enemies_killable"] = 0.0 if fire_measured_absent else None
        if fire_measured_absent:
            notes.append("S2: nothing fires, so nothing can be killed — measured absence")
    else:
        # First CO-PLAY run: from the first segment where both branches are in
        # play (the shared opening briefing is not an exit — both branches sat
        # on it identically) to the first segment after that where either
        # leaves. The first version started at segment 0, so the briefing tap
        # at segment 1 closed the window before it opened and every product
        # fell through to the desync fallback.
        n2 = min(fs["n"], cs["n"])
        co_start = next((i for i in range(1, n2)
                         if fs["inplay"][i] and cs["inplay"][i]), None)
        co_end = n2
        if co_start is not None:
            for i in range(co_start, n2):
                if not (fs["inplay"][i] and cs["inplay"][i]):
                    co_end = i
                    break
        ks = ([] if co_start is None else
              [i for i in range(co_start, co_end)
               if fs["presence"][i] is not None and cs["presence"][i] is not None
               and cs["presence"][i] >= PRESENT_PX])
        window_note = f"pristine co-play window segs {co_start}..{co_end}"
        if len(ks) < KILL_MIN_SEGS:
            # Fallback: every segment where BOTH branches are in play. Weaker —
            # restarts can desynchronise the two campaigns — so it is labelled
            # in the evidence rather than silently substituted.
            ks = [i for i in range(1, min(fs["n"], cs["n"]))
                  if fs["inplay"][i] and cs["inplay"][i]
                  and fs["presence"][i] is not None
                  and cs["presence"][i] is not None
                  and cs["presence"][i] >= PRESENT_PX]
            window_note = "desync-tolerant window (restarts may misalign levels)"
        if len(ks) < KILL_MIN_SEGS:
            res["S2_enemies_killable"] = None
            notes.append(f"S2 UNKNOWN: only {len(ks)} enemy-present control segments")
        else:
            num = sum(fs["presence"][i] for i in ks)
            den = sum(cs["presence"][i] for i in ks)
            ratio = num / max(1, den)
            res["S2_enemies_killable"] = round(
                1.0 - _ramp(ratio, KILL_RATIO_FULL, KILL_RATIO_ZERO), 4)
            ev["killability"] = {"ratio": round(ratio, 3), "segments": len(ks),
                                 "window": window_note}

    # ---- S3 weapon progression --------------------------------------------
    # Fire OUTPUT late in the branch against fire output early. The primary
    # signature is the PROJECTILE-BAND footprint (component areas between a
    # particle and an enemy): what the gun itself puts on screen, measured as
    # the fire-vs-rest contrast inside the branch. The draw-rate contrast is
    # kept as a secondary signature but only when it is a POSITIVE fire
    # signature in BOTH windows — measured on codex-3, kills REMOVE more draws
    # than bullets add early on (contrast -4.67), and a ratio against a
    # clamped negative scored 1.0 'progression' on a product whose weapon
    # never changes. An absolute quantity that can go negative for unrelated
    # reasons is not a growth denominator.
    if fire_key is None:
        res["S3_weapon_progression"] = 0.0 if fire_measured_absent else None
    else:
        fire_seq = [i for i in range(fs["n"])
                    if fs["inplay"][i] and "F" in fbr["plan"][i]]
        rest_seq = [i for i in range(fs["n"])
                    if fs["inplay"][i] and fbr["plan"][i] == ""]

        def contrast(fi, ri):
            if len(fi) < 6 or len(ri) < 2:
                return None
            proj = (statistics.mean(fs["proj_px"][i] or 0 for i in fi)
                    - statistics.mean(fs["proj_px"][i] or 0 for i in ri))
            d = (statistics.mean(fs["dpf"][i] for i in fi)
                 - statistics.mean(fs["dpf"][i] for i in ri))
            return {"proj_px": round(proj, 1), "ddraw": round(d, 2)}

        tf, tr = len(fire_seq) // 3, len(rest_seq) // 3
        early = contrast(fire_seq[:tf], rest_seq[:tr])
        late = contrast(fire_seq[-tf:] if tf else [], rest_seq[-tr:] if tr else [])
        span_s = ((fire_seq[-1] - fire_seq[0]) * SEG_FRAMES / 60.0
                  if len(fire_seq) >= 2 else 0.0)
        ev["progression"] = {"early": early, "late": late,
                             "span_s": round(span_s, 1)}
        if not early or not late:
            res["S3_weapon_progression"] = None
            notes.append("S3 UNKNOWN: not enough in-play fire/rest segments to contrast")
        else:
            # One bullet of the corpus's smallest measured size (18 px^2) is
            # the projectile-band detection floor and minimum denominator.
            #
            # THE BAND IS THE ONLY SCORED SIGNATURE. The draw-rate contrast is
            # reported as evidence but earns nothing: it conflates bullets
            # ADDED with enemies REMOVED, and its window asymmetry scored 1.0
            # 'progression' on a product (tui-multi-1) whose weapon never
            # changes — early kills suppress the early contrast, and the
            # ratio then reads kill-rate decline as weapon growth.
            BULLET_PX = 18.0
            ratios = []
            if late["proj_px"] >= BULLET_PX:
                ratios.append(late["proj_px"] / max(early["proj_px"], BULLET_PX))
            measurable = (late["proj_px"] >= BULLET_PX
                          or early["proj_px"] >= BULLET_PX)
            credit = (round(_ramp(max(ratios), PROG_RATIO_LO, PROG_RATIO_HI), 4)
                      if ratios else 0.0)
            ev["progression"]["ratio"] = round(max(ratios), 3) if ratios else None
            if not measurable:
                # The gun never left a measurable footprint on EITHER
                # signature in either window: growth is unmeasurable here,
                # not absent.
                res["S3_weapon_progression"] = None
                notes.append("S3 UNKNOWN: no measurable fire footprint in either window")
            elif credit == 0.0 and span_s < 60.0:
                # ASYMMETRIC HORIZON RULE. Positive growth stands on any span,
                # but "no growth" asserted from a session the product cut
                # short (died at 40 s) would claim more than was watched.
                res["S3_weapon_progression"] = None
                notes.append(f"S3 UNKNOWN: no growth in {span_s:.0f}s — horizon "
                             "too short to assert absence")
            else:
                res["S3_weapon_progression"] = credit

    # ---- S8 campaign structure --------------------------------------------
    texts = list((fbr.get("text_seen") or {}).keys()) \
        + list((cbr.get("text_seen") or {}).keys())
    dom = (fbr.get("dom_text") or "") + " " + (cbr.get("dom_text") or "")
    blob = " | ".join(texts) + " | " + dom
    played_s = sum(fs["inplay"]) * SEG_FRAMES / 60.0
    intro_hit = bool(TEXT_INTRO.search(blob))
    res["S8a_level_intro"] = 1.0 if intro_hit else 0.0
    ev["intro_texts"] = [t for t in texts if TEXT_INTRO.search(t)][:8]

    # Boss: named on screen, or structurally present — a grid-scale content
    # component holding together for BOSS_MIN_SEGS consecutive segments. The
    # scan runs over corrected in-play segments AND skips any segment inside a
    # level-intro text interval: a mission card is a big connected blob of
    # text, and on codex-1 the un-masked version of this scan credited a
    # structural boss to an end-card the branch died onto.
    boss_text = bool(TEXT_BOSS.search(blob))
    intro_ivs = _text_seg_intervals(fbr, TEXT_INTRO)

    def _in_intro(i):
        return any(a <= i <= b for a, b in intro_ivs)

    boss_struct = False
    boss_at = None
    run = 0
    for i in range(fs["n"]):
        big = (fs["inplay"][i] and not _in_intro(i)
               and (fs["boss_px"][i] or 0) >= BOSS_MIN_PX)
        run = run + 1 if big else 0
        if run >= BOSS_MIN_SEGS:
            boss_struct = True
            boss_at = i
            break
    if boss_text or boss_struct:
        res["S8b_boss_encounter"] = 1.0
    elif played_s >= 135.0:
        # The branch outlived a spec-scale level WITH MARGIN and saw neither
        # the word nor the object. The margin matters: the spec puts the boss
        # at ~2 minutes, so asserting absence at 110 s of play (as the first
        # version did, on pi-2) claims to have watched a level end that was
        # still ten seconds away.
        res["S8b_boss_encounter"] = 0.0
    else:
        res["S8b_boss_encounter"] = None
        notes.append(f"S8b UNKNOWN: only {played_s:.0f}s of play — boss horizon not reached")
    ev["boss"] = {"text": boss_text, "structural": boss_struct,
                  "at_seg": boss_at, "played_s": round(played_s, 1)}

    # Aesthetics: a background fingerprint (field median) sustained-shifted
    # across a LEVEL transition — anchored to the first appearance of a NEW
    # intro-text string after play began, because that is the one event that
    # is a level change by the product's own announcement. Death repaints and
    # confirm taps are NOT transitions (measured: codex-1's death repaints
    # produced five candidate 'transitions' with shift 0.0, which scored a
    # spec item the probe never actually reached). No announced transition
    # inside the budget is UNKNOWN.
    trans_segs = sorted({a for a, _b in intro_ivs if a > 10})
    aest = None
    for t in trans_segs:
        pre = [fs["median"][i] for i in range(max(0, t - 12), t - 2)
               if fs["inplay"][i] and fs["median"][i] is not None]
        post = [fs["median"][i] for i in range(t + 4, min(fs["n"], t + 16))
                if fs["inplay"][i] and fs["median"][i] is not None]
        if len(pre) >= 4 and len(post) >= 4:
            shift = abs(statistics.mean(post) - statistics.mean(pre))
            aest = max(aest or 0.0, 1.0 if shift >= 10.0 else 0.0)
            ev.setdefault("aesthetics", []).append(
                {"seg": t, "shift": round(shift, 1)})
    res["S8c_level_aesthetics"] = aest
    if aest is None:
        notes.append("S8c UNKNOWN: no announced level transition inside the probe budget")

    win_hit = bool(TEXT_WIN.search(blob))
    res["S8d_win_sequence"] = 1.0 if win_hit else None
    if not win_hit:
        notes.append("S8d UNKNOWN: win sequence beyond probe horizon")

    # ---- S9 music layer ----------------------------------------------------
    # From the dedicated idle branch: 30 s with no keys held at all.
    aud = (mbr.get("audio") or []) if mbr.get("ok") else []
    if not aud:
        res["S9_music_layer"] = None
        notes.append("S9 UNKNOWN: idle audio branch failed")
    elif mbr.get("audio_loop_starts", 0) >= 1 or mbr.get("media_loop_playing"):
        res["S9_music_layer"] = 1.0
        ev["music"] = {"loop_source": True}
    else:
        wins = [aud[min(len(aud) - 1, i + MUSIC_WINDOW_SEGS - 1)]
                - (aud[i - 1] if i else 0)
                for i in range(0, len(aud), MUSIC_WINDOW_SEGS)]
        active = sum(1 for w in wins if w >= 3)
        share = active / max(1, len(wins))
        per_min = aud[-1] / max(1e-9, len(aud) * SEG_FRAMES / 3600.0)
        sustained = share >= MUSIC_WINDOW_SHARE and per_min >= MUSIC_MIN_PER_MIN
        res["S9_music_layer"] = 1.0 if sustained else (
            0.5 if aud[-1] >= 3 and active >= 2 else 0.0)
        ev["music"] = {"windows_active": f"{active}/{len(wins)}",
                       "starts_per_min": round(per_min, 1)}

    # ---- S10 impact VFX ----------------------------------------------------
    # A particle fountain is MANY SIMULTANEOUS TINY components. Bullets are
    # bigger and fewer, stars are constant background (the local median
    # subtracts them), and an arriving wave is a few BIG components — none of
    # which moves the tiny-bit count. Burst = the count jumping BURST_TINY
    # above its local median during fire and falling back within 2 segments.
    if fire_key is None:
        res["S10_impact_vfx"] = None
    elif cell_px2 > 40.0:
        # PHYSICALLY UNMEASURABLE, stated rather than scored: on a canvas
        # where one grid cell is >40 px^2 (a ~6x6 px cell), a 2-3 px particle
        # shifts the cell's block mean by only a few percent and never crosses
        # the content threshold. The operator's one particles-credit product
        # (tui-multi-1, 800px canvas) is exactly this case: scoring 0.0 there
        # would report 'no particles' from an instrument that cannot see
        # particles at that resolution.
        res["S10_impact_vfx"] = None
        notes.append(f"S10 UNKNOWN: cell {cell_px2:.0f}px^2 too coarse to "
                     "resolve particle-scale sprites")
    else:
        fire_i = [i for i in range(fs["n"]) if fs["inplay"][i]
                  and "F" in fbr["plan"][i] and fs["tiny_n"][i] is not None]
        bursts = 0
        for pos, i in enumerate(fire_i):
            hood = [fs["tiny_n"][j] for j in fire_i[max(0, pos - 6):pos + 7]
                    if j != i]
            if len(hood) < 5:
                continue
            m = statistics.median(hood)
            nxt = [fs["tiny_n"][j] for j in fire_i[pos + 1:pos + 3]]
            if (fs["tiny_n"][i] - m >= BURST_TINY
                    and nxt and min(v - m for v in nxt) <= BURST_TINY / 2):
                bursts += 1
        if not fire_i:
            res["S10_impact_vfx"] = None
        else:
            per_min = bursts / max(1e-9, len(fire_i) * SEG_FRAMES / 3600.0)
            res["S10_impact_vfx"] = round(
                _ramp(per_min, BURST_RATE_LO, BURST_RATE_FULL), 4)
            ev["vfx"] = {"bursts": bursts, "per_min": round(per_min, 2)}

    res["spec_notes"] = "; ".join(n for n in notes if n)
    return res


# ---------------------------------------------------------------- selfcheck

def selfcheck_spec_scales(steps: int = 60) -> dict:
    """Monotone/saturation/floor verification for every graded spec scale.

    Same contract as the mechanics tier's selfcheck: monotone in the direction
    each scale claims, saturating at the stated anchor, zero at the stated
    floor. Pure arithmetic.
    """
    from rtype_review_score import _ramp

    def mono(name, f, lo, hi, label):
        vals = [f(lo + (hi - lo) * i / steps) for i in range(steps + 1)]
        bad = [(i, vals[i - 1], vals[i]) for i in range(1, len(vals))
               if vals[i] < vals[i - 1] - 1e-12]
        return {"scale": name, "over": label, "monotone": not bad,
                "violations": bad[:3]}

    checks = [
        mono("S2 kill ratio (must FALL)",
             lambda r: -(1.0 - _ramp(r, KILL_RATIO_FULL, KILL_RATIO_ZERO)),
             0.0, 2.0, "population ratio 0..2, negated"),
        mono("S3 progression ratio",
             lambda r: _ramp(r, PROG_RATIO_LO, PROG_RATIO_HI), 0.0, 5.0,
             "late/early output 0..5"),
        mono("S4 charge blob ratio",
             lambda r: _ramp(r, CHARGE_RATIO_LO, CHARGE_RATIO_HI), 0.0, 6.0,
             "hold/tap blob 0..6"),
        mono("S5 time-to-enemy (must FALL)",
             lambda t: -(1.0 - _ramp(t, PACING_FULL_S, PACING_ZERO_S)),
             0.0, 120.0, "seconds to first enemy, negated"),
        mono("S5 continuity",
             lambda c: _ramp(c, CONTINUITY_LO, CONTINUITY_HI), 0.0, 1.0,
             "presence continuity 0..1"),
        mono("S10 burst rate",
             lambda b: _ramp(b, BURST_RATE_LO, BURST_RATE_FULL), 0.0, 10.0,
             "bursts/min 0..10"),
    ]
    sat = {
        "S2 at ratio<=0.35": 1.0 - _ramp(KILL_RATIO_FULL, KILL_RATIO_FULL, KILL_RATIO_ZERO),
        "S3 at 1.6x": _ramp(PROG_RATIO_HI, PROG_RATIO_LO, PROG_RATIO_HI),
        "S3 at 100x (no more)": _ramp(100.0, PROG_RATIO_LO, PROG_RATIO_HI),
        "S4 at 2.2x": _ramp(CHARGE_RATIO_HI, CHARGE_RATIO_LO, CHARGE_RATIO_HI),
        "S5 instant engagement": 1.0 - _ramp(PACING_FULL_S, PACING_FULL_S, PACING_ZERO_S),
        "S10 at 3/min": _ramp(BURST_RATE_FULL, BURST_RATE_LO, BURST_RATE_FULL),
    }
    floors = {
        "S2 at ratio>=0.90": 1.0 - _ramp(KILL_RATIO_ZERO, KILL_RATIO_FULL, KILL_RATIO_ZERO),
        "S3 at 1.15x": _ramp(PROG_RATIO_LO, PROG_RATIO_LO, PROG_RATIO_HI),
        "S4 at 1.4x": _ramp(CHARGE_RATIO_LO, CHARGE_RATIO_LO, CHARGE_RATIO_HI),
        "S5 at 45s": 1.0 - _ramp(PACING_ZERO_S, PACING_FULL_S, PACING_ZERO_S),
        "S10 at 0.5/min": _ramp(BURST_RATE_LO, BURST_RATE_LO, BURST_RATE_FULL),
    }
    out = {"checks": checks, "saturation": sat, "floors": floors}
    out["MONOTONE"] = all(c["monotone"] for c in checks)
    out["SATURATES"] = all(abs(v - 1.0) < 1e-9 for v in sat.values())
    out["FLOORS_ZERO"] = all(v == 0.0 for v in floors.values())
    out["PASS"] = bool(out["MONOTONE"] and out["SATURATES"] and out["FLOORS_ZERO"])
    return out
