# AbstractCode documentation

Start at [`getting-started.md`](getting-started.md) if you want to run something
now. Start at [`architecture.md`](architecture.md) if you want to understand the
shape first.

## Project documentation

| Page | What it covers |
|---|---|
| [`getting-started.md`](getting-started.md) | Running a gateway, installing either client, credentials, your first run |
| [`architecture.md`](architecture.md) | The two clients, the gateway, and the thin-client contract that binds them |
| [`api.md`](api.md) | The gateway surface both clients speak, and the integration points |
| [`workflows.md`](workflows.md) | Agent workflow bundles and how a run selects one |
| [`ui_events.md`](ui_events.md) | The workflow-driven interface event contract |
| [`faq.md`](faq.md) | Recurring questions and known limitations |
| [`troubleshooting.md`](troubleshooting.md) | Symptom-oriented diagnosis and fixes |

## The browser client

| Page | What it covers |
|---|---|
| [`web.md`](web.md) | The browser client in depth, including optional voice features |
| [`deployment-web.md`](deployment-web.md) | Hosting it, and the gateway-first deployment model |
| [`deployment-iphone.md`](deployment-iphone.md) | Safari and progressive web app notes |

## The terminal client

The terminal client keeps its reference documentation beside its source:

| Page | What it covers |
|---|---|
| [`../tui/README.md`](../tui/README.md) | Features, interface tour, keys, themes |
| [`../tui/docs/getting-started.md`](../tui/docs/getting-started.md) | Terminal-specific setup |
| [`../tui/docs/api.md`](../tui/docs/api.md) | Command-line surface and library entry points |
| [`../tui/docs/architecture.md`](../tui/docs/architecture.md) | How the client is built on AbstractTUI |
| [`../tui/docs/troubleshooting.md`](../tui/docs/troubleshooting.md) | Terminal, rendering, and connection problems |
| [`../tui/docs/faq.md`](../tui/docs/faq.md) | Terminal client questions |

## Machine-readable indexes

[`../llms.txt`](../llms.txt) is a concise index of this corpus, and
[`../llms-full.txt`](../llms-full.txt) is the expanded aggregate, both for
language models and tooling.
