#!/usr/bin/env python3
"""Score the review A/B artifacts. Frozen rubric — hash it before scoring.

Scores the PRODUCT, never the agent's own claims. The final answer and README
are deliberately ignored: a premature-completion agent writes a confident
summary of work it did not do, so believing prose is how the measurement gets
captured by the thing it is measuring.

Three tiers, deliberately ordered by how hard they are to fake:

  Tier 0 — VALIDITY GATES (binary, disqualifying). Does it load at all?
  Tier 1 — BEHAVIORAL, 70%. Drive the real page in Chrome and observe.
           These cost real implementation work to pass and are the only
           checks that distinguish a playable game from a plausible one.
  Tier 2 — CONTENT BREADTH on source, 30%. Cheap, and PARTLY GAMEABLE —
           flagged per check. Never weighted above the behavioral tier.

Explicitly NON-SCORING (reported only): file count, total bytes. A working
verifier plausibly yields fewer, better bytes; weighting size would score
verbosity as quality.

Empirically demonstrated gaming risk: a pre-existing, never-verified game.js
passed every string-presence grep (`requestAnimationFrame`, `AudioContext`,
`ArrowUp`, zero TODOs). Presence greps alone cannot tell a game from a
scaffold, which is exactly why Tier 1 exists.

DETERMINISM CONTRACT (Tier 1). A score has to be a property of the artifact,
not of the machine that measured it. On real 30-40 KB artifacts this scorer
used to return different behavioral booleans on consecutive invocations of the
same unchanged file. Four things now hold the reading still, and every one of
them was added because it was measured breaking, not because it seemed prudent:

  * the page's RNG is SEEDED, so the artifact is one game and not a sample from
    a distribution of procedurally generated ones;
  * the page's CLOCK is virtual and advances one fixed step per frame, so
    dt-integrated movement does not depend on machine load;
  * the driver SINGLE-STEPS the animation loop, so "hold a key for 24 frames"
    is exactly 24 frames and the world holds still while it is being sampled;
  * input is HELD (down, N frames, up) rather than tapped, because every game
    here reads a held-keys map and a tap is invisible to it.

Anything that could not be made to hold still was DELETED rather than shipped
noisy — see the note on the reversibility signal in score_one().

Usage:
  python3 scripts/zelda_review_score.py
  python3 scripts/zelda_review_score.py --root untracked/zelda-ab
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
import threading
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
CHROME_CHANNEL = "chrome"

# Instrumentation injected BEFORE any game script runs. Counts what the page
# actually does rather than what its source mentions, and — just as important —
# REMOVES the two things that made the same artifact score differently on two
# consecutive invocations: an unseeded RNG and a wall-clock-driven integrator.
INSTRUMENT = r"""
(function () {
  const RAW_RAF = window.requestAnimationFrame.bind(window);
  const REAL_NOW = performance.now.bind(performance);
  const probe = {raf: 0, frames: 0, paintFrames: 0, heldFrames: 0, draws: 0,
                 ticks: 0, audioStarts: 0, keyListeners: 0, keyTargets: [],
                 errors: []};
  window.__probe = probe;

  // ---- 1. Deterministic RNG ------------------------------------------------
  // MEASURED: several of the real artifacts build their maps with Math.random()
  // at load (`if (Math.random() < 0.08) tiles[y][x] = 2`). The same file is
  // therefore a DISTRIBUTION over games, not a game: on one load the player
  // spawns in open ground and a scripted walk crosses two screens, on the next
  // it spawns boxed in by trees and nothing moves. Seeding turns the artifact
  // back into a fixed object so that a score is a property of the code.
  let _s = 0x9e3779b9 >>> 0;
  Math.random = function () {
    _s ^= (_s << 13); _s >>>= 0;
    _s ^= (_s >>> 17);
    _s ^= (_s << 5);  _s >>>= 0;
    return _s / 4294967296;
  };

  // ---- 2. Virtual clock ----------------------------------------------------
  // Every game in this corpus integrates as `dt = (now - last) / 1000`, so
  // distance travelled per frame is a function of WALL-CLOCK time. Waiting in
  // frames (the previous fix) removed the load-dependence of HOW MANY frames
  // ran but not of how far each one moved: under load a frame takes 40 ms, dt
  // clamps at the game's own ceiling, and the identical scripted walk covers a
  // different distance. Pinning the clock to the frame counter makes the walk a
  // fixed number of game-world steps.
  //
  // Fallback rule: the clock only goes virtual once the page has shown it is
  // driven by rAF or by setInterval. A page looping on setTimeout would
  // otherwise freeze at dt=0, which is a worse failure than jitter.
  let vt = 0;
  const STEP = 1000 / 60;
  const T0 = REAL_NOW();
  const virt = () => (probe.frames || probe.ticks) ? vt : (REAL_NOW() - T0);
  performance.now = function () { return virt(); };
  Date.now = function () { return 1767225600000 + Math.round(virt()); };

  // ---- 2b. Single-stepping -------------------------------------------------
  // The driver hands out animation frames one budget at a time instead of
  // letting the game run while it talks to it over CDP. Without this, "hold
  // ArrowRight for 24 frames" really meant "hold it for 24 frames plus however
  // many elapse between the wait finishing and the keyup round-trip landing",
  // which is 1-3 frames of jitter on every single input and left the player at
  // a slightly different place on every run. Between budgets the callback is
  // withheld and re-armed on the pristine rAF, so the loop stays alive and the
  // page is simply not advancing — which also means every canvas snapshot is
  // taken from a world that is holding still.
  //
  // Caveat: the id returned to the game is the id of the first raw frame, so a
  // game that cancelAnimationFrame()s its own loop mid-pause would not be
  // cancelled. Nothing in this corpus does; the alternative (letting the world
  // drift during measurement) is the worse trade.
  let budget = Infinity;
  window.requestAnimationFrame = function (cb) {
    probe.raf++;
    return RAW_RAF(function tick() {
      if (budget <= 0) { probe.heldFrames++; return RAW_RAF(tick); }
      budget--;
      vt += STEP;
      probe.frames++;
      const before = probe.draws;
      try { return cb(vt); } finally { if (probe.draws > before) probe.paintFrames++; }
    });
  };
  // A setInterval-driven loop is a perfectly good game loop; requiring rAF
  // alone failed it outright. It only drives the virtual clock when nothing
  // else does, so a music timer in a rAF game cannot jerk dt around.
  const si = window.setInterval;
  window.setInterval = function (fn, ms, ...rest) {
    const wrapped = typeof fn === 'function'
      ? function (...a) {
          probe.ticks++;
          if (probe.frames === 0) vt += (ms || 0);
          return fn.apply(this, a);
        } : fn;
    return si.call(window, wrapped, ms, ...rest);
  };

  // ---- 3. Frame gate the driver can trust ----------------------------------
  // THE bug that made this scorer report fiction: the old wait used Playwright's
  // `wait_for_function`, whose DEFAULT polling mode is `requestAnimationFrame` —
  // the very function the probe had monkey-patched to count frames. The waiter
  // therefore satisfied its own condition. Demonstrated on a blank page with no
  // loop whatsoever: `__probe.raf` went 0 -> 30 and the wait returned True. So
  // (a) every `advance_frames` call succeeded on every page, dead or alive,
  // (b) `raf_count >= 30` — the entire "the loop runs" test — was ALWAYS true,
  // and (c) on a live game the counter was driven by the game and the poller in
  // an interleaving that varied run to run, so "advance 30 frames" advanced an
  // unpredictable number of GAME frames. This waiter uses the pristine rAF
  // captured before patching, so it never touches the counters it observes.
  //
  // A page whose loop is setInterval-driven has no frames to budget; it is
  // waited on by tick count and left to run. (Defensive: nothing in the current
  // corpus loops on setInterval alone.)
  // Each wait carries a generation stamp and clears its own backstop timer.
  // Without that, a previous wait's still-pending backstop fired in the middle
  // of the NEXT wait and zeroed its frame budget — the page froze mid-window and
  // the wait timed out. Symptom: 26 spurious "frame stall"s on a fixture whose
  // loop was demonstrably running, and a 46 s scoring run.
  let gen = 0;
  window.__waitFrames = function (n, timeoutMs) {
    return new Promise(function (res) {
      const myGen = ++gen;
      const useTicks = (probe.frames === 0 && probe.ticks > 0);
      const start = useTicks ? probe.ticks : probe.frames;
      const done = () => ((useTicks ? probe.ticks : probe.frames) - start) >= n;
      let settled = false;
      const finish = (v) => {
        if (settled) return;
        settled = true;
        clearTimeout(to);
        if (!useTicks && gen === myGen) budget = 0;
        res(v);
      };
      const to = setTimeout(function () { finish(done()); }, timeoutMs + 250);
      if (!useTicks) budget = n;
      const t0 = REAL_NOW();
      (function chk() {
        if (settled) return;
        if (done()) return finish(true);
        if (REAL_NOW() - t0 > timeoutMs) return finish(false);
        RAW_RAF(chk);
      })();
    });
  };

  // ---- 4. Canvas sampling, computed in-page --------------------------------
  // Compares EVERY pixel of every canvas, per pixel rather than per byte. The
  // old sampler strode the RGBA byte array by 7, which rotates through colour
  // channels and yielded 205k values for a 600x600 canvas but 33k for a 240x240
  // one — so the fixed threshold `energy > 50` was a 6x stricter test on the
  // small canvases, which is the size the real artifacts actually use.
  // The result is an absolute COUNT of changed pixels, not a fraction, for the
  // same reason: the same 16x16 sprite is 0.44% of a 240x240 canvas and 0.07%
  // of a 600x600 one, and this corpus contains both.
  // Everything is reduced in-page. Shipping 200k-element arrays over CDP for
  // every window was most of the old runtime.
  window.__slots = {};
  window.__snap = function (slot) {
    const acc = []; let err = null;
    for (const c of document.querySelectorAll('canvas')) {
      let d;
      try { d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data; }
      catch (e) { err = String(e.message); continue; }
      const npx = c.width * c.height;
      const v = new Int16Array(npx);
      for (let px = 0; px < npx; px++) {
        const i = px * 4;
        v[px] = (d[i] + d[i + 1] + d[i + 2]) >> 2;   // 0..191, fits Int16
      }
      acc.push(v);
    }
    window.__slots[slot] = acc;
    return {n: acc.reduce((s, v) => s + v.length, 0), canvases: acc.length, err: err};
  };
  window.__delta = function (a, b) {
    const X = window.__slots[a] || [], Y = window.__slots[b] || [];
    let ch = 0, sum = 0, n = 0;
    for (let k = 0; k < Math.min(X.length, Y.length); k++) {
      const x = X[k], y = Y[k], m = Math.min(x.length, y.length);
      n += m;
      for (let i = 0; i < m; i++) {
        const d = Math.abs(x[i] - y[i]);
        sum += d;
        if (d > 3) ch++;          // ~12/255 of full-scale brightness
      }
    }
    return {n: n, changed_px: ch, sum: sum};
  };
  window.__hash = function () {
    const s = window.__snap('_h');
    if (!s.n) return s.err ? 'ERR:' + s.err : null;
    let h = 2166136261;
    for (const v of window.__slots['_h']) {
      // A prime stride keeps the hash cheap without aliasing to a single
      // scanline the way a power-of-two stride would on a square canvas.
      for (let i = 0; i < v.length; i += 97) { h ^= (v[i] & 255); h = Math.imul(h, 16777619) >>> 0; }
    }
    return String(h);
  };

  // ---- 5. Everything else --------------------------------------------------
  for (const m of ['fillRect', 'drawImage', 'fillText', 'strokeRect', 'stroke',
                   'fill', 'putImageData', 'clearRect']) {
    const o = CanvasRenderingContext2D.prototype[m];
    if (!o) continue;
    CanvasRenderingContext2D.prototype[m] = function (...a) {
      probe.draws++; return o.apply(this, a);
    };
  }
  // Count audio actually SCHEDULED, not merely constructed.
  for (const name of ['AudioContext', 'webkitAudioContext']) {
    const C = window[name];
    if (!C) continue;
    const W = function (...a) {
      const ctx = new C(...a);
      for (const fn of ['createOscillator', 'createBufferSource']) {
        if (typeof ctx[fn] !== 'function') continue;
        const orig = ctx[fn].bind(ctx);
        ctx[fn] = function (...b) {
          const node = orig(...b);
          if (node && typeof node.start === 'function') {
            const s = node.start.bind(node);
            node.start = function (...c) { probe.audioStarts++; return s(...c); };
          }
          return node;
        };
      }
      return ctx;
    };
    W.prototype = C.prototype;
    window[name] = W;
  }
  // An <audio> element is a legitimate way to ship sound.
  if (window.HTMLMediaElement) {
    const pl = HTMLMediaElement.prototype.play;
    HTMLMediaElement.prototype.play = function (...a) {
      probe.audioStarts++; return pl.apply(this, a);
    };
  }
  const add = EventTarget.prototype.addEventListener;
  EventTarget.prototype.addEventListener = function (t, ...rest) {
    if (t === 'keydown' || t === 'keyup') {
      probe.keyListeners++;
      probe.keyTargets.push(this === window ? 'window' : this === document
        ? 'document' : (this.tagName || 'other'));
    }
    return add.call(this, t, ...rest);
  };
  window.addEventListener('error', (e) => probe.errors.push(String(e.message)));
  window.addEventListener('unhandledrejection', (e) => probe.errors.push('rejection: ' + e.reason));
})();
"""

# Response threshold, in CHANGED PIXELS. Set from measurement, not taste: an
# idle window on every real artifact in this corpus measures exactly 0 changed
# pixels (these renderers are deterministic, so there is no noise floor to clear
# — only a signal floor), while a held arrow moves hundreds to hundreds of
# thousands. 120 px is roughly "an 11x11 sprite moved", comfortably below the
# smallest real response and far above anything a still page produces.
CHANGED_FLOOR = 120
IDLE_DOMINANCE = 3.0
# How long a key is HELD, in frames. These games all read a held-keys map that
# is set on keydown and cleared on keyup; `keyboard.press()` sends both with no
# frame in between, so the game only ever saw the key if a frame happened to
# land inside that microsecond. See drive() for the measurement.
HOLD_FRAMES = 24
WINDOW_FRAMES = 24


def serve_dir(d: Path):
    """Serve `d` over http://127.0.0.1:<port>. Returns (server, port).

    `file://` is not a usable substitute: Chrome blocks ES-module scripts under
    it as a CORS violation (`net::ERR_FAILED`), blocks `fetch` of local JSON,
    and taints the canvas as soon as a local image is drawn — after which
    `getImageData` throws and every behavioral check dies. Measured: a modular
    game scored 0.275 under file:// and passed cleanly over HTTP. A real
    Zelda-scale product is very likely to be modular, so file:// would have
    penalised exactly the more sophisticated artifact.
    """
    handler = partial(QuietHandler, directory=str(d.resolve()))
    srv = ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, srv.server_address[1]


class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, *a):  # noqa: D102 - silence per-request logging
        pass

    def do_GET(self):  # noqa: N802
        # The browser requests /favicon.ico unprompted; a 404 for it would be
        # counted as a page error against the game.
        if self.path == "/favicon.ico":
            self.send_response(200)
            self.send_header("Content-Type", "image/x-icon")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        return super().do_GET()


def sh(*a: str, cwd: Path | None = None) -> tuple[int, str]:
    p = subprocess.run(a, cwd=str(cwd) if cwd else None,
                       capture_output=True, text=True, timeout=180)
    return p.returncode, (p.stdout or "") + (p.stderr or "")


# ---------------------------------------------------------------- Tier 0
def tier0(d: Path) -> dict:
    r: dict = {}
    # Shared with tier1 via resolve_entry(): a product nested one directory down
    # is a layout choice, not an invalid game, but the two tiers used to resolve
    # it separately and could disagree — tier0 accepted the nested entry and
    # tier1 then reported "no index.html" and scored all-zeros behaviourally.
    entry, d = resolve_entry(d)
    r["G0_single_entry"] = entry.is_file()
    r["entry"] = str(entry)
    html = entry.read_text(errors="replace") if entry.is_file() else ""

    # Only SCRIPT refs gate validity. A missing <link rel=icon> is cosmetic;
    # counting it produced SCORE=0.0 on otherwise perfect games.
    refs = re.findall(r'<script[^>]*src=["\']([^"\']+)["\']', html)
    local = [x for x in refs if not re.match(r"^(?:https?:)?//|^data:", x)]
    missing = [x for x in local if not (d / x.split("?")[0].lstrip("/")).is_file()]
    r["G0_refs_resolve"] = not missing
    r["G0_missing_refs"] = missing

    # Only files REACHABLE from index.html gate validity. `rglob("*.js")` meant
    # one stray non-parsing helper anywhere in the tree hard-zeroed the product.
    reachable = [d / x.split("?")[0].lstrip("/") for x in local]
    js = sorted({q for q in reachable if q.suffix == ".js" and q.is_file()})
    if not js:
        js = sorted(d.glob("*.js"))
    bad = []
    for f in js:
        code, out = sh("node", "--check", str(f))
        if code != 0:
            code2, _ = sh("node", "--input-type=module", "--check", str(f))
            if code2 != 0:
                bad.append(f"{f.name}: {out.strip().splitlines()[0][:120] if out.strip() else 'syntax error'}")
    r["G1_js_parses"] = not bad
    r["G1_parse_errors"] = bad
    r["js_files"] = len(js)
    return r


def advance_frames(page, n: int, timeout_ms: int = 10000) -> bool:
    """Block until the page has rendered `n` more animation frames.

    Wall-clock waits made this scorer load-dependent: the SAME artifact scored
    0.786 on an idle machine and 0.51 under load average 6.9. So the probe waits
    for FRAMES instead of seconds — but it must do so with a waiter that is not
    itself an animation-frame consumer (see INSTRUMENT note 3).

    Returns False on a stall, and every caller records it. The previous version
    discarded the return value, so a page whose loop had stopped was measured as
    if the frames had happened, silently and with no trace in the output.
    """
    try:
        return bool(page.evaluate("a => window.__waitFrames(a[0], a[1])", [n, timeout_ms]))
    except Exception:  # noqa: BLE001
        return False


def resolve_entry(d: Path) -> tuple[Path, Path]:
    """(entry_html, serve_root). Shared by tier0 and tier1 so they never
    disagree about which file is the product — tier0 accepted a nested
    index.html and tier1 then reported 'no index.html' and scored zeros."""
    entry = d / "index.html"
    if not entry.is_file():
        cands = sorted(d.rglob("index.html"), key=lambda q: len(q.relative_to(d).parts))
        if cands:
            entry = cands[0]
    return entry.resolve(), entry.resolve().parent


# ---------------------------------------------------------------- Tier 1
def tier1(d: Path) -> dict:
    """Drive the real page. Every check is an OBSERVATION, not a grep."""
    from playwright.sync_api import sync_playwright

    res: dict = {
        "B0_loads_without_exception": False, "B1_frames_advance": False,
        "B2_responds_to_arrows": False, "B3_distinct_screens": 0,
        "B4_audio_scheduled": False,
        "raf_count": 0, "frames": 0, "draws": 0, "ticks": 0,
        "audio_starts": 0, "key_listeners": 0, "frame_stalls": 0,
        "activated_by": None, "canvas": None,
        "idle_changed": 0, "input_changed": 0,
        "console_errors": [], "notes": "",
    }
    entry, root = resolve_entry(d)
    if not entry.is_file():
        res["notes"] = "no index.html"
        return res

    srv = None
    with sync_playwright() as p:
        browser = p.chromium.launch(channel=CHROME_CHANNEL, headless=True,
                                    args=["--autoplay-policy=no-user-gesture-required",
                                          "--mute-audio", "--force-device-scale-factor=1",
                                          "--disable-lcd-text"])
        page = browser.new_page(viewport={"width": 1000, "height": 900},
                                device_scale_factor=1)
        # B0 must mean "no JS exception", not "no console noise". A single
        # console.error or a 404 sprite used to cost the same as being unplayable.
        errs: list[str] = []          # real exceptions
        console_msgs: list[str] = []  # reported, never scored
        page.on("pageerror", lambda e: errs.append(str(e)[:200]))
        page.on("console", lambda m: console_msgs.append(f"console.{m.type}: {m.text[:160]}")
                if m.type == "error" else None)
        page.add_init_script(INSTRUMENT)
        try:
            srv, port = serve_dir(root)
            page.goto(f"http://127.0.0.1:{port}/{entry.name}",
                      wait_until="load", timeout=30000)
            # Web fonts change how text rasterises; sampling before they land
            # made the first canvas hash a different picture from every later
            # one, which showed up as a spurious extra "screen".
            try:
                page.evaluate("() => document.fonts ? document.fonts.ready.then(() => true) : true")
            except Exception:  # noqa: BLE001
                pass
            # ---- helpers ----------------------------------------------------
            stalls = [0]

            def adv(n: int, timeout_ms: int = 8000) -> None:
                # Once the page has repeatedly failed to produce frames it is not
                # going to start, and paying the full timeout for each of the
                # ~50 remaining waits took a page that throws on load from 8 s
                # to 376 s. Keep measuring it — just stop waiting for it.
                t = timeout_ms if stalls[0] < 1 else (300 if stalls[0] < 4 else 0)
                if not advance_frames(page, n, t):
                    stalls[0] += 1

            adv(45, 12000)

            def snap(slot: str) -> None:
                page.evaluate("s => window.__snap(s)", slot)

            def delta(a: str, b: str) -> int:
                r = page.evaluate("ab => window.__delta(ab[0], ab[1])", [a, b]) or {}
                return int(r.get("changed_px") or 0)

            def hold(key: str, frames: int = HOLD_FRAMES) -> None:
                """Press and HOLD, not tap.

                MEASURED: `keyboard.press()` emits keydown and keyup with
                nothing in between. Every game in this corpus reads a held-keys
                map (`keys[e.key] = true` / `= false`), so the game only ever
                observed the key if an animation frame happened to be dispatched
                inside that gap — a coin flip, and the single largest source of
                run-to-run disagreement. Tapping ArrowRight 30 times moved the
                player exactly 0 pixels on all three artifacts tested; holding it
                for 30 frames moved it every time.
                """
                page.keyboard.down(key)
                adv(frames, 6000)
                page.keyboard.up(key)
                adv(2, 4000)

            def responds() -> int:
                """How many canvas pixels a held arrow changes."""
                snap("r0")
                hold("ArrowRight")
                snap("r1")
                out = delta("r0", "r1")
                hold("ArrowLeft")   # put the world back where we found it
                snap("r2")
                return max(out, delta("r1", "r2"))

            # ---- boot readings ----------------------------------------------
            probe = page.evaluate("() => window.__probe") or {}
            res["key_listeners"] = int(probe.get("keyListeners") or 0)
            res["key_targets"] = sorted(set(probe.get("keyTargets") or []))
            res["console_errors"] = console_msgs[:8]
            res["js_exceptions"] = (errs + list(probe.get("errors") or []))[:8]
            res["B0_loads_without_exception"] = not res["js_exceptions"]
            res["canvas"] = page.evaluate(
                """() => {const c = document.querySelector('canvas');
                   return c ? c.width + 'x' + c.height : null;}""")

            hashes: set = set()

            def note_hash() -> None:
                h = page.evaluate("() => window.__hash()")
                if h and not str(h).startswith("ERR"):
                    hashes.add(h)

            note_hash()

            # ---- ACTIVATION -------------------------------------------------
            # These games do not all start by themselves, and they do not all
            # start the same way. MEASURED: one holds a title overlay that only
            # SPACE or C dismisses while ENTER toggles pause — so the old probe's
            # unconditional `press("Enter")` put that game INTO a paused state
            # and then measured a frozen canvas, reporting B1=B2=false and
            # screens=1 on a fully working game. Another renders nothing at all
            # until ENTER calls begin(). So: try a short list of plausible start
            # gestures and, after each, ask the page whether arrows now do
            # anything. Stop at the first that works, so a game that is already
            # running is never poked with keys that could pause it.
            gestures = [("none", None), ("key:Space", "Space"), ("key:Enter", "Enter"),
                        ("click:canvas", None), ("key:z", "z"), ("key:c", "c"),
                        ("key:x", "x"), ("key:j", "j")]
            box = None
            try:
                cv = page.locator("canvas").first
                cv.scroll_into_view_if_needed(timeout=3000)
                box = cv.bounding_box(timeout=3000)
            except Exception:  # noqa: BLE001
                box = None
            for name, key in gestures:
                if name.startswith("key:"):
                    page.keyboard.down(key)
                    adv(3, 4000)
                    page.keyboard.up(key)
                    adv(8, 4000)
                elif name == "click:canvas":
                    # A fixed click at (450,400) missed a 240x240 canvas
                    # entirely; click the element's own centre instead.
                    if not box:
                        continue
                    page.mouse.click(box["x"] + box["width"] / 2,
                                     box["y"] + box["height"] / 2)
                    adv(8, 4000)
                note_hash()
                if responds() >= CHANGED_FLOOR:
                    res["activated_by"] = name
                    break
                note_hash()
            note_hash()

            # ---- B2: arrows move the world more than idling does -------------
            # `moved != base` passed on any autonomously animating page — an
            # animated title screen with an EMPTY keydown handler scored B2. So:
            # alternate idle and held-arrow windows of the SAME frame budget and
            # require the input windows to dominate.
            def window_changed(key: str | None) -> int:
                snap("w0")
                if key:
                    page.keyboard.down(key)
                    adv(WINDOW_FRAMES, 6000)
                    page.keyboard.up(key)
                    adv(2, 4000)
                else:
                    adv(WINDOW_FRAMES + 2, 6000)
                snap("w1")
                return delta("w0", "w1")

            idle = [window_changed(None)]
            in_r = window_changed("ArrowRight")
            note_hash()
            idle.append(window_changed(None))
            in_l = window_changed("ArrowLeft")
            note_hash()
            idle.append(window_changed(None))
            idle.append(window_changed(None))
            # MEDIAN, not max. The idle windows are meant to estimate the page's
            # autonomous animation RATE, and one transient is not a rate: a real
            # game fired a one-off scripted banner into exactly one of four idle
            # windows (0, 0, 27138, 0 changed px) and the max rule declared the
            # whole artifact unresponsive. A page that genuinely animates by
            # itself is large in EVERY idle window, so the median still catches
            # it — which is the case this control exists for.
            idle_med = sorted(idle)[len(idle) // 2]
            in_max = max(in_r, in_l)
            res["idle_windows"] = idle
            res["idle_changed"] = idle_med
            res["input_changed"] = in_max
            res["change_ratio"] = round(in_max / idle_med, 2) if idle_med > 0 else (
                999.0 if in_max >= CHANGED_FLOOR else 0.0)
            res["B2_responds_to_arrows"] = bool(
                in_max >= CHANGED_FLOOR
                and (idle_med <= 0 or in_max > IDLE_DOMINANCE * idle_med))

            # ---- B3: distinct reachable screens under a scripted walk --------
            for key in ("ArrowUp", "ArrowLeft", "ArrowDown", "ArrowRight",
                        "Enter", "Space", "z", "x"):
                hold(key, HOLD_FRAMES)
                adv(6, 4000)
                note_hash()
            res["B3_distinct_screens"] = len(hashes)

            # ---- B1: the loop runs AND the picture is not frozen -------------
            # Requiring only `raf_count >= 30` passed a static canvas with a live
            # rAF loop. Requiring the canvas to change while IDLE went too far
            # the other way: real games hold a still frame until a key is
            # pressed. What B1 must exclude is a canvas that NEVER changes at
            # all — a loop spinning over a dead renderer.
            probe = page.evaluate("() => window.__probe") or {}
            res["raf_count"] = int(probe.get("raf") or 0)
            res["frames"] = int(probe.get("frames") or 0)
            res["draws"] = int(probe.get("draws") or 0)
            res["ticks"] = int(probe.get("ticks") or 0)
            res["paint_frames"] = int(probe.get("paintFrames") or 0)
            res["js_exceptions"] = (errs + list(probe.get("errors") or []))[:8]
            res["B0_loads_without_exception"] = not res["js_exceptions"]
            looping = res["frames"] >= 60 or res["ticks"] >= 60
            res["_animates"] = len(hashes) > 1
            res["B1_frames_advance"] = bool(looping and res["_animates"])

            # ---- B4: audio genuinely SCHEDULED -------------------------------
            # Read after input: most games create the AudioContext on the first
            # gesture, and the reported number used to go stale, showing
            # audio_starts=0 beside a passing B4.
            res["audio_starts"] = int(probe.get("audioStarts") or 0)
            res["B4_audio_scheduled"] = res["audio_starts"] > 0
            res["frame_stalls"] += stalls[0]
            if res["frame_stalls"]:
                res["notes"] = f"{res['frame_stalls']} frame stall(s)"

        except Exception as e:  # noqa: BLE001
            # A driver failure must never masquerade as "the game does nothing":
            # all-zeros from a crashed probe reads identically to a dead game and
            # would silently flatten both arms to 0.
            res["notes"] = f"driver error: {str(e)[:200]}"
            res["DRIVER_FAILED"] = True
        finally:
            browser.close()
            if srv is not None:
                try:
                    srv.shutdown()
                except Exception:  # noqa: BLE001
                    pass
    return res


# ---------------------------------------------------------------- Tier 2
def tier2(d: Path) -> dict:
    src = "\n".join(p.read_text(errors="replace")
                    for p in list(d.rglob("*.js")) + list(d.rglob("*.html")))
    r: dict = {}
    # Content DEFINED and REFERENCED >=2x. Defined-only is dead content.
    # GAMEABLE (weakly): a reference inside an unreachable branch counts.
    def live_terms(pattern: str) -> int:
        names = set(re.findall(pattern, src, re.IGNORECASE))
        return sum(1 for n in names if len(re.findall(re.escape(n), src)) >= 2)

    # Quoted-literal-only regexes found ONE id each in 24 KB of real game code,
    # pinning Tier 2 near its floor for every artifact and silently collapsing
    # the rubric to Tier 1. Identifiers appear far more often as unquoted object
    # keys and variable names, so match those too.
    r["C1_live_map_ids"] = live_terms(
        r"['\"]?\b([a-z0-9_]*(?:map|room|dungeon|region|zone)[a-z0-9_]*)\b['\"]?\s*[:=]")
    r["C2_live_npc_quest_ids"] = live_terms(
        r"['\"]?\b([a-z0-9_]*(?:npc|quest|dialog|villager)[a-z0-9_]*)\b['\"]?\s*[:=]")
    r["C3_stub_markers"] = len(re.findall(r"TODO|FIXME|not implemented|placeholder|stub\b|WIP", src, re.I))

    # C4: declared functions never referenced elsewhere — the
    # premature-completion signature (code written to look complete).
    fns = set(re.findall(r"function\s+([A-Za-z_$][\w$]*)\s*\(", src))
    fns |= set(re.findall(r"(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:function|\([^)]*\)\s*=>)", src))
    orphans = [f for f in fns if len(re.findall(rf"\b{re.escape(f)}\b", src)) < 2]
    r["C4_declared_functions"] = len(fns)
    r["C4_orphan_functions"] = len(orphans)
    r["C4_orphan_fraction"] = round(len(orphans) / len(fns), 3) if fns else 0.0
    return r


def score_one(d: Path) -> dict:
    out = {"dir": str(d), **tier0(d)}
    gates_pass = out["G0_single_entry"] and out["G0_refs_resolve"] and out["G1_js_parses"]
    out["TIER0_PASS"] = gates_pass
    out.update(tier1(d) if gates_pass else {"notes": "tier-0 gates failed; not driven"})
    out.update(tier2(d))
    n, b = 0, 0
    for p in d.rglob("*"):
        if p.is_file():
            n += 1
            b += p.stat().st_size
    out["nonscoring_file_count"], out["nonscoring_total_bytes"] = n, b

    # B5_collision_blocks was REMOVED, not merely reweighted: it passed only
    # when two canvas hashes were IDENTICAL after continued input, so any game
    # with ambient animation (torch flicker, idle NPCs, water) could never
    # satisfy it while a frozen or crashed page satisfied it trivially. It
    # rewarded a static renderer and penalised the richer game — the exact
    # inversion of what this rubric is for. B3 (distinct reachable screens)
    # carries the world-interaction signal instead.
    #
    # The REVERSIBILITY signal ("drive right, drive back left, does the picture
    # return?") was also removed. It was introduced to cover dirty-flag
    # renderers, but that class turned out not to exist in this corpus — the
    # real artifacts repaint every frame and read zero idle energy simply
    # because the world is still. Measured, it was the least stable number in
    # the whole probe: the same artifact produced d_out/d_net of 3440/3440 on
    # one run and 0/0 on the next. Three checks that hold still beat five that
    # do not, so it is deleted rather than reweighted.
    behav = [out.get(k) for k in ("B0_loads_without_exception", "B1_frames_advance",
                                  "B2_responds_to_arrows", "B4_audio_scheduled")]
    out["behavioral_passed"] = sum(1 for x in behav if x)
    out["behavioral_total"] = len(behav)
    if out.get("DRIVER_FAILED"):
        # Unscoreable, not zero. Surfacing this as a score would let a harness
        # bug become an experimental finding.
        out["SCORE"] = None
    elif not gates_pass:
        out["SCORE"] = 0.0
    else:
        t1 = out["behavioral_passed"] / len(behav)
        t2 = (min(out["C1_live_map_ids"], 8) / 8 * 0.4
              + min(out["C2_live_npc_quest_ids"], 8) / 8 * 0.4
              + (1 - min(out["C4_orphan_fraction"], 0.5) / 0.5) * 0.2)
        out["SCORE"] = round(0.70 * t1 + 0.30 * t2, 4)
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--root", type=Path, default=REPO / "untracked" / "zelda-ab")
    a = ap.parse_args()
    root = a.root if a.root.is_absolute() else REPO / a.root
    runs_json = root / "runs.json"
    if not runs_json.is_file():
        print(f"no runs.json under {root} — run scripts/zelda_review_ab.py first", file=sys.stderr)
        return 2
    meta = json.loads(runs_json.read_text())

    self_sha = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    print(f"scorer sha256: {self_sha}\n")

    rows = []
    for r in meta["runs"]:
        d = Path(r["out_dir"])
        if not d.is_dir():
            continue
        s = score_one(d)
        s["arm"], s["rep"], s["run_verdict"] = r["arm"], r["rep"], r["verdict"]
        rows.append(s)
        print(f"{r['arm']:9} rep{r['rep']}  run={r['verdict']:8} tier0={'OK' if s['TIER0_PASS'] else 'FAIL'} "
              f"behav={s['behavioral_passed']}/{s['behavioral_total']} screens={s.get('B3_distinct_screens',0)} "
              f"score={s['SCORE']}", flush=True)

    out = {"scorer_sha256": self_sha, "provenance": meta.get("provenance"), "rows": rows}
    (root / "scores.json").write_text(json.dumps(out, indent=2))

    valid = [r for r in rows if r["run_verdict"] == "VALID"]
    A = sorted(r["SCORE"] for r in valid if r["arm"] == "review")
    B = sorted(r["SCORE"] for r in valid if r["arm"] == "noreview")
    print(f"\nreview   (n={len(A)}): {A}")
    print(f"noreview (n={len(B)}): {B}")
    unscoreable = [r for r in valid if r["SCORE"] is None]
    if unscoreable:
        # A VALID run the scorer could not judge shrinks n silently and would
        # crash the comparison on None-vs-float. Refuse the verdict instead.
        print(f"\n✗ {len(unscoreable)} VALID run(s) unscoreable (driver failure) — "
              f"no verdict. Fix the scorer and re-score; the artifacts persist.")
    elif A and B:
        print("\nVERDICT (pre-registered):")
        if len(A) < 3 or len(B) < 3:
            print(f"  INCONCLUSIVE — n={len(A)}/{len(B)} valid per arm. With n<3 the best "
                  f"possible exact p is 1/C(4,2)=0.167, so significance is unreachable.")
        elif min(A) > max(B):
            # DIRECTIONAL, as pre-registered. Accepting separation in either
            # direction while quoting a one-sided p would make the true p 0.10.
            print("  SEPARATION IN THE HYPOTHESISED DIRECTION — every review run scored")
            print("  above every no-review run. Exact ONE-SIDED permutation p = 1/C(6,3) = 0.05.")
        elif min(B) > max(A):
            print("  SEPARATION IN THE OPPOSITE DIRECTION — every NO-review run scored above")
            print("  every review run. This contradicts the hypothesis; it is not support for it.")
            print("  Two-sided exact p = 2/C(6,3) = 0.10.")
        else:
            print("  NOISE — ranges overlap. Report 'no detectable effect at n=3',")
            print("  NOT 'review does not work'.")
    print(f"\nwrote {root / 'scores.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
