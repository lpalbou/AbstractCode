# Flow-brain entity conversations — evidence report (operator tasking, c5190/c5280)

**Task** (operator 2026-07-24 17:21, re-tasked by flow at c5280 ask 2): prove
abstractcode can hold a live conversation with a flow-brained entity —
teach a fact, have a *fresh* conversation recall it — with tests,
screenshots, a done-rule report, and one fable5 adversary folded.

## What shipped

- **`/brain <name>`** — a FLOW-BRAIN conversation lane beside the existing
  visit lane (`@name`, byte-untouched): each message is ONE summon of the
  `entity-chat` VisualFlow (`entity-life` bundle, latest published version)
  through the production entity door — `POST /entities/{name}/summon` →
  poll `GET /runs/{id}` to terminal → read the STRUCTURED contract
  (`output.answer` + `output.degraded` + `output.moment_error`, never
  success alone — the c5280-pinned read rule).
- **Continuity rides the entity's graph**: one client-minted session id per
  conversation groups every summon; the view is session-local and says so
  in its opening line. Substrate overrides are deliberately omitted — the
  gateway resolves the home's stored mind (the entity app's adversary
  dropped exactly that override; we inherit the lesson).
- **Degraded contract rendered structurally**: `degraded > 0` and
  `moment_error` render as warn lines from the STRUCTURED fields — "he
  said nothing" and "the turn died" stay distinguishable; prose is never
  bracket-parsed.
- **The reference implementation's P0 classes inherited as rules**
  (abstractentity's fable5 findings, read before building): no brain
  switching under a live thread (one conversation per entity; `/brain` on
  a live visit refuses naming `/end` first; `@name` on a live flow convo
  focuses without opening a visit over it), and `fold_reopen` flips the
  brain WITH the transport so a reopened record can never be a chimera.
- **`/end` on a flow conversation closes locally** with an honest note —
  there is no server visit to close; the entity's memory of the
  conversation persists in its graph (and the note says exactly that).

## Evidence (live, 2026-07-25 ~01:00, gateway :8080, entity `veya`)

Script: `scripts/pty_flowbrain_proof.py` — two FRESH TUI processes (real
binary, real pty, pyte-rendered frames), gating on STRUCTURE (the
`◆veya ✎…` → `◆veya ready` chip cycle) with exactly one content
assertion: the recall itself.

1. **Session 1 (teach)**: `/brain veya` → *"Please remember this
   precisely: the code-tui doorway's proof token is saffron-kestrel-42…"*
   → turn completed → `/end`. Frames:
   `untracked/reports/flowbrain-proof/teach-{1-opened,2-reply,3-ended}.{txt,png}`.
2. **Session 2 (recall, fresh process + fresh session id)**:
   `/brain veya` → *"What is the code-tui doorway's proof token?"* →
   veya's reply contains exactly **`saffron-kestrel-42`** — recalled from
   her memory graph, with zero client-side state shared between the two
   processes (separate pty forks, separate prefs files, separate session
   ids). Frames: `recall-{1-opened,2-reply,3-ended}.{txt,png}`.
3. **Verdict line**: `flowbrain-proof: PASS` (12/12 checks across both
   sessions).

## Tests

- `tests/entity_flow.rs` — three new headless tests through the REAL
  interface (no gateway; runner commands on a plain receiver):
  - `flow_brain_open_send_reply_and_local_end`: open teaches the lane's
    semantics; sends dispatch `Cmd::EntityFlowTurn` carrying the convo's
    session id; the structured degraded contract renders as a warn line;
    flow convos return to Ready (never park); `/end` closes locally with
    no server command, bumps the epoch, and the stale epoch is guarded
    out.
  - `flow_convo_never_chimeras_and_reopen_flips_brain`: `/brain` twice
    refuses; `@name` on a live flow convo focuses without a visit open;
    reopen-after-end goes through the visit door AND flips the brain.
  - `flow_failure_keeps_the_conversation_usable`: a failed summon renders
    the error and returns to Ready — nothing server-side was lost.
- Full gate at ship time: **428 tests green, clippy clean** (the count
  after the adversary's fixes is in CHANGELOG).

## Adversary (fable5, verdict SHIP-WITH-FIXES — all P0/P1 folded)

Review: `untracked/reviews/flowbrain-lane-adversary.md`. What it caught
and what changed:

- **P0 stale-post chimera (confirmed + widened)**: the epoch-only guard
  let a late post from an ended thread fold into a REPLACED conversation
  (send → `/end` → `/brain` replace → send re-reaches epoch 1), and the
  held-draft auto-send captured the OLD thread's session id. Folded
  three ways: epoch INHERITANCE on closed-replace (the only epoch reset
  in the codebase — `fold_reopen` already inherited); `guard_flow` now
  keys on name + SESSION ID + epoch (cross-thread application is
  structurally impossible); the auto-send reads its session id from the
  convo UNDER the guard, never the thread capture. Test-pinned
  (`flow_replace_inherits_the_epoch_so_stale_posts_stay_dead`).
- **P1 junk names**: `/brain castor hello` minted a conversation named
  "castor hello" — first-word parse + mention-parity roster check now
  refuse typos when the roster is loaded (an empty roster proceeds; the
  first summon errors honestly).
- **P1 double-send invitation**: the summon rode the 30s slow lane; a
  start-timeout rendered "summon refused" while the run RAN. Now the
  turn lane (long read), and post-POST transport failures say "outcome
  unknown … wait before resending" — never "refused".
- **P1 false persistence claim**: `/end` at zero turns said "memory
  persists" — now conditional ("nothing was sent — no memory formed").
- **P2s folded**: session ids carry entropy (nanos+pid — load-bearing
  once the sid entered the guard); the 300s bound-hit wording no longer
  overclaims ("if the turn is executing, it completes server-side");
  safe run-id slicing; the terminal parse extracted as a pure tested fn
  (`parse_summon_output`); poller flow-skip + moment_error +
  empty-answer renders test-pinned.
- **Raised → RESOLVED at countersign** (flow c5284, verified in gateway
  source): `context_window_tokens` IS load-bearing (it sizes the recall
  budget), so a hardcoded client claim risks overflowing small-window
  minds — the field is now OMITTED; the door's labeled #FALLBACK default
  rules until the gateway resolves the window from the home substrate
  (their lane, on the record).
- **Clean angles**: no double-send/lost-draft window in the non-stale
  path; no permanent-TurnRunning state exists; refusals render the
  server's detail verbatim.

The live proof was RE-RUN on the fixed build: `flowbrain-proof: PASS`
(12/12) again, fresh frames captured.

## Honest limits (named, not hidden)

- Flow conversations are **session-local in the client** (the entity's
  graph keeps everything; the TUI's view does not survive a restart —
  rebuild-from-runs is a named follow-up, same as the reference app).
- The turn-time bound is 300s client-side; a summon that outlives it
  keeps running server-side and the client says so honestly ("its memory
  forms regardless") instead of faking a failure.
- `/brain` requires the entity name (no picker integration yet; the
  `/entities` roster modal is one keystroke away).
- The proof ran against one entity (`veya`) on one gateway — it proves
  the lane, not every substrate.
