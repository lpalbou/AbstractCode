# Cycle-2 adversarial review — PRESENCE + DENSITY chrome (reviewer 2)

Scope: the HDR-1 / REST-1 / CTX-0 / IDLE-1 / OBS-1a-live / OBS-6 / POLISH
chrome that three build agents landed concurrently on top of commit
34b9447. Owned files reviewed line-by-line: `src/ui/chrome.rs`,
`src/ui/transcript_view.rs`, `src/store.rs`, `src/config.rs`,
`src/cli.rs`, `src/commands.rs`, `src/convo.rs`, `src/run_input.rs`.
Read-only receipt tracing: `src/transcript.rs`, `src/ui/mod.rs`,
`src/runner.rs`, `src/gateway/gpu.rs`, `src/protocol.rs`, engine crate
`abstracttui-0.2.1` (canvas/print, truncate, Spinner).

Baseline at review start: 341 tests green, clippy clean.

---

## Findings

### P1

**P1-A — tok/s numerator contamination on splitless-usage providers
(fabrication class). CONFIRMED — handoff (both writer sites unowned).**
CLOSED cycle-3: the exact edit below landed in `wire_llm_meter` +
regression test (see Handoffs §1).

- The strip's `N tok/s (last call)` is minted by `wire_llm_meter`
  (`src/ui/mod.rs:1411-1437`): numerator = `f.stats.output_series.last()`,
  denominator = client-observed wall time.
- But `Fold::fold_usage` (`src/transcript.rs:1192-1196`) deliberately
  substitutes the call's **total_tokens** into `output_series` when the
  receipt is splitless (`input==0 && output==0 && total>0` — the live
  coder-run provider shape, after the raw-usage repair found no split).
  That substitution is correct for the SPARKLINE ("per-call activity")
  and wrong as a throughput numerator: on splitless providers the
  rendered rate divides *input+output+reasoning* by wall time —
  **overstating** output throughput (a 40k-context call at 300 output
  tokens reads ~130× hot), while `src/store.rs:238-244` documents the
  opposite direction ("slightly UNDERSTATES … conservative").
- Split-usage providers (the common lmstudio/openai path) are unaffected;
  the label "(last call)" is present at the only render site
  (`chrome.rs::model_call_segment`, verified single-site by grep).
- **Handoff — exact edit in `src/ui/mod.rs::wire_llm_meter`**: replace the
  `output_series.last()` read with a cumulative-output delta, which is
  receipt-true in both shapes and yields honest ABSENCE for splitless:

  ```rust
  // read: (f.llm_inflight_since, f.stats.llm_calls, f.stats.output_tokens)
  let prev_out: Rc<Cell<u64>> = Rc::new(Cell::new(0));
  // on the Some→None transition with calls > prev_calls:
  let delta = out_now.saturating_sub(prev_out.get());
  if secs > 0.05 && delta > 0 {
      store.last_call_rate.set(Some(delta as f64 / secs));
  }
  // unconditionally each run of the effect:
  prev_out.set(out_now);
  ```

  (`stats.output_tokens` accumulates only genuine output; splitless
  receipts add 0 there, so `delta == 0` → no update → the segment
  renders elapsed-only. Session switches already clear the signal.)
  Also fix the `store.rs` doc claim if the numerator source changes.
- Store-side doc corrected in this review (see fixes): the field's
  comment no longer claims the numerator is guaranteed output-only;
  it names the splitless caveat until the handoff lands.

**P1-B — strip fabricates zero token measurements (both branches) and
the idle summary renders unclipped. CONFIRMED — fixed (owned).**

