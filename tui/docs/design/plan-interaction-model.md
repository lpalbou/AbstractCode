# Interaction-model plan (steer/queue · multiline · /goal · agora serve)

Status: plan-cycle-2 output (2026-07-22, adversarial review pass). Cycle-1
claims were verified against the shipped code (src/runner.rs, src/ui/mod.rs,
src/transcript.rs, src/exec.rs), the engine at 0.2.1 (abstracttui input/,
widgets/textarea*, app/driver.rs, app/overlays.rs, ui/tree.rs), and the REAL
serve-protocol consumer (abstractcode/abstractcode/bridge.py +
headless.py). Decisions that CHANGED are marked "(cycle-2:)" inline.
Implementation-ready; no code has been written against this document yet.

## Severity ranking by user value

1. Steer vs queue — touches every interactive session.
2. Multiline on macOS — honesty + one engine gap (E1) blocks Shift+Enter
   on iTerm2/VS Code/Warp.
3. Agora serve mode — unlocks the headless-collaborator use case.
4. /goal — highest cross-seat lead time; ownership settled on agora.

## Item 1 — Steer vs queue

Current: typing while Running steers (`inject_guidance`, receipts via
`abstract.steer_seen`); Starting refuses AND DROPS the text; no queue.
Convention check: claude-code queues on Enter; codex-cli steers on Enter
(pending-steer folded at the next tool boundary) — the closer cousin.

DECISION: Enter keeps steering (latency-sensitive intent stays
zero-friction); `/queue <text>` is the FIFO lane; `/queue` opens a manager
modal.

### pending_steer

(cycle-2: generalized from a Starting-only buffer to a "no cycling target
yet" buffer — the race is real and wider than Starting.)

The race, verified: the runner's start-success closure writes `run_id` →
`phase` → `fold.begin_run` IN THAT ORDER inside one posted closure
(runner.rs:488-496), and signal writes outside a dispatch batch flush
effects SYNCHRONOUSLY (the engine behavior the `open_modal` doc comment in
ui/mod.rs:118-127 records from a live bug; posted jobs are not auto-batched
— the ingest drain wraps itself in `batch()` precisely because of this). An
effect keyed on `phase`/`run_id` therefore runs BEFORE `begin_run` and reads
the PREVIOUS run's `fold.steer_target()` (transcript.rs:304-310) — the
wrong-target delivery the buffer exists to prevent. Delivering to the new
ROOT id directly is not enough either: guidance inboxes are drained by agent
LOOPS, and wrapper bundles (basic-agent) run the loop in a subrun — a
root-targeted steer is silently never folded. Today's MANUAL steer path has
the same silent window between Running and the first cycle record.

Exact guard (spec): buffer as `PendingSteer { armed_at_root: String
(fold.root_run_id() at buffer time), armed_while_starting: bool, text }`.
Any plain-text submit with NO cycling target buffers: phase == Starting
(armed_while_starting = true), or phase == Running with the fold's cycling
target still unset. A fold-tracking effect delivers `Cmd::Steer` when the
fold's cycling target lands (first reason-cycle record sets `steer_run_id`,
transcript.rs:417-422; exposed via a new `Fold::cycling_target() ->
Option<String>` — `steer_target()` cannot distinguish "root fallback" from
"cycling run known") AND the run identity matches: armed-while-Starting
delivers only when `root_run_id() != armed_at_root` (the new run began —
`begin_run` cleared `steer_run_id` in between, so a stale old-run cycle
cannot satisfy the predicate); armed-while-Running delivers only when
`root_run_id() == armed_at_root`. Multiple buffered submits concatenate in
arrival order (newline-joined). Disposal is explicit and visible: start
failed → error card carrying the undelivered text, buffer cleared; run
finished before any cycle → Info card "steer arrived after the run finished
— resend if still relevant"; `/new` + session switch clear with an echo;
quit drops it silently (moment-bound guidance, unlike the queue). Delivered
steers render as `Item::Steer` exactly like live steers.

