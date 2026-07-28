# Cycle-2 adversarial review — run-conclusion + reliability seam (2026-07-23)

Scope: the concurrent landing of (1) offloaded-answer conclusion
(`FlowOutput.offload_artifact` / `FetchAnswer` / `resolve_offloaded_answer`)
and (2) failed-answer-source conclusion (`Fold::subrun_terminal`,
`runner::finish` subrun branch, exec's answer-source poll) plus the
splitless-usage repair and OBS-1a-live accessors. Files audited:
`src/transcript.rs`, `src/runner.rs`, `src/protocol.rs`, `src/exec.rs`,
`tests/*`. Baseline at review start: 342 tests green.

Verdicts: **5 confirmed (fixed this pass)**, 5 documented P2s, 1 handoff,
9 attacks refuted with evidence.

## Confirmed — fixed in this pass

### F1 (P1) rehydrate folds ledgers in id order: a deep cycling child can
### bind as answer-source before its parent is known

`runner.rs` (pre-fix ~1507): `rehydrate_run_into` sorted bundle ledgers
root-first then **lexicographic run-id order**. The answer-source binding
(`transcript.rs:547`) treats an **unknown parent as first-level**
(`parents.get(rec_run).map(…).unwrap_or(true)`) — correct live, where a
subrun's records can only arrive after its discovering wait record folded
(FollowRun starts the stream). In rehydration that invariant broke: a
depth-2/3 cycling run (coder trees: builder/verifier) whose UUID sorts
before its discovering parent's ledger folds with `parents` empty →
binds `agent_run_id` → its **intermediate** flow end is answer-shaped →
for a root that never completed (failed/cancelled roots; older
still-`waiting` wrapper roots in the 44-run population) the restored turn
renders the delegate's intermediate output as the final answer and
suppresses the honest "(this turn ended without an answer)" card.
Completed roots were shielded only by fold order (root's own end folds
first and wins the `!finished` gate) — shielding by luck, not by design.

**Fix**: fold bundle ledgers in **discovery order** (BFS from the root
through the fold's own `FollowRun` effects — the exact live interleave),
then any discovery-unreachable leftovers (trimmed captures) in id order.
`runner.rs::rehydrate_run_into` + helper `fold_bundle_ledger`.
**Test**: `runner::tests::rehydrate_folds_in_discovery_order_never_id_order`
(grandchild id `aaa-child` sorts before middle run `mmm-level1`; asserts
no false conclusion, no false binding, and the honest no-answer card).

### F2 (P1) exec exits 0 when the answer-source subrun is CANCELLED

`exec.rs` (pre-fix ~406): the answer-source poll called
`fold.subrun_terminal(&agent_rid, "cancelled")` → fold concludes with
`failed == false` (a cancel is not a failure — correct for the fold and
for the TUI queue, which maps it to `RunOutcome::Cancelled`) → exec's
`fold.finished` branch returned `if failed { 1 } else { 0 }` = **exit 0
for a cancelled run**. The root-terminal branch exits 130 for the same
situation observed via the root — scripts reading 0 as "answer produced"
were lied to, and which code you got depended on which poll won.

**Fix**: shared `exit_code_for_status` (completed→0, cancelled→130,
else→1) used by both terminal branches; the subrun-conclusion path
returns 130 when the conclusion came from a cancelled answer-source.
**Test**: `exec::tests::exit_codes_match_the_documented_truth_table`.

### F3 (P1) exec: a cut ledger drain could fabricate "completed without a
### readable final answer"

`exec.rs` (pre-fix ~390-406): when the answer-source run reads terminal,
exec drains its ledger then calls `subrun_terminal`. A `get_ledger` error
mid-drain **broke the loop and concluded anyway** — for status
`completed` that mints the "completed without a readable final answer"
info card while the real answer may sit in the un-drained tail (network
blip → permanently wrong conclusion; the loop never re-reads after
`finished`). The failed/cancelled verdicts are status-truth and don't
need the ledger.

**Fix**: the `completed` verdict now requires a fully-drained ledger; a
cut drain skips the verdict this sweep and retries next sweep (persistent
unreadability ends at the honest `--timeout` 124, never a fabricated
no-answer conclusion).

### F4 (P2, fixed) SSE `done` path skipped the terminal-save-window drain

`runner.rs` (pre-fix 1240-1244): `stream_run`'s `Ok(true)` arm (gateway
sent `event: done`) called `finish()` without `drain_rest` — the
poll-detected terminal path has the belt ("catch records appended in the
terminal-save window"), the SSE path did not. If a subrun's flow-end
record lands in that window, `finish` → `get_run` = completed →
`subrun_terminal("completed")` → fabricated "without a readable final
answer" with no later re-read (the stream thread has returned). Same
class as F3, stream lane.

**Fix**: `drain_rest` before `finish` in the `Ok(true)` arm (cursor-based,
so the extra read is duplicate-free and usually empty).

### F5 (P2, fixed) `subrun_terminal` conclusion on "unknown" was
### caller-dependent

`transcript.rs`: the fold treated any unrecognized status as a failure
conclusion. Both callers guard today (`finish()` early-returns on
"unknown", exec matches terminal statuses) — but the invariant "an
unreadable status never concludes a turn from a subrun" lived only in
call sites. A future caller (rehydrate growth, a new poll lane) passing
"unknown" would kill a healthy run.

**Fix**: structural no-op for `"unknown"` inside `subrun_terminal`
(matching the documented contract in `finish()`), pinned by
`subrun_terminal_ignores_helpers_unknowns_and_goal_iterations` (extended).

## Documented — not fixed (P2)

- **F6 context-lane placeholder leak**: the turn concludes BEFORE the
  offloaded answer fetch resolves (by design — the composer is never
  held hostage). `Fold::chat_messages` reads `Item::Assistant` text, so a
  prompt sent inside the fetch window carries the placeholder text ("
  (retrieving the full answer…)") — or, after a failed fetch, the
  failure label — as the previous assistant turn in `context.messages`.
  Window = one artifact GET (ms on localhost). The failure label is at
  least honest ("stored as artifact X but could not be retrieved").
  Fixing would mean blocking conclude on the fetch (rejected) or turn
  dropping in chat_messages (loses the user's words). Accepted.
- **F7 runner-loop blocking fetches**: `Cmd::FetchAnswer` (like the
  pre-existing `Cmd::FetchImage`) runs its HTTP GET synchronously on the
  runner command loop — a slow gateway can hold Resume/Cancel/Steer
  behind it for up to the 60s read timeout. Pre-existing pattern;
  worth a worker-thread lane if it ever bites live.
- **F8 doubly-terminal race drops the late answer card**: if the ROOT
  fails while the agent's answer record is still in stream flight, the
  root-failed conclusion sets `finished` and the answer record then
  folds into the `!finished`-gated branch → no card. Display-only, needs
  a root that fails after its agent answered (not observed live).
- **F9 rehydrate re-fetches offloaded answers every boot**: each replayed
  prior turn with an offloaded answer issues one `FetchAnswer` (≤
  `--replay-turns`, default 20). Bounded and correct (placeholders must
  resolve); noted as boot-time chatter.
- **F10 exec `answered` vec is write-only**: dead bookkeeping in
  `exec::run` (waits dedup via the fold). Harmless; left to avoid churn.

## Handoff (files not owned by this review)

- **ui/chrome (Lane B), OBS-1a-live render**: `Fold::live_llm_call()`
  returns the started record's OWN `started_at` as epoch ms (record
  truth). The renderer MUST compute elapsed with saturating arithmetic
  (`now_ms.saturating_sub(started_ms)`, render 0s when the server clock
  is ahead of the client clock) — skew is real across machines. As of
  this review the accessors have no production consumer (unit tests
  only); this is the frozen-interface note for whoever wires them.
  CLOSED cycle-3: nobody wires them — the accessor pair is REMOVED as a
  dead second rate authority (chrome renders the monotonic client-clock
  twins, so the skew hazard this note guards against is structurally
  impossible in every remaining render path; verified — no epoch-ms
  subtraction renders anywhere).
- **ui/mod.rs `wire_llm_meter`**: computes tok/s from client-observed
  wall time (understates; labeled "(last call)") — consider swapping to
  `Fold::last_call_rate()` (provider `gen_time` truth) when Lane B lands;
  both exist, only one should render.
  CLOSED cycle-3: one authority now — the client-clock meter, with its
  numerator fixed to the cumulative-output delta (presence P1-A);
  `last_call_rate()` is deleted. A gen_time-truth swap stays a possible
  FUTURE upgrade riding `protocol::gen_time_ms_from_record` (kept,
  tested); it was deliberately NOT wired in cycle-3 (remove dead code,
  not re-plumb).

## Attacks refuted (evidence)

1. **Double/conflicting outcome-mailbox writes**: every conclusion site
   gates the write on the finished-edge — `post_records` (`finished_now =
   f.finished && !was_finished`, runner.rs), `finish()` subrun branch
   (`concluded_now`), `finish()` root branch (`!was_finished`). Orders
   subrun-failed→root-completes and answer→subrun-terminal both write
   exactly once; the queue drain consumes with take-semantics
   (`last_outcome` reset to None on read, ui/mod.rs:1765).
2. **Stale-stream conclusion of a new run**: all three conclusion
   closures re-check `fold.root_run_id() == root && is_following(rid)`
   inside the fold update (post_records runner.rs:1128, subrun finish
   runner.rs:1393, root finish runner.rs:1424); `begin_run` resets
   `followed` to the new root.
3. **`resolve_offloaded_answer` fold-identity corruption**: matching is
   content-addressed by exact placeholder text embedding the artifact id;
   session switch replaces the whole Fold (ui/mod.rs:774) → no-op; a
   second resolve is a no-op (pinned in
   `offloaded_answer_concludes_and_fetch_resolves_the_words`); identical
   artifact ids imply identical bytes (content-addressed store).
4. **FetchAnswer vs StopFollows race**: both are commands on the serial
   runner loop; the fetch is content-addressed and needs no stream state.
5. **`is_flow_end_record` false positives**: scanned every captured live
   ledger fixture (failed_agent_subrun_tree, offloaded_answer_tree,
   coder_run_tree, run_tree_basic_agent, agent_subrun_ledger — 250
   records): `result.completed == true` appears ONLY on terminal `done`
   records; resume records carry `{"resumed": true}`, wait_until
   `{"ready": true}` (pinned in `flow_end_record_detection`).
6. **Splitless repair overwriting a real split**: repair gate is
   `input == 0 && output == 0` only; disagreement with normalized split
   pinned (`rec_split`); NEW tests pin the partial-split (input>0,
   output==0) disagreement, malformed raw JSON-string, and raw-total-only
   (no split ⇒ no repair) cases.
7. **live-call accessors**: ts-less started record → None (never
   fabricate); most-recent-started wins; a non-tracked call's completion
   never clears the slot; failed completions clear; `gen_time <= 0` →
   None (no division); cleared on begin_run/terminal/rehydrate. All
   pinned in transcript tests.
8. **exec hot spin / deadline**: 300ms sleep every sweep (the one
   `continue` lands on the finished branch next sweep); the deadline is
   checked at sweep top and inside the per-run page loop; inner drains
   are bounded by terminal (non-growing) ledgers.
9. **Reattach auto-draining a restored queue**: session queues restore
   PAUSED (`restore_session_queue`, ui/mod.rs:870), so a replayed
   conclusion's Success outcome cannot start stashed work.

## The 44 waiting wrapper roots (attack surface 8), verified by code walk

Reattach to a session whose newest root is a long-answered but
still-`waiting` wrapper: `probe_attach` classifies it live → `attach`
streams the tree from cursor 0 → the agent subrun's answer record folds →
`finished_now` → outcome Success, composer freed, StopFollows; the root
stream keeps observing honestly (no fabricated "completed" — `run_terminal`
only fires if the root ever actually terminates). OLDER waiting roots in
the same session are rehydrated as prior turns and get their answers from
the same fold logic (discovery-order after F1). Every boot repeats the
reattach against the eternal root — cost of the server-side design,
documented in lane-a-diagnosis §3.

## Verdict on the P0 ("the agent never finishes")

Dead for all three classes, with the two fabrication holes this review
found now closed:

- **offloaded answer**: concludes on the placeholder + fetch (fixture +
  unit + live run c61e4ac9 evidence from the build lane); rehydration and
  exec resolve synchronously/asynchronously with honest failure labels.
- **failed answer-source subrun**: concludes via `subrun_terminal` from
  the stream-terminal report (fixture replay of the real 76fc3fcb tree +
  live pty verification from the build lane); helpers/goal iterations
  can't conclude; unknown statuses can't kill a healthy run (now
  structural).
- **honest server-side wait**: parked approvals re-surface on reattach
  (pty probe); eternal wrapper roots conclude client-side from the agent
  answer and never fake a root completion.

Residual (documented in lane-a-diagnosis §1): an agent subrun that dies
terminally BEFORE its first reason cycle never binds as answer-source and
the turn hangs until `--timeout`/cancel; the failed record still renders
an error card (not silence). Structurally indistinguishable from a helper
death with the signals the ledger offers today.
