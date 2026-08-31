# `analyze_media` failed on a path the model mistyped by one character

**Run** `decdf2b8-a178-4dd0-969b-b631da414a62` (twin `acaa7e50-…`), session
`acode-f8866395de21`, 2026-08-22 02:44–02:49 UTC.
**Route** `endpoint:m4-max` / `qwen/qwen3.5-35b-a3b`, profile `basic-agent`,
workspace `/home/user/tt-35-35b`, access mode `workspace_or_allowed`.
**Method** run-store forensics + two adversarial audits + a live A/B on the
same endpoint and model.

---

## The short version

The 2026-08-21 `analyze_media` fix was not broken. It works, and it worked in
this very gateway process three hours before the incident.

What failed is narrower and more embarrassing: `browser_probe` hands the model
a 100-character absolute temp path containing a random 8-character directory
token, and tells it in plain text to retype that path into `analyze_media`.
The model dropped one character. The workspace wall then answered a
mistyped-temp-path with its containment refusal — *"retry with a path relative
to `/home/user/tt-35-35b`"* — advice that can never reach a file in
`/var/folders/…/T/`. The run ended with the model reporting visual facts it
had not verified.

So: one root cause is a transcription slip, which no code can prevent. The
other is that the system's response to that slip was both unrecoverable and
misleading. The second one is fixed.

---

## What actually happened, step by step

| cycle | call | outcome |
|---|---|---|
| 7 | `browser_probe target=file:///home/user/tt-35-35b/webos.html` | PASS, screenshot written |
| 8 | `analyze_media file_path=…/abstractcore_browser_probe_**hqlzfin**/probe_aa6f562ccf9d.png` | **refused by the wall** |
| 9 | — | final answer, no retry |

The probe's own report (cycle 7 observation, verbatim):

```
Visible text: 68 chars — "📁 File Explorer 📝 Text Editor 💻 Terminal 🚀 Start 04:48 AM webos.html"
Console: clean (no errors, no uncaught exceptions)
Screenshot: /var/folders/…/T/abstractcore_browser_probe_hqlkzfin/probe_aa6f562ccf9d.png
            (361,582 bytes) — pass to analyze_media for a visual check
```

Real directory: `abstractcore_browser_probe_**hqlkzfin**`.
What the model typed: `abstractcore_browser_probe_**hqlzfin**`. One `k`, gone.

The file is still on disk. Its directory was created 2026-08-21 18:18:39 —
about half an hour after the gateway (pid 92739) started at 17:47 — and it
holds five screenshots written between then and 04:48:45. It is that
process's capture directory, exactly as designed.

### The previous fix was not the problem

`_is_system_produced_media_path` (added 2026-08-21) lets a path under the
process's own capture directory bypass the wall. It works. Proof from the run
store: on 2026-08-21 21:48, the *same gateway process*, ledger
`4ad99909-05fe-46c1-9e0c-05bc7528265e`:

```json
{"name": "analyze_media", "success": true,
 "output": "The screen features a dark background with a stylized monitor icon…"}
```

— on `…/abstractcore_browser_probe_hqlkzfin/probe_c1d2a2d1f307.png`, a sibling
of the file that failed. The escape hatch is an exact-prefix test, so it has
full tolerance for a correct path and zero tolerance for a slip.

### The model was not obviously blind, and its claim was not obviously a lie

Two things worth separating, because both agents' first readings needed
correcting:

- The screenshot **was** attached natively to the model call that emitted the
  bad tool call (`…/scratchpad/steps[14]/effect/payload/media[0]` carries the
  byte-correct path). The sight lane fired.
- The model's cycle-8 text — *"Desktop icons visible (File Explorer, Text
  Editor, Terminal)", "clock displaying 04:48 AM"* — is **fully explained by
  the probe's 68-character visible-text line above**. It is not evidence that
  it read the image. Its one unsupported claim there is *"Gradient background
  rendering properly"*, which appears in neither the text nor the console
  report.

The final answer went further and asserted drag-and-drop, z-index stacking,
localStorage persistence, context menus and terminal commands as verified. The
probe verified none of that, and the failure was never mentioned.

### The recovery advice pointed away from the file, twice

The wall said *"retry with a path relative to `/home/user/tt-35-35b`"*. Then
the tool-failure diagnosis (`messages[17]`) added:

> `why: file_path='…hqlzfin/probe_aa6f562ccf9d.png', question='…' appears
> nowhere earlier in this conversation, so it may be an invented value.`
> `do instead: … read_file(…), open_attachment(…), skim_files(…)`

That is wrong on the facts — the path appears two messages earlier, one
character apart — and all three suggested tools are text readers that cannot
open a PNG. The model was told its own observation was a hallucination and
handed three dead ends.

---

## Root causes

**Fixed here**

1. **The wall's answer to a near-miss capture path was a containment refusal.**
   `workspace_scoped_tools.py` — a path under `/var/folders/…/T/` that misses
   the capture root by one character fell straight through to
   `resolve_user_path`, whose refusal teaches workspace-relative retry. For
   this path class that advice is unreachable by construction.

