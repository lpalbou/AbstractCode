#!/usr/bin/env python3
"""Visual review + VLM rubric for the R-Type benchmark corpus.

The behavioral scorer (rtype_review_score.py) proves the game flies, shoots and
scrolls; it is blind to the qualities only eyes can judge — whether the sprites
are drawn or blobs, whether "black & white, gameboy style" was honored, whether
enemy fire is distinguishable from the parallax starfield. The operator PLAYED
the corpus and reported exactly those axes varying. This tool captures the
evidence deterministically and puts a FIXED VLM rubric (gpt-5.4, temp 0, same
images, same order, same prompt for every product) on top of it.

ADAPTED FROM scripts/zelda_visual_review.py — the instrument block (seeded RNG,
virtual clock, frozen boot, single-stepped rAF, composite canvas grab) is
IMPORTED from it verbatim, so the two reviewers can never drift on how a page
is served, stepped or photographed. What is R-Type-specific here:

  * ACTIVATION uses the measured ladder of rtype_review_score.py — every
    product boots to a title/briefing flow that needs Enter*2 (70-frame gaps;
    a briefing screen LOCKS input ~1s, measured on codex-2) and one product
    (pi-2) needs Enter*5 to outlast a timed intro. The per-product gesture the
    behavioral scorer VERIFIED (by opposed-arrow differentials) is read from
    --hints and tried first; the ladder is the fallback. This reviewer's own
    scene-change reading is recorded as evidence, never as proof of
    responsiveness — a shmup's title is often busier than its game (measured
    387->354 px on the gesture that demonstrably unlocks the controls).
  * The CHEAT CODE the bench prompt demands ("pressing 3 times 0 on the main
    screen ... unlimited life") is exercised on the title screen. Its easter
    egg is photographed (01b-title-cheat), and if implemented it keeps the
    ship alive long enough for the 2-virtual-minute boss hunt the prompt's
    "each level ... about 2mn ... finish with a boss" implies.
  * The BOSS HUNT: after activation the ship is driven for 7200 virtual
    frames (2 minutes at 60fps) on a fixed autopilot (fire re-pressed every
    chunk so tap-fire and autofire games both shoot; vertical dodge cycles
    up/none/down/none), photographed at 30s/60s/90s/120s. If "GAME OVER" text
    is drawn, the start gesture is re-applied (max 2 restarts, recorded).
  * The HELD-FIRE MOMENT is photographed mid-hold (+4 frames) and late-hold
    (+24), because a 3-frame muzzle flash retracts before a post-hold shot.

OUTPUT (per product, under <out>/<slug>/, slug = path under untracked/ with
'/' -> '--'):

  facts.json  schema "rtype-visual-review/1":
    slug, product_dir, entry, generated_utc, elapsed_s, canvas
    activation  {gesture, source: hint|ladder|none, scene_change_frac,
                 probe_up_px, title_idle_px}
    cheat       {pressed: bool, title_changed_px}
    fire        {key, source, probed {key: px}}
    boss_hunt   {target_frames, reached_frames, restarts, restart_vframes,
                 game_over_text_seen, boss_text_seen, stalled}
    shots       [{file, label, phase, vframe, px_sha, same_as, note}]
    strips      [{file, moment, frames, step, distinct, px_sha}]
    palette     {unique_colors, top}
    text        {canvas_strings, dom_hud_text}
    audio       {starts}
    errors      {js_exceptions, console_errors}
    vlm         null | {model, rubric_version, rubric_sha, images_sent,
                        request, structured, json, usage, raw, error?}

  sheet.html  self-contained dark contact sheet (ids #meta #shots #strips
              #facts #vlm).

  PNG slots (FIXED; a pixel-identical shot is still written and marked
  same_as, so a missing file always means the phase failed):
    00-boot, 01-title, 01b-title-cheat, 02-started, 05-viewport (only when
    DOM holds text), 10-move-up, 11-move-down, 20-fire-mid, 21-fire-held,
    30-t30s, 31-t60s, 32-t90s, 40-t120s-bosshunt,
    60-strip-idle, 61-strip-move, 62-strip-fire.

VLM CONTRACT. One call per product, gpt-5.4 via the local relay (subscription
backed — NO API key, none is ever set here), temperature 0, the SAME 12 slots
in the SAME order for every product (01-title, 02-started, 10-move-up,
20-fire-mid, 21-fire-held, 30/31/32/40, 60/61/62 strips); a missing slot is
sent as an explicit text marker so absence is judged as absence, never
silently skipped. Structured output via response_format json_schema (the relay
advertises it); fallback to prompt-extracted JSON is RECORDED in
facts.vlm.structured. The full request parameters, per-image px_sha, usage
and raw text are stored. Rubric axes (1-5 each, anchored in the prompt):
sprite_quality, gameboy_adherence, enemy_variety, bullet_salience (+ note
naming the failure), vfx_richness, hud_quality, style_coherence, and a
free-text verdict naming the single biggest visual weakness.

DETERMINISM: two runs on the same artifact must produce identical px_sha for
every shot (the VLM answer is temp-0 but not guaranteed byte-identical; the
images it judges are). Verify with --out into a scratch dir and diff facts.

WRITE GUARD: refuses to write anywhere inside the repo except
untracked/rtype-visual/. Paths outside the repo (scratch) are allowed.

Usage:
  python3 scripts/rtype_visual_review.py untracked/rtype-bench/codex-1-product
  python3 scripts/rtype_visual_review.py --all            # the 30-product corpus
  python3 scripts/rtype_visual_review.py --all --vlm      # + VLM judge (quota!)
  python3 scripts/rtype_visual_review.py --aggregate      # rebuild summary only
"""
from __future__ import annotations

import argparse
import base64
import datetime as _dt
import hashlib
import json
import re
import statistics
import sys
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from PIL import Image  # noqa: E402

# Shared harness core — imported, not copied, so the reviewers cannot drift.
# zelda_visual_review's INSTRUMENT already contains BOTH determinism fixes the
# rtype scorer had to retrofit onto the zelda SCORER's instrument: the frame
# budget starts at ZERO (frozen boot) and performance.now is pinned to the
# virtual clock with no wall-clock fallback.
from zelda_visual_review import (  # noqa: E402
    Driver,
    INSTRUMENT,
    make_strip,
    px_sha,
    resolve_entry,
    serve_dir,
    sig_distance,
    upscaled,
)

