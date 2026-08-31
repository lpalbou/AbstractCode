# Token triple (in / out / thinking) + live generation observability

- Status: drafted — per-seat asks ready to send; code-tui slice implementable here
- Source: operator request 2026-08-19, after run `cadac01d` (r-type task)
- Evidence run: `runtime/ledger_cadac01d-a062-40ef-bce4-630a9059e427.jsonl`

## The measured problem

Run `cadac01d` (mlx / Qwen3.8-27B-4bit): TUI showed `27k↑ 16k↓ · 1h32m`.
Reality from the ledger:

| call | wall | usage out | reasoning chars (uncounted) |
|---|---|---|---|
| 1 | 74m16s | 6,831 | **115,560** (~30k tk) |
| 2 | 18m03s | 9,223 | 1,281 |
| 3 | 8.7s | 64 | 85 |
| 4 | 5.8s | 17 | 189 |

True generated volume ≈ 46–48k tokens, not 16k. Displayed throughput
(2.9 tok/s) implied a pathological run; actual decode was a steady
~8.8 tok/s. Cause: MLX usage is computed AFTER the `<think>` strip —
`mlx_provider.py:2673` (`_postprocess_generated_text`) feeds the stripped
text into `_calculate_usage` (`:2681`) — and is a char-based estimate even
though mlx-lm counted the real decoded tokens. Two defects: thinking tokens
invisible, and estimates where exact counts exist.

Secondary finding, same run: no reasoning level was set anywhere
(`params = {temperature: 0.2}` on all 4 calls; no `_runtime.thinking` in run
vars so `_maybe_inject_runtime_thinking` no-ops; resolved route
`reasoning=None, reasoning_source=None`). The Qwen template default is
thinking ON, unbounded → the 74-minute first call. Observability must make
this legible; whether code-tui should *set* an explicit default level is an
open operator decision, out of scope here.

## Target semantics (one decision, applied everywhere)

`usage.output_tokens` = ALL generated tokens (visible + thinking + tool-call
JSON) — the number that makes cost and tok/s true. New sibling field
`usage.reasoning_tokens` = the thinking sub-count. `total_tokens = input +
output`. This matches the OpenAI convention
(`completion_tokens_details.reasoning_tokens`) and Anthropic (thinking is
inside `output_tokens`). Field ABSENT means "provider cannot split" (honest
unknown); `0` means "measured none". Displayed ↓ numbers will jump to the
true (larger) values — that is the point, not a regression.

## Ask: abstractcore seat

1. **MLX (and HF) usage from real decode counts.** The provider iterates
   `stream_generate` chunks; count them. Compute usage BEFORE the think
   strip; set `reasoning_tokens` from the stripped span's token count (exact
   if counted during strip, else tokenize the reasoning text — never a char
   heuristic). Keep `TokenUtils.estimate_tokens` only as a labeled fallback
   (`usage.estimated: true`), never silent.
2. **Map remote providers into the same shape**: OpenAI
   `completion_tokens_details.reasoning_tokens` → `reasoning_tokens`;
   Anthropic thinking-block tokens likewise; providers that cannot split
   omit the field.
3. **`on_delta: Callable[[str], None]` kwarg on `generate()`** (live-tap
   prerequisite): invoked per chunk inside the existing provider loops,
   try/except-wrapped so a callback error can never affect generation.

## Ask: abstractruntime seat

1. **Verify usage passthrough** of the new field in the LLM_CALL handler
   (`integrations/abstractcore/effect_handlers.py` ~:2013) — result dicts
   look verbatim today; confirm no key filtering drops `reasoning_tokens`.
2. **LiveGenerationTap** at the same call site (the one seam holding
   run_id/step_id/attempt AND making the generate call): in-memory registry
   keyed `(run_id, step_id)` with tokens-so-far, tok/s, last-delta age,
   rolling ~2KB tail; batched flush (~1s) of raw chunks to
   `runtime/logs/llm_live/<run_id>.<step_id>.txt`. Ledger semantics
   untouched — live file is ephemeral debug exhaust, deleted on step
   completion by default (keep-last-N config), one final summary line
   (tokens, seconds, tok/s, finish_reason). Flag-only loop warning (n-gram
   score on the tail) is allowed; caps/kills are not (ADR 0001).

## Ask: abstractgateway seat

1. **Run-status payload** (the one the TUI already polls): per-call and
   run-total usage triple, plus live tap counters
   `{tokens_out, tok_s, last_delta_age_s, tail_preview}` for in-flight
   steps, read from the in-process tap registry. No new transport.

## code-tui (this repo — mine)

1. `protocol.rs` `parse_usage`: read `reasoning_tokens` (plus
   `*_details.reasoning_tokens` fallback keys); absent → None, not 0.
2. `exec.rs:975` last-run line and session totals in `chrome.rs`:
   `27k↑ 46k↓ tk (30k think)` — think segment rendered only when the field
   is present (honest unknown otherwise).
3. In-flight status once the gateway exposes live counters:
   `thinking (cycle 1) · 21.8k tk · 8.8 tok/s` replaces the bare timer.
4. Show the run's effective thinking level in the run header when the
   resolved route reports one, and `think: model-default` when absent.

## Ordering

core (fields exist) → runtime/gateway (passthrough + tap) → code-tui last;
TUI degrades gracefully at every intermediate stage. The code-tui parsing
slice (1, 2) can land first — it is inert until the field arrives.
