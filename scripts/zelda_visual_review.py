#!/usr/bin/env python3
"""Visual/playability review for agent-generated Zelda-style browser games.

The behavioral scorer (zelda_review_score.py) proves a game runs and responds;
it says nothing about whether the sprites are drawn, the maps are varied, the
enemies fight back, or a human would call it a game. This tool produces the
EVIDENCE for those judgements: labeled canvas screenshots, film strips that
make animation (or its absence) visible at a glance, and deterministic facts.
It does not score. Claims a machine cannot verify (quest completability,
aesthetic quality) are captured as attempts + images for a human or VLM judge.

OUTPUT CONTRACT (stable interface for the report page; per product dir, under
<out>/<slug>/ where slug = path under untracked/ with '/' -> '--'):

  facts.json  schema "zelda-visual-review/1":
    slug, product_dir, entry, generated_utc, elapsed_s      str/float
    canvas        {count, main "WxH", css "WxH", all [{w,h,cssW,cssH,visible}]}
    activation    {activated_by: str|null, responds_px: int}
    shots         [{file, label, phase, vframe, px_sha, same_as: str|null, note}]
                  px_sha = sha256 of raw canvas pixels (pre-upscale) — the
                  reproducibility comparator. same_as = label of an earlier
                  shot with identical pixels (dup shots are still written).
    strips        [{file, moment, frames, step, distinct, px_sha}]
                  distinct = unique frames in the strip; 1 means NOT animated.
    exploration   {legs, distinct_screens, new_screen_shots}
                  distinct_screens is PERCEPTUAL (coarse luminance grid,
                  10% tolerance) = map-diversity measure; a moved enemy is
                  not a new screen, a room transition is.
    movement      {ArrowUp/Down/Left/Right: changed_px}
    combat        {probed {key: changed_px}, best_key}
    interact      {probed, best_key, new_texts [str], dom_changed}
    inventory     {probed, best_key}
    palette       {unique_colors, top [["#rrggbb", fraction], ...]}
    motion_idle   {changed_px, regions, boxes [[x,y,w,h],...]}  # autonomous
                  motion while player idle: enemies/NPCs/water — honestly
                  labeled "regions", not "enemies"; the idle strip shows which.
    text          {canvas_strings [[str, count],...], dom_hud_text}
    audio         {starts}
    errors        {js_exceptions, console_errors}
    pause_recoveries [str]   # keys that froze the game and were re-pressed
    vlm           null | {model, rubric_version, json, usage, raw}

  sheet.html  self-contained dark contact sheet, relative <img> refs, section
              ids stable: #meta #shots #strips #facts #vlm.

  PNG naming (all canvas-cropped, NEAREST-upscaled to >=360px wide):
    00-boot, 01-title, 02-started, 05-viewport (only when DOM holds HUD/text),
    10-north 11-south 12-east 13-west, 20-explore-01.., 30-combat,
    40-interact, 41-interact-viewport (only when interact changed the DOM),
    50-inventory, 60-strip-idle, 61-strip-walk, 62-strip-combat.
    Slots are FIXED; a shot that is pixel-identical to an earlier one is still
    written but marked same_as, so absence of a file means the phase failed,
    not that it was deduped.

DESIGN DEPARTURES from the requesting spec, with reasons:
  * Combat/inventory/interact evidence is film strips + per-key pixel deltas,
    not one mid-hold screenshot: attack animations here are 3-10 frames and a
    single frame routinely lands on the wrong one.
  * Tab is NOT in the inventory ladder: zero bindings in the corpus and it
    moves browser focus, silently killing every later key event.
  * Enter/p probes are followed by a liveness check and an automatic
    re-press, because measured games toggle PAUSE on those keys.
  * Key ladders were set from the corpus (z/x/c/space attack; e/enter/space
    interact; i/c/1/2/3 inventory), not guessed.
  * Canvas capture is toDataURL of the game's own framebuffer (composited
    when a game layers multiple canvases), not locator.screenshot(): pixels
    are exact, CSS-scale-independent, and identical across runs.

DETERMINISM: inherits the instrument block of scripts/zelda_review_score.py
(seeded RNG, virtual clock, single-stepped rAF, pristine-rAF waiter, held
keys). Two runs on the same artifact must produce identical px_sha for every
shot; verify with --out into a scratch dir and diff the facts.

WRITE GUARD: refuses to write anywhere inside the repo except
untracked/visual-review/. Paths outside the repo (scratch) are allowed.

Usage:
  python3 scripts/zelda_visual_review.py untracked/zelda-ab/review-1-product
  python3 scripts/zelda_visual_review.py --all            # every known corpus dir
  python3 scripts/zelda_visual_review.py DIR --vlm        # + VLM judge (quota!)
"""
from __future__ import annotations

import argparse
import base64
import datetime as _dt
import hashlib
import io
import json
import re
import sys
import threading
import time
import urllib.request
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from PIL import Image, ImageDraw

REPO = Path(__file__).resolve().parents[1]
CHROME_CHANNEL = "chrome"
CHANGED_FLOOR = 120          # changed px that count as "responded" (measured, see scorer)
HOLD_FRAMES = 24
LEG_FRAMES = 40              # one exploration leg
MAX_LEGS = 22
MAX_EXPLORE_SHOTS = 8
STRIP_N = 6
STRIP_STEP = 3
MIN_SHOT_W = 360
VLM_URL = "http://127.0.0.1:8317/v1/chat/completions"
VLM_RUBRIC_VERSION = "v1"

