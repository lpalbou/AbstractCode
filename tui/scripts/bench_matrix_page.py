#!/usr/bin/env python3
"""Build a self-contained matrix page for the review A/B benchmark.

One page where the operator can see every run's facts side by side AND play
each generated game in place. Games load in a sandboxed iframe from a local
HTTP server, so ES modules and canvas reads behave exactly as they do when
scored (file:// breaks both).

Usage:
  python3 scripts/bench_matrix_page.py                 # build + serve + open
  python3 scripts/bench_matrix_page.py --no-serve      # build the HTML only
  python3 scripts/bench_matrix_page.py --port 8901
"""
from __future__ import annotations

import argparse
import html
import http.server
import json
import shutil
import socketserver
import subprocess
import threading
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
OUT = REPO / "untracked" / "bench-site"


def collect() -> list[dict]:
    """Every matrix directory we can find, newest last, with its runs.

    The presence of runs.json IS the membership test. The previous name-glob
    allowlist (*zelda-ab*/*client-bench*/*workflow-bench*/_archive-*) silently
    dropped any bench whose CB_OUT/WB_OUT the operator named something else —
    `rtype-bench` and `zelda-postfix` both produced a complete runs.json and
    appeared nowhere on the page, which reads as "the benchmark produced
    nothing" rather than "the collector could not see it".
    """
    roots = sorted(p for p in (REPO / "untracked").iterdir()
                   if p.is_dir() and (p / "runs.json").is_file())
    out: list[dict] = []
    for root in roots:
        try:
            data = json.loads((root / "runs.json").read_text())
        except Exception:  # noqa: BLE001
            continue
        prov = data.get("provenance") or {}
        for r in data.get("runs", []):
            prod = root / f"{r['arm']}-{r['rep']}-product"
            r = dict(r)
            r["_root"] = root.name
            r["_mode"] = prov.get("mode", "?")
            r["_model"] = prov.get("model", "?")
            r["_reasoning"] = prov.get("reasoning", "?")
            # A cross-client run records its own archived product path.
            if not prod.is_dir() and r.get("archived_product", "").startswith("/"):
                prod = Path(r["archived_product"])
            r["_product"] = str(prod) if prod.is_dir() else ""
            # Agent / route, per run where recorded, else the matrix provenance.
            r["_agent"] = r.get("agent") or r.get("workflow") or {
                "review": "react-agent:react (gateway) +review",
                "noreview": "react-agent:react (gateway)",
            }.get(r["arm"], r["arm"])
            r["_reason_applied"] = r.get("reasoning_applied") or "applied via _runtime.thinking"
            r["_ver"] = (prov.get("client_versions") or {}).get(r["arm"], "")
            out.append(r)
    return out


VR = REPO / "untracked" / "visual-review"


def vr_slug(product: str) -> str:
    """Slug convention from zelda_visual_review.py: path under untracked/, '/'->'--'."""
    try:
        rel = Path(product).resolve().relative_to((REPO / "untracked").resolve())
    except ValueError:
        return ""
    return str(rel).replace("/", "--")


def stage_visual(rows: list[dict]) -> None:
    """Copy each run's visual-review folder into the site; attach thumb + sheet."""
    vr_out = OUT / "vr"
    if vr_out.exists():
        shutil.rmtree(vr_out)
    vr_out.mkdir(parents=True)
    for r in rows:
        r["_vr"] = ""
        r["_thumb"] = ""
        if not r.get("_product"):
            continue
        slug = vr_slug(r["_product"])
        src = VR / slug
        if not (src / "sheet.html").is_file():
            continue
        try:
            shutil.copytree(src, vr_out / slug)
            r["_vr"] = f"vr/{slug}/sheet.html"
            for cand in ("02-started.png", "01-title.png", "00-boot.png"):
                if (src / cand).is_file():
                    r["_thumb"] = f"vr/{slug}/{cand}"
                    break
        except Exception:  # noqa: BLE001
            pass


