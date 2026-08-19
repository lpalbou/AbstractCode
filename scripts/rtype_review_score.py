#!/usr/bin/env python3
"""Score R-TYPE artifacts — a horizontal shoot-'em-up rubric.

WHY THIS FILE EXISTS. The R-Type matrix was first graded with
`zelda_review_score.py`. That was wrong, and the wrongness was not subtle:

  * `C1_live_map_ids` / `C2_live_npc_quest_ids` — 24% of the weight — ask for
    map ids and NPC quest ids. A shoot-'em-up has neither. C2 was 0 in all 24
    products and C1 <= 1 in all 24: two checks carrying a quarter of the rubric
    and zero bits of information.
  * `B2_responds_to_arrows` — 17.5% of the weight — required held-arrow pixel
    change to EXCEED IDLE change by 3x (`IDLE_DOMINANCE`). That control is
    correct for Zelda, whose screen is static until the player moves. In a
    side-scroller the world scrolls by ITSELF: measured idle windows of
    2,157-47,600 changed px, with the ship adding ~1% on top. Median measured
    ratio across the 23 live products: 1.02, against a required 3.0. The check
    was structurally unpassable, and what it actually measured was "this is not
    a scrolling game". 23/24 products failed it while a probe that activates
    the game first found 23/24 demonstrably arrow-responsive.

So ~41% of that rubric was dead or inverted here. The global LOGIC carries over
— does it build, launch and render; are the sprites animated; are there
weapons and power-ups; is the code modular and maintainable — but every
genre-specific NOUN has to be re-derived for the genre being graded.

THE INSTRUMENT CHANGE. Zelda's probe compares an input window against an idle
window in ONE page load. That cannot work when the world moves on its own,
because the autonomous motion is common-mode to both windows and swamps the
signal. This scorer uses a REPLAY DIFFERENTIAL instead: the injected
instrumentation already makes a page deterministic (seeded RNG, virtual clock,
single-stepped frames), so the same artifact can be loaded N times, advanced to
the SAME virtual frame, and given DIFFERENT input. Diffing branch against
branch cancels the scroll, the enemy spawns and the parallax exactly, and what
survives is the effect of the input alone.

That determinism is a PRECONDITION, not an assumption: every product is loaded
twice with identical (empty) input and the two worlds are compared cell by
cell. The result is reported as `replay_cells` and used as this product's own
NOISE FLOOR, so a differential has to beat the drift rather than merely exist.

AND THE PRECONDITION DID NOT HOLD AT FIRST. It failed on 14 of 23 products, and
the cause was the DRIVER, not the artifacts. Two consecutive runs of the same
unchanged corpus disagreed by up to 16 of 70 behavioural points on the same
product and the `deterministic` flag itself flipped on 9 of them. A controlled
experiment — 3 replays x 23 products x 3 instrument variants — isolated two
independent causes, neither of them the asset-loading hypothesis (no product in
this corpus loads a single image, or calls fetch, or reads crypto or `new
Date`):

  (a) `INSTRUMENT` opens with `budget = Infinity`, so the page ran FREELY from
      `load` until the driver's first `__waitFrames` call. Measured: 1 to 10
      uncontrolled frames, terminal counts of 151/152/151 where 150 were asked
      for. Fixed by `GRID_INSTRUMENT`, which clamps the budget at
      document_start.
  (b) `virt()` falls back to the wall clock until the first frame runs, and 15
      of 23 products seed their `lastTime` from `performance.now()` at init.
      Fixed by `CLOCK_PIN`, injected before the instrument.

With both fixes: all 23 live products return identical hashes, identical draw
counts to the unit, and a grid distance of exactly 0 across 3 replays each.
Neither fix touches zelda's INSTRUMENT, which is shared with a validated rubric
and is not defective for a game whose world holds still — the bug only bites
when the world moves on its own between the load and the first measurement.

Rubric weights (100 total):
  Tier 0  VALIDITY GATES, binary and disqualifying — builds, resolves, parses.
  Tier 1  MECHANICS, 80 raw x 0.6875 = 55 — observed in Chrome. Renders,
          flies, shoots, scrolls. Internal weights untouched (see T1_RESCALE).
  Tier 1b SPEC + GENRE, 25 — observed in Chrome over two long campaign
          branches plus dedicated probes (`rtype_spec_tier.py`). Fire
          direction, killability, drop-driven weapon progression, pacing,
          orbs, arcade mode, campaign structure, music layer, delivery
          self-sufficiency. Added because the operator PLAYED the corpus and
          the mechanics tier could not represent one of their findings.
  Tier 2  CONTENT + CODE QUALITY, 20 — read from source. Partly gameable, so
          never weighted above the behavioral tiers, and every gameable check
          is flagged as such in the notes. It was 30 until the source tier was
          shown to be buyable outright; see `t2_score`.

EVERY CHECK IS A NUMBER IN [0, 1], or UNKNOWN. Missing evidence is UNKNOWN and
never 0: a branch that did not run, a canvas that could not be read, or a noise
floor that could not be built are all states in which the instrument has no
verdict, and reporting 0 for them would state that a game was watched and found
not to move. UNKNOWN earns no points — points have to be observed — so `SCORE`
is a FLOOR and `SCORE_CEILING` is what the product would have scored had every
unmeasured check passed. A wide band means re-run, not rank.

SIX OF THE NINE BEHAVIOURAL CHECKS ARE GRADED, and that is the difference
between this version and every earlier one. A threshold on a continuous
measurement discards the measurement twice over: the Tier-1 total took TEN
distinct values across 23 products with 8 of them pinned at a perfect 80/80,
and every single product that moved between two runs of identical code over
identical artifacts moved by FLIPPING a threshold, 8 to 11 points at a time.
`tui-coder-3` is the proof that the flip was not even monotone — its horizontal
differential went DOWN, 21 cells to 16, while the check went False to True.
Graded, the same 23 products take 23 distinct Tier-1 totals, and on identical
measurements the mean run-to-run movement falls by about a third. The three
checks that are still binary — R0, R1, R5 — are VALIDITY GATES, and a gate that
can be half-passed is not a gate. See the "graded Tier-1 scales" section for
each scale, where it saturates, and why the checks whose magnitude is a
statement about implementation STYLE (draw calls, scheduled oscillators) grade
detection confidence only and refuse to grade the magnitude.

Explicitly NON-SCORING (reported only): file count, total bytes. Weighting size
scores verbosity as quality, and this corpus contains a 6-file product that
scored identically to a 51-file one.

ACTIVATION. Every product in this corpus boots into a title/menu/intro state
whose background ALREADY animates, so any rule of the form "the canvas is
moving, therefore the game is running" concludes RUNNING for 23 of 23 and never
presses a start key. Under the now-deterministic instrument the consequence is
unmissable: holding an arrow changes NOTHING on any product, because no product
was ever started, and the 16/23 and 17/23 the first version scored for R2 and
R3 were replay noise. A "the gesture must make the canvas 4x busier" rule is no
better and was refuted directly — a title card is often BUSIER than the game
behind it (measured self-motion before -> after the gesture that demonstrably
unlocks the controls: 387 -> 354, 1317 -> 1038). So a gesture is accepted only
when the ship DEMONSTRABLY answers the stick after it. See `_activate`.

DEAD-CHECK AUDIT. Every run prints the distribution of every check across the
corpus and flags any check that is CONSTANT. A constant check carries zero
information — that is precisely how the Zelda rubric failed here — so it must
be fixed or removed rather than left to pad the denominator. The audit counts
only the products it actually ranks, and audits the POINTS a check contributes
rather than its raw value: both mistakes hid dead weight in the first version.

Usage:
  python3 scripts/rtype_review_score.py --root untracked/rtype-bench
  python3 scripts/rtype_review_score.py --root untracked/rtype-bench --jobs 4
"""
from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import re
import statistics
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

# The harness core is genre-NEUTRAL and stays shared, so the two rubrics can
# never drift apart on how a page is served, stepped or sampled. Only the
# rubric on top of it is R-Type specific. Both files are hashed into the output.
from zelda_review_score import (  # noqa: E402
    CHANGED_FLOOR,
    CHROME_CHANNEL,
    INSTRUMENT,
    advance_frames,
    resolve_entry,
    serve_dir,
    tier0,
)

# Tier 1b — the SPEC/GENRE behavioural checks (operator's corrected prompt,
# 2026-08-04): fire direction, killability, drop-driven weapon progression,
# pacing, orbs, campaign structure, arcade mode, music layer, delivery. Lives
# in a sibling file so the mechanics tier above stays reviewable; it is hashed
# into the rubric sha alongside this file and the harness.
from rtype_spec_tier import (  # noqa: E402
    S_WEIGHTS,
    selfcheck_spec_scales,
    spec_tier,
)

# ---------------------------------------------------------------- constants

# Frames advanced before any measurement, to get past a title screen/attract
# mode and into play. Longer than Zelda's 45: a shmup typically scripts an
# intro scroll before the player has control.
WARMUP_FRAMES = 90
# Frames each input branch is HELD. Held, never tapped — every artifact in this
# corpus reads a held-keys map, and `keyboard.press()` emits keydown+keyup with
# no frame between them (measured on the Zelda corpus: 30 taps moved the player
# 0 px, 30 held frames moved it every time).
BRANCH_FRAMES = 30

# Start gestures, tried in order until the game becomes RESPONSIVE (see
# _activate). "none" is first so a game that is already playing is never poked
# with a key that could pause it — Enter is a pause toggle in part of this
# corpus, which is exactly why it must not be pressed speculatively at a game
# that is already running.
#
# The `*2` forms exist because a SINGLE press is not enough for any product in
# this corpus. MEASURED on four products with an independently verified
# deterministic instrument: `key:Enter` alone left every one of them
# unresponsive to arrows, and `key:Enter*2` — title -> briefing -> play — made
# all four respond (vertical differential 0 -> 31, 0 -> 18, 0 -> 16, 0 -> 42).
# Source confirms the two-step: abstractcode-basic-1 boots to
# `GAME_STATE.TITLE`, codex-2 to `GAME_STATES.TITLE`, pi-2 and opencode-3 to
# `state = 'title'`, tui-multi-1 and tui-multi-3 to `state = 'menu'`.
# `key:Enter*5` is not more of the same: it is the only entry long enough for a
# TIMED intro. MEASURED false negative — pi-2's `world.update()` returns early
# while `state === 'intro'` until `messageTimer` expires at 4.8 s, which is frame
# 289 at its own 60 fps. The longest schedule here reached frame 146 and then
# stopped pressing, so all eight gestures measured exactly 0/0 cells, the product
# was graded on its intro card, and R4 then passed with `fire_key='Enter'` — the
# START key, which is precisely the failure this rubric claims to have closed.
# Five presses at GESTURE_GAP_FRAMES apart reach frame 365, past the slowest
# intro in the corpus with margin.
#
# `key:1*2` / `key:1*5` are the DIGIT-SELECT convention — "[1] Campaign
# [2] Arcade", coin-op "PRESS 1" — which is a menu style, not a product.
# MEASURED false negative without them: abstractcode-basic-2's title reads
# "Select mode: [1] Campaign  [2] Arcade" and its update() consumes ONLY the
# digit keys while in state 'title', so all ten prior gestures measured exactly
# 0/0 cells and a fully playable product (verified by hand: ship answers all
# four arrows, z/j fires, HUD and enemies live) scored 0.143 as if dead. The
# digit is pressed at a game that every earlier gesture has already been shown
# NOT to start, so the same "never poke a running game" rule that protects
# Enter protects this. `*5` for the same timed-intro reason as `key:Enter*5`
# (that product's intro locks input for 5.5 s — frame 330 at its 60 fps).
START_GESTURES = ["none", "key:Enter*2", "key:Enter*3", "key:Space*2",
                  "click:canvas*2", "key:z*2", "key:j*2", "key:x*2", "key:1*2",
                  "key:Enter*5", "key:Space*5", "key:1*5"]

# Fire-button candidates.
#
# `a` and `f` were REMOVED. COUNTED across the corpus: `a`/`KeyA` is read as
# WASD-LEFT by 18 of the 23 live products and as a weapon by none of them. A
# movement key was competing to win the weapon check, and under the old
# first-past-the-post rule it only had to get there before the real fire key.
# `f` is bound by nothing here. `k` was ADDED: abstractcode-basic-3, codex-1,
# opencode-1 and tui-multi-2 bind it as fire or as the charge/alternate weapon.
FIRE_KEYS = ["Space", "z", "x", "j", "k", "c", "Control", "Enter"]

# R4. Extra draw calls per frame that the fire key must add, against the
# no-input control. ABSOLUTE, not a ratio: a game that paints its background
# tile by tile issues 240 draws a frame and a game that blits one image issues
# 3, so a percentage test would be 80x harder on the first. A projectile costs
# about one draw call; measured real deltas after correct activation were +1.0,
# +3.1 and +7.8 draws/frame, and the inert-key control adds exactly 0.00.
FIRE_DRAW_DELTA = 0.5
# A fire key must leave the world RUNNING and the scene INTACT. A pause key
# freezes autonomous motion, a stop kills the draw rate, and a start/restart
# repaints the frame — three ways to change the picture without firing a shot,
# all three of which occur in this corpus.
ALIVE_MOTION_FRAC = 0.35
ALIVE_DRAW_FRAC = 0.40
SCENE_CHANGE_FRAC = 0.45

# Real lines a .js file must carry before it counts as a MODULE rather than as
# padding. Set from the corpus: the real products' median file is well above
# this, and the adversarial 13-stub fixture is entirely below it.
SUBSTANTIAL_LOC = 20

# Genre vocabularies for the source tier. Deliberately generous: the check is
# "is this concept present and WIRED IN", enforced by requiring >= 2 references,
# not "did the author use my preferred noun".
VOCAB = {
    "weapon": r"\b(laser|beam|bullet|projectile|missile|shot|torpedo|plasma|"
              r"wave|charge|spread|homing|reflect|bomb|cannon|blaster)\w*",
    "powerup": r"\b(powerup|power_?up|upgrade|pickup|bonus|capsule|crystal|"
               r"item|shield|speedup|speed_?up|force|pod|bit|option)\w*",
    "enemy": r"\b(enemy|enemies|foe|alien|drone|turret|boss|swarm|formation|"
             r"spawner|wave|mob|hostile)\w*",
    "stage": r"\b(stage|level|checkpoint|parallax|scroll|background|layer|"
             r"terrain|tileset|section)\w*",
    "sprite": r"\b(sprite|frame|anim|animation|atlas|spritesheet|sprite_?sheet|"
              r"texture|image|draw\w*frame)\w*",
}


# Inert key. No game in this corpus binds F7, so a branch that HOLDS it is a
# perfect null treatment: identical protocol, identical page, an input the game
# cannot be reading. Whatever change it produces is the instrument's own noise
# floor, and every differential check must clear it.
#
# MEASURED, and the reason this exists: an attract-loop fixture whose keydown
# handler was literally `function (e) { /* empty */ }` passed
# R3_ship_moves_horizontally and R4_weapon_fires under the hash differential —
# 23 points for a page that provably cannot read input. Two loads of the same
# page do not receive bit-identical frame delivery, so the scrolling starfield
# lands on a different phase and the terminal hashes differ. `hdiff` was reading
# frame-delivery jitter as gameplay.
CONTROL_KEY = "F7"

# Frames of no-input settle after a branch releases its keys, before the LATE
# checkpoint. Persistence is measured here: a real input has consequences that
# outlive the keypress (the ship is somewhere else, an enemy is dead, the score
# moved), while a cosmetic response — a corner LED that lights while a key is
# held — re-converges on the no-input world within a few frames.
LATE_FRAMES = 45

