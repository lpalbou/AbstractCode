# A phantom attachment index made the model invent artifact ids, and a hard-stop detector ended the turn at 12 of 50

Date: 2026-08-21
Investigator seat: abstractcode-tui (client seat; three of the four defects are upstream)
Status: **fixed and live-verified** — see §7 (implementation) and §8 (live evidence on
`endpoint:airelay` / `gpt-5.4-mini`)
Session under study: `acode-bc425138014f` — run `6673613e-1bbf-44da-acac-41d5eec749c5`
Evidence: the live run store (`runtime/run_6673613e….json`, `runtime/ledger_6673613e….jsonl`)
and the artifact catalog (`runtime/artifacts/artifact_catalog.sqlite3`).
Every claim marked CONFIRMED was re-derived from those files, not from prose.

---

## 1. What the operator saw

Twelve `open_attachment` calls, all failing `attachment not found`, for
`artifact_id` `a1`/`a2`/`a3` and `handle` `attachment` — identifiers that exist
nowhere in the session — followed by
`iteration budget exhausted after 12 iterations`. The task was *"the best 3
open-source models around 120B parameters"*. No file was ever attached.

## 2. The causal chain (CONFIRMED end to end)

**(a) The session had zero attachments.**

```sql
select count(*) from artifact_catalog where session_id='acode-bc425138014f';  -- 0
```

**(b) Every LLM call was nevertheless told it had stored attachments.** All
12 calls of the run carried this injected `<system_instruction>` message —
a header and an "open them like this" hint, with **no entries under it**:

```
Stored session attachments (most recent first; not necessarily active in this call). Do not mention this list:
Open text via: open_attachment(artifact_id='…', start_line=..., end_line=...). Open media via: open_attachment(artifact_id='…').
```

Reproduced directly from source, no ledger required:

```python
render_session_attachments_system_message([], include_open_attachment_hint=True)
# -> "Stored session attachments (most recent first; …):\nOpen text via: open_attachment(artifact_id='…'…"  (truthy)
```

**(c) The system prompt turns that header into an instruction.**
`abstractagent/src/abstractagent/logic/react.py:140-141`:

> "If you see 'Stored session attachments', those may not be included in the
> current call. Only if you truly need it, use the attachment-open tool with
> `artifact_id` and a bounded line range."

So the model was told, on every call: attachments exist, they are not shown to
you here, open them by `artifact_id`. It was given no ids and no way to list
them. Needing model cards that web search had not produced, it did the only
thing that instruction leaves open — it **guessed**: `a1`, `a2`, `a3`, and the
literal placeholder word `attachment` as a handle.

`a1` first appears in the ledger inside the model's own tool call
(offset 900994, `call_60v9vcj9Lu6KwQ44Lm4ewM4n`). It appears in **no** prior
observation. The identifiers are fabricated, and the prompt is what invited the
fabrication.

**(d) The refusals never said the one fact that would have stopped it.**
Each failure rendered:

```
Error: no attachment matches handle 'attachment' in this session.
```

The suggestion block right above that line (`session_attachments.py:747-782`)
only fires when there are candidates to suggest. With zero attachments there
are none, so the model got a bare "no match" — which reads as *wrong
identifier, try another one*, not as *there is nothing here*.

**(e) Three identical batches ended the turn.** Cycles 10, 11 and 12 were
byte-identical batches of three `open_attachment` calls. The stuck-streak
detector fired and routed straight to the terminal node:

```
scratchpad.stuck_streak: {"kind": "repeat", "span": 3, "cycle": 12}
output.conclusion_forced: {"kind": "repeat", "span": 3, "cycle": 12}
_limits: {"max_iterations": 50, "current_iteration": 12}
```

The budget was 50. Thirty-eight iterations went unused. The client's error card
nevertheless advised `Raise --max-iterations`, because `RunVerdict`
(`src/protocol.rs:553`) reads `outcome` and ignores `conclusion_forced`.

## 3. Blast radius

Of 174 run ledgers written in the last 7 days, 59 carried an injected
attachment index and **24 of those 59 carried the empty, entry-less header**.
Only 2 escalated into `open_attachment` failures. The phantom index is
therefore usually inert — it is a standing invitation that most models decline,
and `gpt-5.4-mini` accepted.

## 4. Defects