def stage_games(rows: list[dict]) -> None:
    """Copy each product into the site so the page can serve and play it."""
    games = OUT / "games"
    if games.exists():
        shutil.rmtree(games)
    games.mkdir(parents=True)
    for r in rows:
        if not r["_product"]:
            continue
        slug = f"{r['_root']}__{r['arm']}-{r['rep']}"
        dest = games / slug
        try:
            shutil.copytree(r["_product"], dest)
            entry = "index.html" if (dest / "index.html").is_file() else next(
                (p.name for p in dest.glob("*.html")), "")
            r["_play"] = f"games/{slug}/{entry}" if entry else ""
        except Exception:  # noqa: BLE001
            r["_play"] = ""


CARDS_MD = REPO / "docs" / "orchestration-cards.md"


def stage_cards() -> None:
    """Render the orchestration cards to cards.html (markdown lib if present,
    else a readable <pre> fallback — never skip the page)."""
    if not CARDS_MD.is_file():
        return
    text = CARDS_MD.read_text()
    try:
        import markdown  # type: ignore
        body = markdown.markdown(text, extensions=["tables"])
    except Exception:  # noqa: BLE001
        body = "<pre style='white-space:pre-wrap'>" + html.escape(text) + "</pre>"
    (OUT / "cards.html").write_text(f"""<meta charset="utf-8">
<title>Orchestration cards</title>
<style>
  :root {{ color-scheme: light dark; }}
  body {{ max-width: 900px; margin: 24px auto; padding: 0 16px;
          font: 15px/1.6 ui-sans-serif,system-ui,sans-serif; }}
  table {{ border-collapse: collapse; }} td,th {{ border: 1px solid #8884;
          padding: 6px 10px; }} code {{ background: #8882; padding: 1px 4px;
          border-radius: 4px; }} h2 {{ margin-top: 2em; }}
  a.back {{ font-size: 13px; }}
</style>
<a class="back" href="index.html">← matrix</a>
{body}""")


def cell(v: object) -> str:
    return html.escape("" if v is None else str(v))