# Coarse grid used for every cross-branch comparison, in cells.
#
# 48x36 WAS TOO COARSE, and a positive control caught it. A hand-written control
# fixture whose ship provably moves — `if (held.ArrowUp) py -= 3`, 30 held
# frames, 180 px between the two branches on a 480x360 canvas — produced
# EXACTLY 4 changed cells against a floor of 4, and scored R2 and R3 False. At
# 48x36 a cell is 10x10 px, a 16x8 ship covers about two of them, and an opposed
# pair therefore yields ~4 cells no matter how far the ship flew. The check was
# not measuring displacement, it was measuring sprite area against the grid.
# 64x48 puts a 16x8 ship across ~5 cells and leaves headroom above the floor.
GRID_W, GRID_H = 64, 48
# Cells that must differ before a difference is called real. Floor only; the
# operative gate is the measured noise floor (null + control branches). Set at 3
# because the measured floor for a page that does NOT respond is exactly 0 — the
# adversarial attract loops return 0 cells and identical centroids to 12 decimal
# places — so the gap this has to straddle is 0 vs ~5, not 4 vs 5.
MIN_CELLS = 3
# Cells an opposed arrow pair must differ by before a start gesture is accepted
# as having put the game INTO PLAY. Deliberately above MIN_CELLS: see _activate.
ACTIVATION_CELLS = 6
# Centroid separation required to call a response DIRECTIONAL, in grid cells.
# Applied to EXCLUSIVE footprints (see _axis_sep), where a ship moving 9 rows
# separates the two centroids by ~9 rows.
CENTROID_MARGIN = 1.5
# How far above the noise gate a differential must sit before the reading is
# FULLY trusted. It is the point at which the evidence ramp in `_axis_credit`
# and `_persist_credit` saturates: at the gate the differential equals the
# measured noise and earns nothing, at `TIE_FACTOR x gate` it is worth its full
# weight, and in between it is worth the fraction it can support.
#
# SET FROM MEASUREMENT, not from taste. Two runs of identical code over
# identical artifacts moved 5 of 24 products. Across the 72 axis checks those
# runs passed, the signal/gate ratio was 1.17, 1.28, then 2.67, 3.25, 3.33,
# 3.33, 3.67, and on up to 1023. The two ratios below 2.0 are EXACTLY the two
# passes that flipped to False on the rerun — tui-multi-1 vertical and
# opencode-3 horizontal — and every check at 2.67 or above reproduced. So 2.0 is
# a cut placed in an observed gap rather than a chosen tolerance.
#
# IT USED TO BE A BAND REPORTING UNKNOWN, and that was right for a bit: an
# unreproducible bit is worse than no bit. It was wrong as a solution, because
# it moved the cliff instead of removing it — at exactly 2x the gate a check
# was worth 0 points and one cell later 11. A ramp says the same measured thing
# at the resolution the measurement actually has.
TIE_FACTOR = 2.0
# Fraction of the grid above which a difference is a SCENE REPAINT rather than a
# moving object. R4 has always used this to reject a start/restart key; R2/R3
# did not, and pi-3 passed R3 on 3069 of 3072 cells — 99.9% of the screen, 2.2x
# the fraction R4 rejects at. A directional reading taken across a whole-screen
# repaint is not a reading of where the ship is; it is UNKNOWN. Only one check on
# one product in the corpus exceeds even 25% of the grid, so this separates the
# repaint case from every real one with room to spare.
SCENE_CHANGE_FRAC_AXIS = 0.45

# Injected BEFORE zelda's INSTRUMENT, so the `REAL_NOW` that INSTRUMENT
# captures is this constant rather than the wall clock.
#
# THE SECOND HALF OF THE DETERMINISM BUG. INSTRUMENT's `virt()` returns
# `REAL_NOW() - T0` until the first animation frame runs, and 15 of the 23 live
# products call `performance.now()` once at init to seed their `lastTime`. That
# made their FIRST dt a wall-clock measurement of however long the page took to
# parse, and every later frame inherited the offset as a sub-pixel position
# error that the renderer then quantised into a different picture. Pinned, the
# seed is 0 on every load.
#
# PROOF that both halves were needed, from a 3-variant x 3-replay x 23-product
# experiment: as shipped, 3 distinct terminal hashes out of 3 replays. With the
# frame gate alone, frame counts became exact but tui-coder-1 and pi-3 still
# drifted. With the frame gate AND this clock pin, all 23 live products
# returned identical hashes, identical draw counts to the unit, and a grid
# distance of exactly 0 across every replay.
#
# The one thing this changes downstream is `__waitFrames`'s own timeout check,
# which also reads REAL_NOW and can therefore no longer expire. Its independent
# `setTimeout` backstop — a real timer, untouched — still fires at
# `timeoutMs + 250`, so a dead page is still detected as a stall, 250 ms later.
CLOCK_PIN = r"""
(function () {
  window.__realNow = performance.now.bind(performance);
  performance.now = function () { return 0; };
})();
"""

# A second init script, kept separate from the shared harness INSTRUMENT so the
# genre-neutral core stays untouched. Reduces each canvas to a GRID_W x GRID_H
# block-mean image, which is what makes branch-to-branch comparison possible at
# all: `window.__hash()` samples one pixel in 97, so a 16x8 ship contributes ~1.3
# samples and routinely hashes IDENTICALLY from two different positions. Measured
# on a control fixture that moves its ship 90 px on ArrowUp: hash-identical,
# R2 scored False. Block means cannot miss a sprite that lands inside a cell.
GRID_INSTRUMENT = r"""
(function () {
  // Clamp the frame budget to zero at document_start. INSTRUMENT opens with
  // `budget = Infinity`, so between `load` and the driver's first
  // `__waitFrames` round-trip the page ran FREELY for a wall-clock-sized
  // window. MEASURED, 3 replays x 23 products: 1 to 10 uncontrolled frames,
  // and terminal frame counts of 151/152/151, 159/158/160, 152/153/151 where
  // 150 were requested. Two loads of the same file were therefore two
  // different worlds before any input was applied.
  // `__waitFrames(0, 0)` resolves immediately and leaves budget at 0, so the
  // page is frozen until the driver explicitly grants frames.
  if (window.__waitFrames) { try { window.__waitFrames(0, 0); } catch (e) {} }
  window.__grid = function (gw, gh) {
    const c = document.querySelector('canvas');
    if (!c || !c.width || !c.height) return null;
    let d;
    try { d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data; }
    catch (e) { return null; }
    const out = new Array(gw * gh).fill(0);
    const cnt = new Array(gw * gh).fill(0);
    for (let y = 0; y < c.height; y++) {
      const gy = ((y * gh / c.height) | 0);
      for (let x = 0; x < c.width; x++) {
        const gx = ((x * gw / c.width) | 0);
        const i = (y * c.width + x) * 4;
        const k = gy * gw + gx;
        out[k] += (d[i] + d[i + 1] + d[i + 2]) / 3;
        cnt[k]++;
      }
    }
    for (let k = 0; k < out.length; k++) out[k] = cnt[k] ? Math.round(out[k] / cnt[k]) : 0;
    return out;
  };
})();
"""


def sha_of(*paths: Path) -> str:
    h = hashlib.sha256()
    for p in sorted(paths):
        h.update(p.read_bytes())
    return h.hexdigest()


# ---------------------------------------------------------------- grid algebra

def _measurable(*grids) -> bool:
    """True when every grid is PRESENT and comparable.

    EVIDENCE LAW. A missing grid is a measurement that DID NOT HAPPEN — the
    branch failed to drive, the page has no canvas, or `getImageData` threw on a
    tainted one. Every comparison below answers "0 cells changed" for that case,
    which is the same answer it gives for "these two worlds are identical", and
    the two mean opposite things: one is a passed null check, the other is no
    evidence at all.

    The consequence was not hypothetical. `deterministic` is `replay_cells == 0`,
    so a product whose canvas could not be read reported `replay_cells: 0` and
    `deterministic: True` — the rubric's own precondition CONFIRMED from the
    absence of any measurement. Callers that can be wrong in that direction now
    check `measured` and report UNKNOWN instead.
    """
    if any(not g for g in grids):
        return False
    return len({len(g) for g in grids}) == 1


def _cells(a, b, thr: int = 6) -> dict:
    """Compare two block-mean grids.

    Returns the COUNT of cells that differ and the CENTROID of the differing
    region. The count answers "did anything happen"; the centroid answers "where",
    which is what separates a ship that flew up from a HUD counter in the corner
    that lit up. A cosmetic response has the same centroid whatever the key.

    `measured` distinguishes "no difference" from "no measurement"; see
    `_measurable`. It is False ONLY when a grid is missing, never when two real
    grids agree.
    """
    if not _measurable(a, b):
        return {"cells": 0, "cx": None, "cy": None, "mass": 0, "measured": False}
    n = cx = cy = 0
    mass = 0
    for i, (p, q) in enumerate(zip(a, b)):
        d = abs(p - q)
        if d > thr:
            n += 1
            mass += d
            cx += i % GRID_W
            cy += i // GRID_W
    if not n:
        return {"cells": 0, "cx": None, "cy": None, "mass": 0, "measured": True}
    return {"cells": n, "cx": cx / n, "cy": cy / n, "mass": mass, "measured": True}


def _grid_of(br: dict, slot: str):
    return (br or {}).get(slot)


