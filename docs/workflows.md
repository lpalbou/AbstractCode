# Workflows

A run executes a named **workflow bundle** on the gateway. The client picks
which one; the gateway and [AbstractRuntime](https://github.com/lpalbou/abstractruntime)
own everything about how it runs.

Related:
- API and CLI surface: [`docs/api.md`](api.md)
- Interface events a workflow can drive: [`ui_events.md`](ui_events.md)

## Selecting a workflow

```bash
abstractcode --workflow coding-agent:coder
abstractcode --workflow <bundle_id>[@version][:<flow_id>]
```

`--agent` is accepted as an alias. Without either flag the client uses your
saved choice, falling back to `coding-agent:coder` — the verified coding loop,
which ships with the gateway.

Inside a session, `/agent` changes the workflow, and your selection persists to
`~/.abstractcode/prefs.json`.

To see what a given gateway actually has installed:

```bash
abstractcode doctor
```

Installing and managing bundles is a gateway operation, not a client one — see
the [AbstractGateway](https://github.com/lpalbou/abstractgateway) documentation.

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
| `vars._runtime` | run directives — reasoning effort, review mode, tool policy, prompt caching |

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