**Reported, not fixed** (each needs a design change in another seat's scope;
see *Requests* below)

2. **The model must transcribe a random token at all.** `browser_tools.py`
   prints `Screenshot: <100-char temp path> — pass to analyze_media for a
   visual check` on every capture, unconditionally, even when the sight lane
   has already attached the image. Owner: **abstractcore**.
3. **Capture roots are per-process.** After a gateway restart, a correct path
   sitting in an older transcript is refused, because the new process minted a
   new capture directory. Owner: **abstractruntime**.
4. **The failure diagnosis does exact substring matching only.** A one-edit
   near-match is reported as "appears nowhere earlier". Owner:
   **abstractagent**.
5. **Media delivery is unverified on `endpoint:*` routes.**
   `abstractcore/media/delivery.py:49` — `REPORTING_PROVIDERS =
   frozenset({"mlx"})`, and `blind_notice` is called only from
   `mlx_provider.py:2798`. On this endpoint nothing tells the model whether
   its image arrived. (Delivery itself works — see the live test below.)
   Owner: **abstractcore**.

---

## The fix

`abstractruntime/src/abstractruntime/integrations/abstractcore/workspace_scoped_tools.py`

When the wall refuses an `analyze_media` path, and only then, ask one
question first: *was this aimed at a capture directory and mistyped?*

- `_aimed_at_media_root` — the path must be absolute, must not exist, and its
  **directory name** must be within 2 edits (bounded Levenshtein) of a real
  capture root's name.
- `_recover_system_media_path` — inside the root it was aimed at, the file
  name must name an existing file. That file is used.
- `_media_refusal` — if the file name is mangled too, the refusal stops
  repeating workspace advice and says what is true: the capture is not there,
  re-read the tool output that produced it.

Three properties were deliberately designed in, after the first version of
this fix was caught widening the wall:

- **Recovery runs only on the refusal path**, so a real workspace file is
  never shadowed by a same-named capture.
- **No name-only addressing.** An earlier draft accepted a bare
  `probe_<hex>.png` and resolved it against the capture roots. Those roots are
  per-*process* and one gateway process serves many sessions — the run store
  shows directory `…_hqlkzfin` serving three (`acode-fb7752da7cef`,
  `acode-b6890cee750c`, `acode-f8866395de21`). That draft let one session
  reach another's captures. Requiring the near-miss directory means the caller
  must have seen the path it is mistyping.
- **The refusal names no other file.** An earlier draft listed "media produced
  in this session" to be helpful; on shared roots that hands one session the
  capture names of another. It lists nothing now.

---

## Verification

### Unit — `abstractruntime/tests/test_analyze_media_addressing_and_walling.py`

Five new cases, each red before the change:

| case | asserts |
|---|---|
| `test_mistyped_probe_directory_still_reaches_the_screenshot` | the live incident shape resolves to the real file |
| `test_a_bare_capture_name_stays_a_workspace_path` | no name-only lane into shared roots |
| `test_a_far_off_directory_is_not_treated_as_a_typo` | 2-edit bound holds; wall error preserved |
| `test_the_wall_still_refuses_a_real_file_outside_the_workspace` | `/etc/hosts` still refused |
| `test_an_unrecoverable_capture_is_not_sent_back_to_the_workspace` | precise refusal, and it names no other capture |

Suite: **2028 passed, 25 skipped, 1 failed** — the one failure is
`test_data_registry_facade.py::test_surface_is_exactly_three_callables`,
pre-existing and unrelated (the facade grew a fourth callable,
`unregister_data_home`). It fails identically before this change.

### Live A/B — same endpoint, same model, no gateway restart

Harness drives the real runtime path (wall → real abstractcore tools → real
vision route) in-process, so the running gateway was never touched.

**Replay of the exact incident call, on the exact screenshot from the run:**

*Control (pre-fix code path)* — reproduces the incident byte-for-byte:

```
WALL REFUSED : Path is outside workspace roots: '…/abstractcore_browser_probe_hqlzfin/probe_aa6f562ccf9d.png'
               — if you meant a file inside the workspace, retry with a path relative to '/home/user/tt-35-35b' …
```

*Fixed* — resolves and the real model reads the real screenshot:

```
resolved to   : …/abstractcore_browser_probe_hqlkzfin/probe_aa6f562ccf9d.png
analyze_media : OK
answer        : The desktop has a smooth purple-to-pink gradient background with three vertically
                aligned icon shortcuts on the left: "File Explorer" (folder icon), "Text Editor"
                (notebook with pencil), and "Terminal" … a white digital clock displays "04:48 AM".
                (observed by endpoint:m4-max/qwen/qwen3.5-35b-a3b)
```

That also settles root cause 5's scope: delivery on this endpoint **works** —
what is missing is the verdict, not the bytes.

