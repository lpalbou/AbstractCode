# Workflows

A run executes a named **workflow bundle** on the gateway. The client picks
which one; the gateway and [AbstractRuntime](https://github.com/lpalbou/abstractruntime)
own everything about how it runs.

Related:
- API and CLI surface: [`docs/api.md`](api.md)
- Interface events a workflow can drive: [`ui_events.md`](ui_events.md)

## The gateway default

Each gateway has a default agent workflow for the `abstractcode.agent.v1`
interface, set by its operator (gateway setting
`agents.default_workflow`). Out of the box it is the shipped `basic-agent`
bundle. Both clients list it first as **Gateway default → name @version**.

Choosing it is saved as "the gateway default", not as a copy of the workflow
id. The client then starts each run with `flow_id: "@default"` and
`interface: "abstractcode.agent.v1"`, and the gateway resolves the default at
that moment, so a change on the gateway applies to your next turn. The run's
start response names the workflow the gateway actually started, and both
clients show it (for example "running Basic agent @0.0.3 (gateway default)").

When the gateway has no usable default and you have not picked a workflow, the
clients say so and ask you to pick one; they never substitute a workflow of
their own.

## Selecting a workflow

```bash
abstractcode --workflow default
abstractcode --workflow coding-agent:coder
abstractcode --workflow <bundle_id>[@version][:<flow_id>]
```

`--agent` is accepted as an alias. Without either flag the terminal client uses
your saved choice, and otherwise the gateway default. `default` selects the
gateway default explicitly. In headless `exec`, a saved workflow that is no
longer on the gateway stops the run with exit code 2 instead of running a
different agent; `--workflow default` runs the gateway default.

Inside a session, `/workflow` (alias `/agent`) changes the workflow, and your
selection persists to `~/.abstractcode/prefs.json`.

To see what a given gateway actually has installed:

```bash
abstractcode doctor
```

Installing and managing bundles, and setting the default, are gateway
operations, not client ones — see the
[AbstractGateway](https://github.com/lpalbou/abstractgateway) documentation.

### In the browser

The **Workflow** list in the toolbar starts with the gateway default, then the
coding agents published on your gateway. Tick **Show all workflows** to list
every authorized workflow, including shared catalog workflows. The browser is
not restricted to `abstractcode.agent.v1`: ordinary AbstractFlow workflows run
from their registered schema using **Inputs** and **Run workflow**. Generic
workflows receive the configured input object; agent-only model/tool/runtime
settings are not injected. Questions, messages, event waits, and structured
results use the shared workflow chat.

A restored conversation follows how its last run was started: if the gateway
recorded it as started from its default, the next turn follows the gateway
default again; otherwise it keeps its specific workflow. See [web](web.md).

## The `abstractcode.agent.v1` interface

A workflow usable as an AbstractCode agent declares the interface
`abstractcode.agent.v1`. Its boundary pins are:

| Node | Direction | Pins |
|---|---|---|
| On Flow Start | outputs | `provider`, `model`, `prompt` |
| On Flow End | inputs | `response`, `success`, `meta` |

**These pins are declarative, not enforced.** The gateway checks only that a
bundle declares the interface string; nothing validates the pins, so a bundle
that declares `abstractcode.agent.v1` without them is accepted and then fails
at run time. Treat the table as the contract you are expected to honour rather
than one the platform will hold you to.

### What a run receives

A run is given:

| Variable | Sent |
|---|---|
| `vars.prompt` | always — the task text |
| `vars.provider`, `vars.model` | only when explicitly overridden; otherwise the gateway's defaults apply and the keys are absent |
| `vars.tools` | only when a tool allowlist is set for the session |
| `vars.workspace_root` | when a workspace is in play |
| `vars.context.messages` | conversation history |
| `vars.context.attachments` | attachment references, when files were attached |
| `vars._limits` | host limits such as maximum iterations and tokens |
| `vars._runtime` | run directives — reasoning effort, MTP (`speculation`), live reply streaming (`stream`), review mode, tool policy, prompt caching |

Absence is meaningful here: an omitted `provider` means "use server truth", not
"use nothing". A workflow that reads these should treat a missing key as
"unset" rather than substituting its own default.

### What a run returns

On completion the client reads from the run's output:

| Output | Surfaced as |
|---|---|
| `response` | the assistant's answer text |
| `success` | the run's success flag |
| `meta` | metadata attached to the assistant message |
| `scratchpad` | optional working notes, when the workflow emits them |

Because these travel through the run ledger rather than a client-side call, any
client attached to the session sees the same values.

## Interface events

A workflow can drive what the client shows — status lines, messages, and
tool-execution cards — by emitting the events described in
[`ui_events.md`](ui_events.md). These are advisory rendering hints layered over
ledger truth, never a substitute for it.