# ---------------------------------------------------------------------------
# Instrument block: verbatim core from scripts/zelda_review_score.py (seeded
# RNG / virtual clock / single-stepping / pristine-rAF waiter / in-page canvas
# sampling — every line of that machinery was added because its absence was
# measured producing wrong or unstable readings; see that file for the full
# argument). Extended here with: fillText/strokeText string capture, composite
# canvas grab (multi-canvas games), palette census, and connected-component
# motion regions.
# ---------------------------------------------------------------------------
INSTRUMENT = r"""
(function () {
  const RAW_RAF = window.requestAnimationFrame.bind(window);
  const REAL_NOW = performance.now.bind(performance);
  const NATIVE_SETTIMEOUT = window.setTimeout.bind(window);
  const NATIVE_CLEARTIMEOUT = window.clearTimeout.bind(window);
  const probe = {raf: 0, frames: 0, paintFrames: 0, heldFrames: 0, draws: 0,
                 ticks: 0, audioStarts: 0, keyListeners: 0, keyTargets: [],
                 errors: [], texts: {}, textCount: 0};
  window.__probe = probe;

  // Deterministic RNG (same constants as the scorer: the artifact must be one
  // game, not a distribution of procedurally generated ones).
  let _s = 0x9e3779b9 >>> 0;
  Math.random = function () {
    _s ^= (_s << 13); _s >>>= 0;
    _s ^= (_s >>> 17);
    _s ^= (_s << 5);  _s >>>= 0;
    return _s / 4294967296;
  };

  // Virtual clock pinned to the frame counter — with NO real-time fallback.
  // The scorer's pre-first-frame fallback to wall clock leaked: a game that
  // captures t0 = performance.now() during init got a machine-dependent t0
  // and therefore a machine-dependent first-frame dt. Timers are virtual
  // (below), so the freeze the fallback guarded against cannot happen.
  let vt = 0;
  const STEP = 1000 / 60;
  performance.now = function () { return vt; };
  Date.now = function () { return 1767225600000 + Math.round(vt); };

  // ---- virtual timers ------------------------------------------------------
  // setInterval/setTimeout on the WALL clock were the last nondeterminism
  // leak: a music scheduler on setInterval consumed the seeded RNG stream a
  // machine-speed-dependent number of times, desynchronizing every later
  // Math.random() — measured as identical boot pixels but a different
  // activation path on a warm vs cold browser (audio starts 82 vs 127).
  // Timers fire on the VIRTUAL clock: after each granted frame, every timer
  // whose due-time has passed runs. Zero-delay chains progress via the
  // guard loop; pages that never arm rAF are stepped synthetically by
  // __waitFrames, so interval-only loops and pre-rAF setTimeout chains
  // still run — deterministically.
  const timers = new Map();   // id -> {fn, ms, next, repeat}
  let timerSeq = 1;
  window.setInterval = function (fn, ms, ...rest) {
    const id = timerSeq++;
    if (typeof fn === 'function') {
      const per = Math.max(Number(ms) || 0, 1);
      timers.set(id, {fn: () => fn(...rest), ms: per, next: vt + per, repeat: true});
    }
    return id;
  };
  window.setTimeout = function (fn, ms, ...rest) {
    const id = timerSeq++;
    if (typeof fn === 'function') {
      const d = Math.max(Number(ms) || 0, 0);
      timers.set(id, {fn: () => fn(...rest), ms: d, next: vt + d, repeat: false});
    }
    return id;
  };
  window.clearInterval = window.clearTimeout = function (id) { timers.delete(id); };
  function fireDue() {
    for (let guard = 0; guard < 100; guard++) {
      let fired = false;
      for (const [id, t] of [...timers]) {
        if (t.next > vt) continue;
        if (t.repeat) {
          t.next += t.ms;
          if (t.next < vt - 100 * t.ms) t.next = vt;  // cap runaway backlog
        } else {
          timers.delete(id);
        }
        probe.ticks++;
        try { t.fn(); } catch (e) { probe.errors.push('timer: ' + e); }
        fired = true;
      }
      if (!fired) break;
    }
  }

  // Single-stepped rAF: frames are handed out one budget at a time so the
  // world holds still while it is photographed.
  //
  // budget starts at ZERO (the scorer starts at Infinity): the world is
  // frozen from its very first animation frame until the driver grants a
  // budget. MEASURED: with a free-running boot, page.goto returns after a
  // wall-clock-dependent number of game frames, so every capture inherited
  // that offset and a lively artifact produced different pixels on every
  // run (18/18 shots differed). Frozen boot pins shot N to virtual frame N.
  // Games that render their first frame synchronously during script
  // execution still show it; games that only paint inside rAF show a blank
  // 00-boot, consistently — "boot" means "before the first frame".
  let budget = 0;
  window.requestAnimationFrame = function (cb) {
    probe.raf++;
    return RAW_RAF(function tick() {
      if (budget <= 0) { probe.heldFrames++; return RAW_RAF(tick); }
      budget--;
      vt += STEP;
      probe.frames++;
      const before = probe.draws;
      try { return cb(vt); }
      finally {
        if (probe.draws > before) probe.paintFrames++;
        fireDue();   // virtual timers ride the granted frames
      }
    });
  };

  // Frame gate on the PRISTINE rAF (a wait_for_function poller satisfies its
  // own condition; see the scorer).
  let gen = 0;
  window.__waitFrames = function (n, timeoutMs) {
    return new Promise(function (res) {
      const myGen = ++gen;
      if (probe.raf === 0) {
        // No rAF consumer yet: advance the virtual world synthetically so
        // timer-driven pages and pre-rAF setTimeout init chains progress
        // (a timer callback may arm rAF; real frames take over next wait).
        for (let i = 0; i < n; i++) { vt += STEP; probe.frames++; fireDue(); }
        return res(true);
      }
      const start = probe.frames;
      const done = () => (probe.frames - start) >= n;
      let settled = false;
      const finish = (v) => {
        if (settled) return;
        settled = true;
        NATIVE_CLEARTIMEOUT(to);
        if (gen === myGen) budget = 0;
        res(v);
      };
      const to = NATIVE_SETTIMEOUT(function () { finish(done()); }, timeoutMs + 250);
      budget = n;
      const t0 = REAL_NOW();
      (function chk() {
        if (settled) return;
        if (done()) return finish(true);
        if (REAL_NOW() - t0 > timeoutMs) return finish(false);
        RAW_RAF(chk);
      })();
    });
  };

  // ---- composite grab: one ImageData for the game picture -----------------
  // Games here mostly draw one canvas, but layered-canvas games exist; the
  // composite draws every visible canvas at its CSS position, scaled by the
  // sharpest internal/CSS ratio so no pixels are lost.
  function liveCanvases() {
    return [...document.querySelectorAll('canvas')].filter(c => {
      const r = c.getBoundingClientRect();
      return r.width > 0 && r.height > 0 && c.width > 0 && c.height > 0;
    });
  }
  window.__meta = function () {
    return [...document.querySelectorAll('canvas')].map(c => {
      const r = c.getBoundingClientRect();
      return {w: c.width, h: c.height, cssW: Math.round(r.width),
              cssH: Math.round(r.height), visible: !!(r.width && r.height)};
    });
  };
  function grabCanvas() {
    const cs = liveCanvases();
    if (!cs.length) return null;
    if (cs.length === 1) return cs[0];
    let x0 = 1e9, y0 = 1e9, x1 = -1e9, y1 = -1e9, k = 1;
    for (const c of cs) {
      const r = c.getBoundingClientRect();
      x0 = Math.min(x0, r.left); y0 = Math.min(y0, r.top);
      x1 = Math.max(x1, r.right); y1 = Math.max(y1, r.bottom);
      k = Math.max(k, c.width / r.width);
    }
    const off = document.createElement('canvas');
    off.width = Math.max(1, Math.round((x1 - x0) * k));
    off.height = Math.max(1, Math.round((y1 - y0) * k));
    const g = off.getContext('2d');
    g.imageSmoothingEnabled = false;
    for (const c of cs) {
      const r = c.getBoundingClientRect();
      g.drawImage(c, (r.left - x0) * k, (r.top - y0) * k, r.width * k, r.height * k);
    }
    return off;
  }
  window.__png = function () {
    const c = grabCanvas();
    if (!c) return null;
    try { return c.toDataURL('image/png'); } catch (e) { return 'ERR:' + e.message; }
  };
  function grabData() {
    const c = grabCanvas();
    if (!c) return null;
    try {
      return {w: c.width, h: c.height,
              d: c.getContext('2d').getImageData(0, 0, c.width, c.height).data};
    } catch (e) { return {err: String(e.message)}; }
  }

  // ---- luminance slots / delta / hash (per scorer, on the composite) ------
  window.__slots = {};
  window.__snap = function (slot) {
    const g = grabData();
    if (!g || g.err) { window.__slots[slot] = null; return {n: 0, err: g && g.err}; }
    const npx = g.w * g.h, v = new Int16Array(npx);
    for (let px = 0; px < npx; px++) {
      const i = px * 4;
      v[px] = (g.d[i] + g.d[i + 1] + g.d[i + 2]) >> 2;
    }
    window.__slots[slot] = {w: g.w, h: g.h, lum: v};
    return {n: npx};
  };
  window.__delta = function (a, b) {
    const A = window.__slots[a], B = window.__slots[b];
    if (!A || !B) return {n: 0, changed_px: 0};
    const m = Math.min(A.lum.length, B.lum.length);
    let ch = 0;
    for (let i = 0; i < m; i++) if (Math.abs(A.lum[i] - B.lum[i]) > 3) ch++;
    return {n: m, changed_px: ch};
  };
  window.__hash = function () {
    const g = grabData();
    if (!g || g.err) return g ? 'ERR:' + g.err : null;
    let h = 2166136261;
    for (let i = 0; i < g.d.length; i += 389) {  // prime stride over RGBA
      h ^= g.d[i]; h = Math.imul(h, 16777619) >>> 0;
    }
    return String(h);
  };

  // ---- perceptual screen signature ----------------------------------------
  // MEASURED: an exact canvas hash calls every frame with a moved enemy a
  // "new screen" — a lively game read 22 distinct screens in 22 legs while
  // standing in ~4 rooms. Room identity must be perceptual: a coarse grid of
  // mean luminance, compared with a tolerance, so a wandering blob does not
  // count as a map transition but a genuinely different screen does.
  window.__sig = function () {
    const g = grabData();
    if (!g || g.err) return null;
    const COLS = 24;
    const rows = Math.max(12, Math.min(32, Math.round(COLS * g.h / g.w)));
    const out = new Array(COLS * rows).fill(0);
    const cw = g.w / COLS, ch = g.h / rows;
    for (let ry = 0; ry < rows; ry++) {
      for (let rx = 0; rx < COLS; rx++) {
        let sum = 0, n = 0;
        const x0 = Math.floor(rx * cw), x1 = Math.floor((rx + 1) * cw);
        const y0 = Math.floor(ry * ch), y1 = Math.floor((ry + 1) * ch);
        for (let y = y0; y < y1; y += 2) {
          for (let x = x0; x < x1; x += 2) {
            const i = (y * g.w + x) * 4;
            sum += g.d[i] + g.d[i + 1] + g.d[i + 2];
            n++;
          }
        }
        out[ry * COLS + rx] = n ? ((sum / (3 * n)) >> 4) : 0;   // 16 levels
      }
    }
    return out;
  };

  // ---- palette census ------------------------------------------------------
  window.__palette = function (topN) {
    const g = grabData();
    if (!g || g.err) return {unique: 0, top: [], err: g && g.err};
    const m = new Map();
    for (let i = 0; i < g.d.length; i += 4) {
      const key = (g.d[i] << 16) | (g.d[i + 1] << 8) | g.d[i + 2];
      m.set(key, (m.get(key) || 0) + 1);
    }
    const total = g.w * g.h;
    const top = [...m.entries()].sort((a, b) => b[1] - a[1]).slice(0, topN || 16)
      .map(([k, n]) => ['#' + k.toString(16).padStart(6, '0'), +(n / total).toFixed(4)]);
    return {unique: m.size, top: top, pixels: total};
  };

  // ---- autonomous-motion regions ------------------------------------------
  // Connected components (4-neighbour, on a 4x-downsampled diff mask) between
  // two idle snapshots. Components >= 2 cells (~32 real px) are reported with
  // bounding boxes. This counts THINGS THAT MOVE BY THEMSELVES — enemies,
  // NPCs, water, torches — the strip images disambiguate which.
  window.__regions = function (a, b) {
    const A = window.__slots[a], B = window.__slots[b];
    if (!A || !B || A.w !== B.w || A.h !== B.h) return {count: 0, boxes: []};
    const D = 4, gw = Math.ceil(A.w / D), gh = Math.ceil(A.h / D);
    const mask = new Uint8Array(gw * gh);
    for (let y = 0; y < A.h; y++) {
      const gy = (y / D) | 0;
      for (let x = 0; x < A.w; x++) {
        const i = y * A.w + x;
        if (Math.abs(A.lum[i] - B.lum[i]) > 3) mask[gy * gw + ((x / D) | 0)] = 1;
      }
    }
    const seen = new Uint8Array(gw * gh), boxes = [], stack = [];
    for (let s = 0; s < gw * gh; s++) {
      if (!mask[s] || seen[s]) continue;
      let n = 0, x0 = gw, y0 = gh, x1 = 0, y1 = 0;
      stack.push(s); seen[s] = 1;
      while (stack.length) {
        const c = stack.pop(), cx = c % gw, cy = (c / gw) | 0;
        n++;
        if (cx < x0) x0 = cx; if (cx > x1) x1 = cx;
        if (cy < y0) y0 = cy; if (cy > y1) y1 = cy;
        for (const nb of [c - 1, c + 1, c - gw, c + gw]) {
          if (nb < 0 || nb >= gw * gh || seen[nb] || !mask[nb]) continue;
          if ((nb === c - 1 && cx === 0) || (nb === c + 1 && cx === gw - 1)) continue;
          seen[nb] = 1; stack.push(nb);
        }
      }
      if (n >= 2) boxes.push({x: x0 * D, y: y0 * D, w: (x1 - x0 + 1) * D,
                              h: (y1 - y0 + 1) * D, cells: n});
    }
    boxes.sort((p, q) => q.cells - p.cells);
    return {count: boxes.length, boxes: boxes.slice(0, 10)};
  };

  // ---- draw-call + drawn-text capture -------------------------------------
  for (const m of ['fillRect', 'drawImage', 'fillText', 'strokeRect', 'stroke',
                   'fill', 'putImageData', 'clearRect', 'strokeText']) {
    const o = CanvasRenderingContext2D.prototype[m];
    if (!o) continue;
    if (m === 'fillText' || m === 'strokeText') {
      CanvasRenderingContext2D.prototype[m] = function (...a) {
        probe.draws++;
        try {
          const s = String(a[0]).slice(0, 120);
          if (s.trim()) {
            probe.textCount++;
            if (probe.texts[s] !== undefined || Object.keys(probe.texts).length < 400) {
              probe.texts[s] = (probe.texts[s] || 0) + 1;
            }
          }
        } catch (e) { /* text capture must never break the game */ }
        return o.apply(this, a);
      };
    } else {
      CanvasRenderingContext2D.prototype[m] = function (...a) {
        probe.draws++; return o.apply(this, a);
      };
    }
  }
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
  if (window.HTMLMediaElement) {
    const pl = HTMLMediaElement.prototype.play;
    HTMLMediaElement.prototype.play = function (...a) {
      probe.audioStarts++; return pl.apply(this, a);
    };
  }
  // AudioContext.currentTime is the AUDIO-HARDWARE clock — the last real
  // clock a page can read. A lookahead music scheduler polling it scheduled
  // one extra note on a slower run (audio starts 127 vs 128). Audio is muted
  // during review, so pinning it to the virtual clock costs nothing.
  try {
    const proto = (window.BaseAudioContext || window.AudioContext || {}).prototype;
    if (proto) {
      Object.defineProperty(proto, 'currentTime',
                            {get: () => vt / 1000, configurable: true});
    }
  } catch (e) { /* leave the native clock if the override is refused */ }
  const add = EventTarget.prototype.addEventListener;
  EventTarget.prototype.addEventListener = function (t, ...rest) {
    if (t === 'keydown' || t === 'keyup') probe.keyListeners++;
    return add.call(this, t, ...rest);
  };
  window.addEventListener('error', (e) => probe.errors.push(String(e.message)));
  window.addEventListener('unhandledrejection', (e) => probe.errors.push('rejection: ' + e.reason));
})();
"""


