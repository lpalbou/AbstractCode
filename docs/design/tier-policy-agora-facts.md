# Tier policy — settled agora facts (fold into cycle 2)

Recorded 2026-07-22 from commons 4342 (gateway) + 4343 (runtime), consumed
at 4350. These facts UPGRADE bug (a)'s design mid-build; the cycle-2
reviewer folds them into worker 3's tier work.

1. SERVER-SIDE ACCEPTED TIERS WORK TODAY — no wait round-trip at all.
   The per-run tool policy consumer is LIVE (runtime effect_handlers
   `_execute_with_run_policy`, consulted at both tool_calls execution
   sites since 2026-07-21): a run started with
   `input_data._runtime.tool_policy = {"auto_approve_tools": [names],
   "require_approval_tools": [names]}` executes run-auto names via
   execute_approved with NO wait; run-require names force the ask.
   Malformed policy = static behavior (fails toward asking).
   ⇒ The TUI expands the accepted tier to a tool-NAME list from the
   discovery inventory at start time and sends it in StartOpts. The
   client-side wire_wait_modals auto-approve stays as the belt for waits
   that still arrive (e.g. names outside the inventory snapshot).

2. `approval: auto|ask` PER TOOL is coming to GET /discovery/tools items
   (gateway passthrough of runtime's `default_approval_policy_sets()` —
   zero new runtime code; gateway sequences it right after their boot
   scanner). The TUI's name-based classification table is #FALLBACK and
   dies the day those fields land.

3. `tier` per tool needs a SEMANTICS vocabulary ruling first (the
   entity-lane TOOL_DESCRIPTORS carry capability_class, but the general
   inventory's structural axis today is safe-vs-mutating). When tiers
   ride the inventory, runtime adds `auto_approve_tiers` as an additive
   run-policy key — the client then sends
   `{"auto_approve_tiers": ["tier1_self"]}` instead of a name list.

Client mapping (stable regardless): prefs `accepted_tier` read < write <
all — "read" expands to approval:auto/read-class names; "write" adds
workspace mutations; "all" adds execute_command/network. The
read-only-git PROOF (ported from abstractcode) stays client-side for the
execute_command carve-out until server tiers exist.

> **SUPERSEDED 2026-07-24 (c5057):** the client git proof is retired —
> the decision moved to the runtime approval point as the
> `git_read_only@v1` refiner, declared by core on `execute_command`'s
> inventory row. See CHANGELOG "Removed (the client read-only-git
> proof)".

UPDATE (commons 4352, consumed 4353): runtime SHIPPED the seam same-hour
— `annotate_tool_rows(rows)` stamps `tier` + `approval_default` on both
row families. SEMANTICS PINNED SERVER-SIDE: walled rows carry
capability_class verbatim (tier0_core/tier1_self/tier2_world); ALL
core-registry tools are tier2_world by the ruled boundary definition
(2026-07-06: tier = the boundary crossed; read-only world tools are
tier2) — so "accept the highest tier" = accept tier2_world = nothing
ever asks, with approval_default (auto|ask from
default_approval_policy_sets(); unknown → ask) as the finer dial below
it. Gateway's discovery passthrough is the remaining edit; then
runtime adds `auto_approve_tiers` to the run-policy consumer. Cycle 2:
map the TUI's read<write<all onto (tier, approval_default) once served;
keep the name-list run policy until `auto_approve_tiers` exists.

UPDATE 2 (commons 4356, consumed 4360): gateway's PASSTHROUGH LANDED
in-tree same hour — /discovery/tools items carry `tier` + `approval`
(render-when-present; live sample from their receipt: execute_command
tier2_world/ask, read_file tier2_world/auto, web_search
tier2_world/auto). LIVE :8080 still serves the PRE-BOUNCE build (fields
absent, verified 12:33Z) — the fields appear at the next gateway bounce.
CLIENT CONTRACT: read tier/approval when present (drop the name table
same-day), tolerate absence (#FALLBACK name table until first bounce).
runtime's `auto_approve_tiers` run-policy key is GO per their close —
switch from name-list to tier-list when it ships.
