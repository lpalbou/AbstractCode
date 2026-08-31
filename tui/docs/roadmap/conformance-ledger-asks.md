# Conformance — precise gateway/runtime contract asks (2026-07-23)

Product of the conformance hunt over `src/transcript.rs` +
`src/protocol.rs`: every fold decision was audited with the question
*"is this derived from a ledger fact, or inferred from a pattern?"*.
Most inference was replaced with ledger structure (see
`lane-a-diagnosis.md` §1a). The items below are the places the ledger
**genuinely lacks the structural fact** — checked against the
abstractruntime source before being recorded, per the maintainer's rule
(do not hack around a missing contract; ask for the field).

Each ask names: the field, the record/API it belongs on, and why.

## 1. Flow-output answer key is undeclared (interface contract gap)

- **Missing fact**: which key of a flow's final `result.output` object
  carries the ANSWER TEXT.
- **Where it would live**: the `abstractcode.agent.v1` interface
  declaration (bundle entrypoint metadata), or a standard envelope on
  the run-completion record's `result.output`.
- **Today**: `result.output` is the flow-AUTHORED object, passed
  verbatim by the runtime's completion writers
  (`_append_completion_record` serializes `run.output` untouched). The
  abstractagent ReAct workflow declares `{answer, report, iterations,
  outcome, …}` (react_runtime.py done/max_iterations nodes), but wrapper
  bundles author arbitrary shapes — the live coder bundle ends with
  `{report, passed, delivered, …}` and no `answer`. The client therefore
  reads text through a documented precedence ladder
  (`protocol::OUTPUT_TEXT_KEYS` = answer, response, message, text,
  content, report) — a protocol contract by convention, not by
  declaration. A future agent-interface bundle using a seventh key
  renders as "completed without a readable final answer".
- **Ask**: declare the output text key in the interface (e.g. an
  `output_text_key` field beside `interfaces[]` in the bundle entrypoint
  declaration), or standardize the agent-interface completion envelope
  (the ReAct workflow's `{answer, outcome}` pair is already close).

## 2. No answer-source role marker on fan-out spawns (precision ask)

- **Missing fact**: when a root spawns SEVERAL first-level agent-shaped
  children (fan-out/synthesis shapes — e.g. a bundle whose root starts
  three `abstractcode.agent.v1` sub-agents and synthesizes), nothing in
  the ledger says WHICH child's flow end is the turn's answer.
- **Where it would live**: the `start_subworkflow` wait details (beside
  the existing `sub_run_id`/`sub_workflow_id`/`wrap_as_tool_result`),
  e.g. `details.role: "answer_source"` — or an interface-level
  declaration on the bundle.
- **Today**: binding is structural for everything the ledger declares
  (spawned-by-root + agent workflow id + not tool-mode), and
  first-spawned-wins where several qualify — a client POLICY that
  matches every known bundle, not a ledger fact. `wrap_as_tool_result`
  already covers the delegate case; this ask is for deliberate
  multi-agent fan-out at first level.
- **Ask**: an explicit role/answer marker on the spawn record, so the
  choice becomes declaration.

## 3. `abstract.status` payloads carry no structured state (display-only)

- **Missing fact**: whether a status event means "still working" or
  "this activity ended".
- **Where it would live**: the `abstract.status` event payload
  (docs/ui_events.md contract), e.g.
  `{"state": "working" | "done", "text": …}`.
- **Today**: payloads are free text; wrapper-bundle helpers emit
  terminal-sounding texts per round (live: `{"value": "Done"}` every
  poller cycle), so the activity strip clears on a WORD LIST
  ("done"/"finished"/"cancelled"/… — `transcript.rs`, the
  `abstract.status` arm). This is the last free-text match in the fold.
  It gates NOTHING (turn conclusion derives from run status and terminal
  records only) — display cosmetics — but it is still inference where a
  field could exist.
- **Ask**: an optional structured `state` field in the status event
  contract; free-text `text` stays for humans.

## Explicitly NOT asks (checked, the structure already exists)

- **Child workflow identity**: declared by the parent's own spawn record
  (`details.sub_workflow_id` + required `effect.payload.workflow_id`,
  `_handle_start_subworkflow`) and by `GET /runs/{id}` (`workflow_id`,
  `parent_run_id`). Adopted — no ask.
- **Agent-workflow recognition**: the catalog serves each entrypoint's
  run-facing `workflow_id` (`{bundle}@{version}:{flow}`) next to its
  `interfaces[]`; the runtime's Agent-node ids are a documented stable
  contract (`agent_ids.py`). Adopted — no ask.
- **Tool-mode children**: `wrap_as_tool_result` is stamped into the wait
  details by the runtime. Adopted — no ask.
- **Run-end identity**: `result.completed == true` on the terminal
  record (`_append_completion_record`, all call sites; live-verified on
  every flow end of the current gateway's trees). Adopted; marker-less
  `completed+output` records remain accepted as a labeled `#FALLBACK`
  for pre-marker ledgers.
- **Tool-approval waits**: `details.mode == "approval_required"` (the
  runtime's own resume-side check), the `tool_approval:` key mint, and
  `details.executor.kind` are all runtime-minted contracts — citations
  now live on `protocol::is_tool_approval_wait`.
- **Event wait keys**: `evt:{scope}:{scope_id}:{name}` is minted by one
  function (`core/event_keys.py`); the name is everything after the
  third colon. Verified — no ask.
- **`GET /runs/{id}/ledger?after=N` answering 422 when `after` exceeds
  the head** stays documented in `lane-a-diagnosis.md` §5 (gateway
  sharp edge, not a missing fact).
