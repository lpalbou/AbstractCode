# 25% of a session's tool calls failed: an attachment/file namespace split, a one-shot image, and two `edit_file` traps

Date: 2026-08-21
Investigator seat: abstractcode (client seat; the defects are NOT ours)
Session under study: `acode-c8aae4e559a6` — 28 runs, 374 ledger records, 1,193,723 tokens
Status of every claim below: **CONFIRMED** by measurement against the live gateway
ledger unless marked otherwise. Claims marked **REFUTED** are hypotheses this
investigation killed — they are kept because two of them were mine, and the
refutations are the useful part.

---

## 1. Headline

Of 69 tool results in the session, **17 failed (25%)**, and **96% of the
session's 1.19M tokens** were spent inside the four runs that carried them. The
failures are not 17 independent mistakes. They are four defects, three of which
share one shape: **an allowlist or a shape-check whose miss case is silent and
success-shaped**, so no test ever went red.

| n | Class | Owner |
| --- | --- | --- |
| 9 | `analyze_media` — "File … does not exist" | abstractcore + abstractruntime |
| 5 | `edit_file` — identical `pattern`/`replacement` | abstractcore |
| 2 | `edit_file` — `end_line 0 is ambiguous` | abstractcore + abstractruntime |
| 1 | `edit_file` — Python syntax guard | **nobody: correct behavior** |

Plus three defects that produced no error at all and were found only by looking:

- `analyze_media` is the one file-reading tool NOT walled by the workspace
  rewriter — an unwalled read-and-exfiltrate primitive under `workspace_only`.
- The session's worst run reports `meta.tool_calls: 0` while having made 19.
- `open_attachment` silently coerces the same `(0, 0)` line range that
  `edit_file` refuses — one model, one session, two opposite contracts.

## 2. Method

Every ledger record of the session's 28 runs was harvested through
`GET /api/gateway/runs/{id}/ledger` and paired started→completed by `step_id`
(and cross-checked by `call_id`, 69/69). Counts below are re-derived
independently twice — once by this seat, once by an adversarial reviewer
instructed to distrust them. They agree exactly. `mode == "executed"` on all 41
tool batches (no approval/wait path), `finish_reason` is 48 `tool_calls` / 7
`stop` / **zero** `length`, and all 28 runs ended `completed: true`. No failure
hides among the 52 successes.

## 3. Defect 1 — "attachment" and "file" are two namespaces, and only some tools know both

The user attached screenshots. `analyze_media` resolves its argument as a
filesystem path and nothing else:

```python
# abstractcore/abstractcore/tools/common_tools.py:10573
path = _Path(str(file_path or "")).expanduser()
if not path.exists():
    return f"Error: File '{file_path}' does not exist"
```

The model tried both spellings the runtime had shown it — the display filename
and the artifact id — because the injected index hands it exactly those two:

> `Stored session attachments … - Screenshot 2026-08-21 at 4.49.30 AM.png (id=3ac5f9fdc16c889324119baeb9b13af6, image/png, 118,448 bytes)`

Both are "not a path". All 9 failures are that one line. The refusal names no
alternative, so the model retried the same artifact id five times in one run.

**The recovery already exists — for a different tool name.** When `read_file`
fails, the runtime probes the session attachment store, rewrites the failure
into a success and enqueues the image as media
(`abstractruntime/…/effect_handlers.py:4185-4227`). It is gated by
`if seg_item.get("name") != "read_file"` at `:4159`. Had the model called
`read_file("Screenshot ….png")` it would have worked. It called `analyze_media`
with the identical string and hit a wall.

The runtime also already mutates this exact tool's arguments: `analyze_media` is
the sole member of `_SESSION_ROUTE_TOOL_NAMES` (`effect_handlers.py:62`) and gets
a `_session_route` stamp injected at `:3676`. The seam is open; only the probe
is missing.

## 4. Defect 2 — the image rides one call, then vanishes; unless it rode the run's input, in which case it never does

`pending_media` is merged into the call's media and then cleared
unconditionally, **before** `generate()` runs:

```python
# abstractruntime/…/effect_handlers.py:1494-1525
pending_media = runtime_ns.get("pending_media")
...
if isinstance(runtime_ns.get("pending_media"), list):
    runtime_ns["pending_media"] = []
```

So one `open_attachment` buys exactly one look, consumed or not (and a failed
call that retries loses it silently). Measured: **30 LLM calls carried media; 24
of them (80%) completed as pure tool-call turns with no assistant text.** In a
ReAct loop the look is spent emitting the next tool batch, and by the cycle where
the model wants to reason about the picture it is gone. Hence 12
`open_attachment` calls for 2 distinct artifacts, 7 of them byte-identical
repeats.

