# 0023 — Companion placement, expressive motion and separate documentation

- **Status:** planned
- **Created:** 2026-10-30
- **Area:** Code companion preferences/entry points, AbstractTUI Avatar
- **Work item:** abstractcode-0023
- **ADR impact:** none; retain generic presentation, existing Gateway workflows and independent side histories.

## Summary and current code reality

The operator requests Top right and Bottom right choices, more expressive activity
animation, and a companion entry visibly distinct from Documentation. The main
avatar already floats over a bounded region, but has only a top anchor. Ready
and Waiting remain static under the previous animation policy. Avatar clicks
select Run scope correctly but open the same tabbed Docs/This run drawer, which
makes the product roles confusing.

## Scope and boundaries

Persist local placement, defaulting Top right; measure within the actual transcript
region so either anchor clears changing composer, attachment and audio rows.
Keep the same horizontal alignment and transcript width. Give all activity poses
meaningful motion, using a lower-rate idle clock and bounded active cadence.
Reduced motion, hidden and disposed views still stop activity clocks. This is an
explicit change from previous static Ready/Waiting behavior, requested by the user.

Documentation and Companion have distinct entry points, titles and histories while
reusing the engine chat panel and existing Gateway QA workflow. Preserve previously
authorized bounded context in Docs, the companion's fresh conversation context,
voice destination isolation and main-task read-only boundaries. No new provider,
workflow loop, service, shared preference owner or model instance.

## Validation

Placement persistence and unknown-field compatibility; actual top/bottom geometry
through resize and varying composer/audio rows; transcript dimensions unchanged;
Docs↔Companion open/close, separate drafts/history/context and proper header route;
visible multi-frame phase differences at main and preview sizes with fixed audio;
pose-specific frame budgets and reduced/hidden cleanup. Native isolated terminal
captures, targeted and broad checks, docs/skills, local install only. Follow-up to0022.