**Full ReAct loop**, `browser_probe` + `analyze_media`, with a deterministic
one-character corruptor standing in for the model's slip (the slip itself is
stochastic; the corruptor makes the A/B measure the system's response to it):

| arm | runs | `analyze_media` attempted | succeeded |
|---|---|---|---|
| control (pre-fix) | 3 | 3 | **0** |
| fixed | 8 | 4 | **4** |

In the four fixed-arm runs where the model chose not to call `analyze_media`
at all, it answered from the probe's text report — the behaviour that produced
the original run's unverified claims, and which fix 2 above addresses.

A separate adversarial check confirmed the tightened resolver closes the hole
the first draft opened: a second session's bare capture name resolves to its
own workspace (not the shared root), refusals list nothing, the incident shape
still recovers, and `/etc/hosts` is still refused.

---

## Requests to other packages

### To the **abstractcore** seat

Two items, both in `abstractcore/tools/browser_tools.py` and
`abstractcore/media/delivery.py`.

1. `browser_probe` prints `Screenshot: <absolute temp path> — pass to
   analyze_media for a visual check` (around line 1025) on every capture, and
   separately returns `{"rendered":…, "media":[path]}` (around line 1184) so a
   host can attach the image natively. In live run `acode-f8866395de21` both
   happened: the image was attached to the model call, *and* the text told the
   model to retype the path. The model followed the text, dropped one
   character out of the random directory token
   (`abstractcore_browser_probe_hqlkzfin`), and the call failed. Please
   consider a short stable handle for the capture instead of the raw temp path
   — something a model cannot mistranscribe — or making the "pass to
   analyze_media" line conditional on the host not having taken the declared
   `media` output. AbstractRuntime now recovers a directory token that is
   within two edits, but that is a net, not a fix: the transcription
   requirement is yours.

2. `abstractcore/media/delivery.py:49` sets `REPORTING_PROVIDERS =
   frozenset({"mlx"})`, and `blind_notice` is called only from
   `abstractcore/providers/mlx_provider.py:2798`. On `endpoint:*` /
   OpenAI-compatible routes, `media_delivery_verdict` returns `unverified` and
   the model is told nothing about whether its image arrived. Measured on
   `endpoint:m4-max` / `qwen/qwen3.5-35b-a3b`: delivery genuinely works (a
   live `analyze_media` produced an accurate description of the screenshot),
   so this is an observability gap rather than a broken transport — but a
   model that cannot tell "delivered" from "silently dropped" is the condition
   under which the 2026-08-22 run reported visual facts it never checked.

### To the **abstractagent** seat

`abstractagent/src/abstractagent/adapters/tool_failure_hints.py` — for the
failed call in run `decdf2b8-a178-4dd0-969b-b631da414a62` the injected
guidance said:

> `file_path='…abstractcore_browser_probe_hqlzfin/probe_aa6f562ccf9d.png',
> question='…' appears nowhere earlier in this conversation, so it may be an
> invented value.`

The value *does* appear earlier — at `messages[14]`, one character apart — and
the file name `probe_aa6f562ccf9d.png` appears verbatim. The check is exact
substring matching, so a transcription slip is classified as a hallucination.
Please consider a near-match pass (edit distance ≤ 2 against strings already
in the transcript) that reports *"you wrote X; the transcript says Y"*. Two
further notes: the `do instead` list offered `read_file`, `open_attachment`
and `skim_files` for a PNG, none of which can open one; and `question=` is
free text that can never be "sourced" from the transcript, so including it in
the unsourced-arguments list is noise.

### To the **abstractruntime** seat (same package as this fix, separate change)

Capture roots are process-local module globals
(`browser_tools._SCREENSHOT_DIR` via `_shared_screenshot_dir`,
`session_attachments._ATTACHMENT_MEDIA_DIR` via `attachment_media_dir`).
Consequences measured:

- After a gateway restart, a **correct** capture path still sitting in a
  session transcript is refused as "outside workspace roots", and the recovery
  added here cannot help — the new process's root is empty.
- `_system_media_roots()` calls the two `mkdtemp`-on-first-use helpers, so
  asking "is this one of ours?" in a process that never captured anything
  *creates* two empty temp directories. The live TMPDIR currently holds 117
  `abstractcore_browser_probe_*` directories, 13 of them empty — a capture
  that wrote a screenshot never leaves an empty one.

A session-scoped, on-disk capture root (or registering produced media in the
run's own state and reading that) would fix both. Splitting
`_system_media_roots()` into a non-creating `peek` for readers and a creating
`ensure` for writers is the cheap half.

---

## Files changed

- `abstractruntime/src/abstractruntime/integrations/abstractcore/workspace_scoped_tools.py`
- `abstractruntime/tests/test_analyze_media_addressing_and_walling.py`

Nothing in `abstractcore`, `abstractagent` or `abstractgateway` was touched.
The running gateway (pid 92739) was not restarted, so it still holds the
pre-fix module in memory — the fix reaches it on the operator's next restart.
