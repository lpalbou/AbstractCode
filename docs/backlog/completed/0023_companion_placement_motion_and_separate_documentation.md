# 0023 — Companion placement, expressive motion and separate documentation

- **Status:** completed
- **Created:** 2026-10-30
- **Completed:** 2026-10-30
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

The current-audio controls also appear beside the bottom speaking histogram and
in the avatar action row. They pause/resume the selected audio only, preserve
message-level playback controls, and cannot start another reply from a stale click.

## Validation

Placement persistence and unknown-field compatibility; actual top/bottom geometry
through resize and varying composer/audio rows; transcript dimensions unchanged;
Docs↔Companion open/close, separate drafts/history/context and proper header route;
visible multi-frame phase differences at main and preview sizes with fixed audio;
pose-specific frame budgets and reduced/hidden cleanup. Exact selected audio-generation pause/resume and no extra speech request;
80-column button bounds and stable mouse press across meter/tick updates.
Native isolated terminal captures, targeted and broad checks, docs/skills, local install only. Follow-up to0022.

## Completion report

Position is persisted locally as top-right/bottom-right, with Top right as the
backward-compatible default and unknown future values retained. Existing absolute
layout measures against the actual transcript region; both positions keep the
same header-group centre and clear changing composer, attachment and audio rows.
The transcript retains its full width. Short/narrow terminals use the compact
control; no extra focus owner, dependency or positioning API was added.

AbstractTUI's opt-in generic Avatar clock now uses Ready2fps, Waiting about6fps,
and active poses at most12fps. Ready breathes, Listening leans/nods, Thinking
scans, Working hovers with particles, Speaking has syllabic motion even with fixed
levels, and Waiting pulses with sequential indicators. Halo Listening uses inward
rings and Speaking outward rings. Error and paused speech stay still. Hidden,
reduced-motion and disposed views stop clocks. Ready is intentionally no longer
zero-output with animation enabled; reduced-motion/hidden fixtures remain quiet.

Documentation and Companion have separate fixed-title drawers, entry points,
histories and drafts over the existing shared ChatPanel and grounded-QA backend.
The avatar opens Companion; account and narrow Menu entries also work with the
avatar disabled. Docs retains its authorized bounded main-conversation context
plus corpus. Companion retains fresh bounded requests/replies without tool
payloads. No new model, service, execution authority or backend contract was added.

Both the bottom speaking strip and the full avatar action row expose current
speech pause/resume controls with accessible labels/help. They dispatch directly
to the existing player, never start another message, and keep per-message controls.
Meter-level updates are isolated so they cannot dispose a pressed control. Fixed
an independently discovered overflow that put the bottom button outside the
viewport, and a drawer-close teardown callback reading disposed signals.

Validation: Code all-targets1204 passed/23 ignored; AbstractTUI2535 passed/89 ignored;
all-target Clippy, formatting and diff checks passed. Fake-host tests prove exact
selected companion audio generation, one TTS request, no stale replay, useful
80-column meter and click survival across meter/tick updates. Independent QA passed
42 phase/style/size motion cases and16 Playing/Paused×position×size geometry cases,
including hidden/reduced cleanup. Actual-root tests cover position changes,
separate drafts/context, teardown and avatar-disabled entry access.

Installed build SHA256:
f1a7521643a6ec53a26c01b7c4d7a7b809ba72c7f5fde34e95e80845e102b1ad.
Final native PTY checks passed/exit0 at220×60 truecolor and80×24 ANSI256 with saved
position changes, live220→80×18→80×24→220 resizing and distinct Documentation entry.
Private temporary preferences and synthetic playback fixtures preserved user
settings/tasks and physical audio. Independent review Approved. Local install only;
no release/push, live-process restart or Gateway restart. Engine commit0853ea6.
Docs, package skills and LLM aggregates updated.

Evidence: workspace untracked/avatar-position-{code,engine}-tests.log,
avatar-position-engine-clippy.log, avatar-position-{build,release-build,install}.log,
avatar-position-qa/validation-review.md, native capture/report subdirectories,
and avatar-motion/*.gif with benchmark-32x17.json. Shared entity identity and
cross-client subscription architecture remain separate backlog work.

Original path: planned/0023_companion_placement_motion_and_separate_documentation.md;
final path: completed/0023_companion_placement_motion_and_separate_documentation.md.
