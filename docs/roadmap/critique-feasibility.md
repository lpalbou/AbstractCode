# Cross-critique — feasibility, cost accuracy, correctness (lanes 1–3)

Adversarial review of the three research lanes, 2026-07-22. READ-ONLY: every
verdict below was checked against the actual code at the cited lines (v0.3.0
tree), the engine source (`abstracttui` 0.2.x), the gateway source
(`abstractgateway/src/abstractgateway/routes/`), the captured fixtures, and
the three standing plans in `docs/design/`. Where a lane's line-cite was
wrong or its premise overstated, this document says so with the evidence.

Verdicts: **CONFIRM** (estimate + evidence sound) · **RE-ESTIMATE** (real
cost differs — reasoning given) · **MERGE** (same work as another finding)
· **BLOCKED** (needs an external seat first) · **REJECT** (premise wrong or
value not worth the risk) · **SPLIT** (one finding hides two verdicts).

Overall: the lanes are unusually honest by researcher standards — most
line-cites check out exactly, and lane 3's "the ledger already carries it"
structural claim is TRUE (verified: completed `llm_call` results in the
captured fixtures carry `gen_time`/`finish_reason`/`model`/`usage`/
`trace_id`; every record carries `attempt`/`started_at`/`ended_at`; nothing
in `src/` reads any of them today). The systematic bias is not fabricated
evidence — it is **undercounting integration cost on the load-bearing
seams** (`Fold::apply`, `Item::Tool`, `wire_feed`, the runner FIFO) that six
agents just hardened, and **labeling "client-only" what actually needs a
gateway ask** (UX-05 is the worst case). Details per finding.

---

## Lane 1 — engineering (17 items)