REPO = Path(__file__).resolve().parents[1]
CHROME_CHANNEL = "chrome"
VLM_URL = "http://127.0.0.1:8317/v1/chat/completions"
# v2: adds the axes the operator's CORRECTED spec demands (orbs, item drops,
# per-level aesthetic variety, win animation, arcade mode). The core 7 axes
# are worded identically to v1 so old-corpus and rtype3-corpus readings stay
# comparable; the new fields are spec-conformance evidence and are judged for
# both corpora (the old corpus was built to the old prompt — its readings on
# the new fields are context, not failures).
VLM_RUBRIC_VERSION = "rtype-v2"

WARMUP_FRAMES = 90        # scorer's constant: past the boot into the title
GESTURE_GAP_FRAMES = 70   # scorer's constant: briefing screens lock input ~1s
HOLD_FRAMES = 30          # scorer's BRANCH_FRAMES: held, never tapped
HUNT_FRAMES = 7200        # 2 virtual minutes at 60fps — "each level ~2mn"
HUNT_CHUNK = 100
HUNT_SHOTS = ((1800, "30-t30s", "t+30s"), (3600, "31-t60s", "t+60s"),
              (5400, "32-t90s", "t+90s"), (7200, "40-t120s-bosshunt",
                                           "t+120s boss hunt"))
MAX_RESTARTS = 2
SCENE_CHANGE_MIN = 0.10   # perceptual sig fraction that counts as leaving title

# Scorer-measured ladder (START_GESTURES), minus the redundant tail; the hint
# gesture from --hints is tried before any of these.
GESTURES = ["none", "key:Enter*2", "key:Enter*3", "key:Space*2",
            "click:canvas*2", "key:z*2", "key:j*2", "key:x*2",
            "key:Enter*5", "key:Space*5"]
FIRE_CANDIDATES = ["Space", "z", "x", "j", "k"]

# 05-viewport is a FULL-PAGE screenshot, always captured: products in this
# corpus split their presentation between the canvas and DOM chrome (measured:
# tui-multi-1 renders its entire HUD — level name, score, lives, weapon — as
# styled DOM around the canvas; a canvas-only judge scored its HUD "none
# visible"). 63-strip-hunt is shot mid-hunt with fire held at spawning
# enemies, because impact VFX only exist at the moment a bullet lands — a
# held-fire strip over empty space cannot show them.
VLM_SLOTS = ["01-title", "02-started", "05-viewport", "10-move-up",
             "20-fire-mid", "21-fire-held", "30-t30s", "31-t60s", "32-t90s",
             "40-t120s-bosshunt", "60-strip-idle", "61-strip-move",
             "62-strip-fire", "63-strip-hunt"]

# --------------------------------------------------------------- VLM rubric
VLM_PROMPT = """You are judging screenshots of a small browser game. The \
build prompt demanded: "a fully playable r-type game in black & white, \
gameboy style", "various weapons and effects and procedural VFX", "take \
extra care to the graphics, VFX". You judge ONLY the visual qualities — \
mechanics are graded elsewhere. Images arrive as labeled slots, the same \
slots for every game you judge; film strips show 6 consecutive frames 3 \
frames apart; a slot marked MISSING means that phase failed. The 05-viewport \
slot shows the FULL PAGE: some games draw their HUD in styled page chrome \
around the canvas rather than inside it — judge hud_quality from wherever \
the HUD actually is. All other slots show the game's own framebuffer. Judge \
only what is visible.

Score each axis as an INTEGER 1-5:

sprite_quality — the drawn entities (ship, enemies, boss). 1 = blank or \
unrecognizable blobs; 2 = bare geometric primitives (plain rects/circles/\
triangles); 3 = readable silhouettes with some intentional detailing; 4 = \
crisp intentional pixel art (outlines, shading, multi-part shapes); 5 = \
polished sprites that would pass in a shipped Game Boy game.

gameboy_adherence — the demanded "black & white, gameboy style". Judge the \
4-shade monochrome palette (Game Boy green-gray or gray ramp), chunky \
low-res pixels, dot-matrix feel. 1 = full color or smooth vector look; 2 = \
mostly monochrome but with saturated color accents or gradients; 3 = \
monochrome but wrong texture (thin anti-aliased lines, many gray levels); \
4 = near-4-shade with chunky pixels; 5 = convincing 4-shade dot-matrix \
screen.

enemy_variety — DISTINCT enemy designs visible across all images. 1 = none \
visible; 2 = one design; 3 = two; 4 = three; 5 = four or more clearly \
different designs.

bullet_salience — are projectiles instantly distinguishable from the \
background/starfield, BOTH the player's shots and enemy fire? 5 = both \
unmistakable (size/shape/brightness distinct from stars); 4 = both visible, \
minor confusion possible; 3 = player's shots clear but enemy fire could be \
confused with background stars/parallax dots; 2 = only one kind of \
projectile visible at all and it is weakly readable; 1 = cannot tell shots \
from background. In bullet_salience_note name exactly what fails (e.g. \
"enemy bullets are 1px dots identical to starfield"), or "" if nothing does.

vfx_richness — impact particles, explosions, muzzle flashes, screen flash/\
shake evidence. 1 = none visible; 2 = bare single-shape puffs; 3 = simple \
explosion animations; 4 = particle bursts or layered explosion sprites; 5 = \
rich layered VFX (particles + flashes + debris).

hud_quality — score/lives/level/boss-health readability and styling. 1 = no \
HUD visible; 2 = raw unstyled text; 3 = plain but complete text HUD; 4 = \
styled panel or gauges; 5 = polished framed HUD with gauges that match the \
game's art.

style_coherence — does it read as ONE game? Consistent palette, pixel \
density, outline style across ship/enemies/background/HUD. 1 = clashing \
mixed assets (different pixel scales, mismatched styles); 3 = mostly \
consistent with visible seams; 5 = one coherent art direction throughout.

The spec also demands specific R-Type signatures. Report what the images \
show, never guess:

orb_visible — true only if a Force-pod-like companion (an orb/pod attached \
to or flying near the player ship, distinct from bullets) is visible in any \
image; in orb_note say where you saw it or "" if false.

item_drops — true only if a pickup/power-up item (a floating collectible \
distinct from enemies and bullets) is visible in any image; in \
item_drops_note say what it looks like or "" if false.

level_variety — the campaign demands "each level has a different level \
design and aesthetics". If the images span more than one visibly distinct \
level/environment (different background structure, terrain, motifs — not \
just more enemies), score 1-5 for how different they are; if every gameplay \
image shows the same environment, you cannot judge this: return null.

win_animation_seen — true only if an image shows a campaign-victory \
sequence (mission-accomplished graphics); null if no image could show it \
(the capture covers only the first ~2 minutes); false only if an image \
shows the campaign END without any victory graphics.

arcade_mode_on_title — true only if the title/menu screenshot shows a \
second selectable mode (e.g. ARCADE next to CAMPAIGN/STORY); false if the \
title shows a single-mode start only.

verdict — ONE sentence naming the single biggest VISUAL weakness of this \
game.

Reply with STRICT JSON, no markdown fence, exactly these keys:
{"sprite_quality": n, "gameboy_adherence": n, "enemy_variety": n,
 "bullet_salience": n, "bullet_salience_note": "...", "vfx_richness": n,
 "hud_quality": n, "style_coherence": n, "orb_visible": b, "orb_note": "...",
 "item_drops": b, "item_drops_note": "...", "level_variety": n|null,
 "win_animation_seen": b|null, "arcade_mode_on_title": b, "verdict": "..."}"""

