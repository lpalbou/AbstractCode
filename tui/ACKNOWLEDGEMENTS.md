# Acknowledgements

abstractcode-tui stands on a small, deliberate dependency set and two sibling
projects.

## Direct dependencies

| Crate | License | Role |
| --- | --- | --- |
| [abstracttui](https://crates.io/crates/abstracttui) | MIT | The rendering engine: reactive signals, layered compositor, widgets, themes, images, the headless test harness. |
| [ureq](https://crates.io/crates/ureq) | MIT/Apache-2.0 | Blocking HTTP client for the gateway API and SSE ledger streaming (rustls TLS for remote gateways). |
| [serde_json](https://crates.io/crates/serde_json) | MIT/Apache-2.0 | JSON parsing/serialization — gateway payloads carry arbitrary user and model text, so a battle-tested parser is a correctness choice. |

## Sibling projects

- [AbstractGateway](https://github.com/lpalbou/abstractgateway) — the control
  plane this client speaks to: durable runs, workflow catalog, tool
  execution, session history.
- [abstractcode](https://github.com/lpalbou/abstractcode) (Python) — the
  original AbstractCode TUI whose gateway thin-client contracts (web client
  and CLI) define the protocol this port follows.

## Prior art

The transcript/approval/steering interaction model follows the AbstractCode
family; the rendering model (fine-grained reactivity, damage-tracked
compositor) is AbstractTUI's, in the SolidJS tradition.