- `src/ui/chrome.rs:448-462` (pre-fix): the idle `session: N runs · …`
  line is the ONLY strip branch printed without `truncate_ellipsis` —
  engine `canvas.print` clips at CANVAS bounds (verified in
  `abstracttui-0.2.1/src/ui/canvas.rs:129-131`), so at 60–80 cols with a
  queue suffix the row hard-cuts mid-word at the screen edge while every
  sibling branch ellipsizes (POLISH-1's own rule).
- Same branch: `tokens_part` rendered `0 in / 0 out tk` when
  `runs > 0` with all-zero totals (a run that died before any usage
  receipt). Zeros claimed where nothing was measured — the bug (e)
  class. Fixed: all-zero totals omit the tokens part entirely
  (render-when-known), and the line ellipsizes like its siblings.
- RUN half (caught by the live pty capture, frame-02): the run branch
  showed `⠄ thinking (cycle 1) · model call 0s · 5s · 0↑ 0↓ tk` —
  a zero split claimed BEFORE the first usage receipt existed. Fixed
  with the same rule: no token part until any of input/output/total is
  measured; the split appears with the first receipt (test-pinned:
  `run_strip_omits_the_token_split_before_the_first_receipt`).

**P1-I — live gateway admin token at rest in a to-be-committed fixture.
CONFIRMED — fixed (owned).**

- `tests/fixtures/coder_run_tree.json` (UNTRACKED — new from this wave,
  a live-captured run tree) carried the operator's real bearer token
  30 times inside captured `execute_command`/tool-result text. The
  standing rule is "credentials must die with the test" — committing
  this fixture would engrave a live credential into history. Scrubbed
  to `agw_REDACTED_FIXTURE_TOKEN` (replay tests green on the scrubbed
  fixture). NOTE for the operator: the same token also circulates in
  task briefs/terminal history — rotating it after this cycle is the
  clean close.

### P2

**P2-C — wordmark renders twice in the gateway-Down boot state.
CONFIRMED — fixed (owned).**

- IDLE-1's contract is "wordmark exactly once"; the normal branch was
  deduped, but the `Conn::Down` recovery branch
  (`src/ui/transcript_view.rs:864` pre-fix) still pushed its own
  `▲ AbstractCode` line under a header that always carries it.
  Fixed: the recovery block keeps the error + recovery teaching, drops
  the duplicate wordmark. Regression test pins `count == 1` with conn
  Down at boot.

**P2-D — durable-pause strip line lacks the lane prefix in entity
focus. CONFIRMED — fixed (owned).**

- The pending-wait branch prefixes `agent: ` when an entity conversation
  is focused (`chrome.rs:252-257`); the paused branch has the same
  "owns the strip in ANY focus" semantics but carried no prefix — while
  visiting an entity, `⏸ run paused durably on the gateway` read as the
  VISIT being paused (entity turns are non-interruptible and never
  pause; the claim was about the agent lane). Fixed: same prefix rule,
  test-pinned.

**P2-E — `route_label` renders a malformed pair when the default route
carries a model with no provider. CONFIRMED — fixed (owned).**

- `chrome.rs:30-31` (pre-fix): `(dp="", dm="qwen")` →
  `gateway defaults ( · qwen)`. The default route comes from the
  gateway's capability answer; a model-only route is representable.
  Fixed: the parenthetical joins the non-empty halves.

**P2-F — session-id tail truncation duplicated with drifted magic
numbers. CONFIRMED — consolidated (owned).**

- Two inline char-safe tail truncations: header (`>18` keep last 15,
  `chrome.rs:146-154`) and idle strip (`>24` keep last 21,
  `chrome.rs:400-408`) — same idiom, two copies, different constants
  (double-wave drift). Consolidated into one `tail_ellipsis()` helper
  (unit-tested, multibyte-safe); both call sites keep their budgets.

**P2-G — prefs `context_window` bypasses the declaration's own range
rule. CONFIRMED — fixed (owned).**

- `/context` and `--max-tokens` validate through
  `parse_token_count` (1..=1e12, `config.rs:615-634`); a hand-edited
  prefs value loaded unvalidated (`config.rs:456`), so
  `"context_window": 1e18` declared a window the command surface
  refuses, rendering `ctx —/1000000000000.0M tk` in the footer. The
  file's own load posture is "malformed fails toward defaults" — an
  out-of-range declaration now reads as unset (0). Test extended.

**P2-H — two live-call/rate authorities; the fold's pair is dead code.
CONFIRMED — documented + handoff (unowned).**
CLOSED cycle-3: the dead pair is deleted (see Handoffs §2).

- `Fold::live_llm_call()` and `Fold::last_call_rate()`
  (`src/transcript.rs:1129-1138`) have ZERO consumers outside their own
  unit tests (grep-verified). Chrome deliberately consumes the
  client-clock twins (`llm_inflight_since` + `store.last_call_rate`) —
  the RIGHT choice for elapsed: `Instant` is monotonic, so the
  attack-surface "negative elapsed from clock skew" case is structurally
  impossible, whereas the record-epoch interface would need
  `SystemTime::now() - started_ms` and CAN go negative against a
  skewed gateway clock.
- Handoff (transcript.rs owner): either delete both accessors + their
  tests, or annotate them as the exec-lane/record-truth interface if a
  consumer is planned. Two rate authorities that can disagree is the
  double-wave drift this review was hunting; today one of them is
  unreachable, so no user-facing lie — but the "frozen interface —
  Lane B's chrome renders this" comment on `live_llm_call()` is FALSE
  (chrome renders the client-clock twin) and must be corrected either way.

### P2 verdicts on the attack-surface questions (no defect)

- **ctx meter USED signal**: `stats.last_input_tokens` — the newest
  ANSWER-LANE (`telemetry_lane`, `transcript.rs:582-586`) call's input
  tokens, gated `> 0`; delegate-child calls never write it. It is the
  newest measured call, not a stale aggregate. Splitless providers never
  measure it → the declared meter shows `ctx —/window (declared)`
  forever — honest absence, accepted.
- **Session switch resets**: fold replaced (ctx meter → em-dash),
  `totals` zeroed then repaired by rehydrate, `last_call_rate` cleared
  (both `/new` and switch paths, `ui/mod.rs:779-781, 1143-1145`), idle
  card rows are all signal reads (rebind automatically). GPU meter
  deliberately survives (host-scoped, not session-scoped). Queue/goal
  swap per session. Verified coherent.
- **`/context 50` (window smaller than measured use)**: renders the true
  ratio (`ctx 41k/50 tk (82406%, declared)`, severity error) — deliberate:
  the honest number tells the operator their DECLARATION is wrong.
  Pinned by the over-window unit test.
- **`/context 0` vs `off` vs junk**: `0`/`off`/`clear` clear; junk
  refuses loudly naming the accepted forms; all persisted. Covered by
  `context_command_declares_persists_clears_and_refuses`.
- **Idle-card empty gate**: `items.iter().all(Info)` — stricter than
  `is_empty` (boot notices don't hide the card) and a rehydrated
  transcript (non-Info items) suppresses it. Entity focus is never
  empty-state. Verified.
- **Route line pre-resolution**: bare `gateway defaults` until the
  capability route / first served model arrives — the honest
  pre-resolution state (no "(resolving…)" invented, no fabricated pair).
- **Ts-less started record**: elapsed comes from the fold's client-clock
  `llm_inflight_since` (armed on the record's ARRIVAL), so a record
  without a parseable timestamp still ticks from observation time —
  never garbage, never negative. The record-truth `live_call` slot
  (which yields None for ts-less records) is the unconsumed interface
  (P2-H).
- **≥60s hint composition**: one segment
  (`model call 9h20m · N tok/s (last call) — provider may be slow`),
  not two competing sentences — `model_call_segment` is the single
  builder, unit-pinned.

### P3 (documented, no fix)

1. **GPU staleness**: a `Ready` sample renders as current for up to 30s
   while idle (poller cadence ~3s active / ~30s idle). Judged
   acceptable: poll FAILURES surface immediately as `gpu err` (warn
   ink), the cadence is user-taught in `/help` and the `/gpu` toast, and
   the sample carries no timestamp so a staleness cue would require a
   poller-protocol change (gateway/gpu.rs — unowned) for marginal value
   on a utilization dial. NaN is unreachable (JSON numbers can't be NaN;
   `clamp(0,100)` bounds the rest).
2. **Session-switch transient "no runs yet"**: between the switch
   (totals zeroed) and rehydrate landing (1–2s), the idle strip claims
   "no runs yet" for a session with server-side history; permanent under
   `--replay-turns 0` (operator-elected no-replay, where the transcript
   is equally empty so the view is at least self-consistent). Wording
   claims session-scoped knowledge from a view-scoped counter; left as
   is to avoid re-churning wording three agents just settled — flagged
   for the next UX pass.
3. **Header token fact is unqualified**: `128k tk` (header, at rest)
   vs `100k↑ 28k↓ tk session` (footer). The HDR-1 spec named the fact
   "session-tk"; the qualifier costs ~8 columns of a tight middle span.
   Accepted.
4. **"session: 1 runs" grammar** (idle strip). Cosmetic.
5. **Engine Spinner clips its label by CHAR COUNT** (`spinner.rs:110-116`,
   `chars().take(avail)`), not display width — a CJK-heavy goal/activity
   label can paint up to 2× the budget and bleed into the sparkline
   cell. Engine-side (abstracttui 0240-adjacent); agent-lane labels are
   ASCII in practice.
6. **`GpuMeter::Pending` renders `gpu …`** — a deliberate pending glyph,
   not a self-truncation fragment; noted so the footer's no-ellipsis
   invariant tests don't over-assert against it.
7. **Footer ctx meter is app-global during entity focus** — it describes
   the agent lane (the declared window is the agent route's); the
   visit's own spend rides the entity strip (`visit spend N tk`,
   `None`-gated). Judged correct: the footer is the app cockpit, the
   strip is the conversation instrument.
8. **`SessionStats` (fold) vs `SessionTotals` (store)** — near-twin
   structs across the fold/store boundary. Deliberate reactivity mirror
   (the store signal changes only when totals change; reading
   `fold.session` from chrome would wake the header on every record).
   Not dead code; do not "deduplicate" across the boundary.
9. **Route/workflow identity span ellipsizes by design**: at 60 cols the
   header shows `gate…` for the route — the IDENTITY span truncates with
   an ellipsis (a hint that a route exists beats whole-dropping it to
   nothing), while the INSTRUMENT tiers (facts, footer segments, chips)
   drop whole. The width-torture test pins the distinction: a fact
   prefix on screen implies the whole fact on screen.
10. **Session id renders 4× on the idle screen** (header right cluster,
   card `session` row, boot notice, idle strip line — live frame-01).
   Redundant but not dishonest; flagged for the next density pass (the
   card row + strip line are this wave's additions).
11. **`one_line`/`bounded` duplicated across `convo.rs` and
   `transcript.rs`** (byte-identical, both private). The convo copies
   exist because transcript.rs's are private — consolidation needs a
   `pub(crate)` on the transcript.rs side (unowned; see handoffs).

### Theme torture (measured, 26 themes)

WCAG contrast of every chrome ink pair against `surface`, all 26
registry themes (audit test added, `tests/theme_contrast_audit.rs`):

| pair | min | theme at min | floor asserted |
|---|---|---|---|
| text_muted/surface | 4.54 | everforest-light | 3.0 |
| warn/surface | 3.75 | catppuccin-latte | 3.0 |
| error/surface | 3.77 | solarized-light | 3.0 |
| text/surface | 5.40 | everforest-light | 4.5 |
| text_faint/surface | **2.77** | abstract-dark | none (decoration tier) |

Verdict: every information-carrying ink the new chrome uses clears 3:1
on every theme, light extremes included; the session-id faint→muted
promotion is exactly justified (faint bottoms at 2.77 on the DEFAULT
theme). `text_faint` stays separators/hints only — the audit documents
why that rule exists.

### Width torture (60 / 80 / 100 / 120 / 200 / 271)

Headless CaptureTerm tests added (`tests/chrome_width_torture.rs`),
fully-loaded store (skills + mcp + totals + gpu + declared ctx + entity
convo). Pinned at every width: wordmark present, right clusters intact
(header session tail + orb; footer theme + host), no `…` self-truncation
in header facts or footer instruments, the focused entity chip never
yields to facts, ctx meter keeps its slot down to 100 cols, and the
idle-strip summary ellipsizes instead of hard-cutting at 60 cols.

### Live pty proof

`scripts/pty_density_capture.py` (pyte screen, 120×36, frames in
`untracked/cycle2-presence/`). One live run consumed (budget: one).

Idle frame (frame-01, all idle checks PASS, wordmark ×1):

```
▲ AbstractCode  basic-agent  ·  lmstudio · qwen/qwen3.6-35b-a3b  ·  ⌂ acode-density-ws        …sity-1784777087 ●
            describe a task below — the agent runs durably on the gateway
        version     0.4.0 · rendered by AbstractTUI
        workflow    basic-agent
        route       lmstudio · qwen/qwen3.6-35b-a3b
        cwd         /tmp/acode-density-ws
        workspace   server-managed (gateway policy) — /workspace
        session     acode-density-1784777087
        gateway     127.0.0.1:8080 · connected
        skills      none attached — /skills
        mcp         none registered — /mcp
        context     window not declared — /context <tokens> enables the % meter
```

Mid-run strip (frame-02 — the OBS-1a-live ticker from second zero; this
frame is also what surfaced the P1-B run-half zero split, since fixed):

```
⠄ thinking (cycle 1)  ·  model call 0s  ·  5s  ·  0↑ 0↓ tk        ← pre-fix; post-fix the token part waits for the receipt
```

Approval (frame-03: `? write_file · awaiting approval …` + the modal,
answered with `a`), final answer (frame-04) and the post-run footer:

```
ctx 2.1k tk  ·  4.1k↑ 66↓ tk session  ·  ? keys + commands        Dark (Abstract) · 127.0.0.1:8080
```

File proof: the gateway's server-side workspace policy CLAMPED the
client-suggested root — `hi.txt` landed in the gateway's per-run
workspace (`runtime/workspaces/07261a2f…/hi.txt`, mtime after run
start), while `/tmp/acode-density-ws` stayed empty. The script now
checks both roots (structure-gated; file content is model-chosen and
never gated — pty smoke lesson). Idle phase re-verified all-PASS after
the needle fixes (`--idle-only`, no second run spent).

---

## Fixes applied (owned files)

1. **P1-B** `chrome.rs`: idle summary ellipsizes; all-zero token totals
   omit the token part on BOTH strip branches (idle: no fabricated
   `0 in / 0 out`; run: no `0↑ 0↓ tk` before the first receipt). Tests:
   `idle_strip_summary_ellipsizes_at_narrow_widths` (width torture),
   `idle_strip_summary_omits_unmeasured_tokens`,
   `run_strip_omits_the_token_split_before_the_first_receipt`
   (headless).
1b. **P1-I** `tests/fixtures/coder_run_tree.json`: live bearer token
   scrubbed (30 occurrences → `agw_REDACTED_FIXTURE_TOKEN`); replay
   suite green on the scrubbed fixture.
2. **P2-C** `transcript_view.rs`: Down-state wordmark dropped. Test:
   `down_state_card_teaches_recovery_with_one_wordmark`.
3. **P2-D** `chrome.rs`: paused line carries `agent: ` in entity focus.
   Test: `paused_strip_names_the_agent_lane_in_entity_focus`.
4. **P2-E** `chrome.rs::route_label`: parenthetical joins non-empty
   halves (no `( · model)`). Test: `route_label_never_renders_a_dangling_pair`.
5. **P2-F** `chrome.rs`: `tail_ellipsis()` helper replaces both inline
   truncations. Unit test: `tail_ellipsis_keeps_the_tail_multibyte_safe`.
6. **P2-G** `config.rs`: prefs load clamps `context_window` to the
   declaration range (out-of-range → unset). Test extended in
   `prefs_load_tolerates_missing_and_malformed_fields`.
7. **P1-A (doc half)** `store.rs`: `last_call_rate` comment names the
   splitless caveat instead of claiming a guaranteed-output numerator.
8. New: `tests/theme_contrast_audit.rs`, `tests/chrome_width_torture.rs`,
   `scripts/pty_density_capture.py`.

## Consolidation delta

- 2 inline session-id truncations → 1 `tail_ellipsis` helper (P2-F).
- 0 duplicate route-label builders (the old inline header match was
  properly EXTRACTED to `route_label`, shared with the idle card — verified
  against the 34b9447 diff, not a copy).
- 0 duplicate elapsed formatters (`convo::fmt_elapsed` is the single
  authority; the old inline `{}s` and `m/s` formats were deleted in the
  wave — diff-verified).
- 0 dead legend renderers (the old footer key-legend loop was deleted,
  not stranded — diff-verified).
- 1 dead interface PAIR documented for the transcript.rs owner (P2-H).
- `SessionStats`/`SessionTotals` twins: kept deliberately (reactivity
  mirror), documented here so the next wave doesn't "fix" it.

## Handoffs (unowned files)

1. `src/ui/mod.rs::wire_llm_meter` — P1-A numerator fix (exact edit
   above). Until it lands, splitless-usage providers overstate the
   strip's tok/s.
   CLOSED cycle-3: the handed-off edit landed — numerator is the
   cumulative-output delta (splitless receipts add 0 there → honest
   absence, no rate segment); regression-pinned in
   `tests/headless_ui.rs::splitless_receipt_never_mints_a_tok_s_rate`;
   the store.rs caveat comment rewritten to the new semantics.
2. `src/transcript.rs` — P2-H: delete or re-document
   `live_llm_call()`/`last_call_rate()` (the "Lane B's chrome renders
   this" comment is false today).
   CLOSED cycle-3: deleted (accessors, `live_call`/`last_rate` slots,
   their `apply()` writers, and both unit tests — per-lane inflight
   coverage survives in `llm_inflight_is_per_run_and_clears_on_boundaries`).
   The false comment is gone with them. FUTURE UPGRADE note: if a
   gen_time-truth rate (provider-reported timing instead of wall clock)
   is ever wanted, the tested parsers survive in
   `protocol::started_at_epoch_ms`/`gen_time_ms_from_record` — rebuild
   from those; do not resurrect a second live authority beside the
   client-clock one.
3. `src/ui/mod.rs::goal_status` (lines ~996-1005) — a THIRD copy of the
   splitless-tokens fold, drifted: raw numbers without `fmt_tokens`
   (`12000↑ 300↓ tk` where the strip/footer say `12k↑ 300↓`). Adopt
   `chrome::fmt_tokens` + the shared render-when-known rule.
   CLOSED cycle-3: adopted both — `chrome::fmt_tokens` numbers and the
   render-when-known omission (no more `0 tk` before the first receipt).
4. `src/transcript.rs` — make `one_line`/`bounded` `pub(crate)` so
   `convo.rs` can drop its byte-identical private copies (P3-11).
   CLOSED cycle-3: promoted; convo.rs copies deleted; `value_preview`
   also narrowed `pub` → `pub(crate)` (in-crate consumers only —
   `offload_placeholder` stays `pub`, integration tests consume it).
5. (Optional, engine) `abstracttui::widgets::Spinner` label clipping is
   char-count-based, not width-based — CJK labels can bleed 2×; filed
   here for the 0240-adjacent list.

## Gates (final)

- `cargo build --release` clean.
- `cargo test --release`: **356 passed / 0 failed** (baseline 341; the
  delta includes this review's 10 new tests and a concurrent lane's
  additions landing in the same window).
- `cargo clippy --release --all-targets`: zero warnings.
- `cargo fmt --check`: clean repo-wide.
- Live pty: idle phase all-PASS; run phase evidence captured (one run).

## Verdict

The chrome now tells the truth at every width/theme/state tortured
here, with ONE remaining known lie documented and handed off: the
tok/s figure on splitless-usage providers (P1-A, numerator contamination
in unowned `wire_llm_meter`; label and split-provider path are honest).
[CLOSED cycle-3: that last lie is fixed and regression-pinned —
Handoffs §1.]
Everything else found — fabricated zero splits on both strip branches,
the double wordmark in the Down state, the unlabeled paused-lane line in
entity focus, the dangling route pair, the unclipped idle summary, the
out-of-range prefs window, and a live credential in a fixture — is fixed
and regression-pinned in owned files. Width degrade rules (whole-item
drop, chips-over-facts, right-cluster preservation) and theme contrast
floors are now test-enforced rather than asserted in comments.