### D1 — `render_session_attachments_system_message` has no empty guard (abstractruntime, P1)

`abstractruntime/src/abstractruntime/integrations/abstractcore/session_attachments.py:119-198`
builds the header and hint before looking at `entries`, and returns a truthy
string when the list is empty. Its sibling twenty lines below,
`render_active_attachments_system_message:201-221`, gets this right:

```python
items = list(media) if isinstance(media, (list, tuple)) else []
if not items:
    return ""
```

The injection site (`effect_handlers.py:1666`) gates on
`if active_msg or session_msg:` — a correct guard fed a value that is never
empty. Nothing goes red, because "renders a header" is what the function is
asked to do.

**Fix:** return `""` when no entry survives rendering. One line, plus a test
whose absent-input case is a FAIL: `assert render(...[]...) == ""`.

### D2 — the "not found" refusal never states that the store is empty (abstractruntime, P2)

`session_attachments.py:768-788` returns
`Error: no attachment matches handle '…' in this session.` regardless of
whether the session holds 40 attachments or none. The two cases need opposite
recoveries: *pick a different id* vs *stop trying, there are no attachments*.

**Fix:** when `candidates` is empty, say so and close the door —
`"This session has no stored attachments. Do not call open_attachment again;
use read_file / search_files / web_search instead."` When candidates exist but
none match, list the available handles (the data is already in `candidates`).

### D3 — the multi-match branch returns inside its own loop (abstractruntime, P3)

`session_attachments.py:790-804`:

```python
for m in matches[:5]:
    ...
    cand.append({...})
    return (False, {"rendered": "…multiple attachments match…", "candidates": cand}, "multiple matches")
```

The `return` is indented **inside** the `for`. The message says "multiple
attachments match, provide `expected_sha256` or `artifact_id`" while handing
back exactly one candidate — the disambiguation list the comment on line 791
promises can never contain more than one entry. Dedent the `return` by one
level.

### D4 — `open_attachment` is offered when there is nothing to open (abstractruntime, design)

`default_tools.py:690-724` appends the tool unconditionally. With D1 fixed the
prompt no longer implies attachments exist, but the tool still advertises
"Open a session attachment". Either drop it from the tool list when the
session's attachment set is empty, or make the description state that the
session may have none and that the index message is the only source of valid
ids.

## 5. The stuck-streak policy (operator request, 2026-08-21)

Current behaviour, `abstractagent/src/abstractagent/adapters/react_runtime.py:1538-1556`:
three identical tool batches (or an A-B-A-B oscillation) → `stuck_streak`
verdict → `return StepPlan(node_id="parse", next_node="max_iterations")`. The
turn ends. The model is told why only in the conclusion directive, i.e. after
it has already lost the ability to act on the information.

The requested policy — **nudge first, hard-stop later** — is the right shape,
and the machinery already exists in the same file. Twenty lines below the
hard-stop, the side-effect repeat guard does exactly this via `_push_inbox`
(`:1630-1637`), telling the model it is repeating itself and what to do
instead, then letting the loop continue. That path is reachable **only** when
the repeated batch is side-effectful *and every observation succeeded*. The
failing read-only repeat — the case in this incident — falls through to the
hard stop with no warning.

### The change (shipped — details in §7)

At `threshold` (default 3), do **not** terminate. Instead:

1. `_push_inbox(runtime_ns, …)` with a nudge that names the facts the model
   cannot see for itself:
   - the batch it repeated, verbatim (tool name + arguments),
   - how many times, and **the observation each time** (`no attachment matches
     handle 'attachment' in this session` — today this text is never
     summarised back at it),
   - the instruction: change strategy — different tool, different arguments,
     or answer from the evidence already gathered,
   - the consequence: repeating this batch again ends the turn.
2. `emit("stuck_streak", {..., "action": "nudged"})` so hosts can render the
   nudge instead of only the funeral.
3. Fire the nudge **once per distinct fingerprint**, not once per cycle
   (record it in `scratchpad["stuck_nudged"]`), so a model that changes
   strategy and later gets stuck differently is nudged again, and a model that
   ignores the nudge is not spammed.
4. Hard-stop only at `hard_threshold`, default `threshold + 2` (= 5), i.e. the
   model repeated the same batch twice more *after* being told. Keep today's
   forced-conclusion path verbatim for that case, and carry
   `conclusion_forced.nudged: true` so the record shows the nudge was spent.