| ID | Verdict | Effort (claimed → real) | Reality |
|----|---------|------------------------|---------|
| F1 catalog never self-heals | **CONFIRM** | S → S | Every cite exact: `LoadCatalog` sent once at mount (`lib.rs:219-222`), probe ticker re-sends only `Probe` (`ui/mod.rs:1254-1260`), `/tools` accidentally self-heals (`ui/mod.rs:612-615`), empty-state promises reconnection (`transcript_view.rs:761-764`), Start refuses on empty catalog (`ui/mod.rs:412-423`). Edge-triggered re-issue is genuinely small. Highest value/effort ratio in all three lanes. |
| F2 runner FIFO head-of-line | **CONFIRM w/ named hazard** | M → M | Cites exact (`runner.rs:187-193`, `:629-752`, `:389-486`, `:898-945`). The finding's "the move is mechanical" hides one real race it does not spec: a spawned `ProbeAttach` finishing AFTER a user `Start` would send its `Attach` back to the loop and stop the new run's streams to attach the old one — today FIFO ordering prevents this for free. The fix needs an identity guard (attach only if `run_id`/session unchanged and phase not Running from a newer start) and a test pinning it. F2 is also the **enabler** for every fetch-heavy lane-3 modal (see dependency graph) — sequence it first. |
| F3 image memory unbounded | **CONFIRM** | S–M → S–M | Verified: full `Bitmap` behind `Arc` upserted, never pruned (`runner.rs:908-922`, `store.rs:383-394`); 8 MB cap is compressed-size only (`runner.rs:41`); engine decoders have no dimension cap (grep `gfx/decode.rs`); `resize_bilinear` exists (`abstracttui/src/gfx/bitmap.rs:252`) so the worker-side downscale is engine-supported. Note for OBS-5 interplay: keep the compressed bytes server-side story (save-to-disk re-fetches; no conflict). |
| F4 `finish()` lies "completed" | **CONFIRM** | S → S | Exact: `unwrap_or_else(\|\| "completed".into())` at `runner.rs:1253-1257`; 401 path reaches `finish` (`runner.rs:1167-1180`); `run_terminal("unknown")` already lands in the `other` arm (error card + `failed=true`, `transcript.rs:903-908`) so the fold half is nearly free. |
| F5 entity convos unbounded | **CONFIRM w/ nit** | S → S | Verified: `EntityConvo.items` has no truncation (`convo.rs:52`, store comment `store.rs:255-257`). Nit: the fingerprint-scan cost only accrues **while entity-focused** (`wire_feed` reads convos only in the `Focus::Entity` arm, `transcript_view.rs:630-639`); in agent focus convo growth costs zero renders. Still a real slow leak; the shared-helper fix is right-sized. |
| F6 serial boot rehydration | **RE-ESTIMATE (still M, harder than written)** | M → M | The parallel-fetch half is easy (`GatewayClient` is `Clone`, fetches independent). But step (1) "reattach live run first — the same merge works after attach" is optimistic: today's merge is `restored.items.append(&mut f.items); *f = restored` (`runner.rs:696-703`) — a WHOLESALE fold swap. Run it after attach and it clobbers the live fold's run state (`followed`, `parents`, `agent_run_id`, per-run cycles, stats). Attach-first needs a prepend-only merge (items + session totals in, run state untouched) — new fold surface with its own tests. Budget the M accordingly; don't let it shrink to S in the roadmap. |
| F7 silent SSE record loss | **SPLIT** | S → S (notice) / M (re-serve) | Premise verified (`gateway/mod.rs:437-447`): unparseable `step` JSON is skipped and the NEXT event's cursor advances past it. But the finding's optional fix ("do not advance the cursor for the failed event") **is already the behavior** — `on_cursor` only fires on parse success; the loss comes from LATER events advancing past the gap. True re-serve = halt-at-gap + fall to REST refetch, which is M and disproportionate to a server-authored-JSON failure mode. Ship the count+notice half (S, honest labeling); drop the cursor half as written. Unflagged sibling worth one line in the same PR: in the REST fallback, one corrupt record fails the WHOLE `get_ledger` page parse (`get_json` → invalid JSON → retry loop) — same class, different lane. |
| F8 uncancellable in-flight HTTP | **CONFIRM (assessment)** | L, deferred → agree | Windows verified (75 s stream read `gateway/mod.rs:89-92`; 600 s entity turn; stop flags between reads only). Deferral thresholds are sensible. One amendment: NEW-1's never-terminating entity replay stream changes the census (a stale watch socket persists until the next keep-alive read observes the stop flag) — fold that into the documented windows when NEW-1 lands. |
| F9 stale slow-call hint | **CONFIRM defect, CORRECT the repro** | S → S | The fields check out (`transcript.rs:439-445` sets on any followed run's started llm_call; `begin_run` `:241-262` doesn't clear; `rehydrate_run_into` `runner.rs:1367-1369` clears activity/wait only). But the claimed symptom "shows on an idle session" is wrong: the hint renders only inside the running-phase branch (`chrome.rs:358-427`; Idle early-returns at `:312`). The real exposure: reattach to a LIVE run after replaying a prior turn that died mid-llm_call → false "provider may be slow" until the live run's first completion. Narrower than written; the two-line fix is still right. |
| F10 rendering-scale audit | **CONFIRM** | none → none | Bounds verified in code (chunked truncation `transcript.rs:343-371`; fingerprint scan ≤600 items `transcript_view.rs:593-619`). Watch-item 2 (never route token deltas through fold items) should be quoted verbatim in any future streaming design — and it argues against UX-10's reveal animation (see below). |
| Arch: threading model | **CONFIRM** | — | The engine's single-threaded reactive contract + WakeHandle funnel is real; tokio would not change the boundary. Correct call. |
| Arch: channel_source decision | **CONFIRM** | — | Re-examination sound; ledger records are state deltas, not homogeneous samples. |
| Obs: backoff jitter | CONFIRM (no action) | — | One client per gateway today. |
| Obs: drain_rest asymmetry | CONFIRM (doc line) | — | Cheap contract note; do it in the same PR as F4. |
| Obs: UI-thread prefs I/O | CONFIRM (no action) | — | Small JSON, atomic. Watch it if OBS-4/NEW-3 grow prefs. |
| Obs: OnceLock entity client | CONFIRM (doc note) | — | Real for future embedded use; note only. |
| Obs: exec 800 ms polls | CONFIRM (no action) | — | Appropriate for CI. |

