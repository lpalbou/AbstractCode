# Troubleshooting

Symptom → cause → fix. `abstractcode-tui doctor` is the first move for
anything connection-shaped: it prints which source provided the URL/token
and which check failed.

## The screen went (mostly) blank and stays blank

Something outside the app cleared the terminal — Terminal.app's Cmd+K, a
stray `clear`/`printf '\033c'` from another process, an emulator glitch.
The engine repaints only cells it believes changed, so it cannot know the
terminal lost everything. **Press `Ctrl+L`** (or type `/redraw`) — one
full repaint (protocol images re-place too), works even while a modal
is open. Focusing another window and coming back also heals the whole
screen automatically (the engine's redraw-on-focus-gained, on by
default in this app) — that half needs the terminal to report focus
events (DEC 1004; under tmux set `focus-events on`), so `Ctrl+L`
remains the works-everywhere recovery.

## "gateway unreachable" / red orb in the header

The URL points at nothing listening. Is the gateway up
(`abstractgateway serve`)? Is the port right? `doctor` shows the resolved
URL and its source — a stale `ABSTRACTGATEWAY_URL` export overrides the
login store by design.

Wording is evidence-based: "gateway unreachable" means connect-level
failure (refused/DNS — nobody at the address); "gateway timed out" means
the gateway accepted the connection but didn't answer in time (busy —
e.g. in-process inference load — or wedged); "gateway request failed"
means the transfer broke mid-way. The red orb flips on connect-level
evidence immediately, or on a *run* of consecutive timeouts (2 idle
probes / 3 stream-lane failures) — a single slow call never claims the
gateway is gone.

## HTTP 401 on ping

Token rejected. Re-run `abstractcode-tui login`. If the gateway runs with
user auth (`ABSTRACTGATEWAY_USER_AUTH=1`), the token must be a
registry-issued `agw_…` token; a static dev token from a different
configuration will be refused.

## "no agent workflows available"

The catalog has no entrypoint implementing `abstractcode.agent.v1`. Install
the stock `basic-agent` bundle on the gateway (it ships with it), then
`/workflow` to pick it up — the catalog reloads on the next launch or picker
open.

## The run starts, then an error names an LLM provider failure

The gateway could not reach the model (e.g. LM Studio "Model unloaded").
Load a model in your provider, or pick a loaded one with `/model`. The run
retries with backoff server-side; `Esc Esc` cancels it if you would rather
switch models and start over.

## The agent wrote files, but not where I expected

Server-managed workspace policy (the startup notice says so): the gateway
clamps client paths and tools execute in its managed workspace. See the
workspace note in [getting-started.md](getting-started.md#the-five-things-worth-knowing-on-day-one).

## Red "Path escapes workspace_root: '…'" errors in the transcript

These are RUNTIME workspace-policy refusals, not client bugs: every file
tool call is resolved against the run's workspace scope, and a path that
lands outside it is refused with exactly that message (a sibling message,
"Path is outside workspace roots", appears in allowlist mode). The agent
usually recovers by retrying inside the workspace; repeated refusals mean
the task genuinely needs a directory the run was not granted.

To inspect or extend the scope, use `/workspace`:

- **root** — where relative paths anchor (`--workspace <PATH>` at launch;
  defaults to the directory you launched from).
- **access mode** — `workspace_only` (root only), `workspace_or_allowed`
  (root + your allowed paths), `all_except_ignored` (any absolute path —
  the gateway only honors it when it trusts client scope). Default:
  server-managed (nothing sent; the gateway decides).
- **allowed paths** — extra roots sent as `workspace_allowed_paths`;
  they apply in `workspace_or_allowed` mode.

Mode + allowed paths persist in `~/.abstractcode-tui/prefs.json`
(`workspace_mode`, `workspace_allowed`), which headless `exec` reads too
— configure once, applies everywhere.

Honesty note: the GATEWAY enforces the policy server-side. Unless the
operator enabled client scope overrides
(`ABSTRACTGATEWAY_ALLOW_CLIENT_WORKSPACE_SCOPE=1`, or local tool mode),
client-sent paths are clamped to operator-controlled roots — adding a
path in `/workspace` widens what the CLIENT asks for, and the server may
still refuse it.

## Approval modals keep appearing for harmless tools

Tool batches prompt when they classify ABOVE your accepted permissions
level. `/permissions <read|write|all>` sets it (sticky per session):

- `read` (default) — read-only tools (read/list/search/skim/analyze)
  auto-approve; proven read-only `git` commands are approved SERVER-side
  (the runtime's `git_read_only@v1` refiner) and never prompt at all.
- `write` — adds workspace file writes (`write_file`/`edit_file`).
- `all` — everything auto-approves, including arbitrary shell and
  network; nothing is ever asked (per-tool `ask` pins and
  gateway-disabled tools still gate).

Per-tool pins live in prefs.json under `tool_approval.overrides`
(`{"fetch_url": "auto", "read_file": "ask"}`). The approval modal's
second line shows both sides ("permissions: write — this batch needs:
all"), so a prompt always names why it exists. The old `/auto` session
blanket is removed — its spellings open the `/permissions` report.

## An approval modal answered elsewhere is stuck on screen

It clears the moment the ledger shows the run progressed (another client may
have approved). If the connection dropped mid-wait, the polling fallback
picks it up within seconds; the status bar shows the connection error
meanwhile.

## What I type does not appear in the prompt box

Just keep typing: any character returns focus to the composer and is kept,
and so is a paste or a file drop. Focus can sit on the transcript after you
click in the scrollback or press Tab, and the transcript answers only
navigation keys — so if a keystroke seems to vanish, type the next one and
both land. `Tab` still moves focus explicitly, and `/focus <name|agent>`
switches conversations from any state.

## Copy did not reach the clipboard

Copy uses OSC 52 where the terminal advertises it, and falls back to the
host clipboard tool (`pbcopy`, `wl-copy`, `xclip`, `clip.exe`) everywhere
else, so a drag-copy works in macOS Terminal.app, the VS Code and Cursor
terminals, and Warp. If OSC 52 is the only route your setup allows, tmux
needs `set -g set-clipboard on` and some terminals cap the payload size.
Shift-drag (Option-drag on macOS Terminal and iTerm2) bypasses the app and
uses the terminal's native selection.

A selection made while a run streams copies exactly the cells you
highlighted: the transcript holds still for the length of the drag and
returns to the live tail when the region clears.

## Garbled cells / misaligned borders

Your terminal likely renders ambiguous-width characters wide. Use a
terminal/font configured for narrow ambiguous width (the norm), and check
`--caps` for what was detected.

## `exec` hangs then exits 124

The run outlived `--timeout`. The run itself stays durable on the gateway —
the timeout message names the run id; reattach in the TUI with the same
session or inspect it through the gateway's own surfaces.

## Live smoke for a full-stack check

```sh
ACODE_GATEWAY_TOKEN=<token> python3 scripts/pty_live_smoke.py
```

Boots the real binary under a pty against your gateway, drives a prompt →
approval → answer round trip, and verifies a clean exit.