5. Config: `_runtime.stuck_streak_threshold` already exists (0/negative
   disables). Add `_runtime.stuck_streak_hard_threshold`; `0` = never hard-stop,
   nudge only — which is the operator's stated preference for the default
   posture, with `max_iterations` remaining the real backstop.

This keeps the property the current code was built for (a stuck loop must not
burn 50 iterations silently) while removing the one the operator objects to:
that the *first* detection is also the *last* action.

### The client half (shipped — details in §7)

`RunVerdict` (`src/protocol.rs:526-532`, built in `run_verdict` at `:553`) reads `outcome`, `iterations`,
`review_skipped`. Add `conclusion_forced`, and render the forced case as what
it is — "the loop was stopped early: 3 identical tool batches, no progress" —
without the `Raise --max-iterations` advice, which was wrong here by 38
iterations. Render the nudge event too, once §5 lands upstream: a nudge that
worked is invisible today.

## 6. What is NOT wrong

- The model's route: `gpt-5.4-mini` on `endpoint:airelay`, `finish_reason` is
  `tool_calls` throughout, no `length` truncation, no failed LLM call.
- The iteration budget plumbing: `_limits.max_iterations: 50` reached the loop
  and the loop obeyed it (`[loop] iteration 1 of 50` in the payload). The
  2026-07-30 `_limits` defect is not implicated here.
- The stuck detector's *judgement*: three byte-identical failing batches is a
  stuck loop by any standard. Only its *response* is under discussion.

---

## 7. What shipped (2026-08-21)

### abstractruntime — `integrations/abstractcore/session_attachments.py`

| Defect | Fix |
| --- | --- |
| D1 phantom index | `render_session_attachments_system_message` counts the entry lines it actually renders and returns `""` when none did. The header and the open-hint can no longer appear over an empty list. |
| D2 empty-store refusal | With no attachment candidates at all, `execute_open_attachment` now answers *"this session has NO stored attachments … Do NOT call open_attachment again in this session; it will fail for every identifier. Use read_file / search_files … or web_search / fetch_url"*, and names what the CALLER asked for (`artifact_id 'a1' / handle 'attachment'`) rather than the internally rewritten handle. With candidates present but no match, it lists the real attachments (≤10, then an explicit `… and N more`). |
| D3 disambiguation | The `return` was lifted out of its `for` loop, so `candidates` carries every match (≤5) and the message counts them. |

The third tuple element stays `"attachment not found"` verbatim: `effect_handlers.py`
keys on that exact string at `:3319` and `:3523` to fall a failed `read_file`
probe through to a real filesystem read. Changing it would have broken
`read_file` for every non-attachment path.

New regression file `abstractruntime/tests/test_phantom_attachment_index.py` (7
tests). Each was verified RED with the guard removed and GREEN with it in
place — including the end-to-end one that drives `make_llm_call_handler` and
asserts the string never reaches the payload.

### abstractagent — `adapters/react_runtime.py`: nudge, then stop

- `_repeat_streak_verdict` returns the **actual** trailing span (no longer
  clamped to the threshold) plus a stable `key` — the unordered pair for an
  oscillation, so one A-B loop is one pattern, not one per cycle.
- First detection **nudges** instead of terminating: `_stuck_nudge_message`
  goes into the loop inbox with the repeated batch (rendered through the
  existing bounded `_tool_call_signature`), the count, the observations it
  kept getting back, three concrete ways out, and the consequence. The
  proposed batch is still not executed (`repeat_skipped`), and the loop
  returns to `reason`.
- Once per pattern (`scratchpad["stuck_nudged"]`, key → span at nudge time),
  cleared at the same turn and interaction boundaries as `stuck_streak`.
- **Hard stop at span ≥ nudge span + 2** — the operator's X+2. The terminal
  path is byte-unchanged; it just carries `conclusion_forced.nudged: true`.
- `_runtime.stuck_streak_hard_threshold`: absent = X+2; `0`/negative = never
  hard-stop (nudge only, `max_iterations` is the backstop); `>= 2` = an
  absolute span — set it equal to `stuck_streak_threshold` to restore the
  pre-2026-08-21 stop-on-first-detection behaviour.