## Lane 2 — UX/aesthetics (19 numbered + 5 consider-later)

| ID | Verdict | Effort (claimed → real) | Reality |
|----|---------|------------------------|---------|
| UX-01 raw-JSON tool cards | **CONFIRM, leaning M** | S–M → M | Premise exact: `args_preview` is a 200-char JSON one-liner (`transcript.rs:19,1001`), details-on default (`store.rs:300`), and `intent_summary` exists unused outside the modal (`approval_view.rs:205-264`). The honest cost: `Item::Tool` must carry the summary or raw args — that shape change ripples through 3 fold construction sites (`upsert_tool_started`, `finish_tool` fallback, `consider_wait`), the fingerprint (`transcript_view.rs:427-441`), and a large share of the 283-test suite that constructs `Item::Tool` literals. Also: slimmed terminal records can arrive argless (`protocol.rs:229-278`) — the summary must be computed at STARTED/wait time and preserved, not recomputed at completion. This is the hub node of the tool-card-v2 group (see graph); do it first and alone. |
| UX-02 no diff rendering | **SPLIT + CONFLICT (with NEW-5)** | M → M (scoped) / reject the rest | Engine render half verified free (`FeedBlock::Code lang:"diff"` tinting exists — engine `feed.rs:83-84`, `code.rs:61`). But "derive a unified diff client-side when the result carries before/after" collides with NEW-5's verified rule: **the client never has the old file bytes**. Honest scope: (a) `edit_file` find/replace args rendered as −old/+new hunks (the args ARE the change), (b) fresh `write_file` content as a syntax-highlighted Code block, (c) server-provided diffs rendered when a result carries one. A "real" unified diff with context lines is unobtainable client-side — any roadmap entry promising codex-parity diffs is misleading. Resolve UX-02/UX-04a/NEW-5 into ONE design with NEW-5's honesty rule governing. |
| UX-03 `@file` mentions | **MERGE (≡ NEW-6) + RE-ESTIMATE** | M → M (scope-cut) or L (as written) | "Gitignore-aware file index" is the secretly expensive phrase: correct gitignore semantics (negations, anchoring, dir-only, nested files) is a project, and the honest crate answer (`ignore`) breaks this crate's deliberate 3-dependency posture (`Cargo.toml`: abstracttui + ureq + serde_json) — a maintainer decision, not a line item. Honest M scope: bounded walk (depth/entry caps), hardcoded excludes (`.git`, `target`, `node_modules`), longest-prefix filter, entities-first collision rules. Also adopt NEW-6's remote-gateway gate, which lane 2 omitted entirely: the TUI's cwd is the CLIENT's; the agent reads files on the GATEWAY host — same box today, but the provider must gate on a locality check or the completions fabricate paths the agent can't read. |
| UX-04a approve without seeing content | **CONFIRM (dependent)** | S after UX-02 → same | Verified: `format_scalar` shows first line + `(+N more lines)` (`approval_view.rs:146-159`). Correct priority — this is the highest-stakes approval surface. Blocked behind the UX-02 scoped design. |
| UX-04b deny is mute | **CONFIRM** | S → S | Verified fixed string at `modals.rs:295`. Cheap, real value (the deny that teaches). |
| UX-04c "approve all" ambiguous | **CONFIRM, premise overstated** | S → S | The hint row ALREADY reads "A approve all (session)" (`modals.rs:421-424`) and a toast names the session scope on use (`modals.rs:319`). Only the button label is ambiguous. Still worth the relabel; don't sell it as a trust defect. |
| UX-05 context % + cost | **BLOCKED (partially) — the lane's worst "client-only" claim** | M → S client + gateway ask | Verified: NO gateway route serves a model's context window (grep `context_window\|max_context\|context_length` across `routes/gateway.py` = zero hits outside the entity lane; `discovery/providers` returns model id strings only, `runner.rs:1475-1513`). The "gateway capability probe" the finding leans on does not exist. And the cost half is doubly dead: `model_capabilities.json` carries no pricing (lane 3 verified independently). A client-shipped window table would be the fabricated-selection defect class recorded 2026-07-17. Real shape: file a gateway ask (serve declared context window via discovery), ship NEW-4's token thresholds now, add the % when the field lands (render-when-present, tier-fields posture). |
| UX-06 type-to-filter pickers | **CONFIRM** | M → M | Shared `Picker` (`modals.rs:519-531`) has no filter; the subtle cost is index remapping (`on_choose` closures take indexes into the original vec). One shared fix serves /model, /theme, /workflow, /sessions. |
| UX-07 status-bar legend | **CONFIRM** | S → S | Legend + clipping verified (`chrome.rs:652-706`). `?` binding is free (composer-empty context). |
| UX-08 truncation dead-ends | **RE-ESTIMATE** | M → M–L | The pager modal is easy; the missing half the estimate hides: the fold keeps only previews (200/700 chars, `transcript.rs:19-20`) — full content needs per-card provenance (run_id + call_id/step_id) kept at fold time + a fetch lane (`get_ledger` paging + match) on the runner (arrives cheap only AFTER F2). Do the provenance in the same `Item::Tool` reshape as UX-01 or pay the shape-change tax twice. The `/export` sub-half merges with NEW-3. |
| UX-09 session header card | **CONFIRM** | S → S | Verified: version only in `/help` (`modals.rs:1917-1923`), cwd only in `/workspace`, client knows `workspace_root` (`lib.rs:127-135`) and the gateway-managed clamp is already probed (`runner.rs:470-485`) so the card can be honest about which directory actually applies. Absorbs UX-15. |
| UX-10 "writing answer…" | **SPLIT: label S / reveal REJECT** | S+M → S | The record-not-delta premise is right (verified: no delta shape in `protocol.rs`; SSE = ledger records). But "the strip never reads generic working while the model is composing the final answer" is unachievable honestly — the client cannot know whether the in-flight call will yield an answer or more tool calls; "writing answer…" is a prediction, not a receipt, and this app's discipline is receipts. Ship the honest variant: "model call Ns" with OBS-1's tok/s once established. REJECT the 300 ms section-by-section reveal: fake streaming theater, churns the feed with cosmetic multi-block pushes against F10's watch-item guidance, and burns effort on simulating a capability the transport doesn't have. |
| UX-11 drive-ratio author-speak | **CONFIRM** | S → S | `entities.rs` renders `q/p/i` compact form; a legend line or full words is trivial. Cheap honesty win for the only novel surface in the app. |
| UX-12 composer prompt glyph | **CONFIRM** | S → S | Verified bare `▐ ▌` strokes (`chrome.rs:610-614`). |
| UX-13 tool-card coalescing | **RE-ESTIMATE** | M → L (or late, hard M) | This collides with the most-hardened machinery in the crate: keyed in-place updates (`tool_key`, `transcript.rs:946-952`), `finish_tool`'s oldest-unfinished-same-name fallback for id-less providers (`:1042-1062`), approval flips (`consider_wait` `:1100-1141`), and `wire_feed`'s index-keyed fast path where items vanishing mid-list force rebuilds (`transcript_view.rs:558-591`). Retroactive coalescing (N cards → 1) shifts indices per burst; the only sane shape is fold-time aggregation into a standing "read batch" card — which still rewrites the finish/flip matching rules that carry ~15 pinned tests. Sequence strictly AFTER UX-01, treat as its own project, or accept the cheap 80% (UX-01's sentences make 12 one-line cards scannable; clean mode already folds finished-OK cards entirely — verified `render_item` `transcript_view.rs:303`). |
| UX-14 dropdown fuzzy + rows | **CONFIRM** | S → S | Prefix-only verified (`chrome.rs:580`). Subsequence match + row-cap lift is small. |
| UX-15 boot notices | **MERGE → UX-09** | S | Lane says so itself. |
| UX-16 theme picker polish | **SPLIT** | S → S (title) / M or engine ask (swatch) | Title self-truncation verified (`modal_size(44,…)` vs 47-char title, `modals.rs:572-579`). The per-row accent swatch is NOT S: engine `List` rows render label+detail in list-level styles — no per-row ink API (verified `widgets/list.rs`); the swatch needs a custom row painter (replacing List in the picker) or an engine feature. Ship the title fix; file the swatch. |
| UX-17 help modal truncation | **CONFIRM** | S → S | Verified: desc width `rect.right() - rect.x - 18` ignores the scroll gutter, key gutter fixed 18 (`modals.rs:1938-1944`). |
| UX-18 glyph audit | **CONFIRM** | S → S | Doc table + a few glyph swaps. No risk. |
| UX-19 modal edge contrast | **CONFIRM w/ correction** | S → S | Verified: engine Modal panel is fill-only, no border option (`abstracttui/src/app/popups.rs:91-93`). Correction: no engine ask needed — the client can wrap modal content in the existing `Block` with a border. The 26-theme audit half belongs in the engine's contrast harness (engine-adjacent, coordinate). |
| §9 external editor (Ctrl+X) | CONFIRM-defer | S–M claimed → verify engine suspend first | Suspending the engine's raw-mode/altscreen for `$EDITOR` and resuming cleanly is an engine lifecycle question — check `abstracttui` supports suspend/resume before pricing S–M; if not, it's an engine ask. |
| §9 Esc-backtrack fork | CONFIRM-defer (BLOCKED) | — | Lane 2 already says it: needs a session-fork/seed contract (gateway + durable-sessions semantics). Not a client item. |
| §9 image paste | CONFIRM-defer | M | Attachment upload lane exists gateway-side (Python sibling uses it); real M, next wave. |
| §9 `!` shell passthrough | CONFIRM-defer | — | Crosses the thin-client boundary; needs a maintainer ruling first. Correctly parked. |
| §9 queued-next preview | **CONFIRM** | S → S | One strip line reading `queue.first()`. Genuine quick win. |

## Lane 3 — observability/features (13 ranked + serve + 6 Part D)

| ID | Verdict | Effort (claimed → real) | Reality |
|----|---------|------------------------|---------|
| OBS-1 step timings + economics | **SPLIT: labels S ✓ / /stats M / per-call durations need a caveat** | S → S + M | The evidence is REAL and the strongest in the lane: fixtures carry `gen_time`/`finish_reason` on completed llm_call results and `attempt`/`started_at`/`ended_at` on every record (verified in `tests/fixtures/agent_subrun_ledger.json` result keys); nothing in `src/` reads them (grep = zero). `finish_reason=length` rendering is a genuine trust defect — CONFIRM at rank 1. Two corrections: (1) tool durations are STEP-level — one `tool_calls` record covers the whole batch, and for approval-waited batches the started→completed wall-time includes the human; "✓ write_file · 0.3s" must be batch-labeled (or single-call-only) and computed from the completed record's own window, or it lies. (2) The `/stats` breakdown (time in model vs tools vs waiting, slowest calls, per-model split) is a modal + new aggregation state — that half is M, not S. Fold-field + card/strip labels: S stands. |
| OBS-2 `/tree` subrun view | **CONFIRM (honest M) + risk flag** | M → M | Topology exists (`Fold.parents`/`followed`, `transcript.rs:204-207`, populated at `:762-766`) — the finding does NOT oversell this. What's new: per-run token/status/last-activity accumulation in `Fold::apply` (the hottest, most-hardened function in the crate — the answer-lane and delegate-pollution rules live there, `transcript.rs:493-503`) plus a modal. Per-run SSE health additionally needs runner→store plumbing that doesn't exist. Keep v1 to the modal + per-run counters; the transcript-filter v2 is a separate cost. |
| OBS-3 `/runs` activity board | **CONFIRM + regression hazard** | M → M | Route verified (`list_runs` exists session-scoped, `gateway/mod.rs:261-266`; the cross-gateway variant is a trivial new client method). The hazard the lane misses: **Enter-adopts-a-run must re-derive `finish_on_root_only`** — the goal defense is restored from prefs only for the session's recorded goal run (`wire_goal`); adopting a goal-bundle run started elsewhere reintroduces the iteration-1 false-finish P0 that v0.3.0 just fixed. Derive the flag from the run's workflow id against `goal_workflows` at adoption, and pin it with a test. Cancel-from-board also deserves a confirm step (destructive, cross-session). |
| OBS-4 session history browser | **RE-ESTIMATE** | S/M → M | `rehydrate_run_into` reuse is real (`runner.rs:1297-1371`). The undercounted half is the "viewing a past run" MODE: `store.run_id`/phase/steer/queue machinery all assume the bound run is live — a read-only view must not let steer/cancel/approve/queue-drain target a dead run, and must restore the live state on exit. That's state-machine surface on the seams the queue/steer wave just stabilized, not list UI. Do it after F2/F6 (shares the rehydration path). |
| OBS-5 artifact browser | **CONFIRM** | M → M | Routes verified in gateway source (`/artifacts/search` at `routes/gateway.py:7486`; per-run/session lists; `access_action` labeling). New client methods trivial; modal + save-to-disk honest. Uncapped disk save must stream (the 8 MB `artifact_bytes` cap is inline-render-only — right instinct in the finding). |
| OBS-6 `/gpu` meter | **CONFIRM** | S → S | Route verified (`routes/gateway.py:23466`). Poll-only-while-running honors the idle contract. Reuse F1's edge-trigger pattern. |
| OBS-7 wait/schedule visibility | **CONFIRM** | S → S | `consider_wait` ignores non-approval/ask reasons today (falls through, `transcript.rs:1080-1169`) — a `wait_until` strip line is additive and low-risk. |
| NEW-1 `/watch` entity feed | **CONFIRM (high M)** | M → M+ | Endpoint + contract verified via entities plan + gateway source; SSE parser reusable (`gateway/sse.rs` is generic). Honest additions to the price: the envelope fold is a new family-typed protocol module (line-render only — do NOT import the observer's graph semantics), the observer render-honesty rules are test surface not prose, per-watch threads join F8's zombie census, and the feed lane needs F5-class bounds from day one. This is the entities plan's v1.5 — its natural slot is after the entities-v1 defect margin clears. |
| NEW-2 `/summary` + `/ask-run` | **CONFIRM** | S/M → S/M | Endpoints verified in source (`routes/gateway.py:8203`, `:8295`). Token-spend labeling is the right posture; response shapes unprobed (they cost tokens) — budget one manual probe before building the render. |
| NEW-3 `/export` | **CONFIRM w/ honesty caveat** | S → S | The caveat the lane omits: an md export from fold items exports the VIEW — previews truncated (200/700), transcript capped at 500 items with drops. Either label the truncation in the export header (`#TRUNCATION`, cheap) or render md from the history_bundle (then it's M). The json half (bundle verbatim) is complete and honest as written. MERGE with UX-08's `/export` mention. |
| NEW-4 token budgets | **CONFIRM** | S → S | `_limits` passthrough is genuinely a small `run_input.rs` delta (verified structure at `run_input.rs:86-105`); client thresholds trivial. This is the honest core of UX-05 — ship it as the v1 and let the % ride the gateway ask. |
| NEW-5 files-changed | **CONFIRM · MERGE into tool-card-v2** | M → M | Paths come from STARTED records (args present there; slim hits terminal records only — verified `protocol.rs:220-278`). Its "no client-side diff computation" rule should govern the merged diff design (see UX-02). |
| NEW-6 `@file` mentions | **MERGE → UX-03** | M | Same feature; NEW-6's remote-gateway honesty gate is the version to keep. |
| serve/bridge assessment | **CONFIRM** | — | Correct: execution half fully specified in the interaction plan (item 4, incl. the `fold.failed` fix — already shipped in v0.3.0, `transcript.rs:883`); OBS-3 as the fleet observer is the right sequencing; no `--fleet` panel before real fleets. |
| D: OTEL exporter | CONFIRM-reject | — | Right: the ledger is the telemetry substrate; client OTLP instruments the wrong end. |
| D: audit-tail view | CONFIRM-skip | — | Right call. |
| D: checkpoints/undo | CONFIRM-blocked | — | Honest: no runtime snapshot machinery exists; a client cannot fake it. Runtime ask if ever wanted. |
| D: /plan preset | CONFIRM-defer | — | One-liner whenever the interaction lane wants it. |
| D: run comparison | CONFIRM-reject | — | Export + external diff covers it. |
| D: MCP/skills mgmt | CONFIRM-already-planned | — | Entities plan item 4 + gateway ask #4. |

---

## Dependency graph (what enables what)

```
F2 runner spawn-per-bulk ──────────► OBS-3 /runs · OBS-4 history · OBS-5 artifacts
      │                              NEW-1 /watch · NEW-2 summary  (every new fetch
      │                               lane piles onto the FIFO until F2 lands)
      └────► F6 parallel rehydration ──► OBS-4 (same rehydrate path)

F1 Down→Ok resync (edge-trigger pattern) ──► OBS-6 poll gating · catalog/tools heal

UX-01 Item::Tool reshape (summary + raw args + provenance) ─┬─► OBS-1 per-card durations
      = THE hub node; do once, carry all fields             ├─► UX-08 full-content pager
                                                            ├─► UX-13 coalescing (last)
                                                            ├─► NEW-5 files-changed
                                                            └─► UX-02/04a diff rendering
OBS-1 fold fields (gen_time/finish_reason/attempt/timestamps) ──► UX-10 honest label
                                                                  NEW-3 export timings
                                                                  OBS-2 per-run stats
UX-09 session card ◄── absorbs UX-15
NEW-4 thresholds = the shippable core of UX-05 (ctx% waits on a NEW gateway ask)
UX-03 ≡ NEW-6 (one feature) · UX-08 export ≡ NEW-3 (one command)
goal defense (finish_on_root_only) must extend into OBS-3 adoption + OBS-4 viewing
```

Sequencing that falls out: **wave 1** = F1, F4, F9, F5 + the S-class UX polish
(07/09/11/12/14/17/04b/04c) + OBS-1 labels + NEW-4 + OBS-6/OBS-7. **wave 2** =
F2 → F6, UX-01 reshape (with provenance for UX-08), then OBS-1 /stats + NEW-3.
**wave 3** = the M modals (OBS-2/3/5, UX-06, UX-08 pager, NEW-2), diff design
(UX-02+04a+NEW-5 as one), UX-03/NEW-6. **wave 4** = NEW-1, UX-13 (if still
wanted after UX-01), OBS-4.

## Conflicts requiring one decision each

1. **UX-02 vs NEW-5** — client-side diff computation: NEW-5's "the client
   never has the old bytes" is correct; adopt the scoped design (args-derived
   hunks + content highlight + server-diff passthrough). One design doc, not two.
2. **UX-10 reveal vs the receipts discipline + F10 watch-item 2** — reject the
   animation; keep the honest activity label.
3. **UX-08 wants Ctrl+T** for the pager vs the current theme binding — fine,
   but it invalidates the status-bar legend and muscle memory; decide once.
4. **@file dependency posture** (UX-03/NEW-6): `ignore` crate vs hand-rolled
   scope-cut — a maintainer call on the 3-dep discipline, price differs 2×.
5. **UX-05** must be re-labeled from "client-only M" to "S client + gateway
   ask (context window in discovery)" before the maintainer sees it.

## Secretly expensive (tagged small, actually not)

- **UX-13 coalescing** (M→L): fights the keyed-update/finish-matching/feed-sync
  machinery with ~15 pinned tests.
- **UX-03/NEW-6 "gitignore-aware"** (M→L as written): gitignore semantics or a
  new dependency; M only with the scope cut.
- **UX-05 ctx %** (M→blocked): the "gateway capability probe" it leans on does
  not exist (verified); cost data doesn't either.
- **UX-08 full-content viewer** (M→M–L): per-card provenance + fetch lane +
  pager; cheap only if it rides UX-01's reshape.
- **OBS-1's /stats half** (S→M): new aggregation + modal, distinct from the
  S-class labels.
- **OBS-4 read-only viewing mode** (S/M→M): state-machine isolation on the
  queue/steer seams, not list UI.
- **F6 attach-first merge** (inside its M): the fold swap must become a
  state-preserving prepend — new fold surface.
- **UX-16 swatch** (S→M/engine ask): engine List has no per-row ink.
- **§9 external editor** (S–M→unknown): engine suspend/resume unverified.

## Genuine quick wins (S that is really S, evidence-checked)

F1 (catalog self-heal) · F4 (honest unknown terminal) · F5 (convo bounds) ·
F9 (hint clear) · F7-notice half · OBS-1 labels (finish_reason=length, retry
×N, gen_time on cycles — the single best trust increment in the set) ·
NEW-4 (budgets + `_limits`) · OBS-6 (/gpu) · OBS-7 (wait visibility) ·
NEW-3-json (+labeled md) · UX-07 (footer + `?`) · UX-09+15 (session card) ·
UX-11 (drive words) · UX-12 (prompt glyph) · UX-14 (dropdown) · UX-17 (help
widths) · UX-04b/04c (deny reason, relabel) · §9 queued-next preview.

## Risk register (honesty discipline · tests · v0.3.0)

- **Honesty regressions to refuse in review**: UX-10's "writing answer…"
  (prediction as status), any UX-02 output that renders a fabricated diff
  (context lines the client never saw), UX-05 with a client-shipped window
  table (the 2026-07-17 fabricated-selection class), NEW-3 md export without
  a truncation label, NEW-1 rendering anything from reply prose instead of
  envelope fields.
- **Hardened-seam exposure**: `Fold::apply` + `Item::Tool` (UX-01, UX-13,
  OBS-1, OBS-2, NEW-5) — the answer-lane, delegate-pollution, wait-occurrence
  and slim-record rules all live there; every reshape must keep the 283-test
  suite's fold tests green and extend the fingerprint test. Runner FIFO (F2,
  F6) — the ordering contracts (outcome-before-phase, stop_streams-before-
  spawn) are test-pinned; the ProbeAttach thread move needs a new race test.
  `wire_feed` (UX-13, focus/details interplay) — the visibility mirror test
  is the tripwire.
- **v0.3.0 features at risk**: the goal-run defense (OBS-3 adoption, OBS-4
  viewing — extend `finish_on_root_only` derivation); the queue drain
  (OBS-4's viewing mode must not fake Idle); entity convo epoch guards
  (NEW-1 threads must adopt the same stale-guard discipline); the freshly
  shipped splitless-usage strip truth (OBS-1's /stats must carry the same
  provenance labels — lane 3 already says so, hold it to that).

## Count reconciliation

Lane 1 = 10 findings + 2 architecture verdicts + 5 observations (17).
Lane 2 = 19 numbered + 5 consider-later items reviewed (its "13" table label
undercounts its own content). Lane 3 = 13 ranked + serve + 6 Part D (20).
Every discrete claim in all three documents has a verdict above.