VLM_SCHEMA = {
    "name": "rtype_visual_rubric",
    "strict": True,
    "schema": {
        "type": "object",
        "additionalProperties": False,
        "properties": {
            "sprite_quality": {"type": "integer", "minimum": 1, "maximum": 5},
            "gameboy_adherence": {"type": "integer", "minimum": 1, "maximum": 5},
            "enemy_variety": {"type": "integer", "minimum": 1, "maximum": 5},
            "bullet_salience": {"type": "integer", "minimum": 1, "maximum": 5},
            "bullet_salience_note": {"type": "string"},
            "vfx_richness": {"type": "integer", "minimum": 1, "maximum": 5},
            "hud_quality": {"type": "integer", "minimum": 1, "maximum": 5},
            "style_coherence": {"type": "integer", "minimum": 1, "maximum": 5},
            "orb_visible": {"type": "boolean"},
            "orb_note": {"type": "string"},
            "item_drops": {"type": "boolean"},
            "item_drops_note": {"type": "string"},
            "level_variety": {"type": ["integer", "null"], "minimum": 1, "maximum": 5},
            "win_animation_seen": {"type": ["boolean", "null"]},
            "arcade_mode_on_title": {"type": "boolean"},
            "verdict": {"type": "string"},
        },
        "required": ["sprite_quality", "gameboy_adherence", "enemy_variety",
                     "bullet_salience", "bullet_salience_note", "vfx_richness",
                     "hud_quality", "style_coherence", "orb_visible", "orb_note",
                     "item_drops", "item_drops_note", "level_variety",
                     "win_animation_seen", "arcade_mode_on_title", "verdict"],
    },
}

AXES = ["sprite_quality", "gameboy_adherence", "enemy_variety",
        "bullet_salience", "vfx_richness", "hud_quality", "style_coherence"]
# Spec-conformance fields (corrected 2026-08-04 spec): aggregated as fractions
# (booleans) or mean-of-non-null (level_variety); never folded into visual_mean
# so the core-7 mean stays comparable across corpora.
SPEC_BOOLS = ["orb_visible", "item_drops", "arcade_mode_on_title"]
SPEC_NULLABLE = ["level_variety", "win_animation_seen"]


# --------------------------------------------------------------- gestures
def apply_gesture(page, drv: Driver, gesture: str, box) -> None:
    """Scorer's apply_gesture: `key:X*N` / `click:canvas*N`, presses
    GESTURE_GAP_FRAMES apart because briefing screens lock input."""
    head, _, rep = gesture.partition("*")
    n = int(rep or 1)
    if head.startswith("key:"):
        k = head.split(":", 1)[1]
        for _ in range(n):
            page.keyboard.down(k)
            drv.adv(3, 4000)
            page.keyboard.up(k)
            drv.adv(GESTURE_GAP_FRAMES, 9000)
    elif head == "click:canvas" and box:
        for _ in range(n):
            page.mouse.click(box["x"] + box["width"] / 2,
                             box["y"] + box["height"] / 2)
            drv.adv(GESTURE_GAP_FRAMES, 9000)


