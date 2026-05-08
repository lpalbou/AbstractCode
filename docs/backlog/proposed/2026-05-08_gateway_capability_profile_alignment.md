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

## Non-Goals

- Do not add Gateway hardware dependencies to `abstractcode` base installs.
- Do not duplicate Gateway provider/model/default resolver logic.
- Do not make the web client call Core, Runtime, Vision, Voice, Music, or Memory directly.

## Promotion Criteria

Promote when AbstractCode updates its Gateway/web client against Gateway 0.2.4+ capability
contracts or adds generated-media/memory/tool-policy controls.

## Validation Ideas

- Gateway client tests with capability fixtures for lightweight, Apple, and GPU profiles.
- Web/TUI tests proving unavailable Gateway features are hidden or disabled.
- Manual smoke: connect web mode to a lightweight Gateway and verify no local-engine assumptions.

## Guidance For Implementing Agents

Treat Gateway as the remote execution boundary. If local direct-Core mode remains, keep its settings
separate from Gateway mode and label fallbacks explicitly.
