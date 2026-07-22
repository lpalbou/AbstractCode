# Troubleshooting

Symptom → cause → fix. `abstractcode-tui doctor` is the first move for
anything connection-shaped: it prints which source provided the URL/token
and which check failed.

## "gateway unreachable" / red orb in the header

The URL points at nothing listening. Is the gateway up
(`abstractgateway serve`)? Is the port right? `doctor` shows the resolved
URL and its source — a stale `ABSTRACTGATEWAY_URL` export overrides the
login store by design.

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

## An approval modal answered elsewhere is stuck on screen

It clears the moment the ledger shows the run progressed (another client may
have approved). If the connection dropped mid-wait, the polling fallback
picks it up within seconds; the status bar shows the connection error
meanwhile.

## Keys go nowhere after switching themes rapidly

The composer's autofocus re-fires on every theme rebuild (engine 0.2.0);
if a broken state persists, press Tab to refocus the composer.

## Enter or `c` suddenly copies instead of typing

You drag-selected text and the selection region is still active (it stays
visible after the release-copy so `c`/Enter can re-copy). Press Esc or
click once to clear it; any other typed key clears it too. Engine-side
one-shot-copy fix is tracked upstream (AbstractTUI backlog 0290).

## Copy did not reach the clipboard

In-app copy uses OSC 52, which some terminals gate behind a setting
(tmux needs `set -g set-clipboard on`; some terminals cap the payload).
Shift-drag (Option-drag on macOS Terminal/iTerm2) bypasses the app and
uses the terminal's native selection — see AbstractTUI's
`docs/troubleshooting.md` for the full modifier matrix.

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