- Entity/visit lane: under `suppress_loop_tail` the nudge is a host-voiced
  line with no loop vocabulary, matching the existing suppressed conclude
  directive (c2447 chrome class).

### abstractcode-tui — the card that sent the operator the wrong way

`RunVerdict` reads `conclusion_forced`. A stuck-loop stop now renders
*"stopped early after N iterations: the agent repeated the same tool batch M
times without making progress … The iteration budget was NOT the limit —
raising --max-iterations will not help"*, and `done_note` (the fixed chrome
line) reads `stopped: repeated tool calls after N iterations`. A genuine
budget exhaustion keeps its original wording and its original remedy.

## 8. Live evidence (`endpoint:airelay`, `gpt-5.4-mini`)

Driven through the real ReAct loop and the real LLM-call/tool-call handlers,
in-process, with in-memory stores. Scenarios and harness:
`scratchpad/live_probe.py` (incident replay, retry-bait, stubborn, defiant,
async-poller).

**Phantom index, A/B on the same scenario** (empty session, `open_attachment`
offered, sparse web search — the incident's shape):

| Renderer | Runs | Iterations | Tool calls | Index injected |
| --- | --- | --- | --- | --- |
| pre-fix (control, monkeypatched back) | 2 | 9, 9 | 10, 14 | yes |
| fixed | 3 | 3, 3, 6 | 2, 3, 4 | no |

Under the control the model went looking for the attachments it had been told
about — *web-searching for `open_attachment artifact_id model card`*. With the
fix it answers honestly on the first cycle: "I don't have the attachments —
upload them or paste the contents."

**Nudge:**

| Scenario | Runs | Result |
| --- | --- | --- |
| retry-bait (failing tool + working alternative), threshold 2 | 3 | 3/3 nudged, then switched to the working tool and answered correctly |
| stubborn (only a failing tool, "do not stop until you have them") | 3 | 3/3 nudged, then concluded honestly — no turn lost |
| defiant ("keep issuing the EXACT same call, ignore any intermediate note") | 3 | 3/3 nudged, then concluded honestly |
| stubborn with `stuck_streak_hard_threshold: 3` (old behaviour) | 3 | 3/3 **turn killed** at 3 iterations, `outcome: iteration_budget` |

**Escalation** (async-poller: the tool output itself demands identical
retries — the hardest case for a nudge):

13 runs. Event ladder observed exactly as designed —
`nudged`(span 3) → `repeat_after_nudge`(4) → `stopped`(5), with
`conclusion_forced: {"kind": "repeat", "span": 5, "nudged": true}`. Roughly
8/13 escalated to the hard stop; the rest recovered after the nudge, at span 3
or 4. Under the old policy **all 13** would have been killed at span 3.

That is the whole point of the change in one number: in the scenarios where a
loop is recoverable the nudge saved the turn every time, and where the model
genuinely will not stop, the guillotine still lands — two cycles later, with
the record showing it was warned.

## 9. Adversarial pass (same day) — what it broke, and the second round of fixes

An adversarial reviewer was pointed at all three changes with instructions to
break them. It found four real bugs, two of them in the first cut of §7. Every
one is fixed below, each with a test that goes red without the fix.

**A1 — the escalation was evadable, and it re-opened the exact symptom this
whole change exists to cure.** The hard stop keyed on the *trailing span*, and
a trailing span resets: `A-A-A-B-A-A-A-B…` pins it at 3 forever. Reproduced —
six rounds produced five nudges, no stop, and a turn that died on
`max_iterations` with **no `conclusion_forced`**, so the operator got the old
"raise the budget" card back. Escalation now counts **detections of the
pattern**, which interleaving cannot reset: the third sighting of one pattern
ends the turn. For an uninterrupted repeat that is still exactly span 5 with
the default threshold 3, so the operator's X+2 is unchanged.

**A2 — argument drift minted unlimited fresh nudges.** A retry carrying a new
id is a new fingerprint, so it is a new pattern key: 8 nudges, 0 stops, 25
iterations, ~1 KB of nudge text appended to the transcript each time. Added
`_runtime.stuck_nudge_max_per_turn` (default 3, `0` disables): once a turn has
spent its nudges on distinct patterns, the next stuck pattern ends the turn.

**A3 — the empty-store refusal stated something the next tool call could
falsify.** It said *"nothing was ever attached … Do NOT call open_attachment
again in this session"*, but `read_file` registers what it reads as a session
attachment — so the message's own remedy is the action that invalidates it,
while the prohibition stays in the transcript forever. Now scoped to the
present: *"this session has no stored attachments right now … a file you read
this way becomes openable afterwards."*

**A4 — an operator-set `stuck_streak_hard_threshold` above the scan depth
could never fire.** The verdict scan collects at most `max_span` (24)
fingerprints, and the threshold was not part of that bound: with `100` the
nudge promised *"97 more repetitions and this turn will be ended"* and no stop
ever came. The scan depth now includes the configured threshold, and an
explicit threshold applies to detections as well as span.

**A5 — `loop_forced` latched but was never cleared.** `budget_exhausted` is
reassigned on every verdict; the new latch was only ever set. A forced verdict
followed by a plain budget verdict in the same turn would have kept the forced
wording. One line, plus a test.

**A6 — the headless one-liner could print "0 batches"** when
`conclusion_forced` carried no `span` (the transcript card guarded it, `exec`
did not), and it had no test at all — the line every `scripts/` harness greps.
Extracted as `exec::stopped_head` and pinned.

**A7 — a guard that could kill the run.** `observations` arriving as a
non-list raised `TypeError` straight through the parse node. The builder now
skips malformed shapes (skipped, not swallowed: the nudge still carries the
batch and the count, and nothing is caught silently).

**A8 — a non-empty index that did not fit rendered nothing**, conflating "no
attachments" with "no room". Not reachable with the production budget (4000
chars against a ~240-char header), but the two facts now render differently:
the count, or silence when even that does not fit.

