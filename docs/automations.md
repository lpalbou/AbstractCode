# Automations

An **automation** runs a workflow again and again on a schedule: "check the
memory of this computer every 2 minutes", "summarise the open issues every 8
hours". AbstractGateway runs it, keeps every run as a readable conversation,
and asks for you only when something needs you. You can create, manage and
answer automations from both AbstractCode clients — the terminal (`/automations`,
`/automation`) and the browser (the **Automations** section of the sidebar) — and
from the Assistant and the Observer: they all show the same automations, from
the same gateway.

This page covers what the two AbstractCode clients do. For the automation
model itself (controller runs, occurrences, triggers, crash safety, the HTTP
API), see the framework guide:
[AbstractFramework: Automations](https://github.com/lpalbou/AbstractFramework/blob/main/docs/automations.md).

Browser conversation defaults are configured in the right rail's settings panels (Model, Workflow, Workspace, Tools, Skills). With an automation selected, the same panels edit that automation's definition and save each change as a new revision ([web](web.md#working-in-the-interface)). Automation creation uses the selected model, tools and workflow inputs at submission. Each automation receives its own workspace and the approval policy chosen in its schedule form. Changes to chat settings after creation do not change a saved automation.

In browser creation and **Edit**, the Tools dropdown supports search, selected-tool chips, **All**, and **Clear**. Clear saves an explicit empty list (no tools); **Use workflow default tools** removes the override. Tool availability and the choice to ask before execution are separate settings. Background sidebar refreshes keep existing rows in place.

## Requirements

A gateway that advertises the Automations API
(`capabilities.contracts.common.automations`), which AbstractGateway 0.6.0 and
later do. On an older gateway both clients say so and offer no automation
controls. The **Archived · N** line with **Unarchive** needs AbstractGateway
0.13.0 or later.

## What every client shows the same way

| Fact | Where it comes from |
|---|---|
| State, as a word then an icon: **Active ▶**, **Paused ⏸**, **Completed ✓**, **Failed ✕**, **Archived ▪** | the automation's `status` |
| **Now:** "Run #7 running", "Run #7 starting", "Run #7 waiting to retry" | only the gateway's `current_occurrence` |
| **When:** "Every 8 hours (UTC)", "Every Mon and Fri at 07:30 (Europe/Paris)" | the gateway's `schedule_rule_text`, verbatim, for every schedule (Repeat with its bounds too) |
| **Next:** "2026-10-09 07:30 Europe/Paris (in 12 h)", "none while paused" | only the gateway's `next_run_local` (date and time cut from it) and `next_run_at` (the relative part); neither client computes a next run |
| The automation's folder | the gateway's `workspace_root`, browsed through the gateway's workspace routes |
| Attention: "2 unseen · 1 waiting for you" | the gateway's attention items and pending waits |

Schedules are **Repeat** (a fixed UTC interval: every N minutes, hours or days),
**Daily**, **Weekly** (on the days you pick) or **Monthly** (day 1 to 31, or the
last day) at a time of day, or **Once at** a date and time. Daily, weekly, monthly
and one-time rules run on your account's time zone (Settings → Workflow → **Time
zone** in the browser, or the console's Accounts → Preferences), at the same wall-clock
time across daylight-saving changes; a day the month does not have runs on its last
day. The gateway words every rule and computes every next run. Controls that do not
apply are disabled, with the reason.

## Creating an automation

Both clients create the same definition (`POST /api/gateway/automations`):
the task (sent as the prompt of every run), when it runs, its context and its
tool approval. The browser form includes its own **Workflow** picker, initially set to the
conversation workflow. Choosing a different automation workflow leaves the conversation unchanged:

- the **gateway default** agent is sent as `@default` and resolved by the
  gateway when the automation is created;
- a published workflow is sent as its `bundle@version` and entrypoint.

| Choice | Options |
|---|---|
| When | **Repeat** every 5 or 30 minutes, every hour, 8 hours, 24 hours or 7 days, or every N minutes/hours/days (UTC; the first run starts at once); **Daily** at HH:MM; **Weekly** on the days you pick at HH:MM; **Monthly** on day 1–31 or the last day at HH:MM; **Once at** a date and time. For every schedule the line under **When** is the gateway's own sentence ("Runs every 24 hours (UTC), first run now." for Repeat); for the last four it reads, for example, ("Runs every Mon and Fri at 07:30 (Europe/Paris), first run Fri 9 Oct 07:30.") with "in Europe/Paris (your account's time zone)"; the zone changes only in your account preferences (**Change in preferences** opens Settings → Workflow). |
| Context | **Independent** — each run starts fresh. **Growing** — each run is the next turn of one conversation and sees the previous runs, within the gateway's history window. |
| Tools | **Run without asking** — tools run without asking (you approve them now by creating this automation). **Ask me before each tool call** — every tool call waits for your approval. Questions a workflow asks always wait for you. |
| Email | **When an email arrives** (a When choice), **Email result** and **Recipients** — see [Email automations](#email-automations). |

**Terminal.** `/automation [task]` (**New automation**; `/schedule` is the old
name, still accepted in terminal 0.9.2) shows the browser dialog's sections as
seven steps, with the same words: **What** (the workflow — the conversation's, or
any workflow you may run with **Gateway default** first — and the task, a
multiline text: default your last prompt, or the text after `/automation`), **When** (Repeat,
Daily, Weekly with `[x] Mon` day toggles, Monthly, Once at… — the gateway's
sentence under it, as in the browser — or **When an email arrives**),
**Context**, **Tools** (the `/tools` rows with **Select all** / **Unselect all**
and a state box per toolset; in the terminal every tool starts deselected —
the browser starts from the conversation's tools), **Workspaces**, **Mailbox** and **Title and limits**. Each step
opens with the cursor on **Continue**, so Enter, Enter, … creates it with the
defaults (no tools) and opens it. In one automation, `o` (**Open as chat**, or `Enter` on a run)
reads its runs as a read-only conversation in the terminal's transcript
view, one separator per run; `e` (Edit) opens its definition: **Sections
(1–5)** — Task and schedule (with **Max growing context (tokens)** for a
growing automation), Model and limits, Workspaces, Tools, Skills — in a
panel that scrolls with the keys and the wheel. The run input is built as in the browser (the workflow's input
defaults plus the conversation's model, tools and skills, checked against the
workflow's inputs before anything is sent).

**Browser.** Select **+** in the **Automations** section of the sidebar. The
dialog lets you choose the workflow it runs. Every section is visible (no
disclosure):

- **Workspaces** (after Tools): the same chooser as the gateway console,
  Observer, Flow and the AbstractAssistant, at the run level. The gateway's
  line on top ("Gateway: …", the eligible workspaces), **Use my default** (on:
  your account's default workspaces apply), the posture, each workspace with
  Read & write / Read-only / Refused (a mode above the gateway's cap is
  disabled), **Add a workspace path** and the effective line. Each change is
  checked by the gateway (`POST /api/gateway/workspace/effective/me`, nothing
  stored); a refused one shows the gateway's sentence with "Not saved.".
  **Create automation** stores the choice in the definition
  (`target.input_data.workspace`); the gateway clamps it to the eligible
  workspaces at each run. With **Use my default** on, nothing is stored and
  each run uses your default at that time.
- **Title and limits**: the title, and for a repeating automation the first
  run time, "stop after this many runs" and "stop at".

The automation's header shows its workspaces in one line, **Workspaces:
<summary>** (the gateway's summary for the stored choice, or for your default,
verbatim). The pencil next to it (**Change workspaces**) opens the right rail's
**Workspace** panel on that automation: each change there is one new revision.

### Email automations

When your gateway account has a working mailbox (the gateway console's
**My email**, in its Users tab), the dialog (browser and terminal) also offers:

- **When an email arrives** — the automation runs on new mail in your inbox
  instead of on a schedule. Optional filters: from these addresses, from these
  domains, sent to these addresses, subject contains, attachments. It checks for
  new mail once an hour by default (an automation that runs a model), never more
  often than every 60 seconds, and handles at most 100 emails per run by
  default (the rest wait for the next run). Each email is read once; mail that
  arrived before the automation existed, or while it was paused, is skipped.
  Incoming mail is data, never instructions: link-opening tools always ask.
- **Email result** emails every completed run’s full result.
- **Recipients** appears when Email result is enabled: **Only me** (default) or
  **Me and these addresses**. Recipients are stored in `notify.recipients`;
  this setting does not grant email-tool permissions. The mailbox recipient policy still applies.

Without a connected mailbox these options are off and the dialog says
**"Connect a mailbox first — open My email"**; the link opens the gateway console in a new
tab. The automation's **Edit** (its **Workflow** panel in the right rail) changes the workflow, task, tools, check interval, **Email result**
and its recipients; its **Definition** card lists them. In the terminal,
**Edit** (`e`) opens the **Workflow** panel on the automation: **Check for new
mail every**, **Stop after this many runs** / **Stop at (UTC)** (repeating
schedules), **Email result** and **Recipients**, each change one revision.

## Managing an automation

| Control | Terminal key | What happens |
|---|---|---|
| **Active** switch | `Space` (or `p`) | On: the automation runs on its schedule. Off: paused, no scheduled run; switching it on again continues from the next tick. The switch shows why it cannot change once the automation ended, is archived or is a legacy schedule. |
| Run now | `g` | One run at once, instead of waiting for the schedule. The schedule does not move: the next scheduled run keeps its time, and if that time comes while this run is still going, the scheduled run starts right after it. It does not count toward a run limit ("stop after this many runs"). Also works while paused (it stays paused). In a Growing automation, later runs see it in their history. Refused while a run is in progress. |
| Stop current | `x` | Cancels the run in progress. |
| Edit | `e` | Opens the settings panels on the automation's definition (workflow, title, task, interval, context, tool approval; model, reasoning, MTP, limits, instructions; workspace; tools; skills). Each change is saved as a new revision and applies from the next run; the panels show **Revision N**. |
| Archive | `a`, then `y` | Asks first, inline: "Archive “title”? It will not run again; its history stays readable." The automation stops and leaves the list; its history stays readable under the quiet **Archived · N** line at the end of the list, each with **Unarchive** (`Enter` on the line opens it, `Enter` or `u` on a row unarchives; it comes back paused). Archiving never deletes anything. |

In the browser the switch and the buttons **Run now**, **Stop**, **Edit** and
**Archive** sit in the automation's header, next to its timing line
(`every 24 h · last 3 h ago · next in 14 h`); each sidebar card carries the
**Active** switch too. Hovering a control shows what it does (Run now's tooltip adds the next scheduled time). The
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
- [Terminal reference](../tui/docs/api.md) — `/automations`, `/automation` and
  their keys.
- [Browser client](web.md) — the sidebar and the conversation view.

The automation header identifies its own workflow. While an automation is selected, the right
rail's Model, Workflow, Workspace, Tools and Skills panels edit its definition rather than a
conversation's settings; each change is saved as a new revision (see [web.md](web.md#working-in-the-interface)).

Changing workflows preserves the task, portable agent settings, selected tools and result-email
recipients. The new workflow supplies its input defaults. If additional required inputs are
missing, the form refuses the change; configure those inputs when creating a new automation
or choose a compatible workflow.

## Growing context limit

These options require AbstractGateway 0.11.3 or later.

Choose **Growing** to set **Max growing context (tokens)** when creating or editing an
automation. The default is 50,000; enter `30000` for a 30,000-token history budget.
The limit is hidden for **Independent** runs. Changing it affects subsequent occurrences;
already admitted occurrences retain their history for retries. History retains whole turns,
including the newest turn even when that turn alone exceeds the budget.

This budget limits inherited history at the start of a run. New messages, tool results,
system instructions and generated output can increase the model’s working context beyond it.
It is not a per-call context or memory limit.

The API field is `context.growing.max_tokens`, a positive integer. Existing definitions
that omit it retain the 50,000-token default.