### Store/fold/runner deltas

`queue: Signal<Vec<QueuedPrompt{id,text}>>`, `queue_paused: Signal<bool>`,
`pending_steer: Signal<Option<PendingSteer>>`, `last_outcome:
Signal<RunOutcome>`, `goal` (item 3). Fold: `cycling_target()` accessor +
`finish_on_root_only` flag (item 3). (cycle-2:) "no runner protocol
changes" stays true for the `Cmd` enum, but `last_outcome` is WRITTEN BY THE
RUNNER's existing posted closures (start Ok/Err, the `finished_now` branch,
`finish()`) — cycle-1's wording hid a real runner.rs delta; the drain cannot
otherwise distinguish "Idle because completed" from "Idle because the start
failed".

### Drain effect (spec)

Tracks `(phase, queue, queue_paused, fold)` — fold-tracking is load-bearing:
a drain blocked by a pending wait must re-fire when the wait resolves, which
is a fold change with no phase change. Early-returns unless ALL hold:

- `phase == Idle`, queue non-empty, `!queue_paused`;
- `fold.pending_wait.is_none()` — (cycle-2: NEW GUARD) waits CAN arm after
  `finished`: the waits section of `Fold::apply` has no finished gate
  (transcript.rs:711-736), so a helper subrun's ask can be pending while the
  composer is already released; a drain-started run would `begin_run`-wipe
  the prompt (transcript.rs:231) and orphan the wait. Auto-approve resolves
  such waits itself, and resolution re-fires the effect;
- workflow readiness, checked BEFORE dequeuing — (cycle-2: NEW GUARD)
  `start_run` returns WITHOUT a phase change when no workflow is selected
  (ui/mod.rs:329-339) or when the runner tx is dead (ui/mod.rs:398-409): an
  unchecked drain either stalls SILENTLY ARMED (no tracked signal changes →
  the effect never re-fires) or loses the popped item. Client-side refusal →
  `queue_paused = true`, item KEPT, strip names the reason.

The dequeue itself runs as a DEFERRED job (`after(Duration::ZERO)`, the
modal-retire discipline): phase flips to Idle INSIDE runner-posted closures
that keep touching signals afterwards (runner.rs:916-919 sets
`run_started`/sends `StopFollows` after `phase.set(Idle)`); a synchronous
drain would interleave the new run's start with the old run's teardown
writes.

Context correctness (the asked case): a manual plain-text run while Idle
with a non-empty PAUSED queue proceeds and does NOT auto-resume the queue
(unchanged); its answer DOES feed later queued items because `StartOpts`
builds at drain time from `fold.chat_messages(40, 24_000)` — the manual turn
is in `fold.items` by then, and `chat_messages` carries completed
user/answer pairs only. Every drained item likewise sees all completed turns
before it, including other queued turns. (A non-paused armed queue while
Idle is not reachable by the user: the drain effect observes the Idle flip
before any subsequent keystroke is dispatched.)

### Queue semantics table

- run completes (success) → auto-drain next as a NEW run
- run fails / cancelled → `queue_paused = true`, items kept, strip says so
- queued START fails (HTTP or client-side refusal) → (cycle-2:) item
  RESTORED AT HEAD + paused (was: popped) — nothing was spent; `r`/resume
  retries the same item. Loop-free because paused blocks the drain until an
  explicit resume; the transcript keeps the user card + error card as
  evidence either way
- manual run while paused → proceeds; does NOT auto-resume the queue
- `/new` or session switch → (cycle-2:) queue STASHED to its session's prefs
  slot (was: cleared) with a visible echo; the target session's stash loads
  PAUSED
- app quit → (cycle-2: REVERSED from drop-on-quit) the queue PERSISTS —
  write-through to prefs on every mutation, keyed by session id (the
  `touch_session` slot pattern) — and restores PAUSED with a strip notice;
  it NEVER auto-starts on restore. Rationale: the maintainer's contract is
  "piling up requests that each gets executed sequentially" — a silent drop
  breaks the promise, and a quit-time stderr echo is exactly the channel
  this app already documents as unread (`wire_startup_notices`:
  post-teardown stderr is "the one place a developer is no longer looking").
  Restore-paused honors the promise with zero unattended token spend on
  stale context. `auto_approve` stays never-persisted — approval authority
  and pending work are different footguns; the cycle-1 analogy conflated
  them.

