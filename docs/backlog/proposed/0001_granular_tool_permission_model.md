# Granular tool permissions: a default stance + exceptions

- Status: **proposed, BLOCKED on the gateway.** The wire cannot express
  three of the four modes today (evidence below).
- Source: operator, 2026-08-28 — *"the idea is: approve all; deny all;
  deny all + whitelist some; approve all + blacklist some. this would
  give a granular way to users to configure what they want."*
- Gateway half: `abstractgateway/docs/backlog/proposed/0234_deny_verb_for_run_tool_policy.md`.

## The model

Four modes the operator named, which are really **two fields**:

| mode | default | exceptions |
| --- | --- | --- |
| approve all | allow | — |
| deny all | deny | — |
| deny all + whitelist | deny | allow these |
| approve all + blacklist | allow | deny these |

Modes 1 and 2 are 3 and 4 with an empty exception set, so the surface is
one default stance plus a named set that inverts it. That is a smaller
and more legible thing than what exists today, which is a three-rung
tier ladder plus a separate per-tool pin map.

## Why it is blocked (verified 2026-08-29)

The run policy this client sends is two lists, and **both are
allow-shaped**:

- `RunToolPolicy { auto_approve_tools, require_approval_tools }` —
  `src/tool_policy.rs:398-401`, built by `expand_run_policy`
  (`src/tool_policy.rs:426`).
- The runtime's rule is *"Any tool name not in `auto_approve_tools`
  requires approval"*
  (`abstractruntime/.../abstractcore/tool_executor.py:950`). So
  `require_approval_tools` means **ask a human**, never **refuse**.
- No deny-shaped key exists anywhere server-side (grepped
  `abstractruntime/src` and `abstractgateway/src` for `deny_tools`,
  `denied_tools`, `blocked_tools`, `tool_denylist` — the only hits are
  an unrelated security name-denylist and `workspace_ignored_paths`).

So today: **approve all** is expressible (`/permissions all`), and *deny
all*, *deny + whitelist*, and *approve + blacklist* are not. The closest
approximation puts everything in `require_approval_tools`, which asks
instead of refusing — an unattended run then stalls on waits rather than
being refused, and the operator's "never run this" never reaches the
run at all.

The operator's own instinct was right: *"unsure it is ready in
gateway."* It is not.

## Current code reality

- `Tier::{Read, Write, All}` (`src/tool_policy.rs`) is the client's
  three-rung ladder, persisted per session as `accepted_tier` and
  mirrored globally.
- Per-tool pins live in `store.tool_overrides: Vec<(String, String)>`
  (`"auto"` | `"ask"`), edited only via `/tools` → `p`.
- `expand_run_policy` flattens tier + pins into the two name lists over
  the CURRENT inventory, with a served-disabled clamp that forces
  gate-disabled rows into `require_approval_tools`.
- The approval modal offers `a` (this batch) and `A` (this batch **and**
  set permissions to `all`, persisted and seeding future sessions,
  disclosed in-code as "hazard 1"). There is no safer middle rung at the
  prompt, so the tired gesture is the most permissive one.

## Scope

- Replace the tier ladder in `/permissions` with the default+exceptions
  model, once the wire can carry it.
- One surface for both halves: today the stance lives in
  `/permissions` and the exceptions live in `/tools` → `p`, which is why
  nobody finds the safe option.
- Carry the operator's stance to the run truthfully — including a deny
  the run enforces without asking.

## Non-goals

- Not per-argument policy ("deny `rm` outside `/tmp`"). Name-level only.
- Not a replacement for the gateway's risk tiers
  (`observe`/`act`/`outreach`/`destroy`, `GET /tool-grants`). Those are
  a different axis and should SEED the exception set, not compete with
  it — picking "deny all + whitelist observe" should be one gesture.
- Not a silent migration: an operator whose session carries
  `accepted_tier: all` must land somewhere explicit, with a notice, not
  be quietly reinterpreted.

## Dependencies

- **Blocking:** the gateway's deny verb
  (`abstractgateway/.../0234_deny_verb_for_run_tool_policy.md`).
  Without it, modes 2–4 can only be enforced client-side, and a
  client-side deny is a lie the moment anything else drives the same
  session — the run would still be allowed to make the call.
- Ideally also: the gateway serving its own default stance so
  `/permissions` shows where the default came from rather than the
  client's own word for it (thin-client conformance open violation #6).

## Expected outcomes

- `/permissions` states one default and a visible exception list.
- The approval prompt gains the missing middle rung — "always allow THIS
  tool" — so escalating to blanket approval stops being the only way to
  stop being asked.
- An unattended run configured "deny all + whitelist" refuses everything
  else without stalling on waits.

## Validation

- Each of the four modes produces a distinct, checkable wire payload,
  and a run under "deny all + whitelist X" executes X and refuses Y
  **without creating an approval wait**.
- A client-side-only build (before the gateway lands) must REFUSE to
  offer the deny modes rather than offering them and enforcing them
  locally — a permission the server does not enforce must not be
  presented as one (ADR 0001: never claim a guarantee that is not
  there).
- Migration: an existing `accepted_tier` maps to a stated default and
  says so once, per session.
- The served-disabled clamp survives: a gate-disabled tool is never
  auto-approved under any mode.

## Backlog hygiene note

This repository's backlog has no `overview.md` and its four existing
items are unnumbered (`ambient-run-animations.md`,
`planned/smoke-ok.md`, …), which the `backlog` skill treats as a defect
— items should carry a global `NNNN_` prefix and an overview should
index counts and priorities. This item adopts the numbered form rather
than the local one; the existing four are flagged here rather than
renamed, because renaming would break inbound links (`ui/animation`'s
module header cites its path directly).