# ------------------------------------------------------------------ serving
class QuietHandler(SimpleHTTPRequestHandler):
    def log_message(self, *a):  # noqa: D102
        pass

    def do_GET(self):  # noqa: N802
        if self.path == "/favicon.ico":
            self.send_response(200)
            self.send_header("Content-Type", "image/x-icon")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        return super().do_GET()


def serve_dir(d: Path):
    """HTTP-serve `d` (file:// breaks ES modules and taints the canvas)."""
    handler = partial(QuietHandler, directory=str(d.resolve()))
    srv = ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, srv.server_address[1]


def resolve_entry(d: Path) -> tuple[Path, Path]:
    entry = d / "index.html"
    if not entry.is_file():
        cands = sorted(d.rglob("index.html"), key=lambda q: len(q.relative_to(d).parts))
        if cands:
            entry = cands[0]
    return entry.resolve(), entry.resolve().parent


# ------------------------------------------------------------------ driver
class Driver:
    """Helpers over a live instrumented page. All waits are frame-gated."""

    def __init__(self, page):
        self.page = page
        self.stalls = 0

    def adv(self, n: int, timeout_ms: int = 8000) -> bool:
        t = timeout_ms if self.stalls < 1 else (300 if self.stalls < 4 else 0)
        try:
            ok = bool(self.page.evaluate("a => window.__waitFrames(a[0], a[1])", [n, t]))
        except Exception:  # noqa: BLE001
            ok = False
        if not ok:
            self.stalls += 1
        return ok

    def snap(self, slot: str) -> None:
        self.page.evaluate("s => window.__snap(s)", slot)

    def delta(self, a: str, b: str) -> int:
        r = self.page.evaluate("ab => window.__delta(ab[0], ab[1])", [a, b]) or {}
        return int(r.get("changed_px") or 0)

    def hash(self):
        return self.page.evaluate("() => window.__hash()")

    def hold(self, key: str, frames: int = HOLD_FRAMES) -> None:
        """Held, never tapped: these games read a held-keys map and a
        press() (keydown+keyup with no frame between) is invisible to them."""
        self.page.keyboard.down(key)
        self.adv(frames, 6000)
        self.page.keyboard.up(key)
        self.adv(2, 4000)

    def probe_key(self, key: str, frames: int = HOLD_FRAMES, mid: bool = False) -> int:
        """Changed px caused by holding `key`.

        mid=True also samples DURING the hold and returns the max: a 3-frame
        sword swing retracts before the post-hold snapshot, so net-delta alone
        measured a real attack at 179 px while a persistent change (a placed
        bomb) measured 5846 — transient animations need the mid sample.
        """
        self.snap("p0")
        self.page.keyboard.down(key)
        best = 0
        if mid:
            self.adv(4, 4000)
            self.snap("pm")
            best = self.delta("p0", "pm")
            self.adv(max(1, frames - 4), 6000)
        else:
            self.adv(frames, 6000)
        self.page.keyboard.up(key)
        self.adv(2, 4000)
        self.snap("p1")
        return max(best, self.delta("p0", "p1"))

    def sig(self):
        return self.page.evaluate("() => window.__sig()")

    def grab(self) -> Image.Image | None:
        """The game's own framebuffer (composited across canvases)."""
        url = self.page.evaluate("() => window.__png()")
        if not url or str(url).startswith("ERR"):
            return None
        raw = base64.b64decode(url.split(",", 1)[1])
        return Image.open(io.BytesIO(raw)).convert("RGB")

    def texts(self) -> dict:
        return self.page.evaluate("() => window.__probe.texts") or {}

    def dom_text(self) -> str:
        try:
            t = self.page.evaluate("() => document.body ? document.body.innerText : ''") or ""
        except Exception:  # noqa: BLE001
            t = ""
        return re.sub(r"\n{3,}", "\n\n", t.strip())[:800]

    def responds(self, floor: int = CHANGED_FLOOR) -> int:
        """Max changed px over held arrows, all four axes, early exit.

        MEASURED: a player spawned against rocks moves on ONE axis only
        (ArrowUp 3402 px, the other three ~ambient), so a right/left-only
        test read 'unresponsive' and mis-credited activation to a later
        gesture whose side effect (a bomb) changed the canvas. Axes are
        probed in restoring pairs (R then L, U then D) so the world is left
        roughly where it was found."""
        best = 0
        for key in ("ArrowRight", "ArrowLeft", "ArrowUp", "ArrowDown"):
            self.snap("r0")
            self.hold(key)
            self.snap("r1")
            best = max(best, self.delta("r0", "r1"))
            if best >= floor and key in ("ArrowLeft", "ArrowDown"):
                break   # only stop at a restore point
        return best

    def idle_window(self, frames: int = 26) -> int:
        self.snap("iw0")
        self.adv(frames, 6000)
        self.snap("iw1")
        return self.delta("iw0", "iw1")

    def click_start_button(self) -> bool:
        """Click a DOM start button (trusted mouse event at its center).
        MEASURED: one artifact gates ALL input behind
        <button id=startButton>Begin Adventure</button>; canvas clicks and
        keys do nothing until it is pressed."""
        rect = self.page.evaluate("""() => {
          const cands = [...document.querySelectorAll('button, [role=button], input[type=button]')];
          const good = cands.filter(b => {
            const t = (b.textContent || b.value || '') + ' ' + (b.id || '');
            const r = b.getBoundingClientRect();
            return r.width > 0 && r.height > 0 &&
                   /begin|start|play|continue|new game|adventure/i.test(t) &&
                   !/sound|audio|music|mute/i.test(t);
          });
          if (!good.length) return null;
          const r = good[0].getBoundingClientRect();
          return {x: r.left + r.width / 2, y: r.top + r.height / 2};
        }""")
        if not rect:
            return False
        self.page.mouse.click(rect["x"], rect["y"])
        self.adv(10, 4000)
        return True


