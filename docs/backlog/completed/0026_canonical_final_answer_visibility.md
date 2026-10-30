# 0026 — Canonical final answer visibility

- **Status:** completed
- **Created:** 2026-10-30
- **Completed:** 2026-10-30
- **Area:** AbstractCode TUI / AbstractTUI
- **Work item:** abstractcode-0026
- **ADR impact:** none expected; preserve generic presentation and canonical Gateway ledger ownership.

## Summary and current code reality

In session acode-ebb09ff0ae8b, one completed turn leaves its real answer inside collapsed cycle4, speaks that text, and reports no readable final answer. The next turn in the same session displays its answer normally. Ledger shape and completion ordering must be compared rather than assuming every final cycle is a valid answer.

## Scope

Trace authenticated read-only Gateway records and client fold/terminal reconciliation; expose canonical final output exactly once outside reasoning cycles. Keep nonfinal/helper outputs distinct, and retain replay/reconnect and historical speech suppression. No user-run mutations or speculative promotion of arbitrary model text.

## Validation

Reproduce the failing turn with sanitized ledger-shaped fixtures, compare the successful sibling turn, test completion/record order and replay, verify normal visible final and no false completion notice, deduplication and alignment with the selected read-aloud text.

Two adversarial workers investigate these related user reports independently.
Local installation only after review and checks; preserve live sessions and audio.

## Completion report

Read-only Gateway inspection compared the failing and next successful turn in
session acode-ebb09ff0ae8b. Both have canonical agent output.answer atcursor15 and
wrapper output.response atcursor10, byte-identical within each pair. The answers
are3742/7038characters. They were stored correctly; the original network ordering
was not logged and remains inferred from source and the matching screenshot.

A REST fallback reads a ledger page, then probes terminal status. Unlike the other
two terminal paths, it omitted a final drain, so an answer appended between those
reads could be lost. finish also discarded authoritative run.output, and the fold
excluded late canonical completion after producing a missing-answer conclusion.
Cycle narration could still read the same text from the last collapsed cycle.

Added the missing drain and canonical run-output reconciliation through the shared
parser/fold/effect path before status-only fallback. A late own root/bound-agent
completion can repair this turn's provisional notice, summary and verdict state.
No arbitrary cycle/helper output is promoted; failed/cancelled/previous-turn guards
remain. Repeated root/child canonical ends render one final answer; the matching
final cycle is removed. Artifact offload uses the existing fetch lane. Voice
ownership, queue and historical speech suppression are unchanged.

Adversarial cross-review caught and resolved warning-summary duplication and full/
partial scrollback eviction of provisional conclusion rows. Exact owned identities
replace the provisional span; authoritative verdict recovery works even when its
rows were evicted. The standing truncation notice remains. Transcript75 tests and
runner46 include deterministic REST page→terminal-probe→tail races for root/agent,
canonical snapshot/offload recovery, late output, helper/cancel/stale guards and
both real hysteresis truncation cases. Independent replay of both actual saved
bundles yields one normal final answer each with no duplicate cycle or false note.

A concurrent compact-web-settings commit removed layout-specific account help.
Its stale parity assertion was independently reviewed and updated to preserve the
shared labels/status/refusal contract (e09ab3d); no account behavior was changed.

Final integration: Code all-targets1217 passed/23 ignored; AbstractTUI2538 passed/
89 ignored; both all-target Clippy, formatting and diff checks passed. Two explicit
adversarial workers implemented separate faults and independently reviewed each
other; root inspected real artifacts and ran broad/native checks. Final verdict:
Approved. Installed SHA25694d0f5f9badcc961a074428cb76ba8948fec02c20b4f85566bc8238755d65ad9.
Local installation only; no release/push or live user-process/Gateway restart.
Private preferences and synthetic audio fixtures preserved user settings, runs and
physical playback. No inference was started during native validation.

Documentation, existing package skills and LLM aggregates updated. Evidence:
workspace untracked/shape-answer-review.md, shape-overlay-qa/ captures/reports,
shape-overlay-engine-all.log, shape-answer-code-all.log, matching Clippy/build/
install logs, and final-answer-visibility/report.md, sanitized-comparison.json and
actual-replay.log. Original path planned/0026_canonical_final_answer_visibility.md;
final path completed/0026_canonical_final_answer_visibility.md. ADR impact remains none.
