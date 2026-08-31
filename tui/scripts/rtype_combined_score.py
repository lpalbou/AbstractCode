#!/usr/bin/env python3
"""Combine the behavioral R-Type score with the VLM visual rubric — WITHOUT
hiding the seam.

THE TWO INSTRUMENTS MEASURE DISJOINT THINGS, BY CONSTRUCTION:
  * behavioral (scripts/rtype_review_score.py, frozen sha ab7fe7e35cb232aa):
    loads, flies, shoots, scrolls, persists, populates + a source tier.
    It never grades how anything LOOKS.
  * visual (scripts/rtype_visual_review.py, VLM rubric rtype-v2 on 13 fixed
    slots, gpt-5.4 temp 0): sprite quality, gameboy adherence, enemy design
    variety, bullet salience, VFX richness, HUD quality, style coherence.
    It never grades whether anything WORKS.
  The one deliberate near-overlap is R4 (weapon fire EXISTS, measured by
  cells/draw-calls) vs bullet_salience (projectiles are READABLE against the
  background) — existence and legibility are different claims about the same
  entity; a game can max R4 while its bullets are 1px starfield dots, which is
  the operator's own complaint. Tier-2's V_sprite_terms counts sprite VOCAB in
  source (2.25 pts max) and is the only place looks were proxied before; the
  VLM axis supersedes it in meaning but its weight is too small to matter.

HEADLINE FORMULA:
    visual01  = (mean(core 7 axes) - 1) / 4          # 1-5 -> 0-1
    combined  = 0.70 * behavioral + 0.30 * visual01

WHY 70/30:
  * The build prompt is ~a dozen mechanical demands ("fully playable",
    weapons, orbs/pods, campaign, levels, bosses...) against a repeated but
    smaller presentation demand ("black & white gameboy style", "take extra
    care of the graphics, VFX"). Playability is the gate; looks are the
    differentiator. 70/30 states that ordering without letting looks vanish:
    one full VLM point (0.25 after normalization) moves the combined score
    0.075, which is on the order of the entire behavioral arm spread (0.113)
    — a game that LOOKS a grade better can overtake, a half-grade cannot.
  * Instrument maturity: the behavioral score has a measured test-retest
    (14/15 exact, max drift 0.024) and a fixture suite; the VLM judge is one
    temp-0 call per product with no repeat-stability measurement yet. The
    less-validated instrument gets the smaller hand on the wheel.
  * NOT fitted to this corpus: the weights are stated from the prompt and the
    instruments, not tuned to produce any ordering. The variance contribution
    of each component is REPORTED so a reader can see what the choice did.

BOTH COMPONENTS ARE ALWAYS PUBLISHED NEXT TO THE COMBINED NUMBER. A missing
VLM verdict yields combined=None (UNKNOWN, not 0, and never silently
behavioral-only).

Inputs (produced by the visual review run):
  untracked/rtype-visual/summary.json            per-product VLM axes
  untracked/rtype-visual/behavioral_per_rep.json frozen-rubric per-rep scores
Outputs:
  untracked/rtype-visual/combined.json
  untracked/rtype-visual/combined.html
"""
from __future__ import annotations

import argparse
import json
import statistics
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
AXES = ["sprite_quality", "gameboy_adherence", "enemy_variety",
        "bullet_salience", "vfx_richness", "hud_quality", "style_coherence"]
W_BEHAVIORAL = 0.70
W_VISUAL = 0.30


