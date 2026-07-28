# Thin-client conformance — the standing contract

Status: **binding** (maintainer architectural correction, 2026-07-23).
Audited by conformance lane 2 against the full client source the same day;
every classification below carries file:line evidence. New features must
place their state in one of the three classes below **before** merging,
and class-(iii) entries are defects by definition.

## The doctrine (verbatim)

> EVERYTHING we do goes through the gateway and therefore the runtime.
> Every single call is fully traceable. You do not have to guess — we
> know everything for a fact. code-tui is a thin client that mostly
> replays the events of the ledger and communicates decisions/requests.
> Something started in code-tui should be able to continue on other apps
> (e.g. if in observer I see that a decision was gated, I should be able
> to accept there). All our apps MUST be thin clients, interruptible at
> all times. I should be able to launch something on abstractcode,
> disconnect and reconnect later on that same session to see the
> progress. code-tui is NOT the lead — it simply interacts with the
> gateway and therefore the runtime.

Operational consequences the audit enforces:

1. **Server truth is the only truth about runs.** The client renders what
   the ledger says; it never fabricates a status ("completed", "failed")
   the gateway did not report. Unknown is rendered as unknown.
2. **Decisions are communicated, never executed locally.** Approvals,
   answers, cancels, pauses, steers all travel as durable gateway
   commands (`resume`, `cancel`, `pause`, `inject_guidance`) — traceable
   in the run ledger, resumable/answerable from any other app.
3. **UX overlays on server truth are allowed but must be honest and
   labeled.** Where the client's rendering deliberately diverges from the
   raw server state (e.g. freeing the composer when the answer lands
   while the wrapper root still runs), the divergence renders in words.
4. **Client-held state is confined to client concerns** (rendering,
   input, credentials, local preferences) and to **un-submitted intent**
   (composer drafts, the prompt queue). The moment intent becomes work,
   it is a gateway run.

## The three classes

- **(i) legitimate client concern** — rendering, key bindings, theme,
  connection/credentials, local preferences, un-submitted intent.
  No server twin exists or should exist.
- **(ii) UX overlay on server truth** — client behavior layered over
  server facts (conclusion timing, caches of server state, policy
  applied client-side and communicated as commands). Must be HONEST
  (never contradict the server) and LABELED (the divergence or the
  authority is named where the user reads it).
- **(iii) violation** — state or decisions the gateway/runtime owns that
  the client invented (fabricated statuses, shadow transcripts treated
  as authority, silent local policy that the server never sees).
  Fix on sight.

## Authority inventory (audit of 2026-07-23)

### Class (i) — legitimate client concerns

