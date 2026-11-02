# 0020 — Shared companion comment ownership and global cooldown

- **Status:** proposed
- **Created:** 2026-10-30
- **Area:** Gateway and thin-client session observers
- **Work item:** abstractcode-0020
- **ADR impact:** required before introducing a shared scheduler/authority contract.

## Summary and code reality

Code's optional companion (0016) uses a client-owned change gate and monotonic
60-second minimum between attempts, including failures. One running TUI has one
in-flight request and rejects stale/duplicate comments. Several clients observing
the same session can independently request comments through the existing Gateway
pool; shared models remain shared, but inference requests are not globally deduped.
Server run creation timestamps include variable queue/creation delay and are not
an exact measure of the client's attempt clock.

## Later scope and alternatives

Decide whether comments belong to the shared session/agent or are personal
client assistance. For shared comments, reuse the Gateway's existing durable
command/idempotency and authenticated session-event paths where appropriate;
first audit their actual guarantees, then specify one request owner, dedup key,
revision identity, server-owned quota and lease/recovery behavior. Publish an
accepted comment once for subscribing clients to replay, with no per-client
re-inference. Prefer an extension of existing ownership over a daemon/service.
Do not move generic avatar drawing or local accessibility/device choices to the
server. Shared appearance/voice identity is separate proposed0018.

## Validation before implementation

Two clients on one session cause one eligible inference and one replayable
comment; unauthorized clients cannot subscribe/request; owner disconnect/restart
cannot strand a lease or reset quota; duplicates, stale revisions and retry races
are idempotent; disabling shared advice is honored. Define migration from local
opt-in state before any persistent write. No shared scheduler or Gateway mutation
is implemented by this proposal.