SIG_DIFF_LEVEL = 1      # cell differs if quantized luminance gap > 1 (of 16)
SIG_NEW_SCREEN = 0.10   # new screen when >10% of cells differ from EVERY seen one


def sig_distance(a: list | None, b: list | None) -> float:
    """Fraction of grid cells that differ perceptibly between two signatures."""
    if not a or not b or len(a) != len(b):
        return 1.0
    diff = sum(1 for x, y in zip(a, b) if abs(x - y) > SIG_DIFF_LEVEL)
    return diff / len(a)


def is_new_screen(sig: list | None, seen: list[list]) -> bool:
    if not sig:
        return False
    return all(sig_distance(sig, s) > SIG_NEW_SCREEN for s in seen)


def px_sha(img: Image.Image) -> str:
    return hashlib.sha256(img.tobytes()).hexdigest()[:16]


def upscaled(img: Image.Image) -> Image.Image:
    k = max(1, round(MIN_SHOT_W / img.width))
    if k == 1:
        return img
    return img.resize((img.width * k, img.height * k), Image.NEAREST)


# ------------------------------------------------------------------ review
def review_one(pw, browser, product: Path, outdir: Path, vlm: bool,
               vlm_model: str) -> dict:
    t_start = time.time()
    outdir.mkdir(parents=True, exist_ok=True)
    facts: dict = {
        "schema": "zelda-visual-review/1",
        "slug": outdir.name,
        "product_dir": str(product),
        "entry": None,
        "generated_utc": _dt.datetime.now(_dt.timezone.utc).isoformat(timespec="seconds"),
        "canvas": None, "activation": {"activated_by": None, "responds_px": 0},
        "shots": [], "strips": [],
        "exploration": {"legs": 0, "distinct_screens": 0, "new_screen_shots": 0},
        "movement": {}, "combat": {"probed": {}, "best_key": None},
        "interact": {"probed": {}, "best_key": None, "new_texts": [], "dom_changed": False},
        "inventory": {"probed": {}, "best_key": None},
        "palette": None, "motion_idle": None,
        "text": {"canvas_strings": [], "dom_hud_text": ""},
        "audio": {"starts": 0},
        "errors": {"js_exceptions": [], "console_errors": []},
        "pause_recoveries": [], "vlm": None,
    }
    entry, root = resolve_entry(product)
    if not entry.is_file():
        facts["error"] = "no index.html"
        write_outputs(outdir, facts)
        return facts
    facts["entry"] = str(entry)

    shot_hashes: dict[str, str] = {}   # px_sha -> label

    def save_shot(img: Image.Image | None, name: str, label: str, phase: str,
                  vframe: int, note: str = "") -> None:
        if img is None:
            return
        sha = px_sha(img)
        same = shot_hashes.get(sha)
        if same is None:
            shot_hashes[sha] = label
        upscaled(img).save(outdir / f"{name}.png")
        facts["shots"].append({"file": f"{name}.png", "label": label, "phase": phase,
                               "vframe": vframe, "px_sha": sha,
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
        page.goto(f"http://127.0.0.1:{port}/{entry.name}", wait_until="load", timeout=30000)
        try:
            page.evaluate("() => document.fonts ? document.fonts.ready.then(() => true) : true")
        except Exception:  # noqa: BLE001
            pass
        drv = Driver(page)

        def vframe() -> int:
            p = page.evaluate("() => window.__probe") or {}
            return int(p.get("frames") or 0)

        # ---- 00 boot / 01 title --------------------------------------------
        save_shot(drv.grab(), "00-boot", "boot", "boot", vframe())
        drv.adv(45, 12000)
        save_shot(drv.grab(), "01-title", "title", "title", vframe())
        facts["canvas"] = {"all": page.evaluate("() => window.__meta()") or []}
        cs = [c for c in facts["canvas"]["all"] if c.get("visible")]
        facts["canvas"]["count"] = len(cs)
        if cs:
            main = max(cs, key=lambda c: c["w"] * c["h"])
            facts["canvas"]["main"] = f"{main['w']}x{main['h']}"
            facts["canvas"]["css"] = f"{main['cssW']}x{main['cssH']}"

        # ---- ACTIVATION ladder ----------------------------------------------
        # Per the scorer: some games need Space/Enter/click to start and Enter
        # can PAUSE a running one, so stop at the first gesture after which
        # arrows do something. Two measured extensions: a DOM "Begin
        # Adventure" button that gates all input, and a multi-page intro
        # dialogue that eats several Enters before arrows work. The floor is
        # DOMINANCE over title-screen ambient animation, not a constant:
        # ambient motion of ~113 px/window crossed the 120 px constant and
        # faked "responds to arrows" on a game stuck in a modal dialogue.
        # MEDIAN of 3 windows, not one: a title-screen fade landed in a single
        # window (3232 px), inflated the floor to 9696 and no gesture could
        # ever pass — the same one-transient-is-not-a-rate bug the scorer's
        # idle control hit.
        title_idle = sorted(drv.idle_window() for _ in range(3))[1]
        act_floor = max(CHANGED_FLOOR, 3 * title_idle)
        facts["activation"]["title_idle_px"] = title_idle
        gestures = [("none", None), ("click:start-button", None),
                    ("key:Space", "Space"), ("key:Enter", "Enter"),
                    ("click:canvas", None), ("key:z", "z"), ("key:c", "c"),
                    ("key:x", "x"), ("key:j", "j"),
                    ("enter-x4", None)]
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
                drv.adv(3, 4000)
                page.keyboard.up(key)
                drv.adv(8, 4000)
            elif name == "click:start-button":
                if not drv.click_start_button():
                    continue
            elif name == "click:canvas":
                if not box:
                    continue
                page.mouse.click(box["x"] + box["width"] / 2, box["y"] + box["height"] / 2)
                drv.adv(8, 4000)
            elif name == "enter-x4":
                # dismiss a multi-page intro dialogue (measured: "Campaign
                # Notes ... ENTER" swallowing arrows until dismissed)
                for _ in range(4):
                    page.keyboard.down("Enter")
                    drv.adv(3, 3000)
                    page.keyboard.up("Enter")
                    drv.adv(8, 3000)
            r = drv.responds(act_floor)
            if r >= act_floor:
                facts["activation"]["activated_by"] = name
                facts["activation"]["responds_px"] = r
                break
        save_shot(drv.grab(), "02-started", "started", "started", vframe())

        # response floor for every later probe: dominance over POST-start
        # ambient animation (median of 3 idle windows), so wandering enemies
        # and water do not read as key response.
        idles = sorted(drv.idle_window() for _ in range(3))
        resp_floor = max(CHANGED_FLOOR, int(3.0 * idles[1]))
        facts["idle_baseline_px"] = idles[1]
        facts["response_floor"] = resp_floor

        def ensure_alive(after: str) -> None:
            """A probed key may have toggled PAUSE (measured: Enter, p).
            If the world stopped responding, re-press the key and note it."""
            if drv.responds(resp_floor) >= resp_floor:
                return
            try:
                page.keyboard.press(after)
            except Exception:  # noqa: BLE001
                return
            drv.adv(6, 3000)
            if drv.responds(resp_floor) >= resp_floor:
                facts["pause_recoveries"].append(after)

        # ---- DOM HUD? ------------------------------------------------------
        dom0 = drv.dom_text()
        facts["text"]["dom_hud_text"] = dom0
        if dom0:
            try:
                page.screenshot(path=str(outdir / "05-viewport.png"))
                facts["shots"].append({"file": "05-viewport.png", "label": "viewport",
                                       "phase": "started", "vframe": vframe(),
                                       "px_sha": None, "same_as": None,
                                       "note": "full viewport: DOM holds text/HUD"})
            except Exception:  # noqa: BLE001
                pass

        # ---- idle motion: autonomous entities + idle strip ------------------
        # regions come from the LAST baseline window's snapshots (iw0/iw1)
        regions = page.evaluate("() => window.__regions('iw0','iw1')") or {}
        facts["motion_idle"] = {
            "changed_px": idles[1],
            "regions": int(regions.get("count") or 0),
            "boxes": [[b["x"], b["y"], b["w"], b["h"]] for b in (regions.get("boxes") or [])],
        }
        make_strip(drv, outdir, facts, "60-strip-idle", "idle", hold_key=None)

        # ---- movement: 4 sustained holds, shot after each -------------------
        for name, key, fname in (("north", "ArrowUp", "10-north"),
                                 ("south", "ArrowDown", "11-south"),
                                 ("east", "ArrowRight", "12-east"),
                                 ("west", "ArrowLeft", "13-west")):
            ch = drv.probe_key(key, HOLD_FRAMES + 12)
            facts["movement"][key] = ch
            save_shot(drv.grab(), fname, name, "movement", vframe(),
                      note=f"after held {key}, changed_px={ch}")

        # walking strip on the strongest direction
        best_dir = max(facts["movement"], key=facts["movement"].get, default=None)
        if best_dir and facts["movement"][best_dir] >= CHANGED_FLOOR:
            make_strip(drv, outdir, facts, "61-strip-walk", f"walk:{best_dir}",
                       hold_key=best_dir)

        # ---- deep exploration: serpentine walk, shot per NEW screen ---------
        # "New screen" is PERCEPTUAL (coarse luminance grid with tolerance),
        # not an exact hash: an exact hash counts a wandering enemy as a map
        # transition and read 22 "screens" while standing in ~4 rooms.
        seen_sigs: list[list] = []
        s0 = drv.sig()
        if s0:
            seen_sigs.append(s0)
        plan = (["ArrowRight"] * 2 + ["ArrowUp"] * 2 + ["ArrowLeft"] * 3 +
                ["ArrowDown"] * 3 + ["ArrowRight"] * 4 + ["ArrowUp"] * 4 +
                ["ArrowLeft"] * 2 + ["ArrowDown"] * 2)[:MAX_LEGS]
        new_shots = 0
        for i, key in enumerate(plan):
            drv.hold(key, LEG_FRAMES)
            facts["exploration"]["legs"] = i + 1
            s = drv.sig()
            if is_new_screen(s, seen_sigs):
                seen_sigs.append(s)
                if new_shots < MAX_EXPLORE_SHOTS:
                    new_shots += 1
                    save_shot(drv.grab(), f"20-explore-{new_shots:02d}",
                              f"explore-{new_shots}", "explore", vframe(),
                              note=f"new screen after leg {i + 1} ({key})")
        facts["exploration"]["distinct_screens"] = len(seen_sigs)
        facts["exploration"]["new_screen_shots"] = new_shots

        # ---- combat: probe attack keys, strip the strongest -----------------
        for key in ("z", "x", " ", "c"):
            facts["combat"]["probed"][key.strip() or "Space"] = drv.probe_key(key, 14, mid=True)
        best = max(facts["combat"]["probed"], key=facts["combat"]["probed"].get, default=None)
        if best and facts["combat"]["probed"][best] >= CHANGED_FLOOR:
            if facts["combat"]["probed"][best] >= resp_floor:
                facts["combat"]["best_key"] = best
            key = " " if best == "Space" else best
            make_strip(drv, outdir, facts, "62-strip-combat", f"attack:{best}", hold_key=key)
            page.keyboard.down(key)
            drv.adv(4, 3000)
            save_shot(drv.grab(), "30-combat", "combat", "combat", vframe(),
                      note=f"mid-hold of attack key {best}")
            page.keyboard.up(key)
            drv.adv(2, 3000)

        # ---- interact: e / Enter / Space near whatever is here --------------
        # Text baseline taken HERE, after combat: taking it before combat
        # mislabeled "Bomb placed." (drawn by the attack probe) as dialogue.
        texts_before = set(drv.texts().keys())
        dom_before = drv.dom_text()
        for key in ("e", "Enter", " "):
            label = {" ": "Space"}.get(key, key)
            facts["interact"]["probed"][label] = drv.probe_key(key, 10, mid=True)
            if key in ("Enter",):
                ensure_alive(key)
        new_texts = [t for t in drv.texts().keys() if t not in texts_before]
        facts["interact"]["new_texts"] = new_texts[:20]
        dom_after = drv.dom_text()
        facts["interact"]["dom_changed"] = dom_after != dom_before
        bi = max(facts["interact"]["probed"], key=facts["interact"]["probed"].get, default=None)
        if bi and facts["interact"]["probed"][bi] >= resp_floor:
            facts["interact"]["best_key"] = bi
        save_shot(drv.grab(), "40-interact", "interact", "interact", vframe(),
                  note=f"after interact probes; new drawn strings: {len(new_texts)}")
        if facts["interact"]["dom_changed"]:
            try:
                page.screenshot(path=str(outdir / "41-interact-viewport.png"))
                facts["shots"].append({"file": "41-interact-viewport.png",
                                       "label": "interact-viewport", "phase": "interact",
                                       "vframe": vframe(), "px_sha": None,
                                       "same_as": None, "note": "DOM changed during interact"})
            except Exception:  # noqa: BLE001
                pass

        # ---- inventory / weapon switch --------------------------------------
        # Corpus keys: i, c, 1, 2, 3. Tab deliberately EXCLUDED: zero corpus
        # bindings and it moves browser focus off the page, killing all
        # subsequent key events.
        for key in ("i", "c", "1", "2", "3"):
            facts["inventory"]["probed"][key] = drv.probe_key(key, 10, mid=True)
            if key == "i":
                ensure_alive(key)   # 'i' may open a modal inventory = freeze
        binv = max(facts["inventory"]["probed"], key=facts["inventory"]["probed"].get,
                   default=None)
        if binv and facts["inventory"]["probed"][binv] >= resp_floor:
            facts["inventory"]["best_key"] = binv
        save_shot(drv.grab(), "50-inventory", "inventory", "inventory", vframe(),
                  note="after inventory-key probes")

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


def make_strip(drv: Driver, outdir: Path, facts: dict, name: str, moment: str,
               hold_key: str | None) -> None:
    """6 consecutive frames at fixed virtual-time steps, composed into one
    horizontal film strip. A static sprite vs an animated one is visible at
    a glance; `distinct` counts unique frames (1 = nothing animates)."""
    frames: list[Image.Image] = []
    if hold_key:
        drv.page.keyboard.down(hold_key)
    try:
        for _ in range(STRIP_N):
            img = drv.grab()
            if img is not None:
                frames.append(img)
            drv.adv(STRIP_STEP, 4000)
    finally:
        if hold_key:
            drv.page.keyboard.up(hold_key)
            drv.adv(2, 3000)
    if not frames:
        return
    shas = [px_sha(f) for f in frames]
    k = max(1, round(200 / frames[0].width))
    fw, fh = frames[0].width * k, frames[0].height * k
    pad, label_h = 4, 14
    sheet = Image.new("RGB", (len(frames) * (fw + pad) - pad, fh + label_h), (16, 16, 16))
    draw = ImageDraw.Draw(sheet)
    for i, f in enumerate(frames):
        x = i * (fw + pad)
        sheet.paste(f.resize((fw, fh), Image.NEAREST), (x, label_h))
        draw.text((x + 2, 1), f"+{i * STRIP_STEP}f", fill=(200, 200, 200))
    sheet.save(outdir / f"{name}.png")
    facts["strips"].append({"file": f"{name}.png", "moment": moment,
                            "frames": len(frames), "step": STRIP_STEP,
                            "distinct": len(set(shas)),
                            "px_sha": hashlib.sha256("".join(shas).encode()).hexdigest()[:16]})


# ------------------------------------------------------------------ VLM judge
VLM_PROMPT = """You are judging screenshots of a small browser game that was \
supposed to be a Zelda-like action-adventure. Images are labeled; film strips \
show 6 consecutive frames. Judge ONLY what is visible. Reply with STRICT JSON, \
no markdown fence, exactly these keys:
{"sprite_quality": 1-5, "animation_quality": 1-5, "map_aesthetics": 1-5,
 "map_diversity": 1-5, "enemy_evidence": "none|static|moving|attacking",
 "enemy_variety": <int distinct enemy types visible>,
 "npc_dialogue_evidence": true/false, "npc_note": "<=20 words",
 "inventory_evidence": true/false, "inventory_note": "<=20 words",
 "verdict": "<one sentence: is this genuinely playable as a game?>"}
Scales: 1=broken/blank, 2=crude rectangles, 3=readable pixel-art shapes, \
4=coherent styled sprites/tiles, 5=polished. map_diversity judges variety \
ACROSS the exploration shots (identical screens = 1)."""


def vlm_judge(outdir: Path, facts: dict, model: str) -> dict:
    imgs = []
    for s in facts["shots"] + facts["strips"]:
        f = s["file"]
        if s.get("same_as"):
            continue
        if f.startswith(("05-", "41-")) and len(imgs) >= 10:
            continue
        p = outdir / f
        if p.is_file() and len(imgs) < 14:
            imgs.append((s.get("label") or s.get("moment") or f, p))
    content: list[dict] = [{"type": "text", "text": VLM_PROMPT}]
    for label, p in imgs:
        b64 = base64.b64encode(p.read_bytes()).decode()
        content.append({"type": "text", "text": f"[{label}]"})
        content.append({"type": "image_url",
                        "image_url": {"url": f"data:image/png;base64,{b64}"}})
    body = {"model": model, "temperature": 0, "max_tokens": 6000,
            "messages": [{"role": "user", "content": content}]}
    out = {"model": model, "rubric_version": VLM_RUBRIC_VERSION, "images_sent": len(imgs),
           "json": None, "usage": None, "raw": None}
    for attempt in (body, {k: v for k, v in body.items() if k != "temperature"}):
        req = urllib.request.Request(
            VLM_URL, data=json.dumps(attempt).encode(),
            headers={"Content-Type": "application/json",
                     "Authorization": "Bearer local"})
        try:
            with urllib.request.urlopen(req, timeout=300) as r:
                resp = json.loads(r.read())
            txt = resp["choices"][0]["message"]["content"]
            out["usage"] = resp.get("usage")
            out["raw"] = txt[:2000]
            m = re.search(r"\{.*\}", txt, re.DOTALL)
            if m:
                out["json"] = json.loads(m.group(0))
            return out
        except Exception as e:  # noqa: BLE001
            out["error"] = str(e)[:300]
    return out


# ------------------------------------------------------------------ outputs
def write_outputs(outdir: Path, facts: dict) -> None:
    (outdir / "facts.json").write_text(json.dumps(facts, indent=2))
    (outdir / "sheet.html").write_text(render_sheet(facts))


def _esc(s) -> str:
    return (str(s).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;"))


def render_sheet(facts: dict) -> str:
    slug = _esc(facts["slug"])
    rows = []
    mv = facts.get("movement") or {}
    act = facts.get("activation") or {}
    mi = facts.get("motion_idle") or {}
    pal = facts.get("palette") or {}
    ex = facts.get("exploration") or {}

    def fact_row(k, v):
        rows.append(f"<tr><th>{_esc(k)}</th><td>{v}</td></tr>")

    fact_row("product", _esc(facts.get("product_dir")))
    fact_row("canvas", _esc(json.dumps((facts.get("canvas") or {}).get("main"))) +
             f" (css {_esc((facts.get('canvas') or {}).get('css'))}, "
             f"{(facts.get('canvas') or {}).get('count', 0)} canvas element(s))")
    fact_row("activated by", _esc(act.get("activated_by")) +
             f" (responds {act.get('responds_px', 0)} px)")
    fact_row("movement px (U/D/R/L)",
             " / ".join(str(mv.get(k, "-")) for k in
                        ("ArrowUp", "ArrowDown", "ArrowRight", "ArrowLeft")))
    fact_row("distinct screens explored", f"{ex.get('distinct_screens', 0)} "
             f"(over {ex.get('legs', 0)} legs)")
    fact_row("autonomous motion (idle)", f"{mi.get('changed_px', 0)} px in "
             f"{mi.get('regions', 0)} region(s)")
    fact_row("palette", f"{pal.get('unique_colors', 0)} unique colors")
    sw = "".join(f"<span class=sw style='background:{_esc(c)}' title='{_esc(c)} {f}'></span>"
                 for c, f in (pal.get("top") or [])[:16])
    fact_row("top colors", sw)
    fact_row("combat probes", _esc(json.dumps((facts.get("combat") or {}).get("probed"))))
    fact_row("interact probes", _esc(json.dumps((facts.get("interact") or {}).get("probed"))) +
             f" — new drawn strings: {len((facts.get('interact') or {}).get('new_texts') or [])}"
             f", DOM changed: {(facts.get('interact') or {}).get('dom_changed')}")
    fact_row("inventory probes", _esc(json.dumps((facts.get("inventory") or {}).get("probed"))))
    fact_row("audio starts", (facts.get("audio") or {}).get("starts", 0))
    fact_row("pause recoveries", _esc(json.dumps(facts.get("pause_recoveries"))))
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
    newt = "".join(f"<li><code>{_esc(t)}</code></li>"
                   for t in (facts.get("interact") or {}).get("new_texts") or [])
    vlm_html = ""
    if facts.get("vlm"):
        vlm_html = (f"<h2 id=vlm>VLM judge ({_esc(facts['vlm'].get('model'))}, rubric "
                    f"{_esc(facts['vlm'].get('rubric_version'))})</h2>"
                    f"<pre>{_esc(json.dumps(facts['vlm'].get('json'), indent=2))}</pre>")

    return f"""<!doctype html><meta charset=utf-8>
<title>visual review — {slug}</title>
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
<h1 id=meta>visual review — {slug}</h1>
<table>{''.join(rows)}</table>
<h2 id=shots>Shots</h2><div class=grid>{''.join(shot_cells) or '<i>none captured</i>'}</div>
<h2 id=strips>Film strips (6 frames, every {STRIP_STEP} virtual frames)</h2>
<div>{''.join(strip_cells) or '<i>none captured</i>'}</div>
<h2 id=facts>Drawn text (canvas fillText/strokeText)</h2>
<ul>{texts or '<i>none observed</i>'}</ul>
{'<h3>New strings during interact</h3><ul>' + newt + '</ul>' if newt else ''}
{vlm_html}
<details><summary>facts.json</summary><pre>{_esc(json.dumps(facts, indent=2))}</pre></details>
"""


# ------------------------------------------------------------------ main
def slug_for(product: Path) -> str:
    p = product.resolve()
    try:
        rel = p.relative_to(REPO / "untracked")
        return "--".join(rel.parts)
    except ValueError:
        return p.name


def guard_out(out: Path) -> Path:
    """Refuse to write inside the repo anywhere except untracked/visual-review."""
    out = out.resolve()
    allowed = (REPO / "untracked" / "visual-review").resolve()
    try:
        out.relative_to(REPO)
        inside_repo = True
    except ValueError:
        inside_repo = False
    if inside_repo and not (out == allowed or str(out).startswith(str(allowed) + "/")):
        raise SystemExit(f"refusing to write to {out}: inside the repo but not "
                         f"under {allowed}")
    return out


DEFAULT_ROOTS = ("untracked/zelda-ab", "untracked/client-bench")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("products", nargs="*", type=Path, help="product dirs to review")
    ap.add_argument("--all", action="store_true",
                    help=f"review every *-product under {DEFAULT_ROOTS}")
    ap.add_argument("--out", type=Path, default=REPO / "untracked" / "visual-review")
    ap.add_argument("--vlm", action="store_true",
                    help="run the VLM judge on each reviewed artifact (costs quota; "
                         "pilot on ONE artifact before any corpus run)")
    ap.add_argument("--vlm-model", default="gpt-5.4")
    a = ap.parse_args()

    products: list[Path] = []
    if a.all:
        for r in DEFAULT_ROOTS:
            products += sorted((REPO / r).glob("*-product"))
    for p in a.products:
        products.append(p if p.is_absolute() else REPO / p)
    products = [p for p in products if p.is_dir()]
    if not products:
        ap.error("no product dirs given (pass paths or --all)")

    out_root = guard_out(a.out)
    out_root.mkdir(parents=True, exist_ok=True)

    from playwright.sync_api import sync_playwright
    with sync_playwright() as pw:
        # ONE browser for the whole invocation: a paid benchmark shares this
        # machine and the budget is at most one Chrome at a time.
        browser = pw.chromium.launch(
            channel=CHROME_CHANNEL, headless=True,
            args=["--autoplay-policy=no-user-gesture-required", "--mute-audio",
                  "--force-device-scale-factor=1", "--disable-lcd-text"])
        try:
            for prod in products:
                slug = slug_for(prod)
                outdir = out_root / slug
                t0 = time.time()
                facts = review_one(pw, browser, prod, outdir, a.vlm, a.vlm_model)
                ex = facts.get("exploration") or {}
                print(f"{slug:44} {time.time() - t0:5.1f}s "
                      f"shots={len(facts.get('shots') or [])} "
                      f"strips={len(facts.get('strips') or [])} "
                      f"screens={ex.get('distinct_screens', 0)} "
                      f"{'ERROR: ' + facts['error'] if facts.get('error') else ''}",
                      flush=True)
        finally:
            browser.close()
    print(f"\nwrote {out_root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