# --------------------------------------------------------------- review
def review_one(browser, product: Path, outdir: Path, hints: dict,
               vlm: bool, vlm_model: str) -> dict:
    t_start = time.time()
    outdir.mkdir(parents=True, exist_ok=True)
    facts: dict = {
        "schema": "rtype-visual-review/1",
        "slug": outdir.name,
        "product_dir": str(product),
        "entry": None,
        "generated_utc": _dt.datetime.now(_dt.timezone.utc).isoformat(timespec="seconds"),
        "canvas": None,
        "activation": {"gesture": None, "source": None, "scene_change_frac": None,
                       "probe_up_px": 0, "title_idle_px": 0},
        "cheat": {"pressed": False, "title_changed_px": 0},
        "fire": {"key": None, "source": None, "probed": {}},
        "boss_hunt": {"target_frames": HUNT_FRAMES, "reached_frames": 0,
                      "restarts": 0, "restart_vframes": [],
                      "game_over_text_seen": False, "boss_text_seen": False,
                      "stalled": False},
        "shots": [], "strips": [],
        "palette": None,
        "text": {"canvas_strings": [], "dom_hud_text": ""},
        "audio": {"starts": 0},
        "errors": {"js_exceptions": [], "console_errors": []},
        "vlm": None,
    }
    entry, root = resolve_entry(product)
    if not entry.is_file():
        facts["error"] = "no index.html"
        write_outputs(outdir, facts)
        return facts
    facts["entry"] = str(entry)

    hint_key = "/".join(product.resolve().parts[-2:])
    hint = (hints.get("products") or {}).get(hint_key) or {}

    shot_hashes: dict[str, str] = {}

    def save_shot(img, name: str, label: str, phase: str, vf: int,
                  note: str = "") -> None:
        if img is None:
            return
        sha = px_sha(img)
        same = shot_hashes.get(sha)
        if same is None:
            shot_hashes[sha] = label
        upscaled(img).save(outdir / f"{name}.png")
        facts["shots"].append({"file": f"{name}.png", "label": label,
                               "phase": phase, "vframe": vf, "px_sha": sha,
                               "same_as": same, "note": note})

    srv = None
    context = page = None
    try:
        srv, port = serve_dir(root)
        context = browser.new_context(viewport={"width": 1000, "height": 900},
                                      device_scale_factor=1)
        page = context.new_page()
        errs: list[str] = []
        console_msgs: list[str] = []
        page.on("pageerror", lambda e: errs.append(str(e)[:200]))
        page.on("console", lambda m: console_msgs.append(f"console.{m.type}: {m.text[:160]}")
                if m.type == "error" else None)
        page.on("dialog", lambda d: d.dismiss())
        page.add_init_script(INSTRUMENT)
        page.goto(f"http://127.0.0.1:{port}/{entry.name}", wait_until="load",
                  timeout=30000)
        try:
            page.evaluate("() => document.fonts ? document.fonts.ready.then(() => true) : true")
        except Exception:  # noqa: BLE001
            pass
        drv = Driver(page)

        def vframe() -> int:
            p = page.evaluate("() => window.__probe") or {}
            return int(p.get("frames") or 0)

        def texts_matching(pat: str) -> bool:
            return any(re.search(pat, t, re.I) for t in drv.texts().keys())

        # ---- boot + title ---------------------------------------------------
        save_shot(drv.grab(), "00-boot", "boot", "boot", vframe())
        drv.adv(WARMUP_FRAMES, 15000)
        save_shot(drv.grab(), "01-title", "title", "title", vframe())
        facts["canvas"] = {"all": page.evaluate("() => window.__meta()") or []}
        cs = [c for c in facts["canvas"]["all"] if c.get("visible")]
        facts["canvas"]["count"] = len(cs)
        if cs:
            main = max(cs, key=lambda c: c["w"] * c["h"])
            facts["canvas"]["main"] = f"{main['w']}x{main['h']}"
            facts["canvas"]["css"] = f"{main['cssW']}x{main['cssH']}"
        title_sig = drv.sig()
        title_idle = drv.idle_window()
        facts["activation"]["title_idle_px"] = title_idle

        # ---- cheat code on the main screen (OLD-spec corpus only) -----------
        # The ORIGINAL bench prompt demanded: "cheatmode when pressing 3 times
        # 0 on the main screen (it will show an easter egg on the top right
        # corner). The cheatcode give unlimited life." The corrected 2026-08-04
        # spec has NO cheat code, so rtype3 cells are not probed — pressing
        # unspec'd keys at their title is not part of their contract.
        if "rtype3-bench" not in str(product):
            drv.snap("cheat0")
            for _ in range(3):
                page.keyboard.down("0")
                drv.adv(3, 3000)
                page.keyboard.up("0")
                drv.adv(6, 3000)
            drv.snap("cheat1")
            cheat_px = drv.delta("cheat0", "cheat1")
            facts["cheat"] = {"pressed": True,
                              "title_changed_px": cheat_px}
            save_shot(drv.grab(), "01b-title-cheat", "title-cheat", "title", vframe(),
                      note=f"after 0,0,0 on title; changed_px={cheat_px} "
                           f"(includes title ambient ~{title_idle})")
        else:
            facts["cheat"] = {"pressed": False, "title_changed_px": 0,
                              "note": "corrected spec has no cheat code"}

        # ---- activation -----------------------------------------------------
        # Hint first: the gesture rtype_review_score.py VERIFIED via opposed
        # arrow differentials. Ladder as fallback, judged by perceptual scene
        # change from the title — honest for "the camera left the title", NOT
        # proof of responsiveness (recorded as evidence only).
        box = None
        try:
            cv = page.locator("canvas").first
            cv.scroll_into_view_if_needed(timeout=3000)
            box = cv.bounding_box(timeout=3000)
        except Exception:  # noqa: BLE001
            box = None

        hint_gesture = hint.get("activated_by")
        tried: list[str] = []
        chosen, source = None, None
        if hint_gesture and hint_gesture != "none":
            apply_gesture(page, drv, hint_gesture, box)
            tried.append(hint_gesture)
            chosen, source = hint_gesture, "hint"
        elif hint_gesture == "none" and hint.get("activated"):
            chosen, source = "none", "hint"
        else:
            for g in GESTURES:
                if g == "none":
                    continue
                apply_gesture(page, drv, g, box)
                tried.append(g)
                s = drv.sig()
                if s and title_sig and sig_distance(s, title_sig) >= SCENE_CHANGE_MIN:
                    chosen, source = g, "ladder"
                    break
            else:
                chosen, source = "none", "none-worked"
        drv.adv(30, 8000)   # let play settle
        s_now = drv.sig()
        facts["activation"]["gesture"] = chosen
        facts["activation"]["source"] = source
        facts["activation"]["tried"] = tried
        facts["activation"]["scene_change_frac"] = (
            round(sig_distance(s_now, title_sig), 3) if s_now and title_sig else None)
        # responsiveness EVIDENCE (not a gate — a scrolling world self-changes)
        facts["activation"]["probe_up_px"] = drv.probe_key("ArrowUp", 18)
        save_shot(drv.grab(), "02-started", "started", "started", vframe(),
                  note=f"after gesture {chosen} ({source}); scene moved "
                       f"{facts['activation']['scene_change_frac']} from title")

        # ---- full-page viewport (always): DOM HUD/bezel evidence ------------
        # px_sha None: page.screenshot is not part of the determinism contract
        # (fonts/compositor timing); every canvas slot is.
        dom0 = drv.dom_text()
        facts["text"]["dom_hud_text"] = dom0
        try:
            page.screenshot(path=str(outdir / "05-viewport.png"))
            facts["shots"].append({"file": "05-viewport.png", "label": "viewport",
                                   "phase": "started", "vframe": vframe(),
                                   "px_sha": None, "same_as": None,
                                   "note": "full page incl. DOM chrome/HUD"
                                           + ("" if dom0 else " (no DOM text)")})
        except Exception:  # noqa: BLE001
            pass

        # ---- idle strip -----------------------------------------------------
        make_strip(drv, outdir, facts, "60-strip-idle", "idle", hold_key=None)

        # ---- movement -------------------------------------------------------
        up_px = drv.probe_key("ArrowUp", HOLD_FRAMES)
        save_shot(drv.grab(), "10-move-up", "move-up", "movement", vframe(),
                  note=f"after held ArrowUp, changed_px={up_px}")
        dn_px = drv.probe_key("ArrowDown", HOLD_FRAMES)
        save_shot(drv.grab(), "11-move-down", "move-down", "movement", vframe(),
                  note=f"after held ArrowDown, changed_px={dn_px}")
        facts["movement"] = {"ArrowUp": up_px, "ArrowDown": dn_px}
        make_strip(drv, outdir, facts, "61-strip-move", "move:ArrowUp",
                   hold_key="ArrowUp")

        # ---- fire key -------------------------------------------------------
        # Enter/Escape/p are STATE-TOGGLE keys in this corpus (pause, resume,
        # menu). MEASURED: the behavioral scorer's hint for codex-2 is
        # fire_key='Enter'; driving the hunt with it toggled PAUSE on every
        # chunk — 'PAUSED'/'ENTER TO RESUME' drawn 3626 times, SCORE 00000
        # after two minutes, half the photographs of a pause card. The camera
        # never fires with a toggle key; it probes the real candidates instead.
        fire = hint.get("fire_key")
        fsource = "hint"
        if fire in ("Enter", "Escape", "p", "P"):
            fsource = f"probed (hint {fire!r} is a state-toggle key)"
            fire = None
        if fire in (None, "", "None"):
            fsource = fsource if fsource.startswith("probed (") else "probed"
            for k in FIRE_CANDIDATES:
                kk = " " if k == "Space" else k
                facts["fire"]["probed"][k] = drv.probe_key(kk, 8, mid=True)
            fire = max(facts["fire"]["probed"],
                       key=facts["fire"]["probed"].get, default="Space")
        facts["fire"]["key"] = fire
        facts["fire"]["source"] = fsource
        fire_k = " " if fire == "Space" else fire

        # held-fire moment: mid-hold (+4f) catches the muzzle flash, late-hold
        # (+24f) catches the sustained stream.
        page.keyboard.down(fire_k)
        drv.adv(4, 4000)
        save_shot(drv.grab(), "20-fire-mid", "fire-mid", "combat", vframe(),
                  note=f"held {fire} +4 frames")
        drv.adv(20, 6000)
        save_shot(drv.grab(), "21-fire-held", "fire-held", "combat", vframe(),
                  note=f"held {fire} +24 frames")
        page.keyboard.up(fire_k)
        drv.adv(2, 3000)
        make_strip(drv, outdir, facts, "62-strip-fire", f"fire:{fire}",
                   hold_key=fire_k)

        # ---- boss hunt: 2 virtual minutes on autopilot ----------------------
        f0 = vframe()
        hunt = facts["boss_hunt"]
        dodge = ["ArrowUp", None, "ArrowDown", None]
        i = 0
        while vframe() - f0 < HUNT_FRAMES:
            if drv.stalls >= 4:
                hunt["stalled"] = True
                break
            vk = dodge[i % 4]
            # re-press fire every chunk: tap-fire games shoot per keydown,
            # autofire games keep shooting through the hold.
            page.keyboard.up(fire_k)
            page.keyboard.down(fire_k)
            if vk:
                page.keyboard.down(vk)
            drv.adv(HUNT_CHUNK, 9000)
            if vk:
                page.keyboard.up(vk)
            elapsed = vframe() - f0
            for target, fname, label in HUNT_SHOTS:
                if elapsed >= target and not (outdir / f"{fname}.png").exists():
                    save_shot(drv.grab(), fname, label, "hunt", vframe(),
                              note=f"autopilot, {elapsed} frames after start")
            # mid-hunt strip at the first shot point: bullets landing on live
            # enemies — the only deterministic window where impact VFX can show
            if elapsed >= 1800 and not (outdir / "63-strip-hunt.png").exists():
                make_strip(drv, outdir, facts, "63-strip-hunt", f"hunt-fire:{fire}",
                           hold_key=fire_k)
            if not hunt["game_over_text_seen"] and texts_matching(r"game\s*over"):
                hunt["game_over_text_seen"] = True
            if hunt["game_over_text_seen"] and hunt["restarts"] < MAX_RESTARTS:
                # the run died — re-apply the start gesture so the camera gets
                # gameplay, not a game-over card, for the remaining shots
                page.keyboard.up(fire_k)
                apply_gesture(page, drv, chosen if chosen != "none" else "key:Enter*2", box)
                hunt["restarts"] += 1
                hunt["restart_vframes"].append(vframe())
                hunt["game_over_text_seen"] = False
            i += 1
        page.keyboard.up(fire_k)
        hunt["reached_frames"] = vframe() - f0
        hunt["boss_text_seen"] = texts_matching(r"\bboss\b|warning")
        # guarantee the terminal slots exist even if the loop exited early
        for target, fname, label in HUNT_SHOTS:
            if not (outdir / f"{fname}.png").exists():
                save_shot(drv.grab(), fname, label + " (early exit)", "hunt",
                          vframe(), note=f"loop ended at {hunt['reached_frames']} frames")

        # ---- final facts ----------------------------------------------------
        pal = page.evaluate("() => window.__palette(16)") or {}
        facts["palette"] = {"unique_colors": int(pal.get("unique") or 0),
                            "top": pal.get("top") or []}
        tx = drv.texts()
        facts["text"]["canvas_strings"] = sorted(tx.items(), key=lambda kv: -kv[1])[:60]
        probe = page.evaluate("() => window.__probe") or {}
        facts["audio"]["starts"] = int(probe.get("audioStarts") or 0)
        facts["errors"]["js_exceptions"] = (errs + list(probe.get("errors") or []))[:8]
        facts["errors"]["console_errors"] = console_msgs[:8]
        facts["frames_total"] = int(probe.get("frames") or 0)
        facts["frame_stalls"] = drv.stalls

    except Exception as e:  # noqa: BLE001
        facts["error"] = f"driver error: {str(e)[:300]}"
    finally:
        try:
            if context is not None:
                context.close()
        except Exception:  # noqa: BLE001
            pass
        if srv is not None:
            try:
                srv.shutdown()
            except Exception:  # noqa: BLE001
                pass

    facts["elapsed_s"] = round(time.time() - t_start, 1)

    if vlm and "error" not in facts:
        facts["vlm"] = vlm_judge(outdir, facts, vlm_model)

    write_outputs(outdir, facts)
    return facts


