# Ambient run animations (`/animation`) — parked

- Status: **built, not reachable.** The code is in the tree
  (`src/ui/animation/`), it compiles, it is under test, and **no command
  can launch it** — the `/animation` parse arm and its help entry were
  removed on 2026-08-21.
- Verdict (operator, 2026-08-21): *"for the moment, they are mostly
  terrible and uninteresting."*
- Source: operator request 2026-08-21, designed against a creative pass
  and an adversarial pass run the same day.

## What exists

Three ambient variants that replace the transcript pane while a run
works, driven by the run's own events. All three render, all three
degrade to a small pane, all three are pinned by tests:

| variant | what it draws |
| --- | --- |
| `pulse` | the whole run as a strip chart — cycle bars by output tokens, tool ticks on one row per family, a full-height column in error ink wherever a tool failed, the context lane under the declared window. The time axis compresses as the run grows, so minute 90 shows the shape minute 3 did, denser. |
| `desk` | the intern: a diorama where every prop is a real number — the paper stack is the context window, the cups are model calls, the bin (and the misses on the floor) are failed tools, the monitor carries the current tool and file, the typing cadence is the measured tok/s. |
| `drift` | the work's vocabulary as a field of terms, sized by decayed heat, positioned by a hash so they never reshuffle, coloured by origin: the user's brief in house red, the agent's own trail in blue, with the drift percentage printed. |

## What is worth keeping even if the pane never ships

These parts are load-bearing and correct, and they are the expensive half:

- **`animation::Feed`** — an append-fed history keyed on tool KEYS and
  monotonic counters. It survives `transcript::MAX_ITEMS` truncation,
  which any future run-shaped visualisation (a run timeline, a `/status`
  chart, an export summary) will need.
- **`animation::State`** — one honest verdict (working / waiting on the
  model / waiting on you / tools failing / gateway not answering)
  computed from the same signals the activity strip uses, with a
  `motion()` multiplier that goes to zero when the run stops producing.
- **`animation::Family`** — the tool-family classifier (read / write /
  exec / search / net / other) with per-family theme-chart inks.
- **`safe_label` / `safe_word`** — the charset gate that keeps
  run-derived text (tool names, file basenames) from carrying escape
  sequences, RTL overrides or 40 MB blobs onto the screen.
- **The Esc rung** in `ui::handle_escape` — the pane exits BEFORE the
  cancel-arm ladder and clears `last_esc`, so a double-tap to get the
  words back can never kill a long run. This is the rung that makes the
  feature safe to switch back on.

## Why it is parked

The adversarial pass predicted the failure and the operator confirmed it:
an ambient animation has a fixed information content per frame, so its
value decays with exposure while its cost (peripheral motion, CPU, SSH
bytes, professional appearance) stays constant. The three built variants
do not clear the bar the same pass set:

1. **Decision value** — name the decision a user makes differently
   because of this visual. `pulse` half-answers it (a red fence means
   intervene); `desk` and `drift` do not.
2. **Beats the activity strip** — the chrome already reports in-flight
   LLM elapsed, measured tok/s, in-flight tool elapsed, cycle gist and
   failed-tool counts, in one line, without taking the transcript away.
   An animation that replaces the transcript must beat that at something
   it cannot do. Only "the shape of a long run at a glance" qualifies,
   and only `pulse` attempts it.
3. **Aging** — what differs between minute 3 and minute 90? `pulse`
   answers this (accumulated shape); `desk` and `drift` largely do not.

## If it is revisited

The shapes most likely to survive daily use, in order:

1. **The run's cardiogram** (`pulse`, developed further): information
   content grows with the run, a stall is literally a flat line, it reads
   at 6 rows and at 60, redraw is O(1) per tick, and clicking a column
   could jump the transcript to that item — a click that is a query
   rather than a reaction. That last piece is the missing work: the feed
   carries no stable fold index, so click-to-jump was never wired.
2. **A legible state machine with visible dwell** — six states from tool
   outcomes, each with a distinct silhouette, where the current state
   visibly ages. "Stuck in one state for twenty minutes" becomes readable
   without a number.
3. **The workspace as a place** — nodes per touched file, laid out by
   directory, brightening on edit. Answers "how broad has this change
   become?", which is the supervisory question agentic coding actually
   has. Layout stability and name-leakage are the hard parts.

Shapes that were rejected and should stay rejected: a mascot decoupled
from run truth, generative ambience (structurally cannot depict a stall),
word clouds over repository content (the rarest token is the one you
cannot project), and any journey/progress metaphor (the agent does not
know how far along it is, so the metaphor lies).

Hard rules any revival must keep: opt-in per invocation, never
auto-enter; no byte of tool OUTPUT rendered as text; a distinguished
stalled state and failing state; zero frames when nothing changed or the
pane is not visible; the animation never gates or delays approvals,
asks, errors or the composer; and Esc exits before the cancel ladder.

## Re-opening it

One parse arm in `src/commands.rs` and one dispatch arm in
`src/ui/mod.rs`:

```rust
// commands.rs
"/animation" | "/anim" => Command::Animation(if rest.is_empty() { None } else { Some(rest) }),

// ui/mod.rs dispatch
Command::Animation(arg) => { let line = animation::command(store, arg.as_deref()); store.notify(&line); }
```

`animation::command` (variant picking, toggling, refusing junk by name)
is still there and still under test. Preview the variants without a
gateway at any time:

```
cargo run --example animation -- --sheet <dir>
```
