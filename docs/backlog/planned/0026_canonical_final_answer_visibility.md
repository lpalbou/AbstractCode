# 0026 — Canonical final answer visibility

- **Status:** planned
- **Created:** 2026-10-30
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
