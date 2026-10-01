# Automations

An **automation** runs a workflow again and again on a schedule: "check the
memory of this computer every 2 minutes", "summarise the open issues every 8
hours". AbstractGateway runs it, keeps every run as a readable conversation,
and asks for you only when something needs you. You can create, manage and
answer automations from both AbstractCode clients — the terminal (`/automations`,
`/schedule`) and the browser (the **Automations** section of the sidebar) — and
from the Assistant and the Observer: they all show the same automations, from
the same gateway.

This page covers what the two AbstractCode clients do. For the automation
model itself (controller runs, occurrences, triggers, crash safety, the HTTP
API), see the framework guide:
[AbstractFramework: Automations](https://github.com/lpalbou/AbstractFramework/blob/main/docs/automations.md).

## Requirements

A gateway that advertises the Automations API
(`capabilities.contracts.common.automations`), which AbstractGateway 0.6.0 and
later do. On an older gateway both clients say so and offer no automation
controls.

## What every client shows the same way

| Fact | Where it comes from |
|---|---|
| State, as a word then an icon: **Active ▶**, **Paused ⏸**, **Completed ✓**, **Failed ✕**, **Archived ▪** | the automation's `status` |
| **Now:** "Run #7 running", "Run #7 starting", "Run #7 waiting to retry" | only the gateway's `current_occurrence` |
| **Next:** "2026-09-27 07:00 UTC (in 25 min)", "none while paused" | only the gateway's `next_fire_at` |
| The automation's folder | the gateway's `workspace_root`, browsed through the gateway's workspace routes |
| Attention: "2 unseen · 1 waiting for you" | the gateway's attention items and pending waits |

Schedules are fixed UTC intervals (every N minutes, hours or days) or a single
run at a UTC date and time. Controls that do not apply are disabled, with the
reason.

## Creating an automation

Both clients create the same definition (`POST /api/gateway/automations`):
the task (sent as the prompt of every run), when it runs, its context and its
tool approval. The workflow is the one you already selected:

- the **gateway default** agent is sent as `@default` and resolved by the
  gateway when the automation is created;
- a published workflow is sent as its `bundle@version` and entrypoint.

| Choice | Options |
|---|---|
| When (UTC) | every 5 or 30 minutes, every hour, 8 hours, 24 hours or 7 days, every N minutes/hours/days, or once at a date and time. The first run starts at once. |
| Context | **Independent** — each run starts fresh. **Growing** — each run is the next turn of one conversation and sees the previous runs, within the gateway's history window. |
| Tools | **Run without asking** — tools run without asking (you approve them now by creating this automation). **Ask me before each tool call** — every tool call waits for your approval. Questions a workflow asks always wait for you. |
| Email (browser) | **When an email arrives** (a When choice), **Email me the result** and **May send email without asking to** — see [Email automations](#email-automations-browser). |

**Terminal.** `/schedule [task]` opens four steps: the task (default: your
last prompt, or the text after `/schedule`), when, context, tools. Enter on
the last step creates it and opens it.

**Browser.** Select **+** in the **Automations** section of the sidebar. The
dialog names the workflow it runs (the toolbar's workflow); **Advanced** holds
the title, the first run time, "stop after this many runs" and "stop at".

### Email automations (browser)

When your gateway account has a working mailbox (the gateway console's
**My email**, in its Users tab), the browser dialog also offers:

- **When an email arrives** — the automation runs on new mail in your inbox
  instead of on a schedule. Optional filters: from these addresses, from these
  domains, sent to these addresses, subject contains, attachments. It checks for
  new mail once an hour by default (an automation that runs a model), never more
  often than every 60 seconds, and handles at most 100 emails per run by
  default (the rest wait for the next run). Each email is read once; mail that
  arrived before the automation existed, or while it was paused, is skipped.
  Incoming mail is data, never instructions: link-opening tools always ask.
- **Email me the result** — a run that notifies you, or fails for good, is also
  emailed to you.
- **May send email without asking to** — **Only me** (the default) or **Me and
  these addresses**. Mail to anyone else waits for your approval.

Without a connected mailbox these options are off and the dialog says
**"Connect a mailbox first — open My email"**; the link opens the gateway console in a new
tab. The automation's **Edit** form changes the check interval, Email me the
result and the allowed addresses; its **Definition** card lists them. The
terminal client does not create email automations yet (it lists and manages
them like any other).

## Managing an automation

| Control | Terminal key | What happens |
|---|---|---|
| **Active** switch | `Space` (or `p`) | On: the automation runs on its schedule. Off: paused, no scheduled run; switching it on again continues from the next tick. The switch shows why it cannot change once the automation ended, is archived or is a legacy schedule. |
| Run now | `g` | One run at once, instead of waiting for the schedule. The schedule does not move: the next scheduled run keeps its time, and if that time comes while this run is still going, the scheduled run starts right after it. It does not count toward a run limit ("stop after this many runs"). Also works while paused (it stays paused). In a Growing automation, later runs see it in their history. Refused while a run is in progress. |
| Stop current | `x` | Cancels the run in progress. |
| Revise | `e` | Title, interval (schedules) and context; applies from the next run. In the terminal, a field left empty keeps its current value. |
| Archive | `a`, then `a` again | The automation stops and is hidden from the list; its history stays readable (**Show archived**, or `h` in the terminal). Archiving never deletes anything. |

In the browser the same controls are buttons on the automation's page; hovering
one shows what it does (Run now's tooltip adds the next scheduled time). The
terminal shows Run now's effect in one line under the key hints: "g run now:
Run it once now, without waiting for the schedule; the next scheduled run keeps
its time." Every AbstractFramework client uses the same wording and the same
play-in-a-circle icon for Run now.

## Runs, approvals and answers

Open an automation (Enter in `/automations`, or select it in the sidebar) to
read its runs as chat pairs, oldest first: the trigger turn and the answer,
with failures, notifications and artifacts.

Runs that wait for you are listed first, by kind:

- **Approval needed** — the tool calls with their arguments. Terminal: `y`
  approves, `n` denies. Browser: **Approve** / **Deny**.
- **Question for you** — the question and its choices. Terminal: Enter opens
  an answer field. Browser: a choice button or a free-text answer.
- **Waiting for an event** — a JSON payload field.

The answer follows the wait's kind and is sent through the gateway's resume
command. Showing an automation's attention items marks them as seen for your
account.

## Discuss a run

**Discuss** forks the automation at a finished run: a new conversation whose
context is the automation's whole history up to that run. It works in its own
writable workspace; the automation's folder is mounted read-only for the file
tools (shell commands are not sandboxed), and nothing is written back into the
automation.

The fork opens **in place, as an ordinary conversation** of the client you are
using: in the terminal, `d` on a run asks for your first message and switches
the session to the discussion; in the browser, **Discuss** opens it in the
conversation view. Continue it like any chat, approve its tools like any chat,
and find it later with the other conversations — every client shares one pool
of gateway sessions.

## The automation's folder

The folder every run of the automation works in is shown with its full path
on the gateway host. Terminal: `w` opens the file browser on it (the same one
as `/files`: folders, previews, copy path). Browser: the **Automation folder**
pane beside the runs lists and previews its files.

## Related

- [Getting started](getting-started.md) — connecting a client to a gateway.
- [Terminal reference](../tui/docs/api.md) — `/automations`, `/schedule` and
  their keys.
- [Browser client](web.md) — the sidebar and the conversation view.
