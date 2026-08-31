# Cycle-2 adversarial review — integration seam (reviewer 3)

Scope: `src/ui/mod.rs`, `src/ui/modals.rs`, `src/lib.rs`, `src/main.rs`,
`src/gateway/*`, `Cargo.toml`, `CHANGELOG.md`, `README.md`, `docs/*`,
`tests/*` (appends) — plus the cross-cutting audit of the six-agent /
two-wave integration on top of commit 34b9447. Baseline before edits:
full `cargo test --release` green.

## Findings

### P1

- **P1-1 (sse.rs, FIXED)** — a CRLF line terminator split across two
  `push()` chunks fabricates an event boundary. `SseParser::push`
  handles CRLF only when both bytes are in the SAME buffer
  (`sse.rs:34-37`): a chunk ending `…data\r` swallows the CR and cannot
  see the LF; the next chunk starting `\n` parses as an EMPTY line —
  the SSE event-dispatch signal — so one `data: x\r\n` line becomes
  "line + event boundary". Consequences on a CRLF-emitting server: an
  event dispatched early with partial fields, the remainder parsed as a
  second event (for `stream_ledger`: a skipped record + a phantom).
  Probabilistic (~1 / read-size per boundary) but certain over a long
  stream. Fix: a `pending_cr` carry flag — a chunk-final CR records the
  carry; the next push swallows exactly one leading LF. Regression test
  `crlf_split_across_chunks_is_one_line` (unit, sse.rs).

- **P1-2 (modals.rs `open_ask`, FIXED)** — Esc on the ask-user modal
  was a no-op blink: the handler called only `close_modal()`
  (modals.rs:488-495) without recording the dismissal, and
  `wire_wait_modals` re-runs on the close's `modal_epoch` bump — wait
  still pending, not dismissed, no modal open → it reopens the same
  prompt in the same flush. The hint promised "Esc keeps the run
  waiting" (implying the modal closes); the approval modal already does
  this correctly via `dismissed_wait` (modals.rs:357). Fix: Esc records
  `dismissed_wait = step_id`, exactly like approval; Enter on the empty
  composer reopens (the existing `submit()` empty branch — symmetric
  with approval). Regression test `ask_escape_defers_and_stays_closed`
  (headless, appended at end of tests/headless_ui.rs).

### P2

- **P2-1 (ui/mod.rs, FIXED)** — `new_session` and `switch_session`
  duplicated the nine-write session-boundary reset block (fold reset,
  totals, last_call_rate, run_id, phase, auto_approve, paused, queue
  swap, focus) with only the Info text differing. This is exactly the
  drift class this review hunts (one path gains a reset, the other
  forgets). Extracted `reset_session_state(store, ctx, old_sid, note)`;
  both callers now share one authority. No behavior change —
  the pre-existing session-boundary tests pin the matrix.

- **P2-2 (ui/mod.rs + tests, FIXED)** — the Ctrl+L pair (root shortcut
  + action-registry binding) carried an author-verified "can never
  double-fire" claim with no test. A double fire is observable: each
  `veil_and_vacate` parks its layer at `top_z() + 1000`, so a second
  fire in the same dispatch stacks a veil at z=2000. Added
  `ctrl_l_fires_exactly_once_without_a_modal` pinning `top_z == 1000`
  after the keypress turn (and vacate back to 0).

