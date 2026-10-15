# 0004 — A session's tool choices live on the gateway, the same from every client

- **Status:** proposed (decision deferred by the operator 2026-10-10: "i do not want to take the decision now")
- **Created:** 2026-10-10
- **Area:** AbstractGateway (a per-session record + two routes; possibly run start) + TUI (`tui/src/ui/mod.rs`,
  `tui/src/config.rs`) + web (`web/src/workspace/preferences.ts`, `web/src/workspace/app.tsx`)
- **Work item:** `abstractcode-0004`; the gateway half needs its own item in `abstractgateway/docs/backlog/` once decided
- **Decision owner:** the operator. No gateway change may start before an explicit go (operator, 2026-10-10: "i do not
  authorize you to touch the gateway before explaining EXACTLY what you want to do").

## Summary
Which tools a session may use, and which of them run without asking, belong to the SESSION. Today they are stored by
each client on its own machine, so the same session can have different tools depending on which client opens it. The
operator's rule (2026-10-10): "the tools available and authorized for a session are FOR A SESSION; independently of how
i connect to that session, from which client, that OBVIOUSLY should not change". The gateway has no place to keep this
today. This item records the situation, the options with their pros and cons, and the questions to decide.

## Why this matters
- **Thin-client doctrine.** AbstractCode is a thin client of the gateway (operator, 2026-10-10): session information is
  never stored locally; only CLIENT SETTINGS (theme, model, voice…) stay per client and must NOT go to the gateway,
  because one person may use several clients on several machines. A session's tool choices are session information.
- **Safety, not just convenience.** These choices decide what an agent may do unattended. A tool disabled in the TUI is
  enabled again when the same session continues from the web or another machine; a tool pinned to "ask" in one client
  may run without asking from another. The person believes a restriction holds that does not.
- **Same round, same rule.** On 2026-10-10 the other local session state was removed from the TUI: the recent-session
  list (the gateway lists sessions), queued prompts (memory only, for the app's lifetime) and `/goal` (derived from the
  session's live goal run on the gateway). Tool choices are the last session state the TUI still keeps at rest; the
  conformance audit lists them as an OPEN violation (`tui/docs/design/thin-client-conformance.md`).

## Current code reality (2026-10-10)

### TUI
- `tui/src/config.rs:386` `SessionToolPrefs {disabled_tools, tool_overrides, accepted_tier}`, kept per session id in
  `Prefs::session_tool_prefs` (`config.rs:462`), newest first, capped at `SESSION_SLOTS_MAX = 15` (`config.rs:378`), in
  `prefs.json` on this machine. A session past the cap silently loses its choices.
- Written by `persist_tool_prefs` (`tui/src/ui/mod.rs:2384`) on every `/tools` change; read by
  `seed_tool_pref_signals` / `load_session_tool_prefs` (`ui/mod.rs:2417`, `:2438`) at boot and on session switch.
- A session with no slot starts from the top-level baseline (`Prefs::disabled_tools`, `tool_overrides`,
  `tool_accepted_tier` — the last setup on this machine), and camera tools start OFF for it.
- Every run start sends the result: `agent_start_opts` (`ui/mod.rs:1018`) puts the allow-list in `input_data.tools` and
  the expanded approval policy (`tool_policy::expand_run_policy`, `tui/src/tool_policy.rs:426`) in
  `_runtime.tool_policy`. So each RUN on the gateway records what it ran with, but nothing records what the SESSION
  chose.

### Code web client
- Keeps tool choices in browser `localStorage` under `abstractcode.workspace.v2:<identity>`
  (`web/src/workspace/preferences.ts:5`): per person, not per session, and per browser. It sends `tools` only when
  customized and its own `tool_policy` (`web/src/workspace/app.tsx:817-822`).
- Consequence today: the TUI and the web client already disagree about the same session's tools.

### Gateway
- No per-session tool record and no route for one.
- An account-wide DEFAULT grant exists (`abstractgateway/src/abstractgateway/tool_grants.py`): at run start it is
  injected as `_runtime.tool_policy` only when the client sent none (`inject_default_grant`, `tool_grants.py:248`).
- The exact precedent for a per-session record exists: a conversation's WORKSPACES
  (`abstractgateway/src/abstractgateway/session_workspaces.py`). Stored per session in the owner's data folder
  (`<plane>/session_workspaces.json`, schema `gateway_session_workspaces_v1`, with a change history), served by
  `GET`/`PUT /api/gateway/sessions/{session_id}/workspaces` (`routes/gateway.py:29571`, `:29580`), owner or admin
  only, usable before the session's first run, audited. Its own docstring states the doctrine: "thin clients hold no
  state: every app that opens the conversation sees the same one". The gateway also APPLIES it at run start (a run's
  own choice wins, then the session's, then the account's).
- `GET /runs/{id}/input_data` returns only a workflow's declared entry inputs in bundle mode, so `_runtime.tool_policy`
  is generally not readable back from a past run.

## Options

### A. Keep it per client (status quo)
- Pros: no gateway change; works offline from the gateway's record; nothing to migrate.
- Cons: breaks the operator's rule; the same session has different tools per client and per machine; a restriction
  can silently not hold; the 15-session cap drops choices without telling anyone; the web client is not even
  per-session.

### B. Rebuild the session's choice from its newest run
Read the newest run's recorded tool list and policy and treat it as the session's choice.
- Pros: no gateway change.
- Cons: a workaround, not a fix. Lossy: the run carries an EXPANDED allow-list and per-tool policy, not the disabled
  set, the pins and the level the person chose; a tool published after that run cannot be told apart from one the
  person enabled. Changes made in `/tools` before the next run are invisible to every other client. The policy is
  usually not even readable back (entry-pin filtering above). Not recommended.

### C. A session tool record on the gateway, stored and served (the workspaces pattern)
A new gateway module storing `<plane>/session_tools.json` (schema `gateway_session_tools_v1`): per session
`{account, disabled_tools, tool_overrides: {name: auto|ask}, accepted_tier, updated_at, updated_by}` plus a change
history; an unreadable file is an error, never an empty answer. Two routes next to the workspace routes, same
owner-or-admin check:

```
GET /api/gateway/sessions/{session_id}/tools
PUT /api/gateway/sessions/{session_id}/tools   {disabled_tools, tool_overrides, accepted_tier} | {configured: false}
```

Run start unchanged: clients keep sending `tools` + `_runtime.tool_policy`, computed from the record.
- Pros: small and contained (one module, two routes, tests); copies a reviewed, shipped pattern; the record exists
  before the first run and changes are visible to every client immediately; history says who changed what; removes
  the TUI's last local session state and its cap.
- Cons: a gateway release is needed before the TUI half ships. Only clients that READ the record honour it: a client
  that ignores it (today the web client, any script) still starts runs with its own tools, so the rule holds only as
  far as clients cooperate. Approval levels and pins must be validated against the gateway's served vocabulary, or
  the record can hold values no client understands.

### D. C, plus the gateway APPLIES the record at run start
When a run in the session is started without a tool list or policy, the gateway fills them from the session record
(the same precedence as workspaces: run choice, then session, then account default grant).
- Pros: the rule holds for EVERY client, including ones that never learned about the record; the gateway becomes the
  single place that decides a session's tools, as it already is for workspaces.
- Cons: touches run start, the most sensitive path; needs the gateway to expand a level + pins into a policy the way
  the TUI does today (`expand_run_policy`), or to forward them for the runtime to expand: a second implementation to
  keep in step, or a move of that logic server-side. A client that DOES send its own list still overrides the session,
  so "the same from every client" also needs a ruling on whether a run may override its session at all.

## Questions for the operator
1. **Which option: C now, D later, or D at once?** Recommended: C first (small, reviewed pattern), D as a separate item
   once C has shipped, because D changes run start.
2. **May a single run override its session's tools** (a client sending its own list), or must every run in a session
   use the session's record?
3. **What does a brand-new session start with?** Today: this machine's last setup in the TUI, the person's browser
   setup in the web client, camera tools off in the TUI. Options: keep that as a CLIENT SETTING (a per-client default
   that seeds the session record on first use), or use the gateway's account default only. The seed is a setting; the
   session's choice after that is session state.
4. **The web client** is per person, not per session. Bring it onto the record in the same item, or a follow-up?

## Scope (once decided, for option C)
Gateway: the module, the two routes, validation against the served tier vocabulary and the tool inventory, audit and
history, tests. TUI: read the record when a session opens; `PUT` on every `/tools` change, showing the error and keeping
the previous state if the write fails; delete `SessionToolPrefs`, `session_tool_prefs` and the slot cap (old
`prefs.json` files lose the key on the next save, as `session_queues`/`session_goals` did); update the conformance
audit. Web: per the answer to question 4.

## Non-goals
- Moving CLIENT SETTINGS (theme, model, voice, the new-session seed if ruled a setting) to the gateway: the operator
  ruled they stay per client.
- Changing the account default grant (`tool_grants.py`).
- Any change before the operator's go.

## Validation
- Gateway: round trip; unreadable file answers 500, never an empty record; set before the first run; another account
  refused; an unknown level or pin value refused.
- TUI: a choice made in one TUI instance is what a second instance shows for the same session (two harnesses, one fake
  gateway); a failed `PUT` leaves the previous state and says so; `prefs.json` holds no `session_tool_prefs` after save;
  deleting the read on session open turns the cross-client test red.
- With D: a run started with no tool list in a configured session carries the session's policy in its run vars.

## Related
- `tui/docs/design/thin-client-conformance.md` (the OPEN entry for per-session tool choices).
- `abstractgateway/src/abstractgateway/session_workspaces.py` (the pattern), `tool_grants.py` (account default grant).
- `abstractgateway/docs/backlog/proposed/0234_deny_verb_for_run_tool_policy.md` (run tool policy vocabulary).
- Same-day rulings: no local recent-session list; queue in memory only; `/goal` from the gateway.