def _axis_sep(ga, gb, gbase, key: str, thr: int = 6):
    """Signed displacement between two opposed branches along one axis.

    WHY NOT THE OBVIOUS THING. The first version took the centroid of
    "ArrowUp differs from no-input" and compared it with the centroid of
    "ArrowDown differs from no-input". Each of those footprints has TWO blobs —
    where the ship was, and where it went — so both centroids sit near the
    midpoint of their pair and the separation collapses. MEASURED on the control
    fixture whose ship provably moves 180 px between branches: cy 16.55 vs 16.88,
    a separation of 0.33 rows for a movement of 18 rows. The check was reading
    the shared blob, which is the same in both branches by construction.

    So each branch's footprint is restricted to the cells that are ITS OWN —
    changed under this input and not under the opposed one. That is the ship's
    new position with the old position removed, and the separation between the
    two is the actual displacement.

    Returns None for TWO different reasons, which the caller must separate: the
    grids were not measurable at all (checked by the caller, before this is
    called), or neither branch has an exclusive footprint — both inputs changed
    exactly the same cells. The second is a real measurement with no directional
    signal in it, and it is what a cosmetic response looks like.
    """
    if not _measurable(ga, gb, gbase):
        return None
    acc = {"a": [0, 0], "b": [0, 0]}
    for i, (pa, pb, pz) in enumerate(zip(ga, gb, gbase)):
        da, db = abs(pa - pz) > thr, abs(pb - pz) > thr
        if da == db:
            continue                      # shared, or quiet in both: no signal
        side = "a" if da else "b"
        acc[side][0] += (i % GRID_W) if key == "cx" else (i // GRID_W)
        acc[side][1] += 1
    if not acc["a"][1] or not acc["b"][1]:
        return None
    return acc["a"][0] / acc["a"][1] - acc["b"][0] / acc["b"][1]


# ---------------------------------------------------------------- Tier 1

# Frames between the presses of a repeated start gesture. NOT padding: a
# briefing screen commonly LOCKS input for about a second before it will accept
# the key that dismisses it, and a burst of presses is entirely absorbed by that
# lock. MEASURED on codex-2, whose intro guards its exit with
# `stateTimer > 1 && wasPressed(...)`: with the presses 12 frames apart the
# vertical differential was 5 cells and a screenshot of the "measured" frame
# showed a mission card reading "PRESS FIRE TO DEPLOY"; at 45 frames, 2 cells;
# at 70 frames, 34 cells and a screenshot showing the ship, the HUD and six
# enemies. The scorer was grading a briefing card, and the only thing wrong was
# how fast it typed.
GESTURE_GAP_FRAMES = 70


def apply_gesture(page, adv, gesture: str, box) -> None:
    """Apply a start gesture. `key:X*N` / `click:canvas*N` repeat N times."""
    head, _, rep = gesture.partition("*")
    n = int(rep or 1)
    if head.startswith("key:"):
        k = head.split(":", 1)[1]
        for _ in range(n):
            page.keyboard.down(k)
            adv(3, 4000)
            page.keyboard.up(k)
            adv(GESTURE_GAP_FRAMES, 9000)
    elif head == "click:canvas" and box:
        for _ in range(n):
            page.mouse.click(box["x"] + box["width"] / 2, box["y"] + box["height"] / 2)
            adv(GESTURE_GAP_FRAMES, 9000)


def _drive(page, adv, snap, delta, hold, note_hash, keys: list[str] | None,
           start_gesture: str, box) -> dict:
    """Run ONE branch: apply the start gesture, warm up, then hold `keys`.

    Returns checkpoint hashes plus the draw/frame counters. Called once per
    branch on a FRESH page load, so every branch sees the identical
    deterministic world up to the moment its input diverges.
    """
    # ---- start gesture -----------------------------------------------------
    # `key:X*N` presses X N times with frames in between. N > 1 is not padding:
    # every product measured needs TWO presses to get from the title card
    # through the mission briefing and into play, and one press leaves the game
    # sitting on the briefing where the arrows still do nothing.
    apply_gesture(page, adv, start_gesture, box)

    adv(WARMUP_FRAMES, 15000)
    snap("pre")
    grid_pre = page.evaluate("g => window.__grid(g[0], g[1])", [GRID_W, GRID_H])
    pre_probe = page.evaluate("() => ({d: window.__probe.draws, f: window.__probe.frames})") or {}

    # ---- the branch's own input -------------------------------------------
    if keys:
        for k in keys:
            page.keyboard.down(k)
        adv(BRANCH_FRAMES, 10000)
        for k in keys:
            page.keyboard.up(k)
    else:
        adv(BRANCH_FRAMES, 10000)
    snap("post")
    grid_post = page.evaluate("g => window.__grid(g[0], g[1])", [GRID_W, GRID_H])
    note_hash()

    post_probe = page.evaluate("() => ({d: window.__probe.draws, f: window.__probe.frames})") or {}
    df = max(1, int(post_probe.get("f", 0)) - int(pre_probe.get("f", 0)))
    dd = int(post_probe.get("d", 0)) - int(pre_probe.get("d", 0))

    # ---- settle, then the LATE checkpoint ----------------------------------
    # Every branch runs the same number of total frames, so the late grids of
    # two branches are the same virtual instant and remain directly comparable.
    adv(LATE_FRAMES, 10000)
    grid_late = page.evaluate("g => window.__grid(g[0], g[1])", [GRID_W, GRID_H])

    return {
        "hash": page.evaluate("() => window.__hash()"),
        "self_change": delta("pre", "post"),   # how much the world moved at all
        "draws_per_frame": round(dd / df, 2),
        "frames": int(post_probe.get("f", 0)),
        "grid_pre": grid_pre,
        "grid_post": grid_post,
        "grid_late": grid_late,
    }


class _Session:
    """One Chrome and one HTTP server per PRODUCT, reused across every branch.

    COST, and why this is not a micro-optimisation. The activation search plus
    the differential branches plus the fire-key sweep is up to ~40 branches per
    product; launching a cold Chrome and a fresh server for each meant no
    full-corpus run ever completed — two separate adversarial passes each gave
    up after ~45 minutes without producing a single scored row, which left the
    repaired rubric with NO measured corpus distribution to calibrate against.
    An uncalibrated rubric is the defect this whole rewrite exists to fix, so
    the cost regression was blocking correctness, not just speed.

    Each branch still gets a FRESH PAGE, which is what isolation actually
    requires: a new page is a new JS context, new globals, new canvas. The
    browser process and the static file server carry no game state between
    pages.

    AND THE CACHE DOES NOT LEAK EITHER — measured, not assumed, because the
    obvious worry about sharing one browser is that later loads parse from a
    cache the first branch warmed, which is a TIMING difference, and timing
    differences are exactly the class of bug that made this instrument
    non-deterministic before. `browser.new_page()` opens a new BROWSER CONTEXT
    per page (Playwright creates one implicitly and `page.close()` closes it),
    and a Chromium context is a separate storage and cache partition. MEASURED
    on the control fixture, counting requests at the server: branch 2 fetched
    index.html, main.js, stage.js and entities.js from the socket exactly as
    branch 1 did — the identical request list, no cache hits. localStorage,
    sessionStorage, document.cookie and a window global written by one branch
    were all absent in the next. A no-input branch run after an ArrowUp branch
    inside one session was cell-for-cell identical to one from a virgin session
    (0 cells, identical hash, identical draws/frame), so there is no ordering
    effect. `tests/rtype_fixtures/run_fixture_suite.py --isolation` re-runs all
    of that.
    """

    def __init__(self, entry: Path, root: Path):
        self.entry, self.root = entry, root
        self._cm = None
        self.browser = None
        self.srv = None
        self.port = None

    def __enter__(self):
        from playwright.sync_api import sync_playwright
        self._cm = sync_playwright()
        pw = self._cm.__enter__()
        self.browser = pw.chromium.launch(
            channel=CHROME_CHANNEL, headless=True,
            args=["--autoplay-policy=no-user-gesture-required", "--mute-audio",
                  "--force-device-scale-factor=1", "--disable-lcd-text"])
        self.srv, self.port = serve_dir(self.root)
        return self

    def __exit__(self, *exc):
        for close in (lambda: self.browser.close(),
                      lambda: self.srv.shutdown(),
                      lambda: self._cm.__exit__(*exc)):
            try:
                close()
            except Exception:  # noqa: BLE001
                pass
        return False


def _branch(entry: Path, root: Path, keys: list[str] | None, start_gesture: str,
            sess: "_Session | None" = None) -> dict:
    """Run one branch to completion on a fresh page.

    `sess` reuses a product-scoped browser/server (the fast path). Without it
    the function still stands alone and owns its own browser, so callers that
    score a single artifact keep working unchanged.
    """
    if sess is None:
        with _Session(entry, root) as s:
            return _branch(entry, root, keys, start_gesture, s)

    out: dict = {"ok": False}
    port = sess.port
    page = sess.browser.new_page(viewport={"width": 1000, "height": 900},
                                 device_scale_factor=1)
    if True:
        errs: list[str] = []
        page.on("pageerror", lambda e: errs.append(str(e)[:200]))
        # Order matters: CLOCK_PIN must run BEFORE INSTRUMENT (it fixes the
        # REAL_NOW that INSTRUMENT captures) and GRID_INSTRUMENT AFTER it (it
        # freezes the frame budget INSTRUMENT installs).
        page.add_init_script(CLOCK_PIN)
        page.add_init_script(INSTRUMENT)
        page.add_init_script(GRID_INSTRUMENT)
        try:
            page.goto(f"http://127.0.0.1:{port}/{entry.name}", wait_until="load",
                      timeout=30000)
            try:
                page.evaluate("() => document.fonts ? document.fonts.ready.then(() => true) : true")
            except Exception:  # noqa: BLE001
                pass

            stalls = [0]

            def adv(n: int, timeout_ms: int = 8000) -> None:
                t = timeout_ms if stalls[0] < 1 else (300 if stalls[0] < 4 else 0)
                if not advance_frames(page, n, t):
                    stalls[0] += 1

            def snap(slot: str) -> None:
                page.evaluate("s => window.__snap(s)", slot)

            def delta(a: str, b: str) -> int:
                r = page.evaluate("ab => window.__delta(ab[0], ab[1])", [a, b]) or {}
                return int(r.get("changed_px") or 0)

            def hold(key: str, frames: int = BRANCH_FRAMES) -> None:
                page.keyboard.down(key)
                adv(frames, 8000)
                page.keyboard.up(key)
                adv(2, 4000)

            hashes: list[str] = []

            def note_hash() -> None:
                h = page.evaluate("() => window.__hash()")
                if h and not str(h).startswith("ERR"):
                    hashes.append(str(h))

            adv(30, 12000)
            box = None
            try:
                cv = page.locator("canvas").first
                cv.scroll_into_view_if_needed(timeout=3000)
                box = cv.bounding_box(timeout=3000)
            except Exception:  # noqa: BLE001
                box = None

            r = _drive(page, adv, snap, delta, hold, note_hash, keys,
                       start_gesture, box)
            probe = page.evaluate("() => window.__probe") or {}
            r.update({
                "ok": True,
                "audio_starts": int(probe.get("audioStarts") or 0),
                "raf": int(probe.get("raf") or 0),
                "ticks": int(probe.get("ticks") or 0),
                "key_listeners": int(probe.get("keyListeners") or 0),
                "js_exceptions": (errs + list(probe.get("errors") or []))[:6],
                "stalls": stalls[0],
                "canvas": page.evaluate(
                    """() => {const c = document.querySelector('canvas');
                       return c ? c.width + 'x' + c.height : null;}"""),
                "hashes": hashes,
            })
            out = r
        except Exception as e:  # noqa: BLE001
            # A driver failure must never read as "the game does nothing" —
            # all-zeros from a crashed probe is indistinguishable from a dead
            # game and would flatten the corpus.
            out = {"ok": False, "error": str(e)[:200]}
        finally:
            # Close the PAGE, never the browser: the session owns the browser
            # and the server and tears both down once the product is finished.
            # A leaked page would keep its rAF loop alive and compete for the
            # same Chrome process as the next branch — exactly the kind of
            # cross-branch coupling the differential cannot tolerate.
            try:
                page.close()
            except Exception:  # noqa: BLE001
                pass
    return out


def _activate(entry: Path, root: Path, branch) -> dict:
    """Find the gesture that puts the game INTO PLAY, judged by RESPONSIVENESS.

    WHY THE MOTION-BASED SEARCHES BOTH FAILED. Two rules have been tried here
    and neither works, because both ask the canvas how busy it is and a title
    screen is busy:

      * "if the no-gesture branch changes fewer than CHANGED_FLOOR pixels, try
        gestures" — every product in this corpus boots into a title/menu/intro
        state whose background ALREADY animates, so the search was never
        entered. It chose "none" for 23 of 23, and under a deterministic
        instrument the consequence is stark: holding an arrow then changes
        NOTHING on any product, because no product was ever started. The 16/23
        and 17/23 the old rubric scored for R2 and R3 were replay noise.
      * "require the gesture to beat the page's own idle motion by 4x, sustained
        over two windows" — REFUTED by measurement on the correct gestures. A
        title card is often BUSIER than the game behind it, so the motion goes
        DOWN when the game starts. Measured self-motion before -> after the
        gesture that demonstrably unlocks the controls: opencode-1 387 -> 354,
        tui-coder-1 1317 -> 1038, abstractcode-basic-1 79 -> 204 (2.6x, against
        a required 4x). Three of the four products tested would have been graded
        on their title screen by that rule.

    So the criterion is not "is the picture busier", it is "does the ship answer
    the stick". A gesture is accepted only when an OPPOSED PAIR of arrow
    branches lands in different worlds after it. That is the same measurement
    R2/R3 make, so nothing is wasted: the winning gesture's four arrow branches
    are returned and reused.

    "none" is tried first, so a game that is already playing is never poked with
    a key that might pause it. Enter is only ever pressed at a game that has
    already been shown NOT to respond.
    """
    trials: dict = {}
    up = dn = lf = rt = None
    for g in START_GESTURES:
        u, d2 = branch(["ArrowUp"], g), branch(["ArrowDown"], g)
        l2, r2 = branch(["ArrowLeft"], g), branch(["ArrowRight"], g)
        # KEEP THE FIRST GESTURE'S BRANCHES, not the last one tried. When no
        # gesture activates, this function reports gesture "none" and tier1 then
        # runs base/replay/ctrl with "none" — so returning the branches from the
        # LAST gesture attempted compared arrow branches that had pressed `x`
        # twice at startup against control branches that had pressed nothing.
        # MEASURED: on the attract-loop fixture, whose keydown handler is empty,
        # that mismatch alone produced 214 cells of "persistent input effect"
        # and handed R7 to a page that cannot read input. Every branch compared
        # in tier1 must have been driven with the SAME start gesture.
        if up is None or g == "none":
            up, dn, lf, rt = u, d2, l2, r2
        if not (u.get("ok") and d2.get("ok")):
            trials[g] = "branch failed"
            continue
        v = _cells(_grid_of(u, "grid_post"), _grid_of(d2, "grid_post"))["cells"]
        h = _cells(_grid_of(l2, "grid_post"), _grid_of(r2, "grid_post"))["cells"]
        trials[g] = {"vert_cells": v, "horiz_cells": h,
                     "key_listeners": u.get("key_listeners"),
                     "stalls": u.get("stalls")}
        # Stop early when no gesture CAN help. A page that registers no
        # keydown/keyup listener at all cannot be started by pressing anything,
        # and a page whose loop never delivers a frame is not going to start
        # delivering them on the fourth try. Without this the search pays 32
        # page loads to re-establish that a dead page is dead — measured at
        # over two minutes on a static fixture, per product.
        if u.get("key_listeners") == 0:
            trials[g] = dict(trials[g], stopped="no key listeners — no gesture can start this")
            break
        if u.get("stalls", 0) >= 4 and d2.get("stalls", 0) >= 4:
            trials[g] = dict(trials[g], stopped="loop never delivered frames")
            break
        # DECISIVE, not marginal. Accepting the first gesture that merely
        # clears MIN_CELLS locked codex-2 onto `key:Enter*3` at 5 cells while a
        # screenshot of the "measured" frame showed a mission briefing card;
        # once genuinely in play the same product reads 34. Every real
        # activation measured across the corpus is 14-34 cells and every
        # spurious one 0-5, so the bar sits at ACTIVATION_CELLS — above the
        # spurious range, below every real one — and a marginal reading makes
        # the search keep looking rather than settle on a briefing card.
        if max(v, h) >= ACTIVATION_CELLS:
            return {"ok": True, "gesture": g, "activated": True, "trials": trials,
                    "up": u, "down": d2, "left": l2, "right": r2}
    return {"ok": True, "gesture": "none", "activated": False, "trials": trials,
            "up": up, "down": dn, "left": lf, "right": rt}


# ------------------------------------------------------ graded Tier-1 scales
#
# WHY TIER 1 IS NOT A ROW OF BITS ANY MORE. Every behavioural check used to be a
# THRESHOLD on a continuous measurement, and a threshold throws away two things
# at once:
#
#   RESOLUTION. Measured over the 23 ranked products, the Tier-1 total took TEN
#   distinct values, and 8 of 23 sat at a perfect 80/80. R6 collapsed a measured
#   range of 7 to 768 audio starts into ONE value for 22 of them. A rubric whose
#   behavioural tier can only say one of ten things cannot rank eight harnesses:
#   the corpus F-test came out at F(7,15)=0.888, p=0.54, and the minimum
#   detectable difference at n=3 was 0.455 against a total observed arm spread of
#   0.232.
#
#   STABILITY. A bit at a threshold is a coin flip for any product sitting near
#   it. All five products that moved between two runs of identical code over
#   identical artifacts moved by FLIPPING a threshold — R2 on three, R3 on three,
#   R7 on two — and each flip moved 8 to 11 points at once. `tui-coder-3` is the
#   proof that the flip is not even monotone: its horizontal differential went
#   DOWN, 21 cells to 16, while R3 went False to True, because the underlying
#   signed separation reversed from -9.27 to +14.0 between the two runs.
#
# So every check whose underlying quantity is CONTINUOUS is now scored on a
# ramp, and a product sitting near a boundary moves by a fraction of a point
# instead of by a whole weight.
#
# THE FOUR RULES THESE SCALES OBEY, because a graded scale can fail in ways a bit
# cannot:
#
#  1. ANCHORED TO OBSERVABLE MEANING, NEVER TO THIS CORPUS. Every constant below
#     is a fraction of a PHYSICAL quantity — the play-field axis, the grid area,
#     the measured noise floor, the detection threshold that was already there —
#     and is stated as a claim about an artifact, not about the 24 products that
#     happen to be in this directory. A scale fitted to this corpus would rank
#     these 24 beautifully and be useless as the reference the next benchmark is
#     read against, which is the only reason this file exists.
#  2. MONOTONE. More of the good thing may never lower the score. `_ramp` is
#     non-decreasing, `min` and `max` of non-decreasing functions are
#     non-decreasing, and `_selfcheck_graded_scales` verifies it by sampling
#     rather than by assertion. The one deliberate NON-monotonicity in Tier 1 is
#     the whole-screen repaint guard, and it is not a quality scale: past that
#     fraction the measurement has stopped being a measurement of the ship, so
#     the check reports UNKNOWN. That is a validity gate, and validity gates stay
#     binary.
#  3. SATURATING, DELIBERATELY. An unbounded top is an invitation to pad, so
#     every scale reaches 1.0 at a stated point and pays nothing beyond it. Where
#     the underlying magnitude is IMPLEMENTATION STYLE rather than quality — a
#     draw-call count, a count of scheduled oscillators — the scale saturates at
#     the DETECTION THRESHOLD and grades confidence only. Grading those on
#     magnitude would rank renderers and synthesisers, and would hand full marks
#     to a page that pads them.
#  4. THE TRUE GATES STAY BINARY. R0 (loads without throwing), R1 (renders and
#     animates at all) and R5 (the world moves by itself) are validity gates, not
#     qualities. Partial credit for "half loaded" is not a measurement of
#     anything.

# R2/R3. Signed centroid separation between the two opposed arrow branches,
# taken as a FRACTION OF THE PLAY-FIELD AXIS it is measured on, which is what
# the grid already normalises to (GRID_W x GRID_H blocks cover the whole canvas
# whatever its pixel size, so a ship that crosses a fifth of the canvas reads the
# same on 160x144 and on 800x600 — measured on the scaled control fixtures at
# 240x180, 480x360 and 1200x900, whose separations agree to 0.09 of a cell).
#
# FULL CREDIT AT HALF THE AXIS. The two opposed branches hold their keys for
# BRANCH_FRAMES (30 frames, half a second at 60 fps), so a separation of half the
# axis means each branch displaced the ship by about a quarter of the play field
# in half a second — a ship that crosses its own play field in roughly two
# seconds of held input. That is the claim: full marks for demonstrated control
# authority over the whole field, and nothing beyond it, so a ship that teleports
# from edge to edge earns exactly what a responsive one does.
#
# The floor of the ramp is CENTROID_MARGIN, unchanged: below it the two
# footprints are not separated at all and the response is not directional. That
# constant is what makes the check un-fakeable by decoration, and it keeps
# exactly the meaning it had.
AXIS_FULL_FRAC = 0.5

# R8. Fraction of the grid that changes between two no-input checkpoints —
# how much of the screen is ALIVE. The floor is the 0.02 that was already there.
#
# FULL CREDIT AT A FIFTH OF THE FIELD, and the saturation is the point of the
# scale rather than an afterthought: a page that strobes its whole canvas earns
# exactly what a busy, populated shmup scene earns, so repainting everything buys
# nothing. Corroboration that a fifth is where "fully populated" sits comes from
# outside this corpus — the two hand-written control fixtures, which were written
# to be simple but genuinely playable long before this scale existed, measure
# 0.2555 and 0.2565.
SCENE_FLOOR_FRAC = 0.02
SCENE_FULL_FRAC = 0.20

# R4. CONFIDENCE ONLY, and this is a refusal, not an oversight. The two weapon
# signatures are extra draw calls per frame and downrange travel, and neither
# MAGNITUDE is a statement about weapon quality:
#   * a game that paints its background tile by tile issues 240 draws a frame and
#     one that blits an image issues 3, so the same projectile costs a wildly
#     different delta on the two. Measured across this corpus the accepted keys
#     span +1.0 to +80.96 draws/frame, and the 80.96 is a rendering style, not
#     eighty times the weapon.
#   * a page can pad the draw count arbitrarily while a candidate key is held.
# So R4 grades only how far the evidence sits above the DETECTION threshold, and
# saturates at twice it. Every product in the measured corpus is already past
# saturation, so this buys no resolution by construction — what it buys is that a
# product sitting at the threshold moves by a fraction of a point between runs
# instead of by all 16.
FIRE_FULL_DRAW_DELTA = 2.0 * FIRE_DRAW_DELTA
FIRE_FULL_TRAVEL = 3.0 * CENTROID_MARGIN

# R6. Audio, and the same refusal for the same reason plus a measured one.
# `audioStarts` counts calls to start() on a source node, so a game whose music
# is one looping buffer scores 1 and a game whose music is a note sequencer
# scores hundreds for the same music. Grading that magnitude would rank SYNTHESIS
# STYLE. It would also import noise the rest of the instrument does not have: two
# runs of identical code over identical artifacts measured 40 and 20 starts on
# the same product, a 2x swing, where the grid differentials on the same runs
# reproduced to the cell.
#
# What CAN be said with this counter is whether the audio is wired to the game:
# an input branch that schedules more sound than the idle branch is playing
# something BECAUSE the player did something. So presence is the gate and
# reactivity is the quality, and a game with sound effects is distinguished from
# a game with a background loop — which "has a beep" never could.
AUDIO_PRESENCE_SHARE = 0.6
# Full reactivity credit when playing schedules a quarter again as many sound
# events as idling. A quarter is above single-event jitter and far below the
# ratio a per-shot effect produces.
AUDIO_REACTIVE_RATIO = 1.25


def _ramp(x: float, lo: float, hi: float) -> float:
    """Saturating linear ramp: 0.0 at or below `lo`, 1.0 at or above `hi`.

    The only shape used by any graded check in this file, so that "where does
    this check saturate" is answerable by reading two constants, and so that
    monotonicity is a property of ONE function.
    """
    if hi <= lo:
        return 1.0 if x >= hi else 0.0
    return max(0.0, min(1.0, (x - lo) / (hi - lo)))


def _axis_credit(pair_cells: int, sep_signed: float, axis_len: int,
                 gate: int) -> tuple[float, dict]:
    """Graded directional response for R2/R3, in [0, 1].

    Two terms, and the credit is the WEAKER of them, because they answer
    different questions and neither can stand in for the other:

      EVIDENCE      how far the opposed pair sits above this product's own
                    measured noise floor. 0 at the gate, full at TIE_FACTOR x
                    the gate. TIE_FACTOR keeps exactly the meaning it was
                    measured to have — across two runs of identical code every
                    verdict at or above 2x the gate reproduced and every verdict
                    below it flipped — but it now SATURATES the evidence term
                    instead of gating the whole check. That is the difference
                    between "this reading is worth 11 points or 0 depending on
                    which side of 2.0 it lands" and "this reading is worth what
                    it can support".
      DISPLACEMENT  how far the ship actually went, as a fraction of the axis.
                    Floor at CENTROID_MARGIN (below it there is no direction),
                    full at AXIS_FULL_FRAC.

    THE TIE BAND IS GONE AS A SEPARATE STATE. It used to report UNKNOWN between
    1x and 2x the gate, which was the right call for a bit — an unreproducible
    bit is worse than no bit — but it left the cliff intact one step higher: at
    2x the gate exactly, 0 points; one cell more, 11. A ramp through the same
    band is the same measured fact expressed at the resolution it actually has.
    UNKNOWN is still reported, and still means what it always meant: the
    comparison could not be MADE.

    Displacement is measured on the SIGNED separation, so a response in the wrong
    direction earns nothing rather than being scored on its size.
    """
    evidence = _ramp(pair_cells / float(gate), 1.0, TIE_FACTOR) if gate > 0 else 0.0
    sep_frac = sep_signed / float(axis_len)
    disp = _ramp(sep_frac, CENTROID_MARGIN / float(axis_len), AXIS_FULL_FRAC)
    return round(min(evidence, disp), 4), {
        "evidence": round(evidence, 3),
        "displacement": round(disp, 3),
        "sep_frac": round(sep_frac, 4),
    }


def _fire_credit(ddraw: float | None, travel: float | None) -> float:
    """Graded confidence that the accepted key fired something, in [0, 1].

    The BETTER of the two independent signatures, never their average: a game
    whose projectiles are batched into one draw call shows travel and no draw
    delta, and a game whose shots leave the screen inside LATE_FRAMES shows the
    draw delta and no travel. Both are ordinary implementations, and averaging
    would dock each of them half the weight for a rendering decision — the exact
    class of error this rubric was rewritten to remove.

    `travel` must be passed as None unless the travel signature actually cleared
    its own gates; an ungated centroid difference is not a projectile.
    """
    draw_c = _ramp(ddraw if ddraw is not None else 0.0,
                   FIRE_DRAW_DELTA, FIRE_FULL_DRAW_DELTA)
    trav_c = _ramp(travel if travel is not None else 0.0,
                   CENTROID_MARGIN, FIRE_FULL_TRAVEL)
    return round(max(draw_c, trav_c), 4)


def _persist_credit(persist_cells: int, late_gate: int) -> float:
    """Graded persistence for R7, in [0, 1]. CONFIDENCE, not size.

    The SIZE of a persistent difference is not a quality: a ship that flew
    somewhere else leaves an exclusive footprint of about its own sprite area —
    ten cells or so — and the largest persistence in this corpus, 3059 of 3072
    cells, is a product repainting its whole screen. Grading on magnitude would
    dock a real game for having a small ship and pay a page for flashing, which
    is the "measuring sprite area against the grid" mistake this file already
    made once at a coarser grid resolution.

    So R7 grades only how far the persistence sits above the product's own late
    noise floor, on the same 1x-to-TIE_FACTOR ramp as R2/R3 and for the same
    measured reason. Two of the five products that moved between identical runs
    moved on this check.
    """
    if late_gate <= 0:
        return 0.0
    return round(_ramp(persist_cells / float(late_gate), 1.0, TIE_FACTOR), 4)


def _scene_credit(frac: float) -> float:
    """Graded runtime content for R8, in [0, 1]. See SCENE_FULL_FRAC."""
    return round(_ramp(frac, SCENE_FLOOR_FRAC, SCENE_FULL_FRAC), 4)


def _audio_credit(idle_starts: int, input_starts: int) -> float:
    """Graded audio for R6, in [0, 1]: presence gated, reactivity graded.

    Zero for silence. AUDIO_PRESENCE_SHARE for a game that schedules sound but
    schedules no more of it when the player acts. Full marks when the input
    branches schedule at least AUDIO_REACTIVE_RATIO times the idle branch's
    events, which is what a per-shot or per-hit effect produces.

    A game whose sound is entirely input-driven — idle silent, input not — is
    fully reactive by definition and takes full marks.
    """
    if max(idle_starts, input_starts) <= 0:
        return 0.0
    if idle_starts <= 0:
        return 1.0
    ratio = input_starts / float(idle_starts)
    reactive = _ramp(ratio, 1.0, AUDIO_REACTIVE_RATIO)
    return round(AUDIO_PRESENCE_SHARE
                 + (1.0 - AUDIO_PRESENCE_SHARE) * reactive, 4)


def _selfcheck_graded_scales(steps: int = 60) -> dict:
    """Verify the two properties every graded scale in this file claims.

    MONOTONE. More of the good thing may never lower the score. This is sampled,
    not asserted: the previous binary rubric was believed to be monotone and was
    not, and `tui-coder-3` — horizontal differential DOWN from 21 cells to 16,
    check UP from False to True — is what that belief cost.

    SATURATING. Every scale reaches 1.0 at its stated anchor and pays nothing
    beyond it, so no check can be bought by padding the quantity it reads.

    Returns a report; `main --selfcheck` prints it. Pure arithmetic, no browser.
    """
    def mono(name, f, lo, hi, label):
        vals = [f(lo + (hi - lo) * i / steps) for i in range(steps + 1)]
        bad = [(i, vals[i - 1], vals[i]) for i in range(1, len(vals))
               if vals[i] < vals[i - 1] - 1e-12]
        return {"scale": name, "over": label, "monotone": not bad,
                "violations": bad[:3], "min": min(vals), "max": max(vals)}

    out = {"checks": []}
    add = out["checks"].append
    # R2/R3: monotone in the differential, in the separation, and DECREASING in
    # the noise floor, which is the correct direction — more noise, less credit.
    add(mono("R2/R3 evidence", lambda c: _axis_credit(c, 24.0, GRID_H, 3)[0],
             0, 200, "pair_cells 0..200 at gate 3"))
    add(mono("R2/R3 displacement", lambda s: _axis_credit(40, s, GRID_H, 3)[0],
             -GRID_H, GRID_H, "signed separation -48..48 rows"))
    add(mono("R2/R3 displacement (x)", lambda s: _axis_credit(40, s, GRID_W, 3)[0],
             -GRID_W, GRID_W, "signed separation -64..64 cols"))
    add(mono("R2/R3 vs noise (must FALL)",
             lambda g: -_axis_credit(40, 24.0, GRID_H, max(1, int(g)))[0],
             1, 200, "gate 1..200, negated so a rise is a violation"))
    add(mono("R4 draw delta", lambda d: _fire_credit(d, None), 0, 100,
             "extra draws/frame 0..100"))
    add(mono("R4 travel", lambda t: _fire_credit(None, t), 0, GRID_W,
             "downrange travel 0..64 cells"))
    add(mono("R6 reactivity", lambda x: _audio_credit(40, int(40 * x)), 0, 10,
             "input/idle audio ratio 0..10 at 40 idle starts"))
    add(mono("R7 persistence", lambda p: _persist_credit(int(p), 3), 0, 3072,
             "persistent cells 0..3072 at late gate 3"))
    add(mono("R8 scene activity", _scene_credit, 0.0, 1.0,
             "live fraction of the grid 0..1"))
    sat = {
        "R2/R3 at AXIS_FULL_FRAC": _axis_credit(999, AXIS_FULL_FRAC * GRID_H,
                                                GRID_H, 3)[0],
        "R2/R3 at 10x AXIS_FULL_FRAC (no more)": _axis_credit(
            999, 10 * AXIS_FULL_FRAC * GRID_H, GRID_H, 3)[0],
        "R4 at 2x threshold": _fire_credit(FIRE_FULL_DRAW_DELTA, None),
        "R4 at 100x threshold (no more)": _fire_credit(100 * FIRE_DRAW_DELTA, None),
        "R8 at SCENE_FULL_FRAC": _scene_credit(SCENE_FULL_FRAC),
        "R8 at whole screen (no more)": _scene_credit(1.0),
        "R7 at TIE_FACTOR x gate": _persist_credit(6, 3),
        "R7 at 1000x gate (no more)": _persist_credit(3000, 3),
        "R6 at ratio 1.25": _audio_credit(40, 50),
        "R6 at ratio 100 (no more)": _audio_credit(40, 4000),
    }
    out["saturation"] = sat
    out["floors"] = {
        "R2/R3 below CENTROID_MARGIN": _axis_credit(999, CENTROID_MARGIN - 0.01,
                                                    GRID_H, 3)[0],
        "R2/R3 wrong direction": _axis_credit(999, -30.0, GRID_H, 3)[0],
        "R2/R3 at the noise gate": _axis_credit(3, 30.0, GRID_H, 3)[0],
        "R4 below FIRE_DRAW_DELTA with no travel": _fire_credit(
            FIRE_DRAW_DELTA - 0.01, None),
        "R7 at the late gate": _persist_credit(3, 3),
        "R8 below SCENE_FLOOR_FRAC": _scene_credit(SCENE_FLOOR_FRAC - 0.001),
        "R6 silent": _audio_credit(0, 0),
    }
    out["MONOTONE"] = all(c["monotone"] for c in out["checks"])
    out["SATURATES"] = all(abs(v - 1.0) < 1e-9 for k, v in sat.items()
                           if "no more" in k or True)
    out["FLOORS_ZERO"] = all(v == 0.0 for v in out["floors"].values())
    out["PASS"] = bool(out["MONOTONE"] and out["SATURATES"] and out["FLOORS_ZERO"])
    return out


def _blank_t1() -> dict:
    """Every Tier-1 key at its unmeasured default.

    Shared by the measured path and the no-entry path so a product that cannot
    be loaded returns the SAME SHAPE as one that can. When the two paths built
    their own dicts they drifted, and the dead-check audit then read a missing
    key as a distinct value — an instrument defect that hides instrument
    defects.
    """
    # A GRADED CHECK DEFAULTS TO 0.0, NOT False. They score the same — `float`
    # of either is 0.0 — but the dead-check audit compares serialised values, so
    # a corpus in which one product defaulted to `false` and the rest measured
    # `0.0` would show two distinct values for a check that separated nothing.
    # The audit exists to catch dead weight; it must not manufacture variance.
    #
    # SPEC-TIER checks default to None (UNKNOWN): every one of them requires a
    # measurement the blank path never made, and the measured path overwrites
    # them wholesale via `res.update(spec)`.
    return {**{k: None for k in S_WEIGHTS},
            "spec_notes": "", "spec_evidence": {},
        "R0_loads_without_exception": False, "R1_renders_and_animates": False,
        "R2_ship_moves_vertically": 0.0, "R3_ship_moves_horizontally": 0.0,
        "R4_weapon_fires": 0.0, "R5_world_scrolls": False,
        "R6_audio_scheduled": 0.0, "R7_input_persists": 0.0,
        "R8_scene_populated": 0.0,
        "deterministic": None, "activated_by": None, "fire_key": None,
        "diff_vertical": 0, "diff_horizontal": 0, "diff_fire": 0,
        "idle_self_change": 0, "draws_per_frame_idle": 0.0,
        "draws_per_frame_fire": 0.0, "distinct_states": 0,
        "null_cells": 0, "control_cells": 0, "response_gate": 0,
        "activated": False, "replay_cells": None, "fire_draw_delta": None,
        "fire_rejected": [],
        "persistence_cells": 0, "scene_activity": 0.0,
        "canvas": None, "js_exceptions": [], "notes": "",
    }


def _unmeasured_t1(note: str) -> dict:
    """A Tier-1 result for a product that was never observed.

    Every behavioural check is UNKNOWN, not False and not 0.0. `_blank_t1`'s
    zero defaults are correct for the MEASURED path — R4 in particular relies on
    them to record "eight keys were tried and none fired" — but on a path where
    the browser never ran they would assert that a game was watched and found
    not to move.
    """
    res = _blank_t1()
    for k in T1_WEIGHTS:
        res[k] = None
    res["notes"] = note
    return res


def tier1(d: Path) -> dict:
    """Observe the real page. Replay-differential, not idle-comparison.

    Owns ONE browser and ONE server for the whole product (see `_Session`);
    every branch below runs on a fresh page inside them.
    """
    entry, root = resolve_entry(d)
    if not entry.is_file():
        return _unmeasured_t1("no index.html")
    with _Session(entry, root) as sess:
        return _tier1(d, entry, root, sess)


def _tier1(d: Path, entry: Path, root: Path, sess: "_Session") -> dict:
    res: dict = _blank_t1()

    # ---- pick a start gesture ---------------------------------------------
    # Determined ONCE by RESPONSIVENESS, then held fixed for every branch, so
    # the branches differ only in the input under test. Choosing it per-branch
    # would confound the differential with the gesture.
    loads = [0]

    def branch(keys, gest):
        loads[0] += 1
        return _branch(entry, root, keys, gest, sess)

    act = _activate(entry, root, branch)
    gesture = act.get("gesture", "none")
    res["activation_trials"] = act.get("trials")
    res["activated"] = act.get("activated")
    up, down, left, right = act["up"], act["down"], act["left"], act["right"]
    if not act.get("activated"):
        res["notes"] += "NEVER RESPONDED to any start gesture; "
    base = branch(None, gesture)
    if not base.get("ok"):
        # Nothing was observed, so nothing is known. Every behavioural check is
        # UNKNOWN rather than failed: a driver error must not read as a verdict
        # on the game.
        out = _unmeasured_t1(f"driver error: {base.get('error', '?')}")
        out["DRIVER_FAILED"] = True
        out["activation_trials"] = res.get("activation_trials")
        out["activated"] = res.get("activated")
        return out
    res["activated_by"] = gesture
    res["canvas"] = base.get("canvas")
    # R0 IS COLLECTED ACROSS EVERY BRANCH, not just the no-input one. MEASURED:
    # abstractcode-basic-1 loads and plays cleanly and throws only when you
    # SHOOT — its shoot sound sets `OscillatorNode.type = 'custom'`, which is
    # illegal, and the uncaught exception stops the animation loop dead (frames
    # 151 vs 180, draws 12160 vs 14980, one frame stall). Sampling only the
    # base branch reported R0=true for a game that crashes on its own primary
    # verb, and it is the reason R0 read 23/23 — constant, and wrong.
    exc = list(base.get("js_exceptions") or [])
    res["js_exceptions"] = exc
    res["R0_loads_without_exception"] = not exc
    res["idle_self_change"] = base.get("self_change", 0)
    res["draws_per_frame_idle"] = base.get("draws_per_frame", 0.0)

    # ---- determinism precondition -----------------------------------------
    # The whole differential rests on "same input, same world". Verify it
    # rather than assume it; a product that fails is reported UNRELIABLE.
    replay = branch(None, gesture)
    # Compared on the GRID, not on `__hash()`. The hash samples one pixel in 97
    # and is very nearly blind to a projectile: a 4x2 bullet has about a 4%
    # chance of being sampled at all, so hash equality would report "identical"
    # for worlds that visibly differ. The grid cannot miss an object that lands
    # inside a cell.
    if not replay.get("ok"):
        res["replay_cells"] = None
        res["deterministic"] = None
        res["notes"] += "replay branch failed — noise floor UNKNOWN; "
    else:
        rcell = _cells(_grid_of(base, "grid_post"), _grid_of(replay, "grid_post"))
        if not rcell["measured"]:
            # The branch ran but the canvas could not be reduced to a grid. That
            # is NOT a replay of distance zero; it is no replay at all.
            res["replay_cells"] = None
            res["deterministic"] = None
            res["notes"] += "replay grids unreadable — noise floor UNKNOWN; "
        else:
            rc = rcell["cells"]
            res["replay_cells"] = rc
            res["deterministic"] = rc == 0
            if rc:
                res["notes"] += (f"replay drift {rc} cells — differential gates "
                                 f"widened, reading is weaker; ")

    # ---- R1: renders and animates -----------------------------------------
    looping = (base.get("frames", 0) >= 60) or (base.get("ticks", 0) >= 60)
    animates = base.get("self_change", 0) >= CHANGED_FLOOR or len(set(base.get("hashes") or [])) > 1
    res["R1_renders_and_animates"] = bool(base.get("canvas") and looping and animates)

    # ---- R5: the world scrolls by itself ----------------------------------
    # THE side-scroller signature, and the exact thing the Zelda rubric
    # punished. Autonomous motion between two checkpoints with no input at all.
    res["R5_world_scrolls"] = bool(base.get("self_change", 0) >= CHANGED_FLOOR)

    # ---- the NULL TREATMENT ------------------------------------------------
    # `ctrl` holds an inert key through the identical protocol. Together with
    # the replay branch it measures what this instrument reports when NOTHING
    # was done — the floor every differential check has to clear. Without it,
    # frame-delivery jitter and a non-reproducible world both read as gameplay:
    # MEASURED, a fixture with an empty keydown handler scored 23 points of
    # input response, and across the corpus the 14 products that failed the
    # replay check averaged 0.098 HIGHER than the 9 that passed it, because a
    # world that never replays the same way always "differs".
    ctrl = branch([CONTROL_KEY], gesture)
    # A NULL MUST BE MEASURED UNDER THE PROTOCOL IT IS THE NULL FOR. This is the
    # second null-treatment branch, and it exists because the first one was the
    # wrong shape for the comparisons it was gating.
    #
    # R2/R3 compare a key-pressing branch with another key-pressing branch, so
    # their common-mode drift cancels. The floor they were being held to was
    # `ctrl vs base` — a key-pressing branch against a NO-input one — which does
    # not cancel: pressing a key at all costs two CDP round-trips that the
    # no-input branch never pays, and the extra raw frames that slip through
    # during them put the world on a different scroll phase. R7 already knew
    # this and compares against `ctrl` for exactly that reason; the R2/R3 gate
    # did not.
    #
    # MEASURED, and it is the largest single cause of run-to-run instability in
    # the corpus. Two runs of identical code over identical artifacts moved 5 of
    # 24 products, and `ctrl vs base` moved in four of the five: 45 -> 0, 2 ->
    # 13, 0 -> 12, 64 -> 120. Because `gate = 2 x noise`, abstractcode-basic-2's
    # gate swung 90 -> 3 and took R2, R3 and R7 with it — 0.300 of score on a
    # product whose own replay check said `deterministic: True` both times.
    # `ctrl1 vs ctrl2` is the matched null: same key, same round-trips, same
    # everything, differing only in the drift the gate is supposed to absorb.
    ctrl2 = branch([CONTROL_KEY], gesture)

    def cd(a: dict, b: dict, slot: str = "grid_post") -> dict:
        return _cells(_grid_of(a, slot), _grid_of(b, slot))

    # And a third null, matched to the treatment even more closely: the SAME
    # arrow branch run twice. `ctrl` holds an inert key, so it never changes game
    # state; a product can be perfectly reproducible while idle and still not
    # reproducible once the input actually does something. This is the only null
    # that can see that, and R2/R3 rest on it.
    right2 = branch(["ArrowRight"], gesture)

    null_c = cd(base, replay)          # no-input vs no-input
    ctrl_c = cd(ctrl, base)            # REPORTED ONLY: mismatched protocols
    keypress_c = cd(ctrl, ctrl2)       # key-press vs key-press: the matched null
    input_c = cd(right, right2)        # same state-changing input, twice
    # A gate derived from a measurement that did not happen is not a floor, it is
    # a guess — and the guess it makes (noise 0, gate at its tightest) is the one
    # that most readily calls jitter a response. Every check that rests on this
    # gate reports UNKNOWN when it could not be built.
    noise_measured = (null_c["measured"] and keypress_c["measured"]
                      and input_c["measured"])
    null_post, ctrl_post = null_c["cells"], ctrl_c["cells"]
    noise = max(null_post, keypress_c["cells"], input_c["cells"])
    gate = max(MIN_CELLS, 2 * noise)
    res["null_cells"] = null_post if null_c["measured"] else None
    res["control_cells"] = ctrl_post if ctrl_c["measured"] else None
    res["keypress_null_cells"] = keypress_c["cells"] if keypress_c["measured"] else None
    res["input_replay_cells"] = input_c["cells"] if input_c["measured"] else None
    res["response_gate"] = gate if noise_measured else None
    if not noise_measured:
        res["notes"] += "noise floor unmeasurable — R2/R3/R7 UNKNOWN; "

    # `deterministic` used to replay the NO-INPUT branch only, and was then read
    # as a guarantee over the whole measurement. It is not one: two of the five
    # products that moved between identical runs reported `deterministic: True`
    # in both. Reproducibility is now reported for all three protocols — idle,
    # key-held, and state-changing input — and the flag means all three.
    res["stability"] = {"idle": res["replay_cells"],
                        "keypress": res["keypress_null_cells"],
                        "input": res["input_replay_cells"]}
    if res["deterministic"] is not None:
        parts = [res["replay_cells"], res["keypress_null_cells"],
                 res["input_replay_cells"]]
        res["deterministic"] = (None if any(p is None for p in parts)
                                else all(p == 0 for p in parts))
        if res["deterministic"] is False and not res["notes"].startswith("replay drift"):
            drift = [f"{k}={v}" for k, v in res["stability"].items() if v]
            if drift:
                res["notes"] += f"replay drift ({', '.join(drift)}) — gates widened; "

    # The LATE gate is built here, beside the post gate, because R4 needs it: a
    # weapon is judged partly on whether what the key produced is still there
    # after the key is released. Same null treatments, later checkpoint, and the
    # same matching rule — R7 compares key-press branches against `ctrl`, so its
    # floor is the key-press null, not `ctrl vs base`.
    null_late_c = cd(base, replay, "grid_late")
    keypress_late_c = cd(ctrl, ctrl2, "grid_late")
    input_late_c = cd(right, right2, "grid_late")
    late_measured = (null_late_c["measured"] and keypress_late_c["measured"]
                     and input_late_c["measured"])
    late_gate = max(MIN_CELLS, 2 * max(null_late_c["cells"],
                                       keypress_late_c["cells"],
                                       input_late_c["cells"]))

    # ---- R2/R3: the ship answers the stick --------------------------------
    # Branch-vs-branch, so the scroll cancels. Two conditions, both required:
    #   MAGNITUDE  the opposed pair differs by more than the noise floor, and
    #   DIRECTION  the difference is DISPLACED along the axis being tested —
    #              ArrowUp's footprint sits above ArrowDown's.
    # Direction is what makes the check un-fakeable by decoration. A corner LED
    # that lights on any keydown produces a large magnitude and IDENTICAL
    # centroids, so it fails; a ship that flies up cannot.
    # up/down/left/right come from _activate: they ARE the measurement that
    # chose the gesture, so re-running them here would pay for eight page loads
    # to reproduce four results that are already in hand.
    def axis(a: dict, b: dict, key: str, sign: int) -> tuple[float | None, dict]:
        """`a` and `b` are opposed inputs. Magnitude over the product's own noise
        floor, and `a`'s footprint displaced from `b`'s along `key` in direction
        `sign` — both GRADED, see `_axis_credit`.

        Returns a fraction in [0, 1], or None. None is not a weak 0. It means
        the comparison could not be made — a branch never drove, or its canvas
        could not be read — and the check is UNKNOWN. Reporting 0 there
        would state that the ship was observed NOT to move, on the strength of
        never having looked at it, and the product would be docked 11 points for
        an instrument failure.

        0.0 IS still returned, and still means a real observation: the branches
        were compared and the ship did not answer. The difference between 0.0
        and None is the difference between a measurement and no measurement, and
        grading does not blur it.
        """
        ga, gb, gbase = (_grid_of(a, "grid_post"), _grid_of(b, "grid_post"),
                         _grid_of(base, "grid_post"))
        info: dict = {"gate": gate}
        if not (a.get("ok") and b.get("ok")):
            info["unknown"] = "branch did not run"
            return None, info
        if not _measurable(ga, gb, gbase):
            info["unknown"] = "canvas not readable as a grid"
            return None, info
        if not noise_measured:
            info["unknown"] = "noise floor not measured — no gate to clear"
            return None, info
        pair = _cells(ga, gb)
        sep = _axis_sep(ga, gb, gbase, key)
        info.update({"pair_cells": pair["cells"],
                     "sep_raw": None if sep is None else round(sep, 2)})
        if pair["cells"] > SCENE_CHANGE_FRAC_AXIS * (GRID_W * GRID_H):
            # The two branches differ across the whole screen. Something
            # repainted — a restart, a death, a state change — and wherever the
            # ship is, this is not a measurement of it.
            info["unknown"] = ("scene-sized difference "
                               f"({pair['cells']}/{GRID_W * GRID_H} cells) — "
                               "repaint, not displacement")
            return None, info
        if sep is None:
            # MEASURED, and 0.0 rather than UNKNOWN: both branches changed
            # exactly the same cells, so neither has a footprint of its own.
            # That is what a decoration that lights in the same place whatever
            # the key looks like, and it is a real observation of no
            # directional response.
            info["why"] = "no exclusive footprint — both inputs changed the same cells"
            return 0.0, info
        if pair["cells"] <= gate:
            info["why"] = "difference does not clear the noise gate"
            return 0.0, info
        info["separation"] = round(sep * sign, 2)
        # THE TIE BAND USED TO LIVE HERE and reported UNKNOWN between 1x and 2x
        # the gate. It is now the lower half of the evidence ramp inside
        # `_axis_credit`: the same measured fact — readings that close to the
        # floor did not reproduce — expressed as partial credit instead of as a
        # discarded verdict, which removes the 11-point cliff that sat at
        # exactly 2x the gate.
        credit, parts = _axis_credit(pair["cells"], sep * sign,
                                     GRID_H if key == "cy" else GRID_W, gate)
        info.update(parts)
        return credit, info

    ok_v, res["vertical_detail"] = axis(up, down, "cy", -1)
    ok_h, res["horizontal_detail"] = axis(right, left, "cx", +1)
    for b in (up, down, left, right, ctrl, replay):
        for e in (b.get("js_exceptions") or []):
            if e not in exc:
                exc.append(e)
    # `.get(k)` with no default: an unmeasured axis reports None, not 0. A
    # reported 0 is a claim that the two branches were compared and matched.
    res["diff_vertical"] = res["vertical_detail"].get("pair_cells")
    res["diff_horizontal"] = res["horizontal_detail"].get("pair_cells")
    res["R2_ship_moves_vertically"] = ok_v
    res["R3_ship_moves_horizontally"] = ok_h

    # ---- R4: it shoots -----------------------------------------------------
    # POSITIVE PROJECTILE EVIDENCE ONLY, and every candidate is measured before
    # any is chosen.
    #
    # WHAT WAS WRONG. The check accepted the FIRST of eight keys that changed
    # the picture at all. That is "does any key do anything", not "does the ship
    # shoot", and it passed 23 of 23 products — the heaviest check in the rubric
    # carrying zero information. Confirmed false positives, against product
    # source:
    #   * pi-1 scored fire_key='x'. In pi-1, x is a smart bomb gated behind
    #     `player.special > 0 && level.index >= 3`, unreachable at frame 120.
    #     Its actual fire key is z.
    #   * tui-multi-1 scored fire_key='j'. Its own help text reads "Press Enter
    #     or J to launch" — j is the START key. That product's canvas was
    #     frozen for the whole measurement and it still passed R4.
    #   * codex-1, codex-3, pi-2, pi-3 and opencode-1 all fire on z or j and all
    #     scored fire_key='Space', which on several of them is the title-screen
    #     start key. R4 was scoring the title transition.
    #
    # THE SIGNATURE OF A WEAPON is extra objects being RENDERED. That is what a
    # projectile costs, and it is the one thing moving the ship cannot produce:
    # a displaced sprite is the same number of draw calls in a different place.
    # Three guards reject the other ways a key can change the frame without
    # firing.
    #
    # THE DRAW RATE IS NOT THE ONLY SIGNATURE, AND ON ITS OWN IT IS BLIND TO A
    # BATCHED RENDERER. A game that accumulates its projectiles into one Path2D
    # and issues a single `fill()` per frame — a normal optimisation, invisible
    # to a player — costs the SAME number of draw calls with twenty shots on
    # screen as with none. MEASURED on a hand-written control fixture that is
    # fully playable and batches exactly that way: ddraw was 0.00 for its real
    # fire keys and 0.00 for every inert one, so the check could not tell a
    # working weapon from no weapon at all and the fixture lost 16 points. No
    # product in the measured corpus batches, so this was a latent class, not a
    # live error — but "no evidence of extra draws" is not evidence of no
    # weapon, and a rubric may not read it as such.
    #
    # So a SECOND, INDEPENDENT signature is accepted: PROJECTILE TRAVEL. What the
    # key produced is still on the screen after the key is released, and it has
    # moved DOWNRANGE — the centroid of the difference against the inert-key
    # control is further right at the late checkpoint than it was at the post
    # checkpoint. Measured on the two control fixtures: +15.44 and +15.99 cells
    # of rightward travel with 28 and 29 cells still differing, against 0 cells
    # and no measurable centroid for every key of every adversarial fixture.
    # A ship cannot produce it (it stops when the key is released) and a HUD
    # decoration cannot (it is in a fixed place and vanishes on keyup).
    #
    # KNOWN RESIDUAL, stated rather than hidden. The draw-rate path can still be
    # bought by a page that renders decoration WHILE A CANDIDATE KEY IS HELD.
    # The obvious form of that attack is already dead and not by luck: a lamp
    # that lights on ANY keydown lights for the inert control key too, so
    # `ctl_dpf` rises with it and the delta cancels — MEASURED on the cosmetic
    # attract-loop fixture, where the inert branch drew 133.13 draws/frame
    # against the idle branch's 130.13 and every candidate came out at exactly
    # ddraw 0.00. What survives is a page that decorates on the eight FIRE_KEYS
    # specifically while ignoring F7 and the arrows — i.e. one written against
    # this list. It would take R4 and nothing else: R2, R3 and R7 all still fail,
    # because the decoration has no direction and does not outlive the keypress.
    # Closing it needs the draw-rate path to require a persistent trace as well,
    # which would cost a true positive on any product whose shots leave the
    # screen inside LATE_FRAMES and that changes nothing else — a trade that
    # should not be made blind, and no corpus product is such an attack.
    ctl_motion = base.get("self_change", 0)
    ctl_dpf = max(base.get("draws_per_frame", 0.0), ctrl.get("draws_per_frame", 0.0))
    best = None
    rejected = []
    accepted: list = []
    fire_exceptions: list = []
    for k in FIRE_KEYS:
        br = branch([k], gesture)
        if not br.get("ok"):
            rejected.append({"key": k, "why": "branch failed"})
            continue
        fire_exceptions.extend(br.get("js_exceptions") or [])
        cells = cd(br, base)["cells"]
        ddraw = round(br.get("draws_per_frame", 0.0) - ctl_dpf, 2)
        # Against the INERT-KEY branch, not against no-input: pressing any key
        # costs CDP round-trips that the no-input branch never pays, and that
        # common-mode drift is exactly what made an empty-handler page look
        # responsive. See the R7 note.
        fpost, flate = cd(br, ctrl), cd(br, ctrl, "grid_late")
        travel = None
        if (fpost["measured"] and flate["measured"]
                and fpost["cx"] is not None and flate["cx"] is not None):
            travel = round(flate["cx"] - fpost["cx"], 2)
        travels = bool(travel is not None and noise_measured and late_measured
                       and fpost["cells"] > gate and flate["cells"] > late_gate
                       and travel >= CENTROID_MARGIN)
        rec = {"key": k, "ddraw": ddraw, "cells": cells,
               "self_change": br.get("self_change", 0),
               "dpf": br.get("draws_per_frame", 0.0),
               "vs_ctrl_post": fpost["cells"], "vs_ctrl_late": flate["cells"],
               # The POST centroid column of the fire footprint. The spec tier
               # reads it for S1 (fire direction): bullets hang on whichever
               # side of the ship they were fired at during the 30-frame hold,
               # which is measurable even when they leave the screen before the
               # LATE checkpoint and the travel signature therefore cannot be.
               "post_cx": (round(fpost["cx"], 2)
                           if fpost["measured"] and fpost["cx"] is not None else None),
               "travel": travel}
        if br.get("js_exceptions"):
            rec["why"] = f"threw: {str(br['js_exceptions'][0])[:90]}"
        elif br.get("self_change", 0) < ALIVE_MOTION_FRAC * ctl_motion:
            rec["why"] = "world stopped moving — pause, not fire"
        elif (br.get("draws_per_frame", 0.0) < ALIVE_DRAW_FRAC * ctl_dpf
              or br.get("stalls")):
            rec["why"] = "draw rate collapsed — stop/stall, not fire"
        elif cells > SCENE_CHANGE_FRAC * (GRID_W * GRID_H):
            rec["why"] = "scene-sized repaint — start/restart, not fire"
        elif ddraw < FIRE_DRAW_DELTA and not travels:
            rec["why"] = ("no extra draw calls and nothing travelled downrange — "
                          "nothing was rendered that was not already there")
        else:
            # NOTE: the pixel differential is REPORTED but is NOT an additional
            # requirement. Requiring it too produced a FALSE NEGATIVE on
            # tui-coder-1, whose fire key is `KeyZ || Space` in its own source
            # and which shows an unambiguous +2.2 draws/frame while displacing
            # only 4 grid cells — its projectiles are small and the canvas is
            # 400x300. The draw-rate delta already beats the inert-key control,
            # which measures exactly 0.00 on every product tested, so the extra
            # cell gate bought no discrimination and cost a true positive.
            # STRONGEST wins, not first past the post. A key that merely flashes
            # a menu highlight used to beat the real fire key simply by coming
            # earlier in the list. Draw rate ranks first because it is the
            # corpus-validated signal; travel breaks ties among keys that only
            # the second signature can see.
            rec["signal"] = "draw_rate" if ddraw >= FIRE_DRAW_DELTA else "travel"
            # `travels` is carried so the graded credit can tell a GATED travel
            # measurement from a raw centroid difference. Only the gated one is
            # evidence of a projectile.
            rec["travels"] = travels
            rec["credit"] = _fire_credit(ddraw, travel if travels else None)
            accepted.append(rec)
            rank = (ddraw, travel or 0.0)
            if best is None or rank > (best["ddraw"], best.get("travel") or 0.0):
                best = dict(rec, branch=br)
            continue
        rejected.append(rec)
    for e in fire_exceptions:
        if e not in exc:
            exc.append(e)
    res["js_exceptions"] = exc[:8]
    res["R0_loads_without_exception"] = not exc
    res["fire_rejected"] = rejected
    if best:
        res["fire_key"] = best["key"]
        res["fire_signal"] = best.get("signal")
        res["fire_travel"] = best.get("travel")
        res["diff_fire"] = best["cells"]
        res["fire_draw_delta"] = best["ddraw"]
        res["draws_per_frame_fire"] = best["dpf"]
        # GRADED CONFIDENCE, over the STRONGEST-CREDITED accepted key rather
        # than over the key `fire_key` names. The two are almost always the
        # same; where they differ, `fire_key` reports which key the documented
        # selection rule chose (draw rate first, travel to break ties) and the
        # credit reports the best evidence any accepted key produced. Scoring
        # the named key's credit instead would let a key with a marginally
        # higher draw delta but no travel LOWER the score below a key with both
        # signatures — a non-monotonicity in the rubric's own selection rule.
        credit_rec = max(accepted, key=lambda r: r["credit"])
        res["fire_credit_key"] = credit_rec["key"]
        res["R4_weapon_fires"] = credit_rec["credit"]
        # Carried for the spec tier's S1 (fire direction), all from the
        # CREDITED key — the key R4's own points stand on.
        res["fire_post_cx"] = credit_rec.get("post_cx")
        res["fire_post_cells"] = credit_rec.get("vs_ctrl_post")
        res["fire_travel_credit"] = credit_rec.get("travel")
        res["fire_travels_gated"] = bool(credit_rec.get("travels"))
        # Every accepted key, strongest first — the spec tier's charge probe
        # tries the top two, because a charge mechanic often lives on a
        # SECONDARY weapon key that also fired in this sweep.
        res["fire_keys_accepted"] = [
            r["key"] for r in sorted(accepted, key=lambda r: -r["credit"])]
        fire_branch = best["branch"]
    else:
        res["fire_draw_delta"] = None
        res["draws_per_frame_fire"] = base.get("draws_per_frame", 0.0)
        fire_branch = None
        # EXPLICIT, and it matters which of the two this is. "Eight keys were
        # tried and none of them fired" is a measurement; "none of the eight
        # branches ran" is not, and must not be reported as a game that does not
        # shoot.
        drove = [r for r in rejected if r.get("why") != "branch failed"]
        res["R4_weapon_fires"] = 0.0 if drove else None
        if not drove:
            res["notes"] += "no fire-key branch ran — R4 UNKNOWN; "

    # ---- R6: audio ---------------------------------------------------------
    # PRESENCE, THEN REACTIVITY. The old rule was `0 / <10 starts / >=10 starts`,
    # and the 10 was arbitrary in a way that showed: across the ranked corpus it
    # separated exactly ONE product, collapsing a measured range of 7 to 768
    # starts into a single value for the other 22, for 6 points of very nearly
    # dead weight.
    #
    # The fix is NOT to grade the count. `audioStarts` counts start() calls on
    # source nodes, so the count is a statement about how a product SYNTHESISES
    # sound, not about how much sound it has, and it is the noisiest number this
    # instrument produces — 40 against 20 on the same product across two runs of
    # identical code, where every grid differential in those same runs
    # reproduced to the cell. Grading it would have ranked synthesis style and
    # imported that noise into the score.
    #
    # What the counter CAN answer is whether the audio is wired to the game.
    # `base` holds no key; the input branches hold arrows or the fire key through
    # the identical protocol and the identical frame budget. More sound under
    # input than under none is a sound EFFECT — the game reacting — and no
    # amount of background music produces it. See `_audio_credit`.
    audio_idle = int(base.get("audio_starts", 0) or 0)
    audio_input = max([int(b.get("audio_starts", 0) or 0)
                       for b in (up, down, left, right, fire_branch)
                       if b and b.get("ok")] or [0])
    res["audio_starts"] = max(audio_idle, audio_input)
    res["audio_idle_starts"] = audio_idle
    res["audio_input_starts"] = audio_input
    res["R6_audio_scheduled"] = _audio_credit(audio_idle, audio_input)

    # ---- R7: the input has PERSISTENT consequences ------------------------
    # Replaces "3 distinct terminal hashes", which counted the same thing R2/R3
    # already counted and passed on nothing more than two loads disagreeing.
    # Here every branch runs LATE_FRAMES further with no input at all, and the
    # question is whether the worlds have STAYED apart. A game keeps them apart
    # — the ship is elsewhere, an enemy is dead, the score moved. A page that
    # only decorates the frame while a key is down re-converges, and so does
    # one whose "response" was jitter.
    #
    # COMPARED AGAINST THE INERT-KEY BRANCH, NOT AGAINST NO-INPUT. Pressing a
    # key at all costs two CDP round-trips that the no-input branch never pays,
    # and the extra raw frames that slip through during them put the whole world
    # on a different scroll phase. MEASURED: the attract-loop fixture with an
    # EMPTY keydown handler showed 142 cells of "persistence" against the
    # no-input base — a page that cannot read input at all, scoring the check
    # outright. Against `ctrl`, which pressed an inert key through the identical
    # protocol, that common-mode drift cancels and only a real consequence
    # survives.
    # `late_gate` and `late_measured` were built next to the post gate above, so
    # that R4 could use them too.
    _persist_all = [c["cells"] for c in
                    (cd(b, ctrl, "grid_late")
                     for b in (up, down, right, left, fire_branch or ctrl)
                     if b.get("ok"))
                    if c["measured"]]
    # WHOLE-SCREEN REPAINT GUARD, the same one R4 and the axis test already
    # apply, extended here because R7 was the third place it belongs and the
    # only one still missing it. MEASURED: pi-3 was credited the full 8 points
    # for a late-grid difference of 3059 of 3072 cells — 99.6% of the screen.
    # That is a restart, a death, or a scene change, and it is not evidence
    # that an arrow press left a trace; a real one is roughly the ship's own
    # footprint. Reading a repaint as persistence rewards exactly the products
    # that lose their state, which inverts the check.
    #
    # Scene-sized readings are DISCARDED rather than scored 0: the branch did
    # repaint, so what the input left behind underneath is unobservable, not
    # absent. If every branch repaints there is nothing left to read and the
    # answer is UNKNOWN — the evidence law this file now holds to everywhere.
    _repaint_cut = SCENE_CHANGE_FRAC_AXIS * (GRID_W * GRID_H)
    persist_cells = [c for c in _persist_all if c <= _repaint_cut]
    res["persistence_repaints"] = len(_persist_all) - len(persist_cells)
    res["persistence_gate"] = late_gate if late_measured else None
    if not late_measured or not persist_cells:
        # No comparable late grids, or every one of them was a repaint: the
        # question was never answerable, so it has no answer. Not "the input
        # left no trace".
        res["persistence_cells"] = None
        res["R7_input_persists"] = None
        if _persist_all and not persist_cells:
            res["notes"] = (res.get("notes") or "") + \
                "R7 UNKNOWN: every late branch repainted the scene; "
    else:
        persist = max(persist_cells)
        res["persistence_cells"] = persist
        # GRADED against this product's own late noise floor, on the same
        # 1x-to-TIE_FACTOR ramp as R2/R3 and for the same measured reason: R7
        # flipped between identical runs on the two products whose persistence
        # sat just above the floor, and the flip was worth all 8 points both
        # times. The tie band that used to report UNKNOWN here is the lower half
        # of that ramp now. See `_persist_credit` for why the SIZE of the
        # persistence is deliberately not graded.
        res["R7_input_persists"] = _persist_credit(persist, late_gate)

    states = {b["hash"] for b in (base, up, down, right, left)
              if b.get("ok") and b.get("hash")}
    res["distinct_states"] = len(states)

    # ---- R8: the screen is actually populated -----------------------------
    # A RUNTIME content measure, added because the source-side content tier is
    # pure vocabulary and was shown to be worth 20/20 to a static page whose
    # only "content" was a comment listing genre nouns. This asks how much of
    # the canvas is alive between two no-input checkpoints. It cannot be earned
    # by writing the word "sprite".
    live = _cells(_grid_of(base, "grid_pre"), _grid_of(base, "grid_post"))
    if not live["measured"]:
        # An unreadable canvas is not an empty screen.
        res["scene_activity"] = None
        res["R8_scene_populated"] = None
        res["notes"] += "canvas not readable — R8 UNKNOWN; "
    else:
        frac = live["cells"] / float(GRID_W * GRID_H)
        res["scene_activity"] = round(frac, 4)
        # GRADED CONTINUOUSLY, floor and saturation both stated as fractions of
        # the grid so the scale is independent of canvas size. The three buckets
        # it replaces (0 / 0.5 / 1 at 2% and 6%) put 15 of 23 products at the
        # same value and every one of the top eight at exactly 1.0, which is
        # 12 points that could not separate the products they were meant to
        # separate. See SCENE_FULL_FRAC for where it saturates and why.
        res["R8_scene_populated"] = _scene_credit(frac)

    # ---- Tier 1b: the spec/genre tier, in the SAME session ----------------
    # Runs after R4 because it needs the credited fire key, and inside the
    # session so its branches pay no browser start-up. A spec-tier crash must
    # never cost the mechanics measurements already in hand: every S check
    # falls back to UNKNOWN, and the error is reported, not swallowed.
    try:
        spec = spec_tier(sess, entry, root, {
            "activated": bool(res.get("activated")),
            "gesture": gesture,
            "fire_key": res.get("fire_credit_key"),
            "fire_keys_accepted": res.get("fire_keys_accepted"),
            "fire_measured_absent": res.get("R4_weapon_fires") == 0.0,
            "fire_travel": res.get("fire_travel_credit"),
            "fire_travels_gated": res.get("fire_travels_gated"),
            "fire_post_cx": res.get("fire_post_cx"),
            "fire_post_cells": res.get("fire_post_cells"),
            "gate": gate if noise_measured else None,
            "up": up, "down": down, "base": base,
        })
    except Exception as e:  # noqa: BLE001
        spec = {k: None for k in S_WEIGHTS}
        spec["spec_notes"] = f"spec tier crashed: {str(e)[:160]}"
        spec["spec_evidence"] = {}
    res.update(spec)
    if spec.get("spec_notes"):
        res["notes"] = (res.get("notes") or "") + " | " + spec["spec_notes"]

    stalls = sum(b.get("stalls", 0) for b in (base, up, down, right, left) if b.get("ok"))
    if stalls:
        res["notes"] = (res["notes"] or "") + f"{stalls} frame stall(s)"
    return res


# ---------------------------------------------------------------- Tier 2

def strip_noncode(src: str) -> str:
    """Remove comments and string/template literals.

    Everything the content tier claims to measure is "is this concept present
    and WIRED IN". A comment cannot be wired in, and neither can a string. The
    substitution keeps newlines so that per-line counting still works.

    THE TEMPLATE LITERAL WAS NOT BEING STRIPPED. All three quote forms were
    handled by one newline-anchored pattern (`[^q\\\\\\n]`), which is right for
    '...' and "..." — they cannot span lines — and wrong for a backtick, which
    can. A multi-line template literal therefore survived intact into what this
    function calls "code". MEASURED: a fixture whose entire vocabulary sat in one
    10-line template literal scored the FULL content tier, 10/10, for a page with
    no game in it — the same attack the comment stripper was written to stop,
    through the one door it left open. Backticks are now matched across
    newlines, and `${...}` interpolations are kept, because those really are
    code.
    """
    def blank(m: re.Match) -> str:
        return "\n" * m.group(0).count("\n")

    def template(m: re.Match) -> str:
        body = m.group(0)
        return " ".join(re.findall(r"\$\{([^{}]*)\}", body)) + blank(m)

    src = re.sub(r"/\*.*?\*/", blank, src, flags=re.S)
    src = re.sub(r"(?m)//.*$", "", src)
    src = re.sub(r"(?s)<!--.*?-->", blank, src)
    src = re.sub(r"`(?:\\.|[^`\\])*`", template, src)
    for q in ("'", '"'):
        src = re.sub(rf"{q}(?:\\.|[^{q}\\\n])*{q}", "", src)
    return src


def tier2(d: Path) -> dict:
    """Source-side: genre content breadth and code quality.

    Every check here is at least partly GAMEABLE by writing plausible code that
    never runs, which is why the tier is capped at 30 and why `live_terms`
    requires a second reference: a name that is defined and never used again is
    dead content, and dead content is exactly what a premature-completion agent
    produces.
    """
    # DOT-DIRECTORIES ARE EXCLUDED. Five products ship `.cg_rounds/round_N/`
    # snapshot copies of themselves — their own build history. `rglob` swept
    # those in, and duplicated files push concentration DOWN (the same code
    # spread over five copies looks beautifully distributed) and the orphan
    # fraction to ZERO (every name now appears five times, so nothing can be
    # unreferenced). MEASURED inflation: +3.12 points on tui-coder-1, which is
    # what made it the corpus Tier-2 maximum; +2.05 on tui-coder-3; +0.83 on
    # three others. The scorer was grading intermediate build artifacts.
    def _visible(pat):
        return sorted(q for q in d.rglob(pat)
                      if q.is_file()
                      and not any(x.startswith(".") for x in q.relative_to(d).parts))

    js = sorted(set(_visible("*.js") + _visible("*.mjs")))
    html = _visible("*.html") + _visible("*.htm")
    texts = {}
    for p in js + html:
        try:
            texts[p] = p.read_text(errors="replace")
        except Exception:  # noqa: BLE001
            continue
    src = "\n".join(texts.values())
    # CODE ONLY. Comments and string literals are prose, and prose is free.
    # MEASURED: a page whose entire body was `fillText('R-TYPE')` plus a 20-line
    # comment listing genre nouns scored 20/20 on content breadth — the full
    # tier — while a small but genuinely playable shmup that names its arrays
    # `foes` and `shots` scored 1.8/20. The tier was ranking vocabulary, and
    # ranking it upside down.
    #
    # AND CODE ONLY MEANS FROM WHERE CODE LIVES. `src` is every .js file AND
    # every byte of every .html file, so the markup — headings, paragraphs, a
    # <div> of lore — went into the stripper, came out untouched (it is neither
    # comment nor string) and was counted as code. MEASURED: a fixture that put
    # the whole genre vocabulary in visible <p> tags took the content tier
    # outright. HTML prose is now dropped and only inline <script> bodies are
    # kept, which also stops an apostrophe in prose ("shoot-'em-up") from opening
    # a bogus string literal and deleting the real code on the line after it.
    # `src` itself is left whole: `Q_uses_modules` has to see type="module" in
    # the markup, and `Q_stub_markers` deliberately counts a TODO in a comment.
    inline = [m.group(1)
              for p, t in texts.items() if p.suffix in (".html", ".htm")
              for m in re.finditer(
                  r"<script(?![^>]*\bsrc=)[^>]*>(.*?)</script>", t, re.S | re.I)]
    code = strip_noncode("\n".join(
        [t for p, t in texts.items() if p.suffix in (".js", ".mjs")] + inline))
    r: dict = {}

    def live_terms(pattern: str) -> int:
        """Distinct CONCEPTS that are USED, not merely mentioned.

        A concept must appear on at least two DISTINCT LINES of code. One line
        cannot both introduce a concept and wire it in, and a repeated token on
        a single line ("laser laser laser") is a wall of synonyms, which is
        exactly what the old >= 2 total-occurrences rule accepted.

        COUNTED BY STEM, NOT BY SURFACE FORM. The count keys on which VOCAB
        alternative matched, so `section`, `sections`, `SECTION_0` ... `SECTION_12`
        are ONE concept, and so are `shot` and `shots`. Keying on the full match
        made a numbered identifier worth a distinct concept apiece: MEASURED, the
        combined adversarial fixture declares thirteen padding modules as
        `SECTION_0`..`SECTION_12` and scored 19 distinct stage terms — more than
        every real product in the corpus, whose maximum is 11 — and took the full
        stage weight for one concept written thirteen times. By stem it scores 4,
        against a corpus range of 0-3.
        """
        lines = code.splitlines()
        forms: dict = {}
        for m in re.finditer(pattern, code, re.IGNORECASE):
            forms.setdefault((m.group(1) or "").lower(), set()).add(m.group(0).lower())
        out = 0
        for stem, surface in forms.items():
            hits = 0
            for f in surface:
                pat = re.compile(re.escape(f), re.IGNORECASE)
                hits += sum(1 for ln in lines if pat.search(ln))
            if hits >= 2:
                out += 1
        return out

    for concept, pat in VOCAB.items():
        r[f"V_{concept}_terms"] = live_terms(pat)

    # ---- code quality ------------------------------------------------------
    # Normalised, never raw size. A 51-file product and a 6-file product scored
    # identically on behaviour in this corpus, so rewarding bulk would rank
    # verbosity.
    # INLINE SCRIPT COUNTS AS CODE. Taking only .js/.mjs files scored a product
    # that ships its whole game inside <script> tags as having no code at all:
    # MEASURED, abstractcode-coder-1, abstractcode-coder-3 and tui-coder-2 are
    # 47-78 KB of working game and were recorded as Q_total_loc=1, Q_modules=0,
    # and landed in the bottom three of the source tier for a packaging choice
    # rather than for anything about their quality. Each inline block is treated
    # as its own unit, which is also the honest reading of concentration: three
    # <script> blocks in one file are no more modular than one.
    code_js = {p: t for p, t in texts.items() if p.suffix in (".js", ".mjs")}
    for p, t in texts.items():
        if p.suffix not in (".html", ".htm"):
            continue
        for i, m in enumerate(re.finditer(
                r"<script(?![^>]*\bsrc=)[^>]*>(.*?)</script>", t, re.S | re.I)):
            body = m.group(1)
            if body.strip():
                code_js[p.with_name(f"{p.name}#inline{i}")] = body
    loc = {p: len([ln for ln in t.splitlines() if ln.strip()
                   and not ln.strip().startswith("//")]) for p, t in code_js.items()}
    total_loc = sum(loc.values()) or 1
    # A MODULE is a file that carries work. Counting every file made the tier
    # buyable with a shell script: MEASURED, 13 files of `export const K0 = 0;
    # export function step0(v) { return v + K0; }` scored 4.75/5 on modularity
    # and 27.75/30 on the whole tier — higher than 23 of the 24 real products —
    # for a page that renders one string and never loops.
    subst = {p: n for p, n in loc.items() if n >= SUBSTANTIAL_LOC}
    r["Q_modules"] = len(subst)
    r["Q_files"] = len(code_js)
    r["Q_trivial_files"] = len(code_js) - len(subst)
    r["Q_total_loc"] = total_loc
    r["Q_max_file_loc"] = max(loc.values()) if loc else 0
    # Concentration: 1.0 means one file holds everything. Modularity is about
    # DISTRIBUTION, not file count — 20 files with 95% of the code in one of
    # them is a monolith with decoration. Computed over SUBSTANTIAL files only,
    # so padding the tree with stubs cannot dilute it.
    sl = list(subst.values()) or list(loc.values())
    r["Q_concentration"] = round((max(sl) / sum(sl)), 3) if sl else 1.0
    r["Q_uses_modules"] = bool(re.search(r"\b(export|import)\s", code)) or bool(
        re.search(r'type=["\']module["\']', src))
    # A class needs a body worth having: >= 2 members. Three one-line classes
    # were worth a point under the old rule.
    bodies = re.findall(r"\bclass\s+([A-Z]\w+)[^{]*\{(.*?)\n\}", code, re.S)
    r["Q_classes"] = sum(1 for _n, b in bodies
                         if len(re.findall(r"^\s{2,}[\w#\[]", b, re.M)) >= 2)

    # Dead code: functions declared and never referenced anywhere else. Read
    # from stripped code — a name "used" only inside a comment is still dead.
    # INCLUDING CLASS METHODS. The old regex saw only `function f()` and
    # `const f = () =>`. In a class-based codebase nearly all code lives in
    # methods, so the denominator collapsed to whatever stray helpers were left
    # and the ratio became a statement about coding STYLE rather than about
    # dead code. MEASURED: abstractcode-basic-3 — 8 modules, 9 classes — was
    # judged on 7 functions, scored a 28.6% orphan rate and lost more than half
    # the cleanliness points, while a 40 KB procedural monolith with 41
    # functions all called scored a perfect 3.00/3. The check was rewarding
    # monoliths and punishing the more structured product.
    decl = set(re.findall(r"(?:function\s+(\w+)|(?:const|let|var)\s+(\w+)\s*=\s*"
                          r"(?:async\s*)?(?:function\b|\([^)]*\)\s*=>|\w+\s*=>))", code))
    names = {a or b for a, b in decl if (a or b)}
    for m in re.finditer(r"^[ \t]+(?:static\s+|async\s+|\*\s*)?([A-Za-z_$][\w$]*)\s*\([^)]*\)\s*\{",
                         code, re.M):
        n = m.group(1)
        if n not in ("if", "for", "while", "switch", "catch", "function", "do",
                     "return", "constructor", "else", "with", "typeof"):
            names.add(n)
    orphans = [n for n in names if len(re.findall(rf"\b{re.escape(n)}\b", code)) < 2]
    r["Q_functions"] = len(names)
    r["Q_orphans"] = len(orphans)
    r["Q_orphan_fraction"] = round(len(orphans) / len(names), 3) if names else 0.0

    # Stub markers: an honest TODO is still an unfinished game.
    r["Q_stub_markers"] = len(re.findall(
        r"\b(TODO|FIXME|XXX|HACK|not\s+implemented|unimplemented|placeholder)\b",
        src, re.IGNORECASE))
    return r


# ---------------------------------------------------------------- scoring

# Behavioural weights sum to 80, source weights to 20. The split moved from
# 70/30 after the source tier was shown to be buyable outright: a static page
# with a comment listing genre nouns took 20/20 on content breadth, and 13 stub
# files took 4.75/5 on modularity. The file's own rule — gameable checks never
# outweigh observed behaviour — required cutting the tier, not re-tuning it.
# Weights encode what the BRIEF asked for: "if it can launch, build, render",
# "quality and animation of the sprites", "power ups", "weapons", "quality of
# code".
# R5 AND R8 ARE TWO THRESHOLDS ON ONE MEASUREMENT, and used to be paid for
# twice. Both read the SAME two checkpoints of the SAME no-input branch: R5
# asked whether `self_change` (changed pixels) cleared 120, R8 whether
# `_cells(grid_pre, grid_post)` cleared 6% of the grid. Identical comparison,
# different units — 20 of 80 behavioural points riding on one number. R5's
# floor was also resolution-dependent, 120 absolute pixels being 0.5% of a
# 160x144 canvas and 0.025% of an 800x600 one, and this corpus contains both.
#
# They are NOT merged, because they answer different questions at different
# thresholds — "does anything move by itself" is a genre gate, "how much of the
# screen is alive" is a content measure — but the gate is now paid as a gate.
# R5 passed 6/6 on the validation subset and 22/23 on the full corpus, and that
# is a TRUE statement about the corpus rather than a broken check: every one of
# these products really is a scrolling shooter. A check that is correct and
# uniform belongs in the rubric as a guard against the product that fails it,
# at a weight that cannot rank anything, which is what 4 points buys.
#
# SIX OF THE NINE CHECKS ARE GRADED. R6 and R8 were the first two, in three
# buckets each, and three buckets were not enough: R8's put every one of the top
# eight products at exactly 1.0 and R6's separated exactly one product in 23.
# The other four followed once it was measured what the bits were costing —
# Tier 1 took TEN distinct values across 23 products, and every product that
# moved between two runs of identical code moved by flipping one. See the graded
# scales section for each scale, where it saturates, and why the three checks
# that are still binary are gates rather than qualities.
#
# THE WEIGHTS ARE UNCHANGED, deliberately. A perturbation study moved all 18
# weights by +/-30% and 17 of the 18 perturbations left the arm ordering
# IDENTICAL (Kendall tau +1.000), so weighting was measured NOT to be what was
# wrong; the instrument's own resolution was. Re-weighting on top of a grading
# change would confound the two.
T1_WEIGHTS = {
    "R0_loads_without_exception": 8,   # gate: now spans every branch, incl. firing
    "R1_renders_and_animates": 4,      # gate: corpus-uniform, kept as a guard
    "R2_ship_moves_vertically": 11,    # graded: evidence x displacement
    "R3_ship_moves_horizontally": 11,  # graded: evidence x displacement
    "R4_weapon_fires": 16,     # graded: detection confidence only, see _fire_credit
    "R5_world_scrolls": 4,     # gate: same measurement as R8, paid once
    "R6_audio_scheduled": 6,   # graded: presence + reactivity
    "R7_input_persists": 8,    # graded: confidence above the late noise floor
    "R8_scene_populated": 12,  # graded: live fraction of the play field
}
# Checks whose value is a FRACTION of the weight rather than a boolean. The
# complement — R0, R1, R5 — is the set of VALIDITY GATES, and they stay binary
# on purpose: "loads without throwing", "renders and animates at all" and "the
# world moves by itself" are preconditions for the measurement, not qualities
# that can be half-present.
T1_GRADED = {"R2_ship_moves_vertically", "R3_ship_moves_horizontally",
             "R4_weapon_fires", "R6_audio_scheduled", "R7_input_persists",
             "R8_scene_populated"}

# THE 100 POINTS ARE RE-SPLIT: mechanics 55, spec/genre 25, source 20. The
# mechanics tier's INTERNAL weights are untouched — the perturbation study
# showed the arm ordering is insensitive to them, and re-tuning them while
# adding a tier would confound the two changes — so the whole tier is rescaled
# by one factor. Behavioural dominance holds: 80 of the 100 points are still
# observed in Chrome, and the gameable source tier stays at 20.
#
# WHY THE SPEC TIER IS WORTH A QUARTER. The operator played this corpus and
# the mechanics tier could not represent a single one of their findings: a
# wrong-direction weapon, unkillable enemies, a minute-empty field and the
# corpus's only real power-up all scored inside the same few points. Those are
# not polish items — they are what the product was FOR. 25 points is enough
# that a product failing the operator-visible spec items cannot finish above
# one that delivers them, and small enough that the mechanics gates (does it
# run, respond, render) still dominate.
T1_RESCALE = 55.0 / 80.0


def _cap(n: int, full: int) -> float:
    """Saturating credit: `full` distinct live terms earns 1.0, more earns no
    more. Uncapped counts would reward a wall of synonyms."""
    return min(1.0, n / full) if full else 0.0


def t2_score(t2: dict) -> tuple[float, dict]:
    """20 points: content breadth (10) + code quality (10).

    Content was 20 and is now 10. It is a vocabulary count, it was demonstrated
    to be worth full marks to a page with no game in it, and no amount of
    tightening changes the fact that naming a thing is not building it. What it
    still buys is a genuine signal — a product that never says `enemy` in any
    form probably has none — so it is kept, small, and the runtime content
    check R8 carries the weight it used to hold.
    """
    # CAPS ARE SET AT THE CORPUS RANGE, so a check is pinned at neither end.
    # MEASURED, before this change: `weapons` (cap 5, corpus 4-10) took two
    # distinct values across 23 products and `enemies` (cap 5, corpus 4-12) took
    # two — both effectively constant, both pure denominator. In the version
    # before that, with looser term matching, both were EXACTLY constant at full
    # marks for all 23. A cap below the corpus minimum is not a strict check, it
    # is an absent one.
    #
    # `sprites` IS DELETED, not re-capped. It scored 0-2 against a cap of 6 —
    # anti-calibrated at the floor, the mirror of the same mistake — but the
    # deeper problem is that it could not have worked here at any cap: NOT ONE
    # product in this corpus loads an image. Zero `new Image`, zero
    # `createImageBitmap`, zero `<img>`, zero `fetch` across all 24. Every one
    # of them draws its ship procedurally with fillRect and paths, so counting
    # the word "sprite" measures whether the author happened to use that noun.
    # Sprite quality and animation are real parts of the brief; they are
    # measured where they are visible, in R1 and R8, not in the vocabulary.
    # RE-CALIBRATED for stem counting. `live_terms` now counts CONCEPTS rather
    # than surface forms, so every count fell by roughly half and the old caps
    # (10/8/12/8) sat far above the new corpus maximum — a cap above the range
    # is as dead as one below it, pinning every product at partial credit and
    # ranking nothing. MEASURED stem ranges over the 24 products: weapons 0-6,
    # power-ups 0-4, enemies 0-6, stages 0-3.
    parts = {
        "weapons": _cap(t2.get("V_weapon_terms", 0), 6) * 3,
        "powerups": _cap(t2.get("V_powerup_terms", 0), 4) * 3,
        "enemies": _cap(t2.get("V_enemy_terms", 0), 6) * 2.5,
        "stages": _cap(t2.get("V_stage_terms", 0), 3) * 1.5,
    }
    # Code quality, 10 points. Every term is a RATIO or a bounded judgement so
    # that writing more code cannot buy a better score.
    mods = t2.get("Q_modules", 0)
    modular = 0.0
    if mods >= 2:
        modular += 1.5
    if t2.get("Q_uses_modules"):
        modular += 1.0
    if t2.get("Q_classes", 0) >= 3:
        modular += 1.0
    # Spread: a monolith concentrates everything in one file. Only meaningful
    # once there are at least two files carrying real work — otherwise "well
    # distributed" is a statement about how the stubs were arranged.
    if mods >= 2:
        modular += 1.5 * (1.0 - min(1.0, t2.get("Q_concentration", 1.0)))
    parts["modularity"] = round(modular, 2)
    # `clean` and `no_stubs` are ABSENCE tests, and absence is free when there is
    # nothing there. MEASURED: an adversarial page with zero functions and zero
    # TODOs — because it had zero code — collected the full 5 points for having
    # no dead code and no stubs. So both are scaled by whether the product has
    # enough code for the question to mean anything.
    substance = min(1.0, t2.get("Q_total_loc", 0) / 120.0)
    parts["clean"] = round(3.0 * substance
                           * (1.0 - min(1.0, t2.get("Q_orphan_fraction", 0.0) * 2)), 2)
    parts["no_stubs"] = round(substance * (
        2.0 if t2.get("Q_stub_markers", 0) == 0 else (
            1.0 if t2.get("Q_stub_markers", 0) <= 2 else 0.0)), 2)
    return round(sum(parts.values()), 3), {k: round(v, 2) for k, v in parts.items()}


def score_one(d: Path) -> dict:
    r: dict = {"dir": str(d)}
    r.update(tier0(d))
    r["TIER0_PASS"] = bool(r.get("G0_single_entry") and r.get("G0_refs_resolve")
                           and r.get("G1_js_parses"))
    t1 = tier1(d)
    r.update(t1)
    t2 = tier2(d)
    r.update(t2)

    if not r["TIER0_PASS"]:
        # A product that does not build/resolve/parse is not a game. Gate, not
        # a deduction — the tiers below it are meaningless.
        r["SCORE"] = 0.0
        r["SCORE_LEGACY"] = 0.0
        r["T1_POINTS"] = 0.0
        r["T1B_POINTS"] = 0.0
        r["T2_POINTS"] = 0.0
        r["notes"] = (r.get("notes") or "") + " | TIER0 FAIL"
        return r

    t1_pts = round(sum(w * float(t1.get(k) or 0) for k, w in T1_WEIGHTS.items()), 2)
    s_pts = round(sum(w * float(t1.get(k) or 0) for k, w in S_WEIGHTS.items()), 2)
    t2_pts, t2_parts = t2_score(t2)
    r["T1_POINTS"] = t1_pts        # mechanics, on its own 80-point scale
    r["T1B_POINTS"] = s_pts        # spec/genre, on its 25-point scale
    r["T2_POINTS"] = t2_pts
    r["T2_PARTS"] = t2_parts
    r["SCORE"] = round((t1_pts * T1_RESCALE + s_pts + t2_pts) / 100.0, 4)
    # THE PRE-SPEC LENS over the SAME measurements: what this run would have
    # scored under the mechanics-only rubric. Reported so a rescore can show
    # per-product what the new tier changed without re-running anything, and so
    # the two lenses can never drift apart on instrument or corpus version.
    r["SCORE_LEGACY"] = round((t1_pts + t2_pts) / 100.0, 4)
    # UNKNOWN checks are reported as a BAND, not folded silently into the score.
    # An unmeasurable check earns nothing — points have to be observed — but
    # SCORE is then a FLOOR rather than a verdict, and a reader who cannot see
    # the difference between "measured and failed" and "never measured" will
    # rank the two the same. `SCORE_CEILING` is what this product would have
    # scored had every unmeasured check passed; when the band is wide the
    # product should be re-run, not ranked. Spec-tier UNKNOWNs (a boss horizon
    # the probe never reached, a win sequence hours away) widen the band
    # exactly as mechanics UNKNOWNs do.
    unknown = [k for k in T1_WEIGHTS if t1.get(k) is None]
    s_unknown = [k for k in S_WEIGHTS if t1.get(k) is None]
    r["T1_UNKNOWN"] = unknown
    r["T1B_UNKNOWN"] = s_unknown
    r["EVIDENCE_INCOMPLETE"] = bool(unknown or s_unknown)
    r["SCORE_CEILING"] = round(
        ((t1_pts + sum(T1_WEIGHTS[k] for k in unknown)) * T1_RESCALE
         + s_pts + sum(S_WEIGHTS[k] for k in s_unknown) + t2_pts) / 100.0, 4)
    if unknown or s_unknown:
        r["notes"] = ((r.get("notes") or "")
                      + f" | UNMEASURED: {','.join(unknown + s_unknown)}"
                      + f" (score is a floor; ceiling {r['SCORE_CEILING']})")
    return r


# ---------------------------------------------------------------- dead-check audit

def dead_check_audit(rows: list[dict]) -> dict:
    """Flag every check that is CONSTANT across the corpus.

    This exists because the Zelda rubric was applied to R-Type with two checks
    (24% of weight) pinned at zero for all 24 products and a third (17.5%)
    pinned at False for 23 of 24 — and nothing in the tooling said so. A check
    with no variance cannot rank anything; it can only move every product by the
    same amount, which is a change of scale, not of information.
    """
    # AUDIT ONLY THE PRODUCTS THAT ARE ACTUALLY BEING RANKED. A Tier-0 failure
    # is scored 0.0 by gate and every one of its behavioural fields is False,
    # so including it manufactures a second value for checks that are otherwise
    # constant. MEASURED: with the single no-index.html product in the pool,
    # R0 (6 pts) and R4 (14 pts) were reported NEAR-CONSTANT when in truth both
    # passed 23/23 of the ranked products — 20 points of dead weight that this
    # very audit exists to catch, hidden by the audit's own denominator.
    live = [r for r in rows if r.get("TIER0_PASS")] or rows
    audit: dict = {"constant": [], "near_constant": [], "informative": [],
                   "n_ranked": len(live)}
    # AUDIT THE POINTS, NOT THE RAW MEASUREMENT. A check whose value ranges
    # 4..10 looks informative and contributes an identical 2.5/2.5 to every
    # product, because its cap sits below the corpus minimum. MEASURED: that
    # was true of `weapons` and `enemies`, 4.5 points between them, and the
    # audit called both informative because it was reading V_weapon_terms
    # instead of the score it produced. Tier-2 parts are audited as the points
    # they contribute; Tier-1 checks are booleans, so raw and contributed carry
    # the same information.
    per_part: dict = {}
    for r in live:
        for k, v in (r.get("T2_PARTS") or {}).items():
            per_part.setdefault("T2:" + k, []).append(v)
    keys = (list(T1_WEIGHTS) + list(S_WEIGHTS)
            + [k for k in rows[0] if k.startswith(("V_", "Q_"))])
    for k in keys + list(per_part):
        vals = per_part[k] if k in per_part else [r.get(k) for r in live if k in r]
        # UNKNOWN carries no information about whether a check DISCRIMINATES, so
        # it must not be counted as a distinct value. Without this, one product
        # that could not be driven makes every genuinely constant check look
        # informative — the audit reporting variance it does not have is the
        # exact failure mode the audit exists to catch.
        vals = [v for v in vals if v is not None]
        if not vals:
            continue
        uniq = {json.dumps(v) for v in vals}
        weight = T1_WEIGHTS.get(k, 0) or S_WEIGHTS.get(k, 0) or (
            max(v for v in vals if isinstance(v, (int, float))) if k in per_part else 0)
        nums = [v for v in vals if isinstance(v, (int, float))]
        entry = {"check": k, "weight": weight, "values": sorted(uniq)[:6],
                 "n_distinct": len(uniq),
                 "sd": round(statistics.pstdev(nums), 3) if len(nums) > 1 else 0.0}
        if len(uniq) == 1:
            audit["constant"].append(entry)
        elif len(uniq) == 2 and min(
                sum(1 for v in vals if json.dumps(v) == u) for u in uniq) <= max(
                    1, len(vals) // 12):
            audit["near_constant"].append(entry)
        else:
            audit["informative"].append(entry)
    audit["dead_weight"] = sum(e["weight"] for e in audit["constant"])
    return audit


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default="untracked/rtype-bench")
    ap.add_argument("--jobs", type=int, default=3)
    ap.add_argument("--out", default=None)
    ap.add_argument("--glob", default="*-product")
    # Verifies the graded scales' two claimed properties — monotone and
    # saturating — by sampling them. Pure arithmetic, no browser, no corpus, so
    # it is checkable in a second and cannot be quietly skipped because a run
    # takes twenty minutes.
    ap.add_argument("--selfcheck", action="store_true",
                    help="check the graded Tier-1 scales and exit")
    a = ap.parse_args()

    if a.selfcheck:
        rep = _selfcheck_graded_scales()
        for c in rep["checks"]:
            print(f"  {'ok ' if c['monotone'] else 'FAIL'}  {c['scale']:28}"
                  f"  {c['min']:.3f}..{c['max']:.3f}   {c['over']}")
        print("\nsaturation (every entry must be 1.0):")
        for k, v in rep["saturation"].items():
            print(f"  {'ok ' if abs(v - 1.0) < 1e-9 else 'FAIL'}  {k:44} {v}")
        print("\nfloors (every entry must be 0.0):")
        for k, v in rep["floors"].items():
            print(f"  {'ok ' if v == 0.0 else 'FAIL'}  {k:44} {v}")
        print(f"\nGRADED SCALES: {'PASS' if rep['PASS'] else 'FAIL'}"
              f"  (monotone={rep['MONOTONE']} saturates={rep['SATURATES']}"
              f" floors={rep['FLOORS_ZERO']})")
        srep = selfcheck_spec_scales()
        print("\nspec-tier scales:")
        for c in srep["checks"]:
            print(f"  {'ok ' if c['monotone'] else 'FAIL'}  {c['scale']:32} {c['over']}")
        for k, v in srep["saturation"].items():
            print(f"  {'ok ' if abs(v - 1.0) < 1e-9 else 'FAIL'}  sat  {k:40} {v}")
        for k, v in srep["floors"].items():
            print(f"  {'ok ' if v == 0.0 else 'FAIL'}  flr  {k:40} {v}")
        wsum = sum(T1_WEIGHTS.values()) * T1_RESCALE + sum(S_WEIGHTS.values()) + 20
        print(f"  {'ok ' if abs(wsum - 100.0) < 1e-9 else 'FAIL'}  weights: "
              f"{sum(T1_WEIGHTS.values())}x{T1_RESCALE:.4f} mechanics + "
              f"{sum(S_WEIGHTS.values())} spec + 20 source = {wsum}")
        print(f"SPEC SCALES: {'PASS' if srep['PASS'] else 'FAIL'}")
        ok = rep["PASS"] and srep["PASS"] and abs(wsum - 100.0) < 1e-9
        return 0 if ok else 1

    root = Path(a.root)
    dirs = sorted(p for p in root.glob(a.glob) if p.is_dir())
    if not dirs:
        print(f"no product dirs under {root}/{a.glob}", file=sys.stderr)
        return 2

    here = Path(__file__).resolve()
    rubric_sha = sha_of(here, here.parent / "zelda_review_score.py",
                        here.parent / "rtype_spec_tier.py")
    print(f"rubric sha256 {rubric_sha[:16]}  ({len(dirs)} products, jobs={a.jobs})")

    rows: list[dict] = []
    with concurrent.futures.ProcessPoolExecutor(max_workers=a.jobs) as ex:
        futs = {ex.submit(score_one, d): d for d in dirs}
        for f in concurrent.futures.as_completed(futs):
            d = futs[f]
            try:
                r = f.result()
            except Exception as e:  # noqa: BLE001
                r = {"dir": str(d), "SCORE": None, "DRIVER_FAILED": True,
                     "notes": f"scorer crashed: {str(e)[:200]}"}
            name = d.name.replace("-product", "")
            r["arm"] = name.rsplit("-", 1)[0]
            r["rep"] = name.rsplit("-", 1)[-1]
            rows.append(r)
            s = r.get("SCORE")
            print(f"  {name:28} {('%.3f' % s) if s is not None else ' FAIL':>6}"
                  f"  T1={r.get('T1_POINTS', 0):>4}  S={r.get('T1B_POINTS', 0):>5}"
                  f"  T2={r.get('T2_POINTS', 0):>5}"
                  f"  det={r.get('deterministic')}  {r.get('notes', '')[:44]}")

    rows.sort(key=lambda r: (r["arm"], r["rep"]))
    audit = dead_check_audit([r for r in rows if r.get("SCORE") is not None])

    out = Path(a.out or (root / "rtype_scores.json"))
    out.write_text(json.dumps(
        {"rubric_sha256": rubric_sha, "root": str(root), "rows": rows,
         "weights": {"tier1": T1_WEIGHTS, "tier1_rescale": T1_RESCALE,
                     "tier1b": S_WEIGHTS, "tier2_total": 20},
         "dead_check_audit": audit}, indent=1))

    print(f"\n--- dead-check audit ({audit['n_ranked']} ranked products) ---")
    if audit["constant"]:
        print(f"CONSTANT (zero information, {audit['dead_weight']} pts of dead weight):")
        for e in audit["constant"]:
            print(f"  {e['check']:32} w={e['weight']:>3}  always {e['values'][0]}")
    else:
        print("no constant checks — every check separates at least two products")
    for e in audit["near_constant"]:
        print(f"NEAR-CONSTANT {e['check']:28} w={e['weight']:>3}  {e['values']}")

    # How much of the rubric is doing work. A check every product passes moves
    # every score by the same amount: that is a change of SCALE, not of
    # information, and it is what pushes a corpus into a narrow band at the top
    # exactly as the Zelda rubric pushed it into a narrow band in the middle.
    total_w = 100
    live_rows = [r for r in rows if r.get("TIER0_PASS")]
    # EFFECTIVE weights: what a check actually contributes to the 100-point
    # score — mechanics carry their tier rescale, spec checks their own weight.
    eff_w = {**{k: w * T1_RESCALE for k, w in T1_WEIGHTS.items()}, **S_WEIGHTS}
    inf_w = 0.0
    for k, w in eff_w.items():
        # `bool()` would collapse a graded check's 0.5 and 1.0 into one value
        # and report an informative check as dead.
        seen = {float(r.get(k) or 0) for r in live_rows}
        if len(seen) > 1:
            inf_w += w

    # COUNTING A CHECK AS INFORMATIVE BECAUSE IT VARIES AT ALL OVERSTATES IT, and
    # the overstatement is not small: a check that separates ONE product from 22
    # "varies", and this line called R0 and R6 — 14 points between them — fully
    # informative on exactly that basis. So the same weight is also reported
    # ENTROPY-WEIGHTED, against the entropy of a check that separates every
    # product (log2 n). Under that measure the pre-grading rubric was carrying
    # 11.7 of 80, not 72 of 80, and that gap is the whole reason Tier 1 was
    # regraded. Both numbers are printed because they answer different
    # questions: "does this check do anything at all", and "how much".
    def _entropy(vals: list) -> float:
        vals = [v for v in vals if v is not None]
        if not vals:
            return 0.0
        n = len(vals)
        seen: dict = {}
        for v in vals:
            seen[round(float(v), 6)] = seen.get(round(float(v), 6), 0) + 1
        import math as _m
        return -sum((c / n) * _m.log2(c / n) for c in seen.values())

    import math as _math
    hmax = _math.log2(len(live_rows)) if len(live_rows) > 1 else 1.0
    ent = {k: _entropy([float(r.get(k) or 0) for r in live_rows])
           for k in eff_w}
    ent_w = sum(w * min(1.0, ent[k] / hmax) for k, w in eff_w.items())
    behav_total = sum(eff_w.values())
    print(f"\nINFORMATIVE BEHAVIOURAL WEIGHT: {inf_w:.1f}/{behav_total:.0f}"
          f" behavioural pts vary across the ranked products"
          f"  ({inf_w / total_w:.0%} of the 100-pt rubric)")
    print(f"ENTROPY-WEIGHTED, against a check that separates every product "
          f"(H_max {hmax:.2f} bits): {ent_w:.1f}/{behav_total:.0f}")
    # UNKNOWN-share per check, printed beside the distribution: a spec check
    # can be honest and still mostly UNKNOWN (a win sequence beyond the probe
    # horizon). The audit must show the difference between "constant because
    # dead" and "constant because rarely measurable".
    for k, w in list(T1_WEIGHTS.items()) + list(S_WEIGHTS.items()):
        we = eff_w[k]
        vals = [float(r.get(k) or 0) for r in live_rows]
        p = sum(1 for v in vals if v > 0)
        unk = sum(1 for r in live_rows if r.get(k) is None)
        # A GRADED check is only constant when its CONTRIBUTED POINTS are
        # constant, not when every product scores something. Counting "passed"
        # for a graded check would hide a check pinned at half marks.
        flag = "  <-- CONSTANT" if len(set(vals)) == 1 else ""
        tail = (f"  H={ent[k]:.2f} bits, {len(set(vals))} distinct"
                + (f", {unk} UNKNOWN" if unk else "") + flag)
        if k in T1_GRADED or k in S_WEIGHTS:
            vs = sorted(set(vals))
            shown = vs if len(vs) <= 6 else [vs[0], "...", vs[-1]]
            print(f"    {k:32} w={we:>5.1f}  graded {shown}{tail}")
        else:
            print(f"    {k:32} w={we:>5.1f}  gate, passed {p}/{len(live_rows)}{tail}")
    # THE RESOLUTION OF THE TIER AS A WHOLE, which is what actually limits
    # ranking: if the behavioural total can only take a handful of values, no
    # amount of per-check variance can separate the products that share one.
    for label, key, tot in (("T1_POINTS (mechanics tier)", "T1_POINTS", 80),
                            ("T1B_POINTS (spec tier)", "T1B_POINTS", 25)):
        tv = [r.get(key) for r in live_rows if r.get(key) is not None]
        if tv:
            print(f"    {label:32} w={tot:>3}  "
                  f"{len(set(tv))} distinct values over {len(tv)} products"
                  f"  H={_entropy(tv):.2f} of {hmax:.2f} bits")

    # PER-ARM MEANS EXCLUDE TIER-0 FAILURES, exactly as the dead-check audit
    # does. A product that does not build is scored 0.0 BY GATE — that zero is a
    # statement about an infrastructure failure, not a measurement of the arm —
    # and averaging it in moved abstractcode-coder from 0.782 (n=2) to 0.522 and
    # from 6th place to last. `n` is printed so the reduced denominator is
    # visible rather than implied, and the excluded products are listed.
    print(f"\n--- per arm (Tier-0 failures excluded) ---")
    by: dict = {}
    excluded: list = []
    for r in rows:
        if r.get("SCORE") is None:
            continue
        if not r.get("TIER0_PASS"):
            excluded.append(f"{r['arm']}-{r['rep']}")
            continue
        by.setdefault(r["arm"], []).append(r["SCORE"])
    print(f"{'arm':24}{'n':>3}{'mean':>8}{'min':>8}{'max':>8}")
    for arm in sorted(by, key=lambda k: -statistics.mean(by[k])):
        v = by[arm]
        print(f"{arm:24}{len(v):>3}{statistics.mean(v):>8.3f}{min(v):>8.3f}{max(v):>8.3f}")
    if excluded:
        print(f"excluded (TIER0 FAIL, not a measurement of the arm): "
              f"{', '.join(sorted(excluded))}")
    incomplete = [f"{r['arm']}-{r['rep']}" for r in rows if r.get("EVIDENCE_INCOMPLETE")]
    if incomplete:
        print(f"EVIDENCE INCOMPLETE (SCORE is a floor, see SCORE_CEILING): "
              f"{', '.join(sorted(incomplete))}")
    # NOT REPRODUCIBLE is a stronger statement than a wide band and has to be
    # said out loud. Determinism is the precondition the whole replay
    # differential rests on; a product that fails it has verdicts that move
    # between identical runs, and its score is not comparable with the rest.
    # MEASURED: on the one corpus product whose idle replay drifts, the drift
    # sampled 130 cells on one run and 27 on the next, which moved its gate 260
    # -> 54 and its score 0.637 -> 0.747 with nothing else changed.
    shaky = [(f"{r['arm']}-{r['rep']}", r.get("stability"))
             for r in rows if r.get("TIER0_PASS") and r.get("deterministic") is not True]
    if shaky:
        print("\nNOT REPRODUCIBLE — the replay precondition failed, so these "
              "scores are not comparable with the rest:")
        for name, st in sorted(shaky):
            print(f"    {name:24} drift {st}")
    print(f"\nwrote {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
