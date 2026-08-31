# Security Policy

## Reporting

Report vulnerabilities privately to contact@abstractframework.ai. Do not open
public issues for security reports. You will receive an acknowledgement and a
remediation plan; coordinated disclosure is appreciated.

## Scope

abstractcode-tui is a network client for an AbstractGateway you control.
Security-relevant surfaces:

- **Credentials at rest**: the login store (`~/.abstractcode/gateway.json`,
  shared with the Python CLI) holds the gateway bearer token and is written
  with mode 0600 on unix. Preferences (`~/.abstractcode-tui/prefs.json`)
  hold no secrets.
- **Credentials in transit**: the token rides the `Authorization: Bearer`
  header. Use HTTPS gateway URLs for non-loopback deployments; TLS is
  provided by rustls through ureq.
- **Untrusted render input**: everything streamed from a run ledger — model
  text, tool arguments and outputs, markdown, image artifact bytes — is
  treated as untrusted display data. Rendering is bounded (previews are
  truncated with labels) and images decode through AbstractTUI's hardened
  PNG/JPEG decoders. A panic, unbounded allocation, or hang caused by
  crafted ledger content or artifact bytes is a vulnerability-class bug —
  report it.
- **What this client never does**: it does not execute tools locally, does
  not eval model output, and does not write files on the model's behalf —
  tool execution happens on the gateway under its approval and workspace
  policies.

## Hardening notes

- Tokens are never printed by the TUI, `doctor`, or `exec`; error messages
  carry HTTP status + body detail only.
- The gateway's workspace policy (server-managed by default) clamps
  client-provided workspace paths; this client surfaces that posture at
  startup instead of implying local-path writes.