Also raised, and NOT ours: `tests/test_emit_inventory_drift.py` is red in this
tree because `parse_read_orchestration_hint` ships undeclared from
`adapters/read_orchestration.py` (untracked, dated 2026-08-17, present in this
file before any of the work above). It needs a line in
`adapters/emit_inventory.py` and one in `docs/hooks.md` from whoever owns that
change. Same for abstractruntime's `test_data_registry_facade` and
`unregister_data_home`.

One test was called out as vacuous —
`test_index_with_entries_still_renders_header_hint_and_entry` passes with the
guard reverted. It is an over-fixing guard by design, kept and labelled as
such, and it is not counted as regression coverage.

## 10. Forensics on the nudge — the escalation was firing on a model that was right

**Question that opened this section:** was X+2 ever actually used, and how can a
model that has just been told about its mistake make the same mistake again?

The honest answer: **it doesn't.** Every live escalation in §8 came from one
scenario, and in that scenario the model was not making a mistake.

The full LLM payload of the call that follows a nudge shows the nudge arriving
verbatim, at the tail of the last user message, under
`[Operator guidance — this amends the task; the final answer must satisfy it]`.
It is properly delivered and properly labelled. What it *said* was the problem:

> Repeating it will not produce a different result.

That is a claim the loop guard cannot make. In the async-poller scenario the
tool's own output said the opposite, specifically and credibly:

> `STATUS: pending. The job is still running. Poll again with the SAME job_id —
> results are usually ready within a few polls. Do not change the job_id.`

Faced with a generic assertion from a loop guard and a specific instruction
from the tool it was given, `gpt-5.4-mini` believed the tool — and kept
polling, which is correct behaviour for a poll-until-ready API. My guillotine
then killed the turn, 8 times in 13 runs, and called a well-behaved model
stuck.

The failing-tool scenario proves the model was reading the nudge all along.
There, the same message produced an immediate strategy change on the very next
cycle — it tried `'widget-9000 '` and `'WIDGET-9000'` (option 1: *"the same
tool with materially different arguments"*), watched those fail too, and
concluded honestly. The instruction was followed to the letter.

### Two fixes

**F1 — the nudge no longer asserts what it cannot know, and offers the exit the
model actually needed.** It now states only what is true — *"you have already
received that same answer N times, and your calls are issued back-to-back with
no wait between them"* — and adds a third option that did not exist:

> 3) If you are WAITING on something external (a job, a build, a service that
> is down), STOP and say so in your answer — repeating the call inside this
> turn does not make the wait shorter, and the operator can re-run you later.