**The asymmetry that makes this visible.** Attachments that ride a run's own
input are re-forwarded to *every* LLM call of that run
(`abstractruntime/…/visualflow_compiler/compiler.py:937-956`); attachments from
an earlier turn live only in the session index and reach the model only through
the one-shot path. The media patterns per run (M = media on that call) match the
run inputs exactly:

```
f1e42716  context.attachments present   MMMMMMMMMMMMMMMMMM   (18/18)
1c694c19  context.attachments present   MM                   (2/2)
b8835a15  no attachments in input       ...M.M..M...M.M.M.....M
802f92fe  no attachments in input       .M..M
```

So the same screenshot is permanent on the turn it was attached and effectively
unreachable on the next one. The route is not the problem: `gpt-5.4-mini` on
`endpoint:airelay` declares `vision_support: true`, and the token deltas confirm
the images were charged.

**The wording completes the trap.** On a media call the model is told
*"Active attachments are already available in this call. Use their content
directly; do not call tools to re-open them."* That is locally true and globally
misleading: nothing says the look expires, and the sentence forbids re-opening
while the line right below it supplies the filename that `analyze_media` will
reject.

**REFUTED (my hypothesis).** "Drain the media only when the turn produced
assistant content." Wrong predicate: the model *did* consume every look — it
emitted tool calls informed by the screenshot — and 48 of 55 calls in this
session were tool-only, so this rule makes media effectively permanent through a
proxy variable that means something else. At ~3–5k tokens per look per call, with
no cap and no eviction, that is a large silent cost and a late provider failure.

## 5. Defect 3 — `edit_file` refuses `end_line=0` while `open_attachment` coerces it