# --------------------------------------------------------------- VLM judge
def vlm_judge(outdir: Path, facts: dict, model: str) -> dict:
    by_file = {s["file"]: s for s in facts["shots"] + [
        {"file": st["file"], "label": st["moment"], "px_sha": st.get("px_sha")}
        for st in facts["strips"]]}
    content: list[dict] = [{"type": "text", "text": VLM_PROMPT}]
    sent: list[dict] = []
    for slot in VLM_SLOTS:
        f = f"{slot}.png"
        p = outdir / f
        meta = by_file.get(f)
        if p.is_file() and meta:
            b64 = base64.b64encode(p.read_bytes()).decode()
            content.append({"type": "text", "text": f"[slot {slot}: {meta.get('label')}]"})
            content.append({"type": "image_url",
                            "image_url": {"url": f"data:image/png;base64,{b64}"}})
            sent.append({"slot": slot, "px_sha": meta.get("px_sha"),
                         "bytes": p.stat().st_size})
        else:
            content.append({"type": "text",
                            "text": f"[slot {slot}: MISSING — phase failed]"})
            sent.append({"slot": slot, "px_sha": None, "missing": True})

    out = {"model": model, "rubric_version": VLM_RUBRIC_VERSION,
           "rubric_sha": hashlib.sha256(VLM_PROMPT.encode()).hexdigest()[:16],
           "images_sent": sent,
           "request": {"temperature": 0, "max_tokens": 3000,
                       "response_format": "json_schema"},
           "structured": None, "json": None, "usage": None, "raw": None}
    body = {"model": model, "temperature": 0, "max_tokens": 3000,
            "messages": [{"role": "user", "content": content}],
            "response_format": {"type": "json_schema",
                                "json_schema": VLM_SCHEMA}}
    fallback = {k: v for k, v in body.items() if k != "response_format"}
    for structured, attempt in ((True, body), (False, fallback)):
        req = urllib.request.Request(
            VLM_URL, data=json.dumps(attempt).encode(),
            headers={"Content-Type": "application/json",
                     "Authorization": "Bearer local"})
        try:
            with urllib.request.urlopen(req, timeout=600) as r:
                resp = json.loads(r.read())
            txt = resp["choices"][0]["message"]["content"]
            out["usage"] = resp.get("usage")
            out["raw"] = txt[:3000]
            out["structured"] = structured
            m = re.search(r"\{.*\}", txt, re.DOTALL)
            if m:
                out["json"] = json.loads(m.group(0))
            if out["json"]:
                return out
            out["error"] = "no JSON object in response"
        except Exception as e:  # noqa: BLE001
            out["error"] = str(e)[:300]
    return out