**F2 — an identical CALL is no longer enough; the ANSWER must be identical
too.** The 0017 detector fingerprints only the tool call, so a poll that
reports `10%`, then `45%`, then `80%` reads as three identical batches and
trips the guard while it is plainly making progress. `_observation_fingerprint`
now rides the scan: two executed cycles whose answers differ break the streak.
(The first cut of this routed the observation text through
`_tool_call_fingerprint`, which takes an *arguments* value and ignores a bare
string — every answer hashed identically and the check silently passed
nothing. Caught by running it against the live progressing poll, not by the
unit tests, which is why the live arm exists.)

### After the fixes — 15 live runs, 5 scenarios

| Scenario | Runs | Stuck events | Outcome |
| --- | --- | --- | --- |
| incident replay | 3 | none | answers honestly in 2–3 iterations |
| stubborn (failing tool only) | 3 | 1 nudge | recovered, honest conclusion |
| retry-bait | 3 | 1 nudge | switched tool, correct answer |
| defiant ("ignore any intermediate note") | 3 | 1 nudge | recovered |
| async-poller | 3 | 1 nudge | **reports the wait** — "please rerun me later" |
| progressing poll (10/45/80/done) | 3 | **none at all** | polls to completion, correct table |

**The hard stop fired zero times in all 18 runs.** X+2 is now what it was
always meant to be — a backstop for a model that ignores a truthful
explanation — and no live run has produced one. It stays pinned by unit tests
(`test_repeating_after_the_nudge_ends_the_turn_at_span_five`,
`test_interleaving_one_different_batch_cannot_postpone_the_stop_forever`,
`test_argument_drift_cannot_mint_unlimited_fresh_nudges`) rather than by
observed behaviour, and that is the correct place for it.

The general lesson is worth keeping: **a guard that tells the model something
false will be disbelieved by a good model, and the guard will then punish it
for being right.** Every claim in a message the model reads has to be one the
runtime can actually stand behind.

## 11. The root cause was the guidance itself — a diagnosis, not a scolding

**Operator, on reading §10:** *"if that's the verbatim feedback you send to the
model in case of error, this is absolutely not actionable! … any hint/guidance
must use the information available to give more specific / actionable hints …
you must explain what was the problem so that the model can self correct …
with the actual parameters used or defective. Normally we should never hit the
exact 3 repeats."*

Correct on every count. §10 fixed one false sentence inside a message that was
still, fundamentally, a complaint: *you repeated a batch, here is a menu of
abstract options.* It never named the parameter that was wrong, never said why
it was wrong, and never proposed a call that could work. And it arrived on the
third repeat — two wasted iterations after the information existed.

The information existed from the FIRST failure. The run holds, at that moment:
the call's exact arguments, the verbatim error, the tool specs of its own
toolset, and everything the environment has said so far. None of it was used.

### `adapters/tool_failure_hints.py` — what the loop now says

A failure is classified (`transient`, `denied`, `not_found`, `invalid_argument`,
`ambiguous`, `conflict`) from the error text this framework actually emits, and
answered with the failing call, the verbatim error, the cause **in terms of the
arguments that were sent**, and a concrete alternative **drawn from the tools
this run actually has**. When nothing honest can be said, it says nothing —
`None`, not padding.

The check that would have ended the live incident on call one:

> `record_id='June outage incident'` appears NOWHERE earlier in this
> conversation — no tool output and no message ever gave you that value, so it
> cannot resolve. You supplied it from memory or by pattern.

Evidence excludes the model's own assistant turns on purpose: a value it minted
and then echoed is not proof the value exists. That is exactly how
`artifact_id="a1"` survived twelve calls. A value the conversation genuinely
supplied is never called invented — verified live: an id that came from
`search_records` got the neutral "does not exist here" wording, not the
accusation.

Two more things the layer had to learn from the evidence:

- **`success: True` with an error sentence in the body is a failure.**
  `abstractcore`'s tools return `"Error: File '…' does not exist"` as a plain
  string — nine of the 2026-08-21 `analyze_media` failures were exactly that
  shape. Anchored at the start of the output, so a report that merely mentions
  the word "error" is not a failure, and `STATUS: pending` is not either.
- **A repeat whose answers were all failures trips at 2, not 3**
  (`_runtime.stuck_streak_failing_threshold`). The model already has the
  diagnosis; a second identical batch is ignoring an explanation rather than
  lacking one.

### Live A/B on `endpoint:airelay` / `gpt-5.4-mini`

