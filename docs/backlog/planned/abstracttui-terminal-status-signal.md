# abstracttui ask: app-level terminal status signal

- Status: drafted, ready to send to the abstracttui seat
- Source: abstractcode-tui, operator request 2026-08-19 ("show our status as
  an icon in the terminal shell — thinking, question, done")

## What we want

An app-facing door to two terminal status channels, so a long agent run is
legible from the tab strip when the window is not focused:

1. **Window/tab title** — `app::set_terminal_title(&str)`. The title is where
   an emoji status marker actually shows up in every terminal we support
   (`🤔 thinking`, `❓ needs you`, `✅ done`), including in tmux window lists
   and macOS/Windows taskbars.
2. **Progress/attention state** (optional second half) —
   `app::set_progress(ProgressState)` emitting OSC 9;4 (the ConEmu form
   honored by Windows Terminal, WezTerm, ghostty and others):
   `Off | Indeterminate | Value(u8) | Error | Paused`. This is what paints a
   real indicator on the tab itself rather than in its text.

## Why it belongs in the engine

The bytes already exist there, one layer below where an app can reach:
`term::verbs::set_title_bytes` (OSC 0, `sanitize_osc_text` injection defense)
and `Terminal::set_title`, which pushes the title stack (XTWINOPS 22) before
the first set and pops it on leave. That pop is the reason to do this
engine-side: an app writing OSC 0 to stdout itself cannot participate in the
stack, so it leaves its own title behind in the user's shell after exit.

There is no app-facing route today: `set_title` is never called from `app/`,
`RunConfig` has no title field, and `App::run()` owns the `Terminal` for the
life of the loop.

## Precedent to copy

`app::selection::copy_to_clipboard(text)` — queues an OSC 52 payload through
the presenter's byte custody, callable from any component handler, emitted
with the next frame. A title/progress queue is the same shape: a small
pending slot on the driver, coalesced (only the latest title matters),
emitted with the next frame, and reset on leave.

Capability honesty could follow the same rule as OSC 52: terminals that do
not understand the sequence ignore it, and `Capabilities` can advertise
support where it is known.

## How we would use it

| App state | Title | Progress |
| --- | --- | --- |
| idle | `abstractcode-tui — <session>` | `Off` |
| running | `🤔 <workflow> — <elapsed>` | `Indeterminate` |
| approval or ask pending | `❓ needs you — <what>` | `Paused` |
| run concluded | `✅ done — <outcome>` | `Off` |
| run failed | `⚠ failed — <reason>` | `Error` |

The in-app half of this already exists and is honest about each of those
states (`ui::chrome::activity_strip`); the ask is only about mirroring it
where the operator can see it without switching windows.