# --------------------------------------------------------------- outputs
def write_outputs(outdir: Path, facts: dict) -> None:
    (outdir / "facts.json").write_text(json.dumps(facts, indent=2))
    (outdir / "sheet.html").write_text(render_sheet(facts))


def _esc(s) -> str:
    return (str(s).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;"))


def render_sheet(facts: dict) -> str:
    slug = _esc(facts["slug"])
    rows = []
    act = facts.get("activation") or {}
    hunt = facts.get("boss_hunt") or {}
    pal = facts.get("palette") or {}

    def fact_row(k, v):
        rows.append(f"<tr><th>{_esc(k)}</th><td>{v}</td></tr>")

    fact_row("product", _esc(facts.get("product_dir")))
    fact_row("canvas", _esc(json.dumps((facts.get("canvas") or {}).get("main"))) +
             f" (css {_esc((facts.get('canvas') or {}).get('css'))})")
    fact_row("activation", f"{_esc(act.get('gesture'))} [{_esc(act.get('source'))}] — "
             f"scene moved {act.get('scene_change_frac')} from title, "
             f"ArrowUp probe {act.get('probe_up_px')} px")
    fact_row("cheat 0,0,0", _esc(json.dumps(facts.get("cheat"))))
    fact_row("fire", f"{_esc((facts.get('fire') or {}).get('key'))} "
             f"[{_esc((facts.get('fire') or {}).get('source'))}]")
    fact_row("boss hunt", f"{hunt.get('reached_frames')}/{hunt.get('target_frames')} frames, "
             f"restarts {hunt.get('restarts')}, boss text seen: {hunt.get('boss_text_seen')}, "
             f"game-over text: {hunt.get('game_over_text_seen')}, stalled: {hunt.get('stalled')}")
    fact_row("palette", f"{pal.get('unique_colors', 0)} unique colors")
    sw = "".join(f"<span class=sw style='background:{_esc(c)}' title='{_esc(c)} {f}'></span>"
                 for c, f in (pal.get("top") or [])[:16])
    fact_row("top colors", sw)
    fact_row("audio starts", (facts.get("audio") or {}).get("starts", 0))
    errs = (facts.get("errors") or {}).get("js_exceptions") or []
    fact_row("js exceptions", _esc("; ".join(errs)) or "none")
    if facts.get("error"):
        fact_row("REVIEW ERROR", f"<b style='color:#f66'>{_esc(facts['error'])}</b>")

    shot_cells = []
    for s in facts.get("shots") or []:
        dup = f"<span class=dup>= {_esc(s['same_as'])}</span>" if s.get("same_as") else ""
        shot_cells.append(
            f"<figure><img src='{_esc(s['file'])}' loading=lazy>"
            f"<figcaption><b>{_esc(s['label'])}</b> {dup}<br>"
            f"<small>{_esc(s.get('note') or '')} vf={s.get('vframe')}</small>"
            f"</figcaption></figure>")
    strip_cells = []
    for s in facts.get("strips") or []:
        warn = "" if s.get("distinct", 0) > 1 else " <b style='color:#fa0'>(static: 1 distinct frame)</b>"
        strip_cells.append(
            f"<figure class=strip><img src='{_esc(s['file'])}' loading=lazy>"
            f"<figcaption><b>{_esc(s['moment'])}</b> — {s.get('distinct')}/"
            f"{s.get('frames')} distinct frames{warn}</figcaption></figure>")

    texts = "".join(f"<li><code>{_esc(t)}</code> ×{n}</li>"
                    for t, n in (facts.get("text") or {}).get("canvas_strings") or [][:40])
    vlm_html = ""
    v = facts.get("vlm")
    if v:
        j = v.get("json") or {}
        axes = "".join(f"<tr><th>{_esc(a)}</th><td>{_esc(j.get(a))}</td></tr>"
                       for a in AXES + SPEC_BOOLS + SPEC_NULLABLE)
        vlm_html = (f"<h2 id=vlm>VLM judge ({_esc(v.get('model'))}, rubric "
                    f"{_esc(v.get('rubric_version'))}, sha {_esc(v.get('rubric_sha'))}, "
                    f"structured={_esc(v.get('structured'))})</h2>"
                    f"<table>{axes}</table>"
                    f"<p><b>bullet note:</b> {_esc(j.get('bullet_salience_note'))}</p>"
                    f"<p><b>orb note:</b> {_esc(j.get('orb_note'))} — "
                    f"<b>item note:</b> {_esc(j.get('item_drops_note'))}</p>"
                    f"<p><b>verdict:</b> {_esc(j.get('verdict'))}</p>"
                    f"<details><summary>usage/raw</summary><pre>"
                    f"{_esc(json.dumps({'usage': v.get('usage'), 'error': v.get('error')}, indent=1))}"
                    f"</pre></details>")

    return f"""<!doctype html><meta charset=utf-8>
<title>rtype visual review — {slug}</title>
<style>
 body{{background:#111;color:#ddd;font:14px/1.5 system-ui,sans-serif;margin:20px;max-width:1400px}}
 h1{{font-size:20px}} h2{{font-size:16px;border-bottom:1px solid #333;padding-bottom:4px}}
 table{{border-collapse:collapse}} th,td{{text-align:left;padding:3px 10px;border-bottom:1px solid #222;vertical-align:top}}
 th{{color:#9ab;white-space:nowrap}}
 .grid{{display:flex;flex-wrap:wrap;gap:12px}}
 figure{{margin:0;background:#1a1a1a;padding:6px;border-radius:6px}}
 figure img{{image-rendering:pixelated;max-width:380px;display:block}}
 figure.strip img{{max-width:100%}}
 figcaption{{font-size:12px;color:#aaa;max-width:380px}}
 .dup{{color:#fa0;font-size:11px}}
 .sw{{display:inline-block;width:18px;height:18px;border:1px solid #444;margin-right:2px;vertical-align:middle}}
 code{{color:#8c8}} li{{font-size:12px}}
 details pre{{font-size:11px;background:#181818;padding:8px;overflow:auto}}
</style>
<h1 id=meta>rtype visual review — {slug}</h1>
<table>{''.join(rows)}</table>
<h2 id=shots>Shots</h2><div class=grid>{''.join(shot_cells) or '<i>none captured</i>'}</div>
<h2 id=strips>Film strips</h2>
<div>{''.join(strip_cells) or '<i>none captured</i>'}</div>
{vlm_html}
<h2 id=facts>Drawn text (canvas fillText/strokeText)</h2>
<ul>{texts or '<i>none observed</i>'}</ul>
<details><summary>facts.json</summary><pre>{_esc(json.dumps(facts, indent=2))}</pre></details>
"""


# --------------------------------------------------------------- aggregation
ARM_LABELS = {  # bench-root -> presentation label suffix (results-page names)
    "rtype-bench": "",
    "rtype-medium2": "",
    "rtype-flows2": " (flows2 experiment)",
    "rtype3-bench": " [rtype3]",
}


def arm_of(slug: str) -> str:
    root, _, prod = slug.partition("--")
    arm = re.sub(r"-\d+-product$", "", prod)
    return arm + ARM_LABELS.get(root, f" ({root})")


def aggregate(out_root: Path) -> dict:
    rows = []
    for f in sorted(out_root.glob("*/facts.json")):
        facts = json.loads(f.read_text())
        v = (facts.get("vlm") or {}).get("json") or {}
        rows.append({"slug": facts["slug"], "arm": arm_of(facts["slug"]),
                     "vlm": v, "boss_hunt": facts.get("boss_hunt"),
                     "cheat": facts.get("cheat"),
                     "palette_colors": (facts.get("palette") or {}).get("unique_colors"),
                     "verdict": v.get("verdict"),
                     "bullet_note": v.get("bullet_salience_note")})
    arms: dict[str, dict] = {}
    for r in rows:
        if not r["vlm"]:
            continue
        a = arms.setdefault(r["arm"], {k: [] for k in AXES})
        for k in AXES:
            if isinstance(r["vlm"].get(k), (int, float)):
                a[k].append(r["vlm"][k])
    spec: dict[str, dict] = {}
    for r in rows:
        if not r["vlm"]:
            continue
        s = spec.setdefault(r["arm"], {k: [] for k in SPEC_BOOLS + SPEC_NULLABLE})
        for k in SPEC_BOOLS + SPEC_NULLABLE:
            s[k].append(r["vlm"].get(k))
    table = {}
    for arm, vals in sorted(arms.items()):
        table[arm] = {}
        for k in AXES:
            if vals[k]:
                table[arm][k] = round(statistics.mean(vals[k]), 2)
                table[arm][k + "_sd"] = round(statistics.pstdev(vals[k]), 2)
        judged = [v for v in (vals[AXES[0]] or [])]
        table[arm]["n"] = len(judged)
        sv = spec.get(arm) or {}
        for k in SPEC_BOOLS:
            bs = [b for b in sv.get(k, []) if isinstance(b, bool)]
            table[arm][k] = f"{sum(bs)}/{len(bs)}" if bs else "—"
        for k in SPEC_NULLABLE:
            nums = [x for x in sv.get(k, []) if isinstance(x, (int, float))
                    and not isinstance(x, bool)]
            trues = [x for x in sv.get(k, []) if x is True]
            nulls = [x for x in sv.get(k, []) if x is None]
            if k == "level_variety":
                table[arm][k] = (f"{round(statistics.mean(nums), 1)} (n={len(nums)}, "
                                 f"unknown={len(nulls)})") if nums else f"unknown x{len(nulls)}"
            else:
                table[arm][k] = f"{len(trues)} seen, {len(nulls)} unknown"
        per_product_means = []
        for r in rows:
            if r["arm"] == arm and r["vlm"]:
                nums = [r["vlm"][k] for k in AXES if isinstance(r["vlm"].get(k), (int, float))]
                if nums:
                    per_product_means.append(statistics.mean(nums))
        if per_product_means:
            table[arm]["visual_mean"] = round(statistics.mean(per_product_means), 3)
    summary = {"schema": "rtype-visual-summary/1",
               "generated_utc": _dt.datetime.now(_dt.timezone.utc).isoformat(timespec="seconds"),
               "rubric_version": VLM_RUBRIC_VERSION,
               "axes": AXES, "per_arm": table, "products": rows}
    (out_root / "summary.json").write_text(json.dumps(summary, indent=2))

    head = "".join(f"<th class=num>{a.replace('_', ' ')}</th>" for a in AXES)
    spec_head = "".join(f"<th class=num>{a.replace('_', ' ')}</th>"
                        for a in SPEC_BOOLS + SPEC_NULLABLE)
    body_rows = []
    order = sorted(table, key=lambda a: -(table[a].get("visual_mean") or 0))
    for armname in order:
        t = table[armname]
        tds = "".join(f"<td class=num>{t.get(a, '—')}<small> ±{t.get(a + '_sd', 0)}</small></td>"
                      for a in AXES)
        spec_tds = "".join(f"<td class=num>{_esc(t.get(a, '—'))}</td>"
                           for a in SPEC_BOOLS + SPEC_NULLABLE)
        body_rows.append(f"<tr><td>{_esc(armname)}</td><td class=num>{t.get('n')}</td>{tds}"
                         f"<td class=num><b>{t.get('visual_mean', '—')}</b></td>{spec_tds}</tr>")
    prods = []
    for r in sorted(rows, key=lambda x: (x["arm"], x["slug"])):
        v = r["vlm"] or {}
        tds = "".join(f"<td class=num>{v.get(a, '—')}</td>" for a in AXES)
        spec_tds = "".join(
            f"<td class=num>{_esc('null' if v.get(a) is None and a in v else v.get(a, '—'))}</td>"
            for a in SPEC_BOOLS + SPEC_NULLABLE)
        prods.append(f"<tr><td><a href='{_esc(r['slug'])}/sheet.html'>{_esc(r['slug'])}</a></td>"
                     f"{tds}{spec_tds}<td>{_esc(r.get('verdict') or '')}</td></tr>")
    (out_root / "summary.html").write_text(f"""<!doctype html><meta charset=utf-8>
<title>rtype visual review — summary</title>
<style>body{{background:#111;color:#ddd;font:14px/1.5 system-ui;margin:20px;max-width:1500px}}
table{{border-collapse:collapse;margin:12px 0}}th,td{{text-align:left;padding:3px 9px;border-bottom:1px solid #262626}}
th{{color:#9ab}}td.num,th.num{{text-align:right}}small{{color:#777}}a{{color:#8ab4ff}}</style>
<h1>R-Type visual review — VLM rubric {VLM_RUBRIC_VERSION} (gpt-5.4, temp 0)</h1>
<p>Scores 1-5 per axis, mean ± population SD over reps. Mechanics are NOT graded here —
this table is the looks-only complement to the behavioral score.</p>
<table><tr><th>arm</th><th class=num>n</th>{head}<th class=num>visual mean</th>{spec_head}</tr>
{''.join(body_rows)}</table>
<h2>Per product</h2>
<table><tr><th>product</th>{head}{spec_head}<th>verdict</th></tr>{''.join(prods)}</table>
""")
    return summary


# --------------------------------------------------------------- main
def slug_for(product: Path) -> str:
    p = product.resolve()
    try:
        rel = p.relative_to(REPO / "untracked")
        return "--".join(rel.parts)
    except ValueError:
        return p.name


def guard_out(out: Path) -> Path:
    out = out.resolve()
    allowed = (REPO / "untracked" / "rtype-visual").resolve()
    try:
        out.relative_to(REPO)
        inside_repo = True
    except ValueError:
        inside_repo = False
    if inside_repo and not (out == allowed or str(out).startswith(str(allowed) + "/")):
        raise SystemExit(f"refusing to write to {out}: inside the repo but not "
                         f"under {allowed}")
    return out


# The 30-product OLD corpus: rtype-bench contributes only the arms the merged
# benchmark reused from it (codex/opencode/pi); medium2 and flows2 entirely.
CORPUS = (("untracked/rtype-bench", ("codex-", "opencode-", "pi-")),
          ("untracked/rtype-medium2", None),
          ("untracked/rtype-flows2", None))
# The corrected-prompt corpus (2026-08-04 rerun), reviewed with --rtype3 as
# cells land. A product dir is only picked up once its bench run has finished
# writing (presence in runs.json), so a mid-write cell is never photographed.
RTYPE3_ROOT = "untracked/rtype3-bench"


def rtype3_products() -> list[Path]:
    root = REPO / RTYPE3_ROOT
    runs = root / "runs.json"
    done: set[str] = set()
    if runs.is_file():
        try:
            for r in json.loads(runs.read_text()).get("runs", []):
                d = r.get("out_dir") or r.get("archived_product") or ""
                if d:
                    done.add(Path(d).name)
        except Exception:  # noqa: BLE001
            pass
    out = []
    for p in sorted(root.glob("*-product")):
        if not done or p.name in done:
            out.append(p)
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("products", nargs="*", type=Path)
    ap.add_argument("--all", action="store_true",
                    help="review the 30-product OLD benchmark corpus")
    ap.add_argument("--rtype3", action="store_true",
                    help="review finished cells of the corrected-prompt "
                         "rtype3-bench corpus (skips ones already reviewed "
                         "unless --redo)")
    ap.add_argument("--redo", action="store_true",
                    help="with --rtype3: re-review cells that already have facts")
    ap.add_argument("--out", type=Path, default=REPO / "untracked" / "rtype-visual")
    ap.add_argument("--hints", type=Path,
                    default=REPO / "untracked" / "rtype-visual" / "activation_hints.json")
    ap.add_argument("--vlm", action="store_true", help="run the VLM judge (quota!)")
    ap.add_argument("--vlm-model", default="gpt-5.4")
    ap.add_argument("--vlm-only", action="store_true",
                    help="re-run ONLY the VLM judge on existing shots (no browser)")
    ap.add_argument("--aggregate", action="store_true",
                    help="rebuild summary.json/summary.html from existing facts and exit")
    a = ap.parse_args()

    out_root = guard_out(a.out)
    out_root.mkdir(parents=True, exist_ok=True)

    if a.aggregate:
        s = aggregate(out_root)
        print(json.dumps(s["per_arm"], indent=1))
        return 0

    products: list[Path] = []
    if a.all:
        for root, prefixes in CORPUS:
            for p in sorted((REPO / root).glob("*-product")):
                if prefixes and not p.name.startswith(prefixes):
                    continue
                products.append(p)
    if a.rtype3:
        for p in rtype3_products():
            if not a.redo and (out_root / slug_for(p) / "facts.json").is_file():
                continue
            products.append(p)
    for p in a.products:
        products.append(p if p.is_absolute() else REPO / p)
    products = [p for p in products if p.is_dir()]
    if not products:
        ap.error("no product dirs given (pass paths or --all)")

    hints = {}
    if a.hints and a.hints.is_file():
        hints = json.loads(a.hints.read_text())

    if a.vlm_only:
        for prod in products:
            outdir = out_root / slug_for(prod)
            fp = outdir / "facts.json"
            if not fp.is_file():
                print(f"{slug_for(prod)}: no facts.json — run the capture first")
                continue
            facts = json.loads(fp.read_text())
            facts["vlm"] = vlm_judge(outdir, facts, a.vlm_model)
            write_outputs(outdir, facts)
            j = (facts["vlm"] or {}).get("json") or {}
            print(f"{slug_for(prod):44} vlm={'ok' if j else 'FAIL'} "
                  f"{json.dumps({k: j.get(k) for k in AXES}) if j else facts['vlm'].get('error')}",
                  flush=True)
        aggregate(out_root)
        return 0

    from playwright.sync_api import sync_playwright
    with sync_playwright() as pw:
        browser = pw.chromium.launch(
            channel=CHROME_CHANNEL, headless=True,
            args=["--autoplay-policy=no-user-gesture-required", "--mute-audio",
                  "--force-device-scale-factor=1", "--disable-lcd-text"])
        try:
            for prod in products:
                slug = slug_for(prod)
                outdir = out_root / slug
                t0 = time.time()
                facts = review_one(browser, prod, outdir, hints, a.vlm, a.vlm_model)
                hunt = facts.get("boss_hunt") or {}
                print(f"{slug:46} {time.time() - t0:6.1f}s "
                      f"shots={len(facts.get('shots') or [])} "
                      f"strips={len(facts.get('strips') or [])} "
                      f"hunt={hunt.get('reached_frames', 0)}f "
                      f"restarts={hunt.get('restarts', 0)} "
                      f"{'vlm=ok' if (facts.get('vlm') or {}).get('json') else ''} "
                      f"{'ERROR: ' + facts['error'] if facts.get('error') else ''}",
                      flush=True)
        finally:
            browser.close()
    aggregate(out_root)
    print(f"\nwrote {out_root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
