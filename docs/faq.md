# FAQ

**How is this different from the Python `abstractcode`?**
The Python CLI runs the agent loop in-process (AbstractAgent + AbstractRuntime
+ AbstractCore) and uses the gateway as its control plane. This client is a
pure thin client: the agent executes ON the gateway, and the TUI renders the
durable run ledger. Same workflows, same approvals, same steering — different
execution home. If you want local tools and local execution, use the Python
CLI; if the gateway is your execution home (shared runs, durable transcript,
attach from anywhere), use this.

**Do I lose my run if the terminal dies?**
No. Runs are durable gateway objects. Relaunch with the same session and you
come back to the state you left: prior turns replay IN FULL DETAIL from
their run ledgers (prompts, reasoning cycles, tool cards, answers — through
the same fold as live streaming, so the details toggle applies), a live run
reattaches with its original prompt and full activity, and pending
approvals re-surface. Replay depth defaults to the last 20 turns
(`--replay-turns N` raises it, `0` disables) because each turn costs one
history-bundle fetch carrying its complete run-tree ledgers.

**Can I pause a run?**
Yes — `/pause` pauses the whole run tree durably on the gateway (it stops
consuming tokens at the next step boundary and SURVIVES quitting the
client). `/resume` continues it. The activity strip shows the paused state,
including after a restart.

**Where does the agent write files?**
On the gateway host, under its workspace policy. The default posture is
server-managed: client-supplied paths are clamped to the gateway's workspace
root or a managed per-session folder, and the app says so at startup. For
trusted local setups, start the gateway with
`ABSTRACTGATEWAY_ALLOW_CLIENT_WORKSPACE_SCOPE=1` and pass `--workspace`.

**How does conversation memory work?**
Two layers. Live, the client carries the visible conversation (completed
user/answer turns, capped like the server's own defaults) into each new run —
so follow-ups always see prior turns, even while earlier runs are still
finalizing on the gateway. Across restarts, the gateway's server-side session
history (`use_session_history`) seeds prior completed turns into the model.
The visual transcript is per-launch; the model's memory is not.

**What do the token numbers mean?**
Input/output tokens summed from the run's completed LLM calls (the ledger's
usage records). `ctx` is the input tokens of the LATEST call — the live
context size the model actually received. `cache` counts prompt tokens the
provider served from its cache (shown only when the provider reports hits).
The sparkline shows output tokens per reasoning cycle. Idle, the strip shows
session totals across runs.

**Is prompt caching on?**
The gateway enables prompt caching automatically for every run when the
provider supports it — nothing to configure client-side. `/cache` shows the
posture for the effective route (supported + mode), observed cache hits, and
the latest context size. Local providers (LM Studio) often cache without
reporting hit counts; the panel says so instead of inventing zeros.

**Can I turn prompt caching off to compare?**
`exec --no-prompt-cache` opts a headless run out, which is enough to A/B one
gateway against itself. It is `exec`-only, and it reaches the model calls of the
run it starts — so pair it with `--workflow react-agent:react`. Flow-graph
bundles (`coding-agent`, `basic-agent`, `multiagent-coding`, and the default
workflow) run their agent loop in a child run that does not inherit the posture,
so they cannot be A/B'd from the client. Confirm the lane you actually got from
the run ledger, not from the flag. See
[Caching and context](api.md#caching-and-context) for the full scope and what to
expect from the cache.

**Which model serves "gateway defaults"?**
The gateway's configured text route (its Multimodal console page). The
header names it as soon as the catalog loads, and switches to the model
that ACTUALLY served once a run reports one — bundle pins or server config
can influence resolution, so the served model is the final truth.

**How do I limit which tools the agent gets?**
`/tools`, `Space` to toggle. Untouched, the workflow's own tool defaults
apply. Once you customize, the checked set is exactly the allowlist sent
with each run. `/skills` attaches gateway skills the same way; `/mcp` shows
MCP servers declared on the gateway (their tools join the inventory).

**Why did my turn finish while the run still shows as waiting on the
gateway?**
Agent bundles can keep helper subflows (status watchers) polling after the
agent produced its answer. The answer is the turn's finish line — the client
releases the composer the moment it lands and lets the root run finalize
server-side.

**Can I run several tasks in parallel?**
One active run per app instance. Launch a second instance with a different
`--session` for a parallel conversation — both stream independently from the
same gateway.

**Does steering interrupt the model?**
No. Guidance lands in the runtime's durable steer sidecar and folds into the
agent's next reasoning cycle; the in-flight LLM call is never cut.

**Which terminals work?**
Anything VT100-descended. AbstractTUI detects capabilities and degrades in
the open (truecolor → 256 → 16; images → unicode mosaic). Check what your
terminal offers with `abstractcode-tui --caps`.

**Where are my settings?**
Connection: `~/.abstractcode/gateway.json` (shared with the Python CLI).
Preferences (theme, workflow, route, session, tool/skill selections, recent
sessions): `~/.abstractcode-tui/prefs.json`.

**Why did my queued prompt not start?**
The queue only advances after the current run **succeeds** — a failure or a
cancel pauses it (running the follow-up anyway would build on a turn that
never finished). It also always restores **paused** after a relaunch or a
session switch: a restore never auto-starts work. Open `/queue` and press
`r` to resume; the strip shows `N queued (paused — /queue resumes)`
whenever prompts are waiting on you.

**How do I stop the constant approval prompts?**
Set the permissions level: `/permissions write` auto-approves proven
read-only tools plus workspace file writes (the runtime clamps those to the
run's workspace); `/permissions all` auto-approves everything. Be clear-eyed
about `all`: it auto-approves **arbitrary shell commands and network
egress**, and it is sticky per session (and seeds new ones) — use it on
gateways whose workspace and tools you trust, not as a reflex. Finer
control: in `/tools`, `p` pins one tool `auto` (always approves, even above
the level) or `ask` (always prompts, even below it — pins gate even at
`all`). The approval modal's `A` sets `all` for the session.

**Why deny by default in `exec`?**
Unattended runs should not mutate a machine because nobody was there to say
no. `--permissions all` is the explicit opt-in; denials carry an explanation the
model sees, so it finishes as best it can without the tool.