- **P2-3 (gpu.rs, FIXED — comment honesty)** — the module doc claimed
  stop/cadence flips apply "within ~250ms"; that is true only while the
  thread is in its sliced SLEEP. Mid-`host_gpu_metrics()` HTTP call the
  thread is uninterruptible for up to the 60s read timeout (its posted
  sample is still generation-gated — correctness holds; the lag claim
  didn't). Comment tightened; no code change needed (bounded, gated).

- **P2-4 (repo hygiene, FIXED)** — `hi.txt` ("hi", 2 bytes, mtime 01:23
  tonight) at the repo root: live-pty-smoke residue (an agent-run
  `write_file` into the workspace root = repo root). Deleted. Smoke
  scripts should point `--workspace` at a temp dir so approval-proof
  writes never land in the tree.

- **P2-5 (packaging/docs, FIXED)** — Cargo.toml still said 0.3.0 over a
  feature wave (new commands `/context`, `/gpu`, `/redraw`, Ctrl+L,
  conclusion fixes) → bumped 0.4.0 (SemVer minor). CHANGELOG's
  Unreleased (six concurrent appenders) merged into one dated
  `[0.4.0] - 2026-07-23` section; every claim spot-verified against the
  tree (see verification notes below). `llms-full.txt` regenerated
  (`scripts/update_llms_full.py`) AFTER the merge — it embeds
  CHANGELOG.md and was already stale against docs/api.md.

### P3 / notes (no action needed)

- `store.images` survives session switches — verified benign: it is a
  keyed, capped (32) artifact-id → bitmap CACHE (`image_for` /
  `upsert_image`); a wiped fold references no old artifact ids, entries
  age out by cap. Not session state.
- `store.last_outcome` is not reset at session boundaries — verified
  benign: the mailbox is consumed synchronously by `wire_queue_drain`
  the moment phase reads Idle, so it cannot survive to a boundary.
- `wire_llm_meter` across a session switch — verified correct: the
  fold reset zeroes `stats.llm_calls`, so the `calls > prev_calls`
  guard rejects a rate computation against the stale `prev_start`;
  both cells then re-seed from the fresh fold. `last_call_rate` itself
  is explicitly reset by the (now shared) session-boundary block.
- GPU meter deliberately NOT session-scoped (gateway-host fact);
  `context_window` deliberately NOT reset (persisted global) — both
  confirmed by intent, not accident.
- An old gpu-poller thread can linger up to the 60s HTTP read timeout
  after `/gpu` off before observing the GEN bump — bounded, its posts
  are generation-gated, no leak across repeated toggles (each start
  bumps GEN; superseded threads exit at the next slice/loop check).

## Claim verification (CHANGELOG merge)

Spot-checked against the tree before merging: `--context`/
`--context-window` aliases (cli.rs:136); 10-row completion dropdown
(chrome.rs:680); `IMAGE_ENTRY_CAP = 32` (store.rs:127); boot `LoadMcp`
(lib.rs:240); F1 self-heal test (ui/mod.rs unit); F7 on_skipped counted
notice + `is_following` guard (runner.rs:1226-1231); FetchAnswer wired
in live runner (runner.rs:752), rehydration (runner.rs:1151) and exec
(exec.rs:275, polling `answer_run_id` at :386); `?` legend + prefix_fit
whole-drop (chrome.rs:947/1009); heartbeat = chrome band only
(ui/mod.rs `heal_chrome_rows`); version single-sourced from
`CARGO_PKG_VERSION` (cli.rs:5, help title modals.rs:1961). No further
overclaims found beyond the already-corrected HDR-2b one.

## Cross-cutting audit results

- **Effects wired exactly once**: all 15 `wire_*`/`spawn_*` calls in
  `root()` have exactly one production call site (grep-verified; the
  second `wire_conn_self_heal` site is its own unit test).
- **No duplicate dispatch arms**: `dispatch_command` has one arm per
  `Command` variant; the previously-folded `/gpu` collision is the only
  one that ever existed (comment at ui/mod.rs:696 records it). `/`
  parse table has single mappings for context/gpu/redraw.
- **submit() ordering**: `?` → empty-reopens-wait → command parse →
  `@mention` route → focus route. `?` cannot collide with the parser
  (commands parse leading-`/` only).
- **Boot order** (lib.rs): prefs → conn → session mint → mount{store
  seeds incl. `context_window` BEFORE first paint → runner spawn → boot
  loads as posted mpsc commands (LoadMcp never blocks paint) → queue
  restore PAUSED → goal restore} → run. `wire_startup_notices` surfaces
  the engine diagnostics lane. Pty env isolation intact
  (`ABSTRACTCODE_PREFS_FILE`, `ABSTRACTCODE_GATEWAY_*`,
  config.rs:48-67).
- **gateway/entities.rs**: no drift observed — entity lane surface
  unchanged tonight, `stop_poller` still on the runner Shutdown path
  beside `gpu::stop()` (runner.rs:204-206).
- **modals.rs**: help modal wrap verified safe at 80 cols — `help_rows`
  wraps against the CLAMPED size (`modal_size` first, desc_w derived
  from it) and the body lives in a `Scroll` with exact `content_size`,
  so doubled row counts scroll instead of overflow; fit is test-pinned
  at real content + geometry. Approval Esc-defers + `f` JSON toggle +
  per-tool `p` pins present. Picker `retire()`/0250 comments accurate
  for 0.2.1 (activation via engine `on_activate`; Button mouse path
  still disposal-unsafe, so the deferral stays). No dead modal code
  found (every `open_*` has a dispatch caller).

## Handoffs (reviewer-owned files)

- **Reviewer 2 — `tests/chrome_width_torture.rs` (in flight at my gate
  time, mtime minutes old)**: (a) `cargo fmt --check` flags one
  import-wrap diff in it; (b) its
  `chrome_degrades_whole_item_at_every_width` assertion fails at width
  60 (`gate…` fragment in the header facts — a red-first test or a
  mid-fix against chrome.rs). Both are inside your file pair
  (test + chrome.rs); everything else in the tree is green around it
  (353/353 with that one test target excluded). Deliberately not
  touched by me — an fmt/fix write against your live edit would
  clobber.
  CLOSED cycle-3 (verified, no edit needed): the suite passes (2/2)
  and `cargo fmt --check` is clean repo-wide — reviewer 2's mid-flight
  fix landed as they reported.
- Informational for reviewer 2: `commands.rs` HELP_EXTRA "?" row +
  `docs/api.md` key table agree (the pinned help-wrap test re-derives
  if wording changes); `chrome.rs` `ctx_meter` thresholds match the
  CHANGELOG words (warn ≥75%, error ≥90%).

## Gates (final, 2026-07-23 ~05:25)

- `cargo test --release` — **353 passed / 0 failed** across every
  suite except reviewer 2's in-flight `chrome_width_torture.rs` (1
  failing assertion there, theirs, see handoff). Includes the three
  new regression tests; the ask-Esc fix carries a
  fails-without-the-fix proof.
- `cargo clippy --release --all-targets` — **0 warnings**.
- `cargo fmt --check` — clean EXCEPT the same in-flight reviewer file.
- `cargo build --release` — binary builds as **0.4.0** (Cargo.lock
  refreshed).
- Live pty smoke (scripts/pty_live_smoke.py, release binary, live
  gateway) — **PASS**: boot wordmark → session line → prompt →
  approval modal → `a` approve (one retry round) → final `✦ assistant`
  card → tool write proven on disk in the gateway-managed workspace
  (`aa34a25b…/pty-proof.txt`) → clean Ctrl+C exit code 0.

## Integration verdict

**Coherent enough to hand the maintainer as one release (0.4.0), with
one open edge**: reviewer 2's chrome-width test file must land green +
fmt'd before the final tag. Everything the six build agents shipped
cross-checks: no double-wired effects, no duplicate dispatch arms, no
dead modal code, session-boundary semantics correct by intent, boot
order correct, version/docs/CHANGELOG now agree on 0.4.0, and the
live end-to-end path (start → approval → answer → conclusion →
composer freed → exit) is proven against the real gateway.