One rule ties the last three rows together: a queue only auto-drains within
the session + process continuity it was armed in; ANY restore (quit/reopen,
session switch) lands PAUSED and visible.

### Composition with the entities plan

(cycle-2: NEW section — the two plans MUST compose; cycle-1 predated the
entities plan's `Focus` concept.)

The queue belongs to the AGENT lane ONLY. `/queue` (with or without text)
under `Focus::Entity(_)` refuses with "queue is agent-lane — /focus agent
(entity turns already hold your draft and send it as the next turn)"; the
entity lane's held-draft mechanism (plan-entities-mcp.md items 1/3) is its
own between-turns lane and gets no second queue. The drain runs regardless
of focus (the agent lane keeps executing in the background; the chips row
shows it). Phase-swapped composer placeholder, status-bar legend, and the
"N queued / paused" strip hints render only under `Focus::Agent` — under
entity focus the composer belongs to the entity lane's banner, and an
agent-lane "enter steer" hint there would lie.

### Discoverability

Phase-swapped composer placeholder; status-bar legend swap while running
(`enter steer · /queue later`); activity strip appends `· N queued` /
paused notice; /help + completion entries. All Focus::Agent-scoped (above).

Queue modal keys: ↑↓ select, x remove, u/d reorder, c clear, r resume
(paused only), e pop-to-composer, Esc close. (0250 discipline: the List
binds Enter/keys itself; modal closes defer one tick.)

### Tests

Steer-vs-queue split; buffered steer delivered ONLY on the new tree's first
cycle (assert no `Cmd::Steer` before a reason record lands; wrong-run
predicate: an old-run cycle record must not trigger delivery); manual
Running-pre-cycle steer buffers instead of targeting root; buffer disposal
(start failure → error card + cleared; finish-without-cycle → Info +
cleared); drain deferral (no start inside the `finished_now` posted
closure); drain blocked by a pending wait, resumes after the wait resolves;
client-side refusal pauses + keeps the item; HTTP start failure restores at
head + pauses; FIFO drain carries prior + manual answers in context; halt on
failure/cancel + explicit resume; session-switch stash/load-paused; quit
persistence round-trip restores paused; `/queue` refused under entity focus;
modal reorder/remove/edit; manual-send-while-paused.

## Item 2 — Multiline convention (macOS)

Engine-verified truth: `SubmitPolicy::EnterSubmits` inserts \n on
alt||shift Enter; legacy wire cannot distinguish Shift+Enter from Enter
(both 0x0d); parser already understands kitty `CSI 13;2u` and xterm
modifyOtherKeys.

ENGINE GAP (E1, the valuable one — re-verified at 0.2.1): `Driver::new`
pushes kitty enter-flags once from ENV-detected caps
(app/driver.rs:157-194); the runtime probe upgrades `caps.kitty_keyboard`
on a reply (driver.rs:560-576) but `apply_caps_upgrade` (driver.rs:669-685)
refreshes PRESENT caps (rendering) only — nothing re-emits the keyboard
flags push. So iTerm2 ≥3.5, VS Code, Cursor, and Warp (all probe-answering)
never get Shift+Enter today, while WezTerm is env-claimed but stock config
has the protocol OFF (over-claim, E3). E2 nuance (cycle-2: sharpened):
`Driver::caps()` IS public, but only reachable by embedders driving their
own `Driver`; under `app::run` there is no accessor — the ask stands as
`app::current_caps()`/`use_caps(cx)`. Terminal.app: no distinct Shift+Enter
on tested versions (macOS-26 rewrite unresolved — live recipe below).

APP ACTIONS (now) — (cycle-2: wiring made precise; the cycle-1 "root
shortcut" placement was wrong):

- The chord arrives as `Key::Char('j')+CTRL` on BOTH wires: legacy 0x0a is
  decoded by the C0 arm `0x01..=0x1a → Ctrl+letter`
  (abstracttui input/legacy.rs:38) — the byte is never a "newline key", so
  nothing about 0x0a reaches the TextArea as text; kitty reports
  `CSI 106;5u` to the same event (input/kitty.rs). Lock latches are
  stripped before chord matching (app/events.rs:86).
- The TextArea consumes NOTHING for it: the edit model inserts chars only
  under `!ctrl && !alt` (widgets/textarea_model.rs:411) and unmatched keys
  return `Ignored` (textarea_model.rs:451), so the widget handler leaves
  the event unconsumed and tree dispatch falls through to SHORTCUT
  resolution over the root→focus path, deepest match winning
  (ui/tree.rs:624-647). No engine change is needed.
- Placement: NOT at root — a root shortcut fires with focus ANYWHERE
  (Ctrl+J while the transcript pane holds focus would inject a newline into
  an unfocused composer). Register on the COMPOSER's own element
  (chrome.rs `composer()`: chain `.shortcut(KeyChord::new(Mods::CTRL,
  Key::Char('j')), …)` on the TextArea element before `.build()`): it then
  fires only when the composer is on the focus path, and modal trees
  swallow keys wholesale while open (app/overlays.rs:188-193 + 376-386),
  which is the wanted behavior for approval prompts.
- Handler: `state.replace_range(caret..caret, "\n")` via
  `TextAreaState::caret_byte()`; the caret lands after the insertion by the
  method's contract.
- Update placeholder/help/faq to the honest matrix (do not claim WezTerm
  unconditionally). Ctrl+J is documented as the works-everywhere chord —
  it IS the LF byte on the legacy wire, so every terminal carries it
  (claude-code documents the same chord).

ENGINE ASKS to file (first-app backlog): E1 push flags on probe false→true
transition (+ exit restore); E2 `app::current_caps()` / `use_caps(cx)`; E3
probe-confirm the WezTerm claim; E4 (nice-to-have) Ctrl+J folded into
SubmitPolicy.

Live-verification recipe (exec phase, per terminal):
`printf '\x1b[>1u'; cat -v` → press Enter / Shift+Enter / Option+Enter /
Ctrl+J → expect `^[[13;2u` for Shift+Enter where the protocol is live;
restore with `printf '\x1b[<u'`. Then in-TUI: type a, chord, b → two
lines vs submit.

## Item 3 — /goal (agora-settled contract; client half adjusted to the shipped mechanism)

Agora ruling (framework 4296 + agent 4294, consumed at 4302): option (c) —
a generic goal-agent workflow bundle owned by the FLOW seat; no
client-side loop (dies with the client); no agent-package native mode
(review_mode already provides the in-run verifier half). Bundle shape:
while-loop, one ReAct cycle per iteration with review_mode ON, goal +
progress in run vars, wait_until pacing, stop = verified-done OR
max_cycles; outer-loop progress evidence = review verdict or
deterministic gate, never model self-report; inject_guidance steers a
live goal run (it is not a restart mechanism).

Client half (claimed as claim:coder-tui-goal-command):

- Discovery: consistent with the SHIPPED mechanism — /workflow filters
  bundle entrypoints by their `interfaces[]` array against
  `AGENT_INTERFACE_V1 = "abstractcode.agent.v1"`
  (runner.rs:1159 + `agent_workflows_from_bundles`), so a goal bundle
  carrying only `abstractcode.goal.v1` stays out of /workflow
  automatically. Build: generalize the parser to
  `workflows_with_interface(v, interface_id)` (one function, two
  constants) instead of a second copy.
- Stub behavior (cycle-2: made explicit — the flow seat has NOT published
  the bundle, so the client half ships DARK behind catalog discovery):
  `/goal <text>` with zero goal-interface entrypoints → honest notice "no
  goal workflows on this gateway (abstractcode.goal.v1) — the goal bundle
  is not published yet"; `/goal` with no active goal run says so. The
  feature lights up on catalog load when the bundle appears; no client
  release is coupled to flow's publish.
- (cycle-2: WITHDRAWN) cycle-1's "Fold needs NO changes" claim is wrong for
  the plausible bundle shapes. Verified hazard: `agent_run_id` latches on
  the FIRST first-level cycling subrun (transcript.rs:430-438), and an
  answer-shaped flow end from that run sets `finished = true` and renders a
  FINAL answer (transcript.rs:746-803; pinned by the "agent-loop subrun
  answer finishes the turn" test). Every checked-in ralph flow that loops
  an Agent NODE (`while` + `agent`: abstractflow/examples/flows/
  84aa8534.json ralph-agent, 31eae641.json v3, c91fec16.json v2) starts one
  subrun PER ITERATION — the TUI would declare the goal run finished at
  iteration 1, release the composer, and the next submit would
  `stop_streams` + `begin_run` over the live goal run. The cycle-1 citation
  ("verified against the live ralph bundle shape, flow ee1a6daa") is not
  reproducible: that flow id appears nowhere in the repo, and the
  checked-in ralph flows contradict the claim. Two halves, BOTH required:
  (a) contract constraint added to the flow ask — per-iteration interim
  results must ride `answer_user` (renders non-final,
  transcript.rs:676-708), and first-level subrun flow ends must not carry
  answer-shaped output before the goal verdict; (b) client defense that
  holds under ANY bundle shape — a /goal-started run sets a fold flag
  `finish_on_root_only = true`: `finished` fires only on ROOT flow-end /
  root-terminal, never from a first-level subrun answer. (b) is the one
  real fold delta this plan adds.
- `/goal <text>` starts the goal run (refuse while a run is active);
  `/goal` shows status (goal text, cycle, elapsed, tokens); `/goal stop`
  cancels durably. `store.goal: Signal<Option<String>>` + prefs for
  restart labeling; reattach rides the existing probe — (cycle-2:) the
  goal-run id persists in prefs so reattach RESTORES
  `finish_on_root_only` for a live goal run (without it, a restart
  mid-goal reintroduces the iteration-1 false finish).

Blocked on: flow publishing the bundle id + input contract (asked at
commons 4302; proposed verdict schema
`{goal_met: bool, evidence: string, remaining: [string]}`) — (cycle-2:) the
same ask now carries the interim-results constraint from (a) above.

## Item 4 — Native agora integration (`abstractcode serve`)

DECISION: `abstractcode serve` — a JSONL protocol-v1-parity subcommand
so the EXISTING Python `abstractcode bridge --executable` drives this
client as its fleet child. Do NOT re-port the adversary-hardened bridge
policy to Rust (deferred until a Python-free deployment is named).

Shape: stdin reader thread → command channel; the poll-fold-resolve loop
extracted from exec.rs into headless.rs, parameterized by an EVENT SINK +
injected wait resolvers (exec keeps printing through its sink; serve emits
JSONL) — one lane, two doors. (cycle-2:) the extraction must FIX, not
inherit, a shipped defect found during verification: `Fold.failed` is a
DEAD FIELD (only `begin_run` ever writes it, always false —
transcript.rs:225 is the sole write), so exec's finished-path exit code
returns 0 even when a root FAILURE record set `finished`
(exec.rs:222-229 vs transcript.rs:807-817). `final.status` / exit codes
must derive from failure records + run status; set-or-delete the field.

Event schema (cycle-2: PRUNED against the real consumer + kept to Python
parity). bridge.py reads EXACTLY: `event` (name), `iteration` (cycle),
`tool` (tool_call / tool_result / approval_required), `success`
(tool_result), `call_id` + `args` (approval_required), `status` + `answer`
+ `error` (final) — bridge.py:445-507 — and ignores every other event name
(any JSON object line is safe). The Python serve additionally emits:
run_started, phase, thought, denied, status, steer_queued, ack, state (the
reply to op `status`), error, llm_call (cache tap). Rust serve v1:

- MUST (consumer-load-bearing or op-completing): `ready`,
  `cycle{iteration}`, `tool_call{tool, call_id, args}`,
  `tool_result{tool, success, call_id, result_preview, result_chars,
  truncated}`, `approval_required{tool, args, call_id}` — ONE PER CALL,
  emitted sequentially (emit, await decision, emit the next),
  `ask_user{prompt, wait_key}`, `final{status, answer, error, run_id}`,
  `error{reason}`, `ack` (op correlation), `state` (the `status` op's
  reply — cycle-1 listed the op without its reply event).
- SHOULD (cheap from the fold; parity): `run_started{run_id}`, `thought`,
  `denied{tool, call_id, reason}`, `status{text}`, `steer_queued`.
- NOT v1: `phase` (init-only, zero consumers), `llm_call` cache-tap events
  (gateway usage lives in the fold; add when a consumer names the need —
  a labeled absence, never silent).
- (cycle-2: stale parenthetical removed) the tool_result `call_id`
  correlation gap is ALREADY CLOSED Python-side (headless.py:246-252
  forwards call_id on tool_result since agency's c1608 finding) — carrying
  call_id is PARITY, not novelty.

Ops: `prompt` (refused while busy), `approve {call_id, decision
allow|deny|all, reason?}`, `answer {text, wait_key?}`, `steer`, `cancel`,
`status`, `quit`. Held replies are turn-scoped and cleared loudly at turn
end, with the Python mismatch semantics (call_id/wait_key disagreement →
error event + the reply is consumed-invalid, headless.py:1142-1199,
1336-1351).

Batch-approval mapping (cycle-2: sharpened — the one semantic difference,
now fully specified): gateway approval waits are BATCH-level — the resume
payload is `{"approved": true}` or `{"approved": false, "reason": …}` for
the WHOLE batch (runner.rs `Resume` path, ui/modals.rs:131-141,
exec.rs:181-196); no per-call resume exists. Serve emits approval_required
per call and collects per-call decisions: all allow ⇒ approved:true; ANY
deny ⇒ approved:false with joined reasons — which denies calls the
controller ALLOWED. Emit `denied` events for EVERY call of a mixed batch,
reason prefixed "batch denied (gateway approvals are batch-level): …", so
the controller's log tells the truth about what actually ran. Decision
"all" arms session auto-approve: later batches resume without emitting
approval_required (Python `_approve_all_session` parity).

Flag parity (cycle-2: CORRECTED against the real spawn line —
bridge.py:311-325): the bridge ALWAYS passes `--no-review` and passes
`--base-url` when configured; cycle-1's "unknown flags error" would brick
every bridge spawn. Accept: `--agent react` (→ default workflow
resolution; other agent ids → refuse naming the workflow mechanism),
`--provider`/`--model` (→ StartOpts), `--max-iterations`, `--skill`
(repeatable → input_data.skills), `--permission-mode` (write → default
posture; full-auto → auto-approve; read-only → REFUSE honestly),
`--no-review` (accepted as a no-op + one stderr note: the gateway lane has
no in-seat verifier to disable), `--base-url` (accepted + loud `#FALLBACK`
stderr warning: provider endpoints are the gateway's server-side config;
the flag cannot be honored by a thin client — warn-and-continue keeps a
configured fleet booting, refusal would brick it). Genuinely unknown flags
still error.

Sizing honesty (cycle-2: added — cycle-1 carried no estimate): exec.rs is
374 lines today; the Python serve half alone (serve_command + _ServeState +
helpers, minus the driver) is ~350. Expect ~600-900 new/moved Rust lines:
extraction (~150 moved + sink/resolver seams), serve loop + held replies +
batch mapping (~300), flag parity + CLI (~100), fixtures + tests
(~200-300). An estimate materially below this is probably skipping the
held-reply/mismatch semantics or the conformance fixtures.

Deployment note: agora peer tools for the seat come from the GATEWAY
process toolset (not the child env) — document the posture.

Tests: op parsing incl. held-reply lifecycle (turn-end discard events,
call_id/wait_key mismatch); mixed-batch denial fold (denied events for
allowed calls, joined reason); spawn with the bridge's EXACT flag line
(incl. --no-review/--base-url) reaches `ready`; fixture ledger →
byte-asserted JSONL event sequence; failure-record exit-code truth (the
fold.failed fix); cross-implementation conformance fixtures shared with the
Python repo; live bridge smoke.

## Build order (cycle-2: added — 3 parallel workers × 5 cycles)

Cycle 1 — serializing bases (each file has ONE owner per cycle):

- W1: store.rs + config.rs deltas (queue/pending_steer/last_outcome/goal
  signals; prefs schema: per-session queue slots + goal-run id) + the
  runner.rs posted-closure `last_outcome` writes. Items 1 and 3 both sit on
  these.
- W2: exec.rs → headless.rs extraction (event sink + resolver seams; fixes
  the fold.failed exit-code lie; exec's printed output stays byte-identical
  — pin with the existing tests/run_tree_replay.rs fixtures). exec.rs is
  single-owner this cycle.
- W3: item 2 complete (composer-element Ctrl+J + placeholder/help/faq —
  smallest item, ships first) + transcript.rs deltas (`cycling_target()`,
  `finish_on_root_only`) with unit tests. transcript.rs is single-owner
  this cycle.

Cycles 2-3 — parallel tracks:

- W1: queue machinery — submit routing + pending_steer buffer/delivery
  effect + drain effect + strip/placeholder hints (owns ui/mod.rs +
  chrome.rs).
- W2: serve subcommand — reader thread, op loop, held replies, batch
  mapping, flag parity (owns headless.rs + cli.rs).
- W3: /goal client — `workflows_with_interface` generalization
  (runner.rs parse fns only — coordinate with W1's runner closure edits:
  parse fns cycle 2, closures are cycle-1 W1 work, disjoint), store.goal
  wiring, reattach restore. commands.rs is shared by /queue (W1) and /goal
  (W3): W1 lands its entries in cycle 2, W3 rebases in cycle 3 — one owner
  per cycle.

Cycle 4 — hardening + test infrastructure:

- W1: queue modal + headless_ui.rs coverage for drain/buffer/persistence.
- W2: serve conformance — NEW infra is needed here and does not exist
  today: a serve-protocol harness (in-process stdin/stdout loop or spawned
  binary) + fixture sync with the Python repo. The fixture-ledger replay
  and CaptureTerm/Driver harnesses DO exist (tests/run_tree_replay.rs,
  tests/headless_ui.rs) and cover everything else.
- W3: entity-focus composition tests (queue refusal, hint scoping) +
  /goal dark-mode tests.

Cycle 5 — live + external:

- pty smoke: bridge-spawned serve against a live gateway (filesystem-proof
  discipline); item-2 terminal matrix (operator-run recipe); /help +
  README sync; defect margin.
- Blocked-on-external at any cycle, none blocking the five: E1-E4
  (abstracttui seat — Shift+Enter stays honestly absent on probe-answering
  terminals until E1; Ctrl+J ships regardless), goal bundle (flow seat —
  /goal ships dark), conformance blessing (code seat).

## Responsibilities

- This app: item-1 queue machinery + persistence + modal + hints; item-2
  composer-element Ctrl+J + honest text + live verification; item-3 client
  surface incl. `finish_on_root_only` (dark until flow publishes); item-4
  serve subcommand + conformance fixtures + the fold.failed fix.
- abstracttui: E1–E4 (filed in the engine's first-app backlog series).
- gateway: none blocking (informational: agora toolset posture G1).
- flow: goal bundle (asked, commons 4302) + (cycle-2:) the answer_user
  interim-results constraint added to that ask. code: serve conformance +
  ralph-loop convergence (asked, commons 4302).