def build(rows: list[dict]) -> str:
    body: list[str] = []
    for r in rows:
        verdict = r.get("verdict", "?")
        vclass = {"VALID": "ok", "DISCARD": "bad"}.get(verdict, "warn")
        arm = r["arm"]
        play = r.get("_play") or ""
        thumb = (f'<img class="thumb" loading="lazy" src="{cell(r["_thumb"])}" alt="">'
                 if r.get("_thumb") else "")
        vr_link = (f' <a class="vr" href="{cell(r["_vr"])}" target="_blank">📷 review</a>'
                   if r.get("_vr") else "")
        play_cell = (
            f'{thumb}<button class="play" data-src="{cell(play)}" '
            f'data-title="{cell(r["_root"])} · {cell(arm)}-{r["rep"]}">▶ play</button>{vr_link}'
            if play else '<span class="dim">—</span>'
        )
        body.append(f"""<tr data-arm="{cell(arm)}" data-verdict="{cell(verdict)}">
  <td class="mono dim">{cell(r['_root'][:28])}</td>
  <td><span class="arm {cell(arm)}">{cell(arm)}</span> {r['rep']}</td>
  <td><span class="pill {vclass}">{cell(verdict)}</span></td>
  <td class="num">{cell(r.get('iterations_used'))}</td>
  <td class="num">{cell(r.get('llm_calls'))}</td>
  <td class="num">{cell(r.get('tool_calls'))}</td>
  <td class="num">{cell(r.get('review_count'))}</td>
  <td class="num">{cell(r.get('review_events'))}</td>
  <td class="num">{cell(r.get('total_bytes'))}</td>
  <td class="num">{cell(r.get('elapsed_s'))}</td>
  <td class="mono">{cell(r.get('_agent'))}</td>
  <td class="mono dim">{cell(r.get('wire_model') or r.get('model_requested') or r['_model'])}</td>
  <td class="mono dim">{cell(r.get('wire_thinking') or r.get('reasoning_requested') or r['_reasoning'])}
      <span class="{'warn-r' if 'NOT APPLIED' in str(r.get('_reason_applied')) else 'dim'}">
      {'⚠ not applied' if 'NOT APPLIED' in str(r.get('_reason_applied')) else ''}</span></td>
  <td class="mono dim">{cell(r.get('_ver'))}</td>
  <td>{play_cell}</td>
  <td class="note">{cell(r.get('discard_reason'))}</td>
</tr>""")

    return f"""<meta charset="utf-8">
<title>AbstractCode — review A/B matrix</title>
<style>
  :root {{ color-scheme: light dark; --bg:#fff; --fg:#111; --line:#d8d8de; --dim:#6b6b76;
           --ok:#0a7d3f; --bad:#b3261e; --warn:#8a6d00; --rev:#3b5bdb; --nrev:#7048e8; }}
  @media (prefers-color-scheme: dark) {{ :root {{ --bg:#14141a; --fg:#eceef2; --line:#2c2c36;
           --dim:#9a9aa8; --ok:#4ade80; --bad:#ff6b6b; --warn:#fcc419; --rev:#748ffc; --nrev:#b197fc; }} }}
  body {{ margin:0; padding:24px; background:var(--bg); color:var(--fg);
          font:14px/1.5 ui-sans-serif,system-ui,-apple-system,sans-serif; }}
  h1 {{ font-size:19px; margin:0 0 4px; }}
  .sub {{ color:var(--dim); margin-bottom:18px; }}
  .wrap {{ overflow-x:auto; border:1px solid var(--line); border-radius:10px; }}
  table {{ border-collapse:collapse; width:100%; min-width:1050px; }}
  th,td {{ padding:8px 10px; border-bottom:1px solid var(--line); text-align:left;
           white-space:nowrap; }}
  th {{ font-size:11px; text-transform:uppercase; letter-spacing:.05em; color:var(--dim);
        position:sticky; top:0; background:var(--bg); }}
  .num {{ text-align:right; font-variant-numeric:tabular-nums; }}
  .mono {{ font-family:ui-monospace,SFMono-Regular,Menlo,monospace; font-size:12px; }}
  .dim {{ color:var(--dim); }}
  .note {{ color:var(--dim); font-size:12px; white-space:normal; max-width:280px; }}
  .pill {{ padding:2px 8px; border-radius:99px; font-size:11px; font-weight:600;
           border:1px solid currentColor; }}
  .pill.ok {{ color:var(--ok); }} .pill.bad {{ color:var(--bad); }} .pill.warn {{ color:var(--warn); }}
  .arm {{ font-weight:600; }} .arm.review {{ color:var(--rev); }} .arm.noreview {{ color:var(--nrev); }}
  button.play {{ font:inherit; padding:3px 10px; border-radius:6px; cursor:pointer;
                 border:1px solid var(--line); background:transparent; color:var(--fg); }}
  button.play:hover {{ border-color:var(--rev); color:var(--rev); }}
  img.thumb {{ height:44px; image-rendering:pixelated; border-radius:4px;
               vertical-align:middle; margin-right:6px; border:1px solid var(--line); }}
  a.vr {{ color:var(--rev); text-decoration:none; font-size:12px; }}
  .filters {{ margin:0 0 12px; display:flex; gap:8px; flex-wrap:wrap; align-items:center; }}
  .filters button {{ font:inherit; padding:4px 12px; border-radius:99px; cursor:pointer;
                     border:1px solid var(--line); background:transparent; color:var(--fg); }}
  .filters button[aria-pressed="true"] {{ border-color:var(--rev); color:var(--rev); font-weight:600; }}
  dialog {{ border:1px solid var(--line); border-radius:12px; padding:0; background:var(--bg);
            color:var(--fg); width:min(96vw,1000px); }}
  dialog header {{ display:flex; justify-content:space-between; align-items:center;
                   padding:10px 14px; border-bottom:1px solid var(--line); }}
  dialog iframe {{ width:100%; height:70vh; border:0; display:block; background:#000; }}
  .caveat {{ margin-top:18px; padding:12px 14px; border:1px solid var(--warn);
             border-radius:8px; color:var(--dim); font-size:13px; max-width:900px; }}
</style>
<h1>AbstractCode — verifier (<code>review_mode</code>) A/B matrix</h1>
<div class="sub">Zelda prompt on <code>react-agent:react</code> · route verified per run from the
runtime's durable store · click <b>▶ play</b> to run any generated game.</div>

<div class="filters">
  <a href="cards.html" style="align-self:center;margin-right:6px">📇 orchestration cards</a>
  <button data-f="all" aria-pressed="true">all</button>
  <button data-f="review">review only</button>
  <button data-f="noreview">no-review only</button>
  <button data-f="VALID">valid only</button>
</div>

<div class="wrap"><table>
<thead><tr>
  <th>matrix</th><th>run</th><th>verdict</th>
  <th class="num">turns</th><th class="num">llm calls</th><th class="num">tools</th>
  <th class="num">review rounds</th><th class="num">review recs</th>
  <th class="num">bytes</th><th class="num">wall s</th>
  <th>agent</th><th>model</th><th>reasoning</th><th>version</th><th>game</th><th>note</th>
</tr></thead>
<tbody>{''.join(body)}</tbody>
</table></div>

<div class="caveat">
<b>Read the columns carefully.</b> <b>turns</b> = ReAct loop iterations
(<code>_limits.current_iteration</code>) — the agent's actual think→act cycles.
<b>llm calls</b> is higher in the review arm because each verifier pass is its own
model call, so the two columns answer different questions: turns measures how much
the agent <i>worked</i>, llm calls measures what it <i>cost</i>.
<b>review rounds</b> is <code>scratchpad.review_count</code> (verifier passes actually taken);
<b>review recs</b> is ledger records for the <code>review</code> node — two independent
sources that must agree, and a disagreement discards the run.
Scores are deliberately absent: the behavioural scorer is not yet reliable on real
40&nbsp;KB artifacts, so judge the games by playing them.
</div>

<dialog id="dlg"><header><b id="dlg-title"></b>
<button onclick="document.getElementById('dlg').close()">close</button></header>
<iframe id="dlg-frame" sandbox="allow-scripts allow-same-origin"></iframe></dialog>

<script>
document.querySelectorAll('button.play').forEach(b => b.onclick = () => {{
  document.getElementById('dlg-title').textContent = b.dataset.title;
  document.getElementById('dlg-frame').src = b.dataset.src;
  document.getElementById('dlg').showModal();
}});
document.getElementById('dlg').addEventListener('close', () => {{
  document.getElementById('dlg-frame').src = 'about:blank';
}});
document.querySelectorAll('.filters button').forEach(btn => btn.onclick = () => {{
  const f = btn.dataset.f;
  document.querySelectorAll('.filters button').forEach(x =>
    x.setAttribute('aria-pressed', String(x === btn)));
  document.querySelectorAll('tbody tr').forEach(tr => {{
    const show = f === 'all' || tr.dataset.arm === f || tr.dataset.verdict === f;
    tr.style.display = show ? '' : 'none';
  }});
}});
</script>
"""


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--port", type=int, default=8899)
    ap.add_argument("--no-serve", action="store_true")
    a = ap.parse_args()

    rows = collect()
    OUT.mkdir(parents=True, exist_ok=True)
    stage_games(rows)
    stage_visual(rows)
    stage_cards()
    (OUT / "index.html").write_text(build(rows))
    print(f"built {OUT/'index.html'} — {len(rows)} runs, "
          f"{sum(1 for r in rows if r.get('_play'))} playable")
    if a.no_serve:
        return 0

    class H(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *ar, **kw):
            super().__init__(*ar, directory=str(OUT), **kw)

        def log_message(self, *ar):
            pass

    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer(("127.0.0.1", a.port), H) as srv:
        url = f"http://127.0.0.1:{a.port}/"
        print(f"serving {url}  (ctrl-c to stop)")
        threading.Thread(target=lambda: subprocess.run(["open", url]), daemon=True).start()
        try:
            srv.serve_forever()
        except KeyboardInterrupt:
            pass
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