def slug_to_key(slug: str) -> str:
    return slug.replace("--", "/", 1)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--visual-root", type=Path,
                    default=REPO / "untracked" / "rtype-visual")
    a = ap.parse_args()

    summary = json.loads((a.visual_root / "summary.json").read_text())
    beh = json.loads((a.visual_root / "behavioral_per_rep.json").read_text())["per_rep"]

    per_rep = []
    for prod in summary["products"]:
        key = slug_to_key(prod["slug"])
        v = prod.get("vlm") or {}
        nums = [v[k] for k in AXES if isinstance(v.get(k), (int, float))]
        visual01 = round((statistics.mean(nums) - 1) / 4, 4) if len(nums) == len(AXES) else None
        b = beh.get(key)
        combined = (round(W_BEHAVIORAL * b + W_VISUAL * visual01, 4)
                    if b is not None and visual01 is not None else None)
        per_rep.append({"key": key, "arm": prod["arm"], "behavioral": b,
                        "visual01": visual01, "combined": combined,
                        "axes": {k: v.get(k) for k in AXES}})

    arms: dict[str, dict] = {}
    for r in per_rep:
        arms.setdefault(r["arm"], []).append(r)
    table = []
    for arm, rows in sorted(arms.items()):
        bs = [r["behavioral"] for r in rows if r["behavioral"] is not None]
        vs = [r["visual01"] for r in rows if r["visual01"] is not None]
        cs = [r["combined"] for r in rows if r["combined"] is not None]
        table.append({
            "arm": arm, "n": len(rows), "n_combined": len(cs),
            "behavioral_mean": round(statistics.mean(bs), 3) if bs else None,
            "behavioral_sd": round(statistics.pstdev(bs), 3) if len(bs) > 1 else None,
            "visual01_mean": round(statistics.mean(vs), 3) if vs else None,
            "visual01_sd": round(statistics.pstdev(vs), 3) if len(vs) > 1 else None,
            "combined_mean": round(statistics.mean(cs), 3) if cs else None,
            "combined_sd": round(statistics.pstdev(cs), 3) if len(cs) > 1 else None,
        })
    table.sort(key=lambda t: -(t["combined_mean"] or -1))

    # seam accounting: corpus-level correlation + variance contributions
    pairs = [(r["behavioral"], r["visual01"]) for r in per_rep
             if r["behavioral"] is not None and r["visual01"] is not None]
    corr = None
    if len(pairs) >= 3:
        xs, ys = zip(*pairs)
        sx, sy = statistics.pstdev(xs), statistics.pstdev(ys)
        if sx and sy:
            mx, my = statistics.mean(xs), statistics.mean(ys)
            corr = round(sum((x - mx) * (y - my) for x, y in pairs)
                         / (len(pairs) * sx * sy), 3)
    var_b = statistics.pvariance([p[0] for p in pairs]) if pairs else 0
    var_v = statistics.pvariance([p[1] for p in pairs]) if pairs else 0

    out = {
        "schema": "rtype-combined/1",
        "weights": {"behavioral": W_BEHAVIORAL, "visual": W_VISUAL},
        "formula": "combined = 0.70*behavioral + 0.30*(mean(core7)-1)/4",
        "behavioral_rubric": "ab7fe7e35cb232aa (frozen)",
        "visual_rubric": summary.get("rubric_version"),
        "seam": {
            "pearson_r_behavioral_vs_visual": corr,
            "n_pairs": len(pairs),
            "behavioral_variance": round(var_b, 5),
            "visual01_variance": round(var_v, 5),
            "weighted_sd_behavioral": round(W_BEHAVIORAL * (var_b ** 0.5), 4),
            "weighted_sd_visual": round(W_VISUAL * (var_v ** 0.5), 4),
        },
        "per_arm": table,
        "per_rep": per_rep,
    }
    (a.visual_root / "combined.json").write_text(json.dumps(out, indent=2))

    def esc(s):
        return str(s).replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

    rows_html = []
    for t in table:
        rows_html.append(
            f"<tr><td>{esc(t['arm'])}</td><td class=num>{t['n_combined']}/{t['n']}</td>"
            f"<td class=num>{t['behavioral_mean']}<small> ±{t['behavioral_sd']}</small></td>"
            f"<td class=num>{t['visual01_mean']}<small> ±{t['visual01_sd']}</small></td>"
            f"<td class=num><b>{t['combined_mean']}</b><small> ±{t['combined_sd']}</small></td></tr>")
    rep_rows = []
    for r in sorted(per_rep, key=lambda x: -(x["combined"] or -1)):
        rep_rows.append(f"<tr><td>{esc(r['key'])}</td>"
                        f"<td class=num>{r['behavioral']}</td>"
                        f"<td class=num>{r['visual01']}</td>"
                        f"<td class=num><b>{r['combined']}</b></td></tr>")
    (a.visual_root / "combined.html").write_text(f"""<!doctype html><meta charset=utf-8>
<title>R-Type combined score (behavioral + visual)</title>
<style>body{{background:#111;color:#ddd;font:14px/1.5 system-ui;margin:20px;max-width:1100px}}
table{{border-collapse:collapse;margin:12px 0}}th,td{{text-align:left;padding:3px 10px;border-bottom:1px solid #262626}}
th{{color:#9ab}}td.num,th.num{{text-align:right}}small{{color:#777}}
.note{{background:#181c22;border:1px solid #2a3038;border-radius:8px;padding:.7rem 1rem;color:#9aa4b1}}</style>
<h1>Combined = 0.70 x behavioral + 0.30 x visual</h1>
<p class=note>behavioral = frozen rubric ab7fe7e3 (mechanics; mean of two passes).
visual = (mean of 7 VLM axes - 1)/4, gpt-5.4 temp 0, rubric {esc(summary.get('rubric_version'))}
(looks only). The two instruments grade disjoint qualities; both components are shown
beside every combined number — the seam is part of the result.
Corpus Pearson r(behavioral, visual) = {esc(out['seam']['pearson_r_behavioral_vs_visual'])}
over n={out['seam']['n_pairs']}; weighted SD contributions
{out['seam']['weighted_sd_behavioral']} (behavioral) vs {out['seam']['weighted_sd_visual']} (visual).</p>
<h2>Per arm</h2>
<table><tr><th>arm</th><th class=num>n</th><th class=num>behavioral</th>
<th class=num>visual [0-1]</th><th class=num>combined</th></tr>{''.join(rows_html)}</table>
<h2>Per rep</h2>
<table><tr><th>product</th><th class=num>behavioral</th><th class=num>visual</th>
<th class=num>combined</th></tr>{''.join(rep_rows)}</table>
""")
    print(json.dumps({"per_arm": table, "seam": out["seam"]}, indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