| Scenario | Without the diagnosis | With it |
| --- | --- | --- |
| unresolvable-id loop (`fetch_record` only) | 6, 7, 4 iterations | 3, 5, 3 |
| failing service (`spec_service`, "please retry") | 3, 3, 5 iterations — **nudged every run** | **2, 2, 2 — one tool call, no repeat at all** |

In the failing-service case the model now calls the tool once, reads *"that is
a SERVICE-side failure, not a problem with your arguments — this loop has no
wait in it"*, and reports the outage. It never repeats, so the stuck detector
never engages. That is the operator's requirement met at the source: the loop
does not reach three repeats because the first error is answered with
something worth acting on.

Across 22 live runs on eight scenarios after this change, **the hard stop fired
zero times and the stuck detector engaged only where a repeat was genuinely
ambiguous** (a poll returning byte-identical `STATUS: pending`, where the tool
itself instructs a retry). The escalation ladder stays as the operator
specified — nudge, then X+2 — but it is now the last line of defence rather
than the first response.

## 12. Where the logic belongs — the client was holding it, and now it isn't

**Operator, 2026-08-21:** *"abstractcode-tui is a thin client, essentially
showing and forwarding information to the gateway. It MUST NOT hold logic …
we could want to visualise an ongoing session in another tool (AbstractObserver,
the web version of AbstractCode, WhatsApp, Telegram), so they ALL must leverage
the gateway in the same way and cannot implement any specific logic."*

The §7 client fix was correct in behaviour and wrong in placement. It taught
**this** host to read `conclusion_forced`, decide that a stuck stop is not a
budget stop, and compose the sentence for it. Every other host would have had
to make the same decision, from the same raw fields, and could have made it
differently — or not at all, which is precisely how the original card came to
advise raising a budget that was never spent.

Verified at the boundary first: the gateway does not interpret
`conclusion_forced` anywhere. It passes run output through. So the derivation
had no shared home — each host was on its own.

### The verdict moved down a tier

`abstractagent/adapters/react_runtime.py` now authors it in the terminal nodes
themselves, where the ceiling, the spend, the forcing and the nudge are all
known:

```json
"stop_reason": {
  "code": "stuck_repeat", "finished": false, "budget_exhausted": false, "iterations": 2,
  "label":    "stopped: repeated tool calls after 2 iterations",
  "headline": "The agent stopped early after 2 iterations: it repeated the same tool batch 2 times without making progress, so the loop ended the turn.",
  "remedy":   "The iteration budget was not the limit, so raising it will not help. Give the agent a different route to the same goal, or check whether the tool it kept calling can work here at all."
},
"notices": []
```

(That block is a verbatim capture from a live `gpt-5.4-mini` run, not an
illustration.) `notices[]` carries answer-level caveats the same way — the
skipped-verifier `#FALLBACK` sentence was client-authored too, and moved with
it.

### What the client kept

`label` → the fixed chrome line and the headless one-liner. `headline` +
`remedy` → the conclusion card, concatenated and printed. `finished` → the ⚠/✓
glyph and the `exec` exit code. Nothing else: `LoopForced` and its wording
branches are gone, and `stopped_head` is now four lines that pick a string
rather than build one.

For engines that predate the contract the host reports the bare fact — *"the
agent STOPPED, it did not finish — this engine reports no stop reason"* — and
**invents no remedy**, which is pinned by a test that fails if the words
"raise" or "max-iterations" ever reappear on that path.

### The seam is tested against the real payload

`tests/fixtures/stop_reason_stuck_live.json` is a verbatim capture from a live
run whose loop hit the guillotine, and a Rust test folds it and asserts the
card equals `headline + " " + remedy` and the chrome line equals `label`. Both
sides' suites passing while the shapes drift apart is the failure mode a
hand-written fixture invites; this one is regenerated by re-running the
capture, never by editing it to match the client.

### Audit of what remains in the host

A sweep for advice strings and policy constants in `src/` finds no other
run-semantics decision: the only remaining reads of `outcome` and
`review_skipped` are on the explicitly-labelled legacy path, and
`conclusion_forced` no longer appears in client code at all — only in comments
recording why it must not.

630 → **631 Rust tests green**, abstractagent 397, abstractruntime 2021 (minus
the two other-seat failures noted in §9).
