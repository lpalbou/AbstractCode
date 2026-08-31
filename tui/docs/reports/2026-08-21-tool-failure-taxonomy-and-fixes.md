# 10.6% of tool calls failed across 14 sessions — four defects fixed, three refusals defended

Date: 2026-08-21
Investigator seat: abstractcode-tui (client seat; every defect below is in
`abstractcore` or `abstractruntime`)
Corpus: 14 most recent gateway sessions — 900 runs listed, 1,668 ledger records
harvested, **461 tool results, 49 failures (10.6%)**, 8,539,623 tokens
Method: two adversarial design agents with opposing mandates (minimalist vs.
systems), reconciled here. Every claim re-verified against HEAD by execution.

---

## 1. Taxonomy

| n | bucket | meaning |
| --- | --- | --- |
| **22** | PLATFORM DEFECT | the platform refused a call whose intent was unambiguous |
| 18 | REAL EXTERNAL FAILURE | commands exiting non-zero, dead URLs, genuinely missing files |
| 7 | FLOW ERROR-AS-SIGNAL | a workflow node firing a doomed `read_file("")` on purpose |
| 2 | CORRECT REFUSAL | the python-syntax guard, the destructive-command guard |

Concentration matters: 16 of the 22 platform defects are one session, one
model, one file, one 20-minute window. Per tool, `analyze_media` failed **9 of
10 calls** — and the 10th, in a different session, is the worst failure in the
corpus (§3.1). The platform-defect turns cost **435,332 tokens (5.1% of
8.5M)**, excluding the retry turns that followed them.

## 2. The shape behind the recurrence

Four cross-cutting concerns are implemented as per-tool dispatch by NAME
through chains whose fall-through returns the input unchanged:

| table | members | miss case |
| --- | --- | --- |
| `workspace_scoped_tools.rewrite_tool_arguments` | 12 tools | `return out` — silent, **unsafe** |
| `effect_handlers` attachment recovery (`:4159`) | `read_file` only | `continue` — silent |
| `arg_canonicalizer.canonicalize_tool_arguments` | `read_file` only | `return args` — silent |
| `inventory._CLASSIFICATION_BY_NAME` | ~27 tools | **`RuntimeError`** — loud |

`analyze_media` is in exactly one of them: the one whose miss case fails the
build. It is missing from every table whose miss case is a clean return. That
is not carelessness; it is which tables have a mechanism that makes joining
mandatory. The fix for the class is therefore a mechanism, not a lecture — §4.4.

## 3. What was fixed

### 3.1 `analyze_media` could not see, in two different ways

**Addressing (9 failures).** The tool resolves `file_path` as a filesystem
path only. The user's screenshots were session attachments, addressed by
artifact id or by the display filename that the runtime itself puts in the
model's system message. Neither is a path. In one run the model proved it had
the right identifier twelve times — `open_attachment(artifact_id=X)` succeeded
while `analyze_media(file_path=X)` was told the file does not exist, in the
same turn.

*Fix* — `effect_handlers.py`, pre-execution: resolve `file_path` against the
session attachment store, materialize the bytes with their real suffix (the
tool gates on it), rewrite the argument. Pre-execution, not post-execution
result-rewriting like `read_file`'s recovery: this tool must actually run on
real bytes. An ambiguous name (two attachments, one filename) does **not**
resolve — guessing which one the operator meant is the failure class this work
exists to remove. A miss leaves the argument untouched, so a genuinely absent
path still refuses exactly as before.
New: `session_attachments.materialize_attachment_path` (artifact-id-keyed
cache, FIFO-capped at 64 — an unbounded temp dir is the silent growth this
codebase refuses elsewhere).

**Delivery — a confident description of nothing.** The single `analyze_media`
"success" in the whole corpus returned, with `success=True`:

> *"No image or visual content is present in this conversation… The prompt
> arrived without an attached file, screenshot, or embedded picture…
> (observed by mlx/mlx-community/Qwen3.8-27B-4bit)"*

The model is vision-capable; the **transport** is not. `mlx_provider.py`
reduces a structured multimodal message to its first `{"type":"text"}` part
and discards the image parts, and both of its exception paths were
`logger.warning` + continue-without-media. The tool's decode gate proves the
FILE is an image; nothing proved the ROUTE carried it.

