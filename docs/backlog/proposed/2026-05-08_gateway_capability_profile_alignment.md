# Proposed: Gateway Capability Profile Alignment

## Metadata
- Created: 2026-05-08
- Status: Proposed
- Completed: N/A

## Context

AbstractCode has local CLI/TUI workflows and a Gateway-first web host. The Gateway path should let
coding clients reuse durable runs, prompt-cache control, tools, memory, generated media, and future
workflow bundles without importing the full backend stack in the client.

Gateway install profiles are now explicit:

- `abstractgateway[server]`: lightweight remote/provider deployment.
- `abstractgateway[apple]` / `abstractgateway[gpu]`: full native Python local-engine deployments.
- Docker: lightweight server plus explicit NVIDIA server image only.

## Problem

AbstractCode needs to support local developer ergonomics without becoming another configuration
authority for provider keys, Core defaults, generated media, memory stores, or hardware engine
selection.

## Proposed Direction

Split the responsibility clearly:

- keep local CLI/TUI direct-Core behavior as an explicit local mode;
- make Gateway/web mode a thin client over Gateway discovery and run APIs;
- consume Gateway capability discovery for provider/model catalogs, prompt-cache support, tool
  inventory/policy, workspace policy, generated image/audio/music readiness, memory readiness, and
  host metrics such as GPU utilization;
- document that local hardware backends are installed on the Gateway host with
  `abstractgateway[apple]` or `abstractgateway[gpu]`.

## Detailed Plan

1. Treat Gateway/web mode as the canonical durable path.
   - Keep direct local CLI/Core mode explicit and separate.
   - Use Gateway run APIs, workflow bundle APIs, ledger/history/artifact APIs, and capability
     discovery for web mode and Gateway CLI subcommands.
   - Do not let web mode infer provider/model/tool capability from local AbstractCode settings.

2. Add a capability-aware Gateway client.
   - Parse provider/model catalogs, prompt-cache support, tool inventory/default approval policy,
     workspace policy, memory/KG readiness, generated image/audio/music readiness, and host metrics.
   - Expose a typed capability object for TUI and web clients.
   - Preserve `#FALLBACK` labels when falling back to older Gateway contracts.

3. Upgrade coding workflows to use Gateway features.
   - Use Gateway prompt-cache session endpoints for long coding sessions when advertised.
   - Use Gateway tool inventory and approval policy for safe command/file/tool execution.
   - Attach generated media or user media as Gateway artifacts, then pass artifact refs through runs.
   - Surface KG memory availability for project memory features without adding local Memory deps.

4. Update UI behavior.
   - Web provider/model pickers should come from Gateway.
   - GPU meter should remain Gateway-sourced and optional.
   - Generated artifacts should be rendered in the run transcript with artifact download/open
     affordances.
   - Workspace controls should reflect Gateway workspace policy and mounts.

5. Test split-mode behavior.
   - Add fixtures for lightweight, Apple, GPU, offline, and older Gateway payloads.
   - Verify direct local mode and Gateway mode do not share hidden config defaults.

## Non-Goals

- Do not add Gateway hardware dependencies to `abstractcode` base installs.
- Do not duplicate Gateway provider/model/default resolver logic.
- Do not make the web client call Core, Runtime, Vision, Voice, Music, or Memory directly.

## Promotion Criteria

Promote when AbstractCode updates its Gateway/web client against Gateway 0.2.4+ capability
contracts or adds generated-media/memory/tool-policy controls.

## Expected Outcomes

- AbstractCode web/Gateway mode benefits from Gateway-generated media, memory, prompt-cache, tools,
  and workflow capabilities without local backend installs.
- Local direct-Core mode remains available but clearly separate.
- Browser and TUI behavior adapt to the connected Gateway profile.

## Validation Ideas

- Gateway client tests with capability fixtures for lightweight, Apple, and GPU profiles.
- Web/TUI tests proving unavailable Gateway features are hidden or disabled.
- Manual smoke: connect web mode to a lightweight Gateway and verify no local-engine assumptions.

## Guidance For Implementing Agents

Treat Gateway as the remote execution boundary. If local direct-Core mode remains, keep its settings
separate from Gateway mode and label fallbacks explicitly.