`start_line == 0` is clamped to 1 with a note ("0-based habit … semantically
safe", `common_tools.py:9182`); `end_line == 0` is refused as ambiguous
(`:9204`). Both failing calls sent `{"start_line": 0, "end_line": 0}` with a
non-empty pattern and replacement.

**REFUTED (my proposed fix).** I proposed treating `end_line=0` as end-of-file in
find/replace mode, arguing a pattern must still match so nothing can be deleted
wrongly. The reviewer killed it with a probe: `require_unique_match =
max_replacements is None` (`:9046`) — and every failing call in this session
passed `max_replacements: 1`, which **disables** the multi-match ambiguity
refusal. Widening the scope therefore edits the first match anywhere in the file
and reports success with a mild "4 more match(es) remain." note. It would also
make `0` the only end value below `start_line` that escapes the reversed-range
refusal. Two tests pin the current behavior, one of them written after the
*previous* occurrence of this exact trace.

**And "teach it harder" has already failed once.** The schema descriptions that
exist today were added after a live trace of `start_line=0, end_line=0`; I
confirmed they were present in this run's `payload.tools`. The model read them
and sent `0` anyway. A third wording pass is not a fix.

**What is actually wrong** is the inconsistency, and it is one layer up:
`execute_open_attachment` silently coerces `start < 1 → 1` and `end < start →
start` (`session_attachments.py:541`), and the model sent `(0, 0)` to
`open_attachment` five times in this session and got clean successes. Same model,
same neutral-fill instinct, opposite contracts. One coercion policy at the
runtime argument layer would end the class; per-tool wording will not.

## 6. Defect 4 — the no-op guard answers a question nobody asked

`if not use_regex and pattern == replacement:` (`:9316`) fires before any preview
handling. Three of the five failures passed `preview_only: true`: the model was
using `edit_file` as a "does this text exist, and where" probe, and got a refusal
that explains `edit_file`'s four modes without answering.

**REFUTED (my proposed fix).** Letting identical-pattern previews through renders
`'No changes would be applied.'` — no path, no line number, no match count. It
does not answer the probe, and the observed model went `preview=true` →
`preview=false` on the very next attempt, so the loop moves one turn later.

**What would help** is one line in the refusal naming the tools that DO answer
it: `search_files` and `read_file(start_line, end_line)` were both available and
100% successful in this session (15/15 and 5/5). The guard is right; its teaching
block is complete about `edit_file` and silent about the actual question.

## 7. Defect 5 — `analyze_media` is outside the workspace wall (security)

`rewrite_tool_arguments` walls twelve tool names and then returns the arguments
untouched (`abstractruntime/…/workspace_scoped_tools.py:609-679`).
`analyze_media` is not among them. Under `workspace_only`, `edit_file
"/etc/hosts"` raises *Path escapes workspace_root* and `analyze_media
"/etc/hosts"` passes through unchanged.

The tool's own source states the stakes: *"analyze_media is the one file-reading
tool that ships file BYTES to a possibly-remote vision provider, so it is the
tool MOST in need of the boundary the sibling read tools already enforce"*
(`common_tools.py:10583`). Its only boundary today is `.abstractignore` — a
per-directory ignore file, not the run's workspace policy.

This is a documented recurrence: the `browser_probe` branch immediately above
exists because *"a new spelling this rewriter did not cover, so local-file probes
bypassed the workspace wall"*. That fix closed the hole for one tool name.

Note for whoever fixes it: walling `analyze_media` does **not** fix Defect 1.
Relative paths never raise (they are anchored under the root), so the
scope-error recovery still never fires for a bare filename. Defect 1 needs the
positive attachment probe; this needs the rewriter branch. Two changes.

## 8. Defect 6 — the worst run reports zero tool calls (observability)

```
run e72c9edf   "meta": {"iterations": 23, "tool_calls": 0, "tool_results": 0}
run 1d59b704   "meta": {"iterations": 18, "tool_calls": 13, "tool_results": 13}
```

`e72c9edf` made 19 tool calls and carried 8 of the 17 failures. Cause:
`_extract_agent_tool_activity` (`visualflow_compiler/compiler.py:2989-3005`)
reads `scratchpad["node_traces"]`; when the subtree was offloaded that value is
`{"$artifact": "…"}` — still a `dict`, so the guard passes, iteration yields the
`$artifact` key, every entry is skipped and the function returns empty lists.
The function already defends against `$artifact` refs one level in
(`_dicts_excluding_refs`, `:3025`) but not against the whole map being a ref. The
output is a confident `0`, not an `unknown`, so any cost-attribution or triage
query keyed on `meta.tool_calls` silently omits the session's worst run.

## 9. Not a bug

The `Refused: edit would introduce a Python syntax error / 325:17 invalid syntax
else:` failure is the guard working. The model's scoped replacement dropped an
`elif` arm and orphaned its `else:`; `_python_parse_error` caught it and printed
the offending line. One failure out of seventeen was the platform protecting the
file.

## 10. Filings

Two seats own everything above. Requests are written to be forwarded verbatim.

**abstractcore** (`abstractcore/tools/common_tools.py`)
1. `analyze_media`: when the path does not exist, say what else it could be —
   *"…does not exist. If this is a session attachment, it has an artifact id;
   ask the host to resolve it."* Today the refusal is a dead end and cost 9
   failed calls in one session.
2. `edit_file` no-op refusal: add one line naming `search_files` and
   `read_file(start_line, end_line)` as the way to locate text. Three of five
   no-ops were `preview_only` probes for "where is this".
3. Do NOT relax the `end_line=0` refusal or the no-op guard. Both were probed and
   both relaxations are unsafe or useless; the reasoning is in §5 and §6.

**abstractruntime** (`integrations/abstractcore/*`, `visualflow_compiler/*`)
1. Extend the `read_file` attachment-store recovery (`effect_handlers.py:4185`)
   to `analyze_media`, or resolve attachment handles at the `_session_route`
   stamp that already targets this exact tool (`:3676`). One `name ==` check away.
2. Add an `analyze_media` branch to `rewrite_tool_arguments`
   (`workspace_scoped_tools.py`) — the unwalled-exfiltration hole of §7.
3. Reconcile the media lifecycle (§4): the one-shot drain and the
   re-forward-every-call path are two contracts for one concept. Whichever way it
   unifies, "Active attachments are already available in this call" must stop
   being true only for one call, and the model needs a way to know the look
   expires.
4. One line-range coercion policy for all tools (§5), so `open_attachment` and
   `edit_file` cannot disagree about `(0, 0)`.
5. `_extract_agent_tool_activity`: treat a `$artifact` map as *unknown*, not zero
   (§8).

**abstractcode (this seat): no code change.** The client uploaded correctly,
the artifact ids and handles it minted resolve, and no failure traces to it. One
observation for a later UX pass, not a defect: chips say *"rides your next
message"*, which is exactly true — and the consequence, that the model cannot
see the image on the turn after, is invisible to the operator.

## 11. The shape worth remembering

Three of these defects (`analyze_media` missing from the wall allowlist,
`analyze_media` missing from the attachment-recovery gate,
`_extract_agent_tool_activity` returning `0` for an offloaded subtree) are the
same failure: **a check whose absent-input case returns a clean, success-shaped
value.** Nothing throws, every per-lane test stays green, and the hole is found
only when a model walks into it and burns a million tokens. The `browser_probe`
walling test was written to close exactly this shape — for one tool name.