*Fix* — the provider records the drop structurally
(`response.metadata["media_dropped"]`) and warns; `vision_fallback`
`_generate_description` raises `VisionGenerationError` on it, so
`analyze_media` falls through to its configured fallback or produces its
honest refusal naming the route. Deliberately **not** prose-matching the
model's answer: the error-substring class stays banned.

**Walling (security, zero measured failures).** `analyze_media` was absent
from `rewrite_tool_arguments`. Under `workspace_only`, `read_file
"/etc/hosts"` raised while `analyze_media "/etc/hosts"` passed through — on
the one file-reading tool whose own source says it "sends image bytes to the
configured vision route (possibly a remote provider)". Relative paths also
resolved against the gateway's cwd, not the workspace, so the tool's own
documented example was broken under the runtime.

*Fix* — it walls like its siblings. The two fixes meet at one seam: an
attachment resolves to a path this process wrote, which must not then be
walled. That is expressed as a predicate
(`_is_system_produced_media_path`, covering the runtime's materialization dir
and the browser probe's screenshot dir) rather than an ordering rule between
two distant call sites — an ordering rule rots the moment a third rewrite is
added. The probe→`analyze_media` screenshot handoff is pinned by test.

### 3.2 `search_files` refused two spellings of its own default (2 failures)

`output_mode='lines'` and `output_mode='context_lines'`, in two different
sessions. The second is the tool's own doing: `when_to_use` listed
`context_lines=N ... output_mode=files_with_matches|count` in one
comma-separated run and never named `content`, so the model bound the
neighbouring PARAMETER as a mode VALUE — and sent `context_lines: 5` alongside,
which is the tell.

*Fix* — a **closed** synonym map in the tool (not in the canonicalizer: these
are argument VALUES whose validity is defined by branches inside
`search_files`, and separating them would make the validation read as a lie),
plus `content` named in the hint. `bogus` and `count_files` still refuse —
`count_files` is genuinely ambiguous between two modes, and a catch-all would
trade a loud refusal for a silent wrong mode.

### 3.3 The worst run reported zero tool calls

Run `e72c9edf`: `meta {"iterations": 23, "tool_calls": 0, "tool_results": 0}`
after making 19 tool calls and carrying 8 of the session's 17 failures; its
sibling reported 18/13 correctly. An offloaded `node_traces` is
`{"$artifact": "<id>"}` — still a dict, so the isinstance guard passed and the
ref's own keys were walked as node ids.

*Fix* — the ref is detected (`is_artifact_ref`) and the counts are **omitted**
with `meta["tool_activity"] = "unknown: node traces were offloaded"`. A zero
that means "I could not look" is indistinguishable from "nothing happened" to
every cost and triage query. Both meta construction sites are covered; a
one-sided guard leaves the lie half-fixed. Three closure-local helpers were
hoisted to module level so the behaviour is testable at all.

### 3.4 The mechanism, so this class stops recurring

`abstractruntime/tests/test_every_path_bearing_tool_is_walled.py` derives
path-shaped parameters from the **live** builtin inventory and fails until
every one is classified either `WALLED` (with a wall branch) or `NOT_A_PATH`
(with a written reason). Verified in both directions: un-walling
`analyze_media` turns it red, and injecting a new tool with a `file_path` and
no decision turns it red.

This is the systems reviewer's real deliverable without its production table.
The loud gate lives where this repo already enforces one
(`_CLASSIFICATION_BY_NAME`'s refusal), and it costs one test file instead of a
27-row contract map plus a rewrite of twelve working branches.

## 4. What was deliberately NOT changed

**`edit_file` `end_line=0` / `start_line=-1` (4 failures).** I proposed
treating them as "unset". Refuted by probe: `require_unique_match =
max_replacements is None`, and every failing call passed
`max_replacements: 1`, which **disables** the multi-match ambiguity refusal —
so widening the scope silently edits the first match anywhere in the file and
reports success. Two tests pin the current behaviour, one written after the
*previous* occurrence of this exact trace; the schema descriptions added then
were present in the failing run's live `payload.tools` and the model sent `0`
anyway. Both reviewers converged on keeping the refusal. The real defect is
the **disagreement** — `edit_file` refuses `end_line=0`, `read_file` refuses
`end_line=-1` that `edit_file` documents as the EOF sentinel, and
`open_attachment` silently clamps both — and reconciling three tools' line-range
vocabulary is its own change, with its own blast radius, on the second-most-used
mutating tool. Filed, not folded in.

**`edit_file` identical `pattern`/`replacement` (5 failures).** Two were
`preview_only=False` — a genuine no-op, correctly refused. Three were
`preview_only=True`, a "locate this text" probe. Letting them through renders
`'No changes would be applied.'` — no path, no line, no count — which does not
answer the probe, while `search_files` and `read_file` already do (15/15 and
5/5 successes in the same corpus). Softening it would write a no-op to disk and
report success. Unchanged.

**The flow's doomed `read_file("")` (7 failures).** Deliberate, per
`abstractflow/scripts/build_coding_agent_workflow.py:576`: "the tool errors, G4
sees script_ok=false". It is another seat's file and a bundle rebuild
(`coding-agent@0.2.7` → a new version). It matters beyond tidiness: 14% of the
corpus's "failures" are theatre, and any future failure-rate measurement is off
by that much until the gate sets `script_ok=false` directly instead of firing a
call it expects to fail.

**The media lifecycle.** `pending_media` is drained after one call whether
consumed or not, while `context.attachments` is re-forwarded to every call — an
attachment is permanent on the turn it arrived and single-use forever after. 30
media-carrying calls in one session, 24 of them pure tool-call turns that
dropped the image unused. Real, and the reason the model re-opened the same
screenshot 12 times. Not folded in: the obvious fix ("drain only on a turn with
assistant content") is the wrong predicate — 48 of 55 calls in that session were
tool-only, so it makes media effectively permanent through a proxy that means
something else, at 3–5k tokens per look per call with no cap. Filed as its own
design question.

**`browser_probe` viewport.** Already fixed at HEAD (`9e2c435`, 04:28 — nine
minutes after the last failing session). No action.

## 5. Files touched

| package | file | change |
| --- | --- | --- |
| abstractruntime | `integrations/abstractcore/session_attachments.py` | `attachment_media_dir`, `materialize_attachment_path` (capped cache) |
| abstractruntime | `integrations/abstractcore/effect_handlers.py` | pre-execution attachment resolution for `analyze_media` |
| abstractruntime | `integrations/abstractcore/workspace_scoped_tools.py` | `analyze_media` wall branch + `_is_system_produced_media_path` |
| abstractruntime | `visualflow_compiler/compiler.py` | offloaded-trace detection; unknown-not-zero at both meta sites; three helpers hoisted to module level |
| abstractcore | `tools/common_tools.py` | closed `output_mode` synonym map; `when_to_use` names `content` |
| abstractcore | `providers/mlx_provider.py` | records dropped media parts structurally + warns |
| abstractcore | `media/vision_fallback.py` | refuses a caption the route never transported |

New tests: 4 files, 40 cases.
`test_analyze_media_addressing_and_walling.py`,
`test_every_path_bearing_tool_is_walled.py`,
`test_offloaded_node_traces_report_unknown_not_zero.py` (abstractruntime);
`test_search_files_output_mode_synonyms.py`,
`test_analyze_media_refuses_undelivered_image.py` (abstractcore).

## 6. Verification

- **abstractruntime**: 2,008 passed, 25 skipped, 1 failed — `test_data_registry_facade` , pre-existing, in the operator's uncommitted work, in a file this change never opened.
- **abstractcore `tests/tools`**: pristine tree 7 failures / with these changes 6. **Zero introduced**; the delta is one `analyze_code` test that flaps run to run in the operator's uncommitted `code_analysis.py` work.
- **abstractcode-tui**: 14 suites green, untouched by this work.
- Every fix mutation-checked: reverting it turns its test red, and the tree restores byte-identically by checksum.

## 7. Deployment

Both packages are **editable installs** — no wheel rebuild. But the gateway
process has held these modules in memory since Thu 05:54; nothing here is live
until it restarts, and **a restart ships every uncommitted working-tree edit**,
including the `fetch_url`/`analyze_code`/`data_registry_facade` work whose tests
are currently red. Review those before restarting. The restart is the
operator's — this seat does not kill the gateway.
