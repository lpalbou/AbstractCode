# 1163 — The page reconnects its live streams after the app server restarts, and says so meanwhile

> Package: AbstractCode web (`workspace/stream_gate.ts`, the view/ledger stream transports), AbstractUIC panel-chat (connection state, reconnect budget) for the shared half
> Type: defect
> Status: planned
> Created: 2026-11-08
> Priority: P1 (a tool approval was shown in the tray and never in Code, 2026-11-08 03:47 → 10:44)
> Labels: streams, reconnect, approvals, deploy

## What happened (2026-11-08, the operator's conversation run `c00883d1…`)

- 03:28:51 the Code app server was restarted under the operator's open page (a local deploy of 0.15.15 while a run was
  live; gateway log: three `Unhandled exception on GET /apps/code/api/gateway/runs/c00883d1…/view/stream`,
  `…/ledger/stream`, `…/me/events/stream`: "peer closed connection without sending complete message body").
- 03:47:55 the run's react child parked on `tool_approval:…` for two `execute_command` calls (ledger entry 48,
  `mode: approval_required`). The tray showed "needs you". The Code page kept showing the earlier tool rows as
  "Running" and never showed the approval card.
- 10:44:46 the operator approved from the tray; the run resumed (then failed for an unrelated provider reason, root
  1164).

The operator's reading: "either it was wrong to surface it in the systray, or it was wrong not to surface it in
AbstractCode". The tray was right; the page was wrong.

## Current code reality

`stream_gate.ts` closes streams when the tab is hidden and reopens them from the last cursor; the panel-chat
controller carries a reconnect budget ("so the controller's reconnect budget is never spent", stream_gate.ts:9) and
exposes `connection: "reconnecting" | …` (`conversation_model.ts:146`, `workflow_progress.ts:82-88` "Reconnecting" /
"Disconnected"). What happens when the app server (the proxy upstream) goes away for ~7 s and comes back is not
covered by a test: the operator's page stayed on stale rows for seven hours, so either the budget ran out or the
resumed stream never re-fetched the view.

## Scope

1. Reproduce on the fixture: open a conversation, park it on an approval, restart the Code app server (and separately
   the gateway) under the page; the approval card must appear within seconds of the server's return, with no reload.
2. Reconnect has no final give-up while the tab is visible: exponential backoff capped at 30 s, and every
   reconnection re-fetches the replay view (not only the deltas) so a wait that was raised during the gap is shown.
3. The page says what is happening: the kit's "Reconnecting"/"Disconnected" state is visible in the run status bar
   (not only in the progress model), and the composer explains why Send is disabled meanwhile.
4. Tests: a stream transport that drops for 10 s and returns; the view is re-fetched; the approval card renders;
   removing the re-fetch turns the test red.
5. Process (framework, untracked notes): never deploy an app or restart the gateway under a live run of the
   operator without saying so first; the deploy script refuses when the gateway lists a running or waiting root run
   less than 10 minutes old unless `--force` is given with a reason.

## Related

Root 1140 (one shared browser connection), root 1158 (scalability), the kit event-wait fix of 2026-11-08 (panel-chat 0.9.7).

## Status 2026-11-08 14:10

Done and live (Code 0.15.18/0.15.19, kit 0.10.16/0.10.17 local): no give-up, re-read on reconnection, run state stays
the ledger's in the status bar, one connection banner with Retry now / Stop retrying, the deploy script refuses under a
live run. Still open, next release: at 400 px the run status bar cuts its last actions ("Conclude", "View activity");
a kit/Code layout fix, about an hour.