| Item | Where | Note |
|---|---|---|
| Theme, details toggle, key bindings, scroll, composer | `src/commands.rs`, `src/ui/*` | pure rendering/input |
| Connection resolution + login store (flag > env > store > default) | `src/config.rs `resolve_gateway_url`/`resolve_gateway_token`` | client credentials; `doctor` names the winning source |
| Session id minting | `src/config.rs `mint_session_id`` | session ids are client-chosen by the gateway contract |
| Recent-session picker labels (first prompt, MRU) | `src/config.rs `SessionEntry`/`recent_sessions`` | local convenience naming; history itself stays server-side |
| Operator-DECLARED context window | `src/config.rs `Prefs::context_window``, `--max-tokens` | always labeled "declared"; rides runs as `_limits.max_tokens`; never a client capability table |
| Run-start request parameters (workspace scope, `/tools` allowlist, `/skills`, provider/model, `max_iterations`) | `src/run_input.rs `build_input_data`` | requests the SERVER enforces (policy may clamp); absent keys keep server defaults |
| Tool-approval tier + per-tool pins (the user's standing policy) | `src/config.rs `Prefs::tool_accepted_tier`/`tool_overrides``, `src/tool_policy.rs` | expanded server-side per run (see class ii); pins are explicit user acts |
| Image cache + decode-time downscale | `src/store.rs:upsert_image`, `src/runner.rs:downscale_for_transcript` | render caching of immutable artifacts |
| Headless wait policy (`exec`: tier decides, ask-user refusal) | `src/exec.rs `resolve_approval`` | the operator's standing instruction where no human is present; every decision travels as a resume payload NAMING the rule |
| Un-submitted intent: composer drafts, buffered steers, the prompt queue | `src/store.rs `Store::queue`` (see class ii for labeling) | plural composer text; becomes a traceable gateway run the moment it starts |
| exec exit codes (0/1/130/124) | `src/exec.rs `exit_code_for_status`` | client mapping of server statuses for scripts; 124 leaves the run durable and says so |

### Class (ii) — UX overlays on server truth (honest + labeled)

1. **Turn conclusion / composer release** — `src/transcript.rs`
   (`Fold::finished`), `src/runner.rs:1139` (`apply_stream_records`),
   `src/runner.rs:1405` (`finish`), `src/exec.rs` polling loop.
   The client frees the composer when the ANSWER-SOURCE run delivers
   (root flow end, first-level agent flow end, or that agent run's
   terminal status) — while wrapper roots (basic-agent@0.0.2/0.0.3)
   stay `waiting` forever on a status-poller subflow (44 live waiting
   roots on the audit gateway; `docs/roadmap/lane-a-diagnosis.md` §3/§5).
   Honesty labels:
   - TUI, subrun-concluded turns render `SUBRUN_CONCLUSION_NOTE`
     (`src/runner.rs:1127`): *"turn concluded — the wrapper root run
     stays open on the gateway and finalizes server-side"*. Root-stream
     conclusions render nothing (the root really ended). Test-pinned.
   - Failed answer-source: *"the agent run ended: failed — … the wrapper
     run may keep polling on the gateway"* (`src/transcript.rs `subrun_terminal``).
   - exec prints *"done · … (run {id} finalizes on the gateway)"*.
   - The client NEVER claims the root completed: `run_terminal` renders
     nothing for "completed", errors honestly for "unknown"
     (`src/transcript.rs` `run_terminal`); the idle strip says
     "session: N runs", never a status (`src/ui/chrome.rs`); the
     sessions picker rows carry id/label/last-used, no statuses.
   Server-side ask on the record: the wrapper bundle should terminate
   when its agent completes (`docs/roadmap/conformance-ledger-asks.md`).
2. **Client-carried conversation context** — `src/ui/mod.rs `start_run``
   (`start_run` → `Fold::chat_messages`, `src/transcript.rs `chat_messages``),
   `src/run_input.rs `StartOpts::messages``. Runs always request the SERVER seed
   (`use_session_history: true`); the client ALSO carries whole
   completed turns derived from **gateway ledgers** (live folds +
   boot-time `history_bundle` rehydration — never a locally-authored
   transcript), because the server seed reads prior COMPLETED roots
   only and the wrapper defect leaves roots non-completed, starving it.
   Caps mirror the server seed defaults (40 msgs / 24k chars). This is
   an **interim workaround**: when wrapper roots terminate properly (or
   the seed learns answered-but-open roots), the client half should be
   deleted. Ask on the record.
3. **The prompt queue (`/queue`)** — `src/store.rs `Store::queue``,
   `src/config.rs `Prefs::session_queues`` (`session_queues`), drain in
   `src/ui/mod.rs `wire_queue_drain``. CLIENT-HELD future work: un-submitted prompts,
   drained as normal traceable gateway runs one at a time by this
   client. Verified server-side: the gateway has **no completion-chained
   queue primitive** (`POST /runs/schedule` is time-based —
   start_at/interval/repeat only), so nothing server-side could hold
   these today. Labels: `/help` names the locality ("held by THIS
   CLIENT per session — other apps see runs only once started"); every
   restore lands PAUSED; the quit echo lists what stays. The laptop
   dying pauses the queue — it never loses started work (runs are
   durable). Server-primitive ask on the record.
4. **`/goal`** — `src/store.rs `Store::goal``, `src/config.rs `Prefs::session_goals``,
   `src/ui/mod.rs `wire_goal`` (`wire_goal`). CONFORMANT by design: the loop
   runs SERVER-side (one durable run of a `abstractcode.goal.v1` bundle;
   `goal`/`max_cycles` ride `input_data`) — the client never relaunches
   runs to iterate. Client-held state is a label + run pointer
   (strip text, `finish_on_root_only` fold bookkeeping so iteration
   subrun ends don't conclude the display). `/help` says the loop is
   server-side. The goal bundle itself is another seat's deliverable;
   `/goal` stays honestly dark until one is published.
5. **Tool-approval policy expansion + client belt** —
   `src/tool_policy.rs `expand_run_policy`` (`expand_run_policy` →
   `_runtime.tool_policy` at run start: the SERVER executes the policy
   with no wait round-trip), `src/ui/mod.rs `auto_approve_wait`` (`auto_approve_wait`:
   the residual client belt answers leftover waits via durable resumes,
   toast NAMES the admitting rule). Classification prefers gateway-served
   `approval`/`tier` facts; the name table applies ONLY when facts are
   absent (`src/tool_policy.rs `classify_call_with``, delegation test-pinned) and is
   labeled: `classify_source`/`batch_name_table_names`
   (`src/tool_policy.rs `classify_source`/`batch_name_table_names``) expose the source; exec's approval log
   appends *"tier from the client name table for: … (#FALLBACK — no
   gateway approval facts served)"*. The read-only-git PROOF stays a
   deliberate client override above server truth (documented ruling —
   the server cannot prove a specific command inert). **SUPERSEDED
   2026-07-24 (c5057): the client proof is retired — the decision moved
   to the runtime approval point as the `git_read_only@v1` refiner; a
   git command with no served facts is now name-table classified and
   carries the #FALLBACK label like any other table decision.**
   Remaining wiring:
   the TUI approval modal's "needs: tier" line should carry the same
   source label (lane-3 file; handoff in the asks doc).
6. **Entity roster cache** — `src/entities.rs roster cache (`load_cached_roster`/`save_cached_roster`)`. Last-good server
   state cached beside prefs, ALWAYS labeled "as of HH:MM"; refreshed
   async; never blocks on or substitutes for a live read.
7. **Reattach / rehydration** — `src/runner.rs:672` (`probe_attach`).
   The gateway is the source of truth for what happened: prior turns
   replay from `history_bundle` ledgers through the SAME fold as live
   streams (catalog agent-workflow-id declarations re-seeded into every
   rehydration fold — `Fold::set_agent_workflows`, wired from
   `load_catalog` + `probe_attach`,
   `src/runner.rs:agent_workflow_ids_from_bundles`); the live run is
   found by server status; the elapsed clock back-dates to the run's
   server `created_at`. **No transcript is persisted client-side**
   (prefs hold ids/labels/queues/goals only).
8. **`RunOutcome` mailbox** — `src/store.rs `RunOutcome``. Client scheduling
   state for the queue drain; `Success` = "the turn concluded with a
   usable conclusion", never a claim about the root's server status
   (doc-pinned on the type).
9. **Session-mismatch start cancel** — `src/runner.rs:1069`
   (`apply_start_binding`): a run whose start raced a session switch is
   cancelled DURABLY through the gateway and the cancel is announced.
   A client policy, executed as a traceable command.
10. **Honest degradation** — worker/stream panics surface on screen and
    flip the phase Idle ("no command loop means no pause/cancel/steer
    can be delivered — a spinner claiming otherwise would lie",
    `src/runner.rs panic surfacing in `spawn``); undecodable SSE records are counted and
    rendered, never silently skipped.

### Class (iii) — violations

**None found live in the audited tree.** The wave preceding this audit
removed the known fabrication class; the contract entries below pin them
closed:

- ~~Fabricated "completed" when the terminal status could not be read~~ —
  fixed (F4): `finish()` retries then reports honest "unknown"
  (`src/runner.rs:1405`, `src/transcript.rs` `run_terminal` — "run ended
  but the final status could not be read from the gateway").
- ~~Client-side "finished" silently contradicting a waiting root~~ —
  now labeled (class ii item 1).
- ~~Client transcript persisted and replayed as authority~~ — never
  present in prefs (verified: `session_queues`/`session_goals`/
  `recent_sessions` carry no message content beyond the first-prompt
  label); in-memory context is ledger-derived (class ii item 2).

## Rules for future changes

1. A new piece of client state must be classified (i)/(ii)/(iii) in this
   document in the same change that introduces it.
2. Class-(ii) overlays name their divergence in rendered words, next to
   where the divergence shows (transcript card, strip, help line) — not
   only in code comments.
3. The client never writes a run status the gateway did not report.
   "Unknown" renders as unknown; fail-safe postures (pausing the queue)
   are allowed and must say why.
4. Every decision (approve/deny/answer/cancel/pause/steer) travels as a
   gateway command so any other app can see it in the ledger and could
   have made it. No decision may be consumed purely client-side.
5. Anything the gateway grows a primitive for (queue, schedule,
   completion-chained work) migrates: the client half demotes to a
   renderer of the server primitive, and the interim local feature is
   deleted, not kept as a fallback truth.
