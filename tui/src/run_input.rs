//! Build `input_data` for gateway agent runs (`abstractcode.agent.v1`).
//!
//! Session continuity is SERVER-SIDE: `use_session_history: true` asks the
//! gateway to seed `context.messages` from the session's prior completed
//! runs (the durable-sessions contract) — this client never carries the
//! transcript authority.

use serde_json::{json, Value};

#[derive(Debug, Clone, Default)]
pub struct StartOpts {
    pub provider: String,
    pub model: String,
    /// Gating mode for workflows that support it (the multi-agent coder's
    /// `gating_mode` pin, already shipped server-side: "wait" | "auto",
    /// default "wait"). Empty = absent = the workflow's default (gated),
    /// byte-parity for every existing caller. "auto" runs unattended,
    /// skipping the workflow's human-approval pauses — a SEPARATE axis
    /// from tool approval (permission mode still governs every shell
    /// step). Only "auto" is ever sent; "wait" is left absent.
    pub gating_mode: String,
    /// Reasoning effort override (the first-citizen directive, c5710).
    /// Empty = absent = gateway default, byte-parity with provider/model.
    /// Rides `_runtime.thinking` — the wire key that ALREADY exists
    /// end-to-end (gateway StartRunRequest.thinking compiles to it;
    /// agent children inherit it; core consumes generate(thinking=...)).
    /// "reasoning" is the UI word; `thinking` is the wire spelling —
    /// one vocabulary, no third name (contract v1, plan v13).
    pub reasoning: String,
    pub workspace_root: Option<String>,
    pub workspace_mode: Option<String>,
    /// Extra allowlisted root directories (`workspace_allowed_paths`) —
    /// the gateway mounts them in `workspace_or_allowed` mode; server
    /// policy may clamp paths outside operator-controlled roots.
    pub workspace_allowed: Vec<String>,
    pub max_iterations: u32,
    /// The operator named the budget on this invocation (`--max-iterations`),
    /// as opposed to inheriting the client's own default.
    ///
    /// Gates whether `_limits.max_iterations` rides at all — see the long note
    /// at the `_limits` composition below. Only an explicit declaration is
    /// worth suppressing the runtime's complete `_limits` seeding for.
    pub max_iterations_explicit: bool,
    pub system: String,
    /// Project instructions (`AGENTS.md`) to APPEND to the agent's system
    /// prompt — rides `_runtime.system_prompt_extra`. Empty = absent = server
    /// truth, byte-parity for every existing caller.
    ///
    /// Reach, corrected: this lands for NATIVE-LOOP bundles, whose root run
    /// vars are the loop's vars. Flow-graph Agent children do NOT inherit it —
    /// the compiler rebuilds each child `_runtime` and copies a fixed set that
    /// includes `thinking` but not this key (`compiler.py:1347-1396`); a child
    /// gets a system prompt only from its own node pin
    /// (`compiler.py:1417-1421`). An earlier version of this comment claimed
    /// inheritance "with no server-side change" — that was wrong, and it is the
    /// same reach limit `review_mode` has.
    ///
    /// This client used to send NOTHING here while `system` stayed hardcoded
    /// empty, so the gateway agent never read the repo's own conventions.
    pub system_prompt_extra: String,
    /// Verifier-before-conclude (`_runtime.review_mode`). `None` = absent =
    /// server default, which is **OFF**
    /// (`abstractagent/adapters/react_runtime.py:2244-2246` reads the key and
    /// falls back to `False`).
    ///
    /// THE premature-completion fix: before
    /// accepting any tool-call-free response as final, a strict verifier LLM
    /// call re-reads the transcript and can force more tool calls
    /// ("only count actions supported by the tool outputs"). This client sent
    /// the key nowhere, so a gateway run concluded the first time the model
    /// stopped calling tools — exactly "stops iterating too soon and claims
    /// completion too early".
    ///
    /// Reach: `review_mode` is absent from abstractruntime and
    /// abstractgateway entirely (verified 2026-07-30), so it lands for
    /// NATIVE-LOOP bundles — react-agent, codeact-agent, memact-agent —
    /// whose root run vars are the loop's own vars. Flow-graph bundles
    /// (basic-agent, coding-agent, multiagent-coding) need the runtime
    /// compiler to inherit the key the way it already inherits `thinking`;
    /// see the cross-package request in the analysis report.
    pub review_mode: Option<bool>,
    /// Verifier round budget (`_runtime.review_max_rounds`); 0 = absent =
    /// the loop's own default (1). abstractcode uses 3.
    pub review_max_rounds: u32,
    /// The chosen workflow has review nodes at all. False for memact (see the
    /// composition site). Defaults to false so `StartOpts::default()` states
    /// no posture; both real call sites compute it from the workflow.
    pub review_capable: bool,
    /// Prior conversation turns (role, content) carried by the client.
    /// Client-provided messages WIN over the server-side session seed —
    /// needed live because wrapper bundles can leave prior roots
    /// non-completed (helper pollers), starving the seed.
    pub messages: Vec<(String, String)>,
    /// Explicit tool allowlist (`input_data.tools`). `None` = the workflow's
    /// own defaults; `Some(list)` overrides the flow's tools pin — this is
    /// how the `/tools` on/off selection reaches the agent.
    pub tools: Option<Vec<String>>,
    /// Gateway skills attached to this run (`input_data.skills` — resolved
    /// server-side into the agent's skills block, card 0087).
    pub skills: Vec<String>,
    /// `/goal` runs (plan item 3): `(goal_text, max_cycles)` ride
    /// `input_data.goal` / `input_data.max_cycles` — the goal-agent bundle
    /// contract. The prompt still carries the goal text so the ledger's
    /// `input_data.prompt` stays readable for rehydration/user cards.
    pub goal: Option<(String, u32)>,
    /// Server-side per-run tool policy (`input_data._runtime.tool_policy`).
    /// The runtime consumer executes `auto_approve_tools` with NO wait
    /// round-trip and force-asks `require_approval_tools` (facts #1). Both
    /// empty = no `tool_policy` key (server defaults own approval). The
    /// client-side wait auto-approve stays a belt for waits that still
    /// arrive (names outside this inventory snapshot).
    pub tool_policy: crate::tool_policy::RunToolPolicy,
    /// OPERATOR-DECLARED context window in tokens (CTX-0; 0 = absent).
    /// Rides as top-level `_limits.max_tokens` — ADR-0008's canonical
    /// "total context window" key, the same field the runtime's
    /// RuntimeConfig seeds. An honest declaration, not a capability
    /// claim: the gateway/runtime may clamp or ignore it.
    pub context_window: u64,
    /// Uploaded attachment refs (WHOLE objects from the upload route) —
    /// ride as `context.attachments`, the ONE key the agent lane's media
    /// normalization prefers (`extract_media_from_context`) and the only
    /// key abstractflow's live follow-up sends. Filled by the worker at
    /// send time; empty = no key.
    pub attachments: Vec<Value>,
    /// Prompt-cache posture (`_runtime.prompt_cache`). `None` = absent =
    /// server truth (the runtime's own default, ON). `Some(false)` opts the
    /// run out of the prepare/reuse lane so a single gateway can serve an
    /// A/B measurement. Only an explicit posture is ever sent — an untouched
    /// caller keeps byte-parity.
    pub prompt_cache: Option<bool>,
}

pub fn build_input_data(prompt: &str, opts: &StartOpts) -> Value {
    // NO CLIENT ITERATION BUDGET (operator ruling 2026-08-21: "remove the
    // limit from client, only the limit on the runtime should remain").
    //
    // This used to inject 50 on every run that did not ask for a number. The
    // framework's ruled default is 20 (`abstractagent/adapters/
    // generation_params.py`), and since `react_runtime` began seeding
    // `_limits` from the flat key the injected 50 actually landed — so the
    // same prompt got 50 from this TUI and 20 from AbstractObserver, a
    // bridge, or a script. The hard ceiling (100) is the runtime's, enforced
    // once at `Runtime.start()`.
    //
    // What remains is an operator REQUEST, not a client limit: an explicit
    // `--max-iterations` rides `_limits.max_iterations` below, where the
    // runtime clamps it. Absent that, this client says nothing about
    // iterations and the server's default applies to every client alike.
    let mut input = json!({
        "prompt": prompt,
        "context": { "task": prompt },
        "use_session_history": true,
    });
    if opts.max_iterations_explicit && opts.max_iterations > 0 {
        // The legacy flat key, kept ONLY for the explicit case: older engines
        // read it, newer ones seed `_limits` from it (0029 #6), and the
        // `_limits` entry below is authoritative on both.
        input["max_iterations"] = json!(opts.max_iterations);
    }
    if !opts.messages.is_empty() {
        input["context"]["messages"] = Value::Array(
            opts.messages
                .iter()
                .map(|(role, content)| json!({"role": role, "content": content}))
                .collect(),
        );
        // The agent lane reads use_context to fold explicit messages in.
        input["use_context"] = json!(true);
    }
    if !opts.attachments.is_empty() {
        input["context"]["attachments"] = Value::Array(opts.attachments.clone());
    }
    if let Some(tools) = opts.tools.as_ref() {
        input["tools"] = json!(tools);
    }
    if !opts.skills.is_empty() {
        input["skills"] = json!(opts.skills);
    }
    if let Some((goal, max_cycles)) = opts.goal.as_ref() {
        input["goal"] = json!(goal);
        input["max_cycles"] = json!(max_cycles);
    }
    let mut runtime = serde_json::Map::new();
    if !opts.provider.trim().is_empty() {
        input["provider"] = json!(opts.provider.trim());
        runtime.insert("provider".into(), json!(opts.provider.trim()));
    }
    if !opts.model.trim().is_empty() {
        input["model"] = json!(opts.model.trim());
        runtime.insert("model".into(), json!(opts.model.trim()));
    }
    // Reasoning effort (first-citizen directive): `_runtime` only — the
    // documented inheritance lane (Agent children inherit it). NOT
    // mirrored to a top-level key: flows would need a declared
    // `thinking` input pin to read it, and basic-agent has none.
    if !opts.reasoning.trim().is_empty() {
        runtime.insert("thinking".into(), json!(opts.reasoning.trim()));
    }
    // Project instructions (AGENTS.md): APPENDED to whatever system prompt
    // the workflow bakes in, never replacing it — `system_prompt_extra` is
    // additive by contract, so a bundle's own persona and gate wording stay
    // intact while the repo's conventions reach the model. Cross-client
    // parity with abstractcode's composition.
    if !opts.system_prompt_extra.trim().is_empty() {
        runtime.insert(
            "system_prompt_extra".into(),
            json!(opts.system_prompt_extra.trim()),
        );
    }
    // Verifier-before-conclude. Sent only when the operator has a posture
    // (Some) so an untouched caller keeps server truth; the round budget
    // rides only alongside an ENABLED verifier — a budget with review off
    // would be a claim about a loop that never runs.
    // memact has NO review nodes: `MemActAgent` deprecation-warns on these
    // kwargs (`abstractagent/agents/memact.py:88-96`) and the Python client
    // deliberately withholds them for that agent kind
    // for that agent kind. Sending them would be noise the server has
    // to warn about, so the posture is simply not stated.
    if let Some(review) = opts.review_mode.filter(|_| opts.review_capable) {
        runtime.insert("review_mode".into(), json!(review));
        if review && opts.review_max_rounds > 0 {
            runtime.insert("review_max_rounds".into(), json!(opts.review_max_rounds));
        }
    }
    // Gating mode: top-level `input_data.gating_mode` — the coder
    // workflow reads it as a declared start pin (on_flow_start resolves
    // declared pins input-first). Only "auto" is sent; absent = the
    // workflow's default ("wait" = gated), so existing callers are
    // unchanged. Not a `_runtime` key — it is a workflow input, not a
    // runtime routing fact.
    if opts.gating_mode.trim() == "auto" {
        input["gating_mode"] = json!("auto");
    }
    // Server-side tool policy: expand the accepted tier into a name list
    // the runtime consumer honors with no wait round-trip. Composed INTO
    // the same `_runtime` map as provider/model (never clobbering a key a
    // sibling lane may add there — one map, additive inserts).
    if !opts.tool_policy.is_empty() {
        let mut policy = serde_json::Map::new();
        if !opts.tool_policy.auto_approve_tools.is_empty() {
            policy.insert(
                "auto_approve_tools".into(),
                json!(opts.tool_policy.auto_approve_tools),
            );
        }
        if !opts.tool_policy.require_approval_tools.is_empty() {
            policy.insert(
                "require_approval_tools".into(),
                json!(opts.tool_policy.require_approval_tools),
            );
        }
        runtime.insert("tool_policy".into(), Value::Object(policy));
    }
    // Prompt cache: composed into the SAME `_runtime` map. Sent only when
    // the operator stated a posture, so a default run keeps server truth.
    if let Some(pc) = opts.prompt_cache {
        runtime.insert("prompt_cache".into(), json!(pc));
    }
    if !runtime.is_empty() {
        input["_runtime"] = Value::Object(runtime);
    }
    // `_limits`: the runtime's canonical limits namespace, top-level (root
    // run vars; subflows inherit parent `_limits`).
    //
    // STALE AS OF 0029 #6 — read this whole comment as history, not as
    // current behaviour. `abstractagent/adapters/react_runtime.py` now seeds
    // `limits["max_iterations"]` from the flat/legacy key when the caller did
    // not set `_limits` itself, so the flat key DOES land. The live
    // consequence is the opposite of the one this comment was written to
    // prevent: an implicit run from this client now imposes 50 where every
    // other client gets the ruled framework default of 20
    // (`generation_params.py`). Filed as finding 1 of the 2026-08-21 re-audit
    // in `docs/design/thin-client-conformance.md`; not changed here because
    // it alters every run's budget.
    //
    // The iteration budget MUST ride here, not only as the flat
    // `max_iterations` key above. The flat key alone is silently discarded:
    // when `_limits` is absent from the start vars, abstractruntime seeds it
    // with ITS OWN defaults (`core/runtime.py:1163-1165` → `max_iterations:
    // 20`, `core/config.py:51`), and abstractagent then resolves the budget
    // from `_limits` FIRST (`adapters/generation_params.py:94-100`) while
    // treating "the key is present" as proof the CALLER set it
    // (`adapters/react_runtime.py:114-119`) — so the runtime's own default
    // out-votes the operator. Wire-verified 2026-07-30: 31 consecutive
    // gateway runs started with `--max-iterations 50` carried
    // `scratchpad.max_iterations: 50` beside `_limits.max_iterations: 20`,
    // and the loop obeyed 20.
    //
    // The old behaviour also had a perverse tell that pins the diagnosis: a
    // run that ALSO declared a context window got its budget honoured, purely
    // because `_limits` then existed and the runtime skipped its seeding.
    // `max_tokens` = total context window (ADR-0008), sent only when the
    // operator declared one — absence keeps server truth.
    //
    // …WHICH IS ALSO THE TRAP. `_limits` seeding is ALL-OR-NOTHING: sending a
    // partial dict makes the runtime skip its fill entirely
    // (`core/runtime.py:1166`) and `get_limits` never backfills a dict that is
    // already present (`core/vars.py:85-89`). abstractgateway's own seeder
    // documents the rule and complies by merging the runtime's FULL
    // `to_limits_dict()` first (`entity_visits.py:_seed_run_vars`). This
    // client cannot do that: `to_limits_dict` derives `max_tokens` from model
    // capabilities and then clamps `max_input_tokens` arithmetically
    // (`core/config.py:70-135`) — facts a thin client does not hold, and
    // hard-coding the 32768 fallback would CAP a 262k-window model.
    //
    // So an unconditional partial `_limits` would trade a wrong iteration
    // budget for blinded context accounting: no `max_tokens` means
    // `generation_params.py` can never fire the context-overflow warning and
    // the gateway serves no context percentage — reviving exactly the
    // instrument blindness that let a 132k-token flood pass unwarned in
    // `untracked/agent_quality_investigation.md`. Losing the warning is worse
    // than losing the budget.
    //
    // Therefore: send `_limits` ONLY when the operator declared something for
    // it to carry. A default run keeps the runtime's own complete seeding.
    // The real fix belongs server-side — abstractagent should treat a flat
    // `max_iterations` as caller-explicit — and is filed as such in
    // `docs/reports/2026-07-30-abstractcode-parity.md`.
    let mut limits = serde_json::Map::new();
    if opts.max_iterations_explicit && opts.max_iterations > 0 {
        limits.insert("max_iterations".into(), json!(opts.max_iterations));
    }
    if opts.context_window > 0 {
        limits.insert("max_tokens".into(), json!(opts.context_window));
    }
    if !limits.is_empty() {
        input["_limits"] = Value::Object(limits);
    }
    if !opts.system.trim().is_empty() {
        input["system"] = json!(opts.system.trim());
    }
    if let Some(root) = opts.workspace_root.as_deref() {
        if !root.trim().is_empty() {
            input["workspace_root"] = json!(root.trim());
        }
    }
    if let Some(mode) = opts.workspace_mode.as_deref() {
        if !mode.trim().is_empty() {
            input["workspace_access_mode"] = json!(mode.trim());
        }
    }
    // List shape deliberately (the gateway accepts list or newline string
    // and preserves the shape through its sanitize pass).
    let allowed: Vec<&str> = opts
        .workspace_allowed
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    if !allowed.is_empty() {
        input["workspace_allowed_paths"] = json!(allowed);
    }
    input
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_input_keeps_gateway_defaults() {
        let input = build_input_data("hello", &StartOpts::default());
        assert_eq!(input["prompt"], json!("hello"));
        assert_eq!(input["use_session_history"], json!(true));
        // The client no longer states a budget of its own.
        assert!(input.get("max_iterations").is_none());
        assert!(input.get("_limits").is_none());
        assert!(
            input.get("provider").is_none(),
            "empty provider stays absent"
        );
        assert!(input.get("_runtime").is_none());
        // NO `_limits` on a default run. `_limits` seeding is all-or-nothing
        // server-side, so a partial dict suppresses the runtime's COMPLETE
        // fill — including the `max_tokens` the context-overflow warning and
        // the gateway's context meter both need. An undeclared run must leave
        // that seeding intact; only an explicit declaration earns the key.
        assert!(
            input.get("_limits").is_none(),
            "an undeclared run must not suppress the runtime's own _limits seeding"
        );
        assert!(input.get("workspace_root").is_none());
        assert!(input.get("workspace_allowed_paths").is_none());
        assert!(input.get("tools").is_none(), "no override = workflow tools");
        assert!(input.get("skills").is_none(), "no skills key when empty");
    }

    #[test]
    fn tools_and_skills_ride_input_data() {
        let opts = StartOpts {
            tools: Some(vec!["read_file".into(), "write_file".into()]),
            skills: vec!["coredoc".into()],
            ..StartOpts::default()
        };
        let input = build_input_data("go", &opts);
        assert_eq!(input["tools"], json!(["read_file", "write_file"]));
        assert_eq!(input["skills"], json!(["coredoc"]));
        // An explicit EMPTY allowlist is honored (all tools off).
        let none = StartOpts {
            tools: Some(Vec::new()),
            ..StartOpts::default()
        };
        assert_eq!(build_input_data("go", &none)["tools"], json!([]));
    }

    #[test]
    fn provider_model_ride_both_surfaces() {
        let opts = StartOpts {
            provider: "lmstudio".into(),
            model: "qwen3-4b".into(),
            gating_mode: "auto".into(),
            reasoning: "high".into(),
            workspace_root: Some("/tmp/proj".into()),
            workspace_mode: Some("all_except_ignored".into()),
            max_iterations: 20,
            max_iterations_explicit: true,
            ..StartOpts::default()
        };
        let input = build_input_data("do it", &opts);
        assert_eq!(input["provider"], json!("lmstudio"));
        assert_eq!(input["_runtime"]["model"], json!("qwen3-4b"));
        assert_eq!(input["_runtime"]["thinking"], json!("high"));
        // Top-level `thinking` deliberately absent: `_runtime` is the
        // documented inheritance lane; flows without a thinking pin
        // would never read a top-level copy.
        assert!(input.get("thinking").is_none());
        assert_eq!(input["workspace_root"], json!("/tmp/proj"));
        assert_eq!(input["workspace_access_mode"], json!("all_except_ignored"));
        // BOTH surfaces: the flat key for back-compat, `_limits` because
        // that is the one the loop's resolver actually reads first.
        assert_eq!(input["max_iterations"], json!(20));
        assert_eq!(input["_limits"]["max_iterations"], json!(20));
    }

    #[test]
    fn workspace_allowed_paths_ride_as_a_list() {
        let opts = StartOpts {
            workspace_mode: Some("workspace_or_allowed".into()),
            workspace_allowed: vec![
                "/srv/data".into(),
                "  ".into(), // blank entries never reach the wire
                "/opt/shared ".into(),
            ],
            ..StartOpts::default()
        };
        let input = build_input_data("go", &opts);
        assert_eq!(
            input["workspace_allowed_paths"],
            json!(["/srv/data", "/opt/shared"])
        );
        assert_eq!(
            input["workspace_access_mode"],
            json!("workspace_or_allowed")
        );
        // All-blank list = absent key (gateway defaults own the scope).
        let blank = StartOpts {
            workspace_allowed: vec!["   ".into()],
            ..StartOpts::default()
        };
        assert!(build_input_data("go", &blank)
            .get("workspace_allowed_paths")
            .is_none());
    }

    #[test]
    fn goal_params_ride_input_data() {
        let opts = StartOpts {
            goal: Some(("make the suite green".into(), 8)),
            ..StartOpts::default()
        };
        let input = build_input_data("make the suite green", &opts);
        assert_eq!(input["goal"], json!("make the suite green"));
        assert_eq!(input["max_cycles"], json!(8));
        assert_eq!(
            input["prompt"],
            json!("make the suite green"),
            "the prompt mirrors the goal for ledger/rehydrate readability"
        );
        assert_eq!(input["use_session_history"], json!(true));
        // Non-goal runs carry neither key.
        let bare = build_input_data("hello", &StartOpts::default());
        assert!(bare.get("goal").is_none());
        assert!(bare.get("max_cycles").is_none());
    }

    #[test]
    fn declared_context_window_rides_limits() {
        // CTX-0: the operator-declared window rides the runtime's
        // canonical `_limits.max_tokens` (total context window,
        // ADR-0008) at TOP LEVEL — never inside `_runtime` (that map is
        // provider/model/tool_policy's; the `runtime.len()==3` pin in
        // the fully-loaded test guards it).
        let opts = StartOpts {
            context_window: 262_144,
            ..StartOpts::default()
        };
        let input = build_input_data("go", &opts);
        assert_eq!(input["_limits"]["max_tokens"], json!(262_144));
        assert_eq!(
            input["_limits"].as_object().map(|m| m.len()),
            Some(1),
            "ONLY the declared window — the iteration budget was not declared, \
             and fabricating limits the operator never set is what blinds the \
             runtime's own seeding"
        );
    }

    /// The `_limits` gate: an EXPLICIT `--max-iterations` earns the key, the
    /// client's own default never does.
    ///
    /// Both halves matter. Without the key an explicit budget is silently
    /// out-voted by the runtime's 20 (wire-verified: 31 runs asked for 50 and
    /// ran 20). With the key sent unconditionally, every default run loses the
    /// runtime's complete `_limits` fill and with it the context-overflow
    /// warning — trading a wrong budget for a blind instrument.
    #[test]
    fn limits_iteration_budget_rides_only_when_the_operator_declared_it() {
        let implicit = build_input_data(
            "go",
            &StartOpts {
                max_iterations: 50,
                max_iterations_explicit: false,
                ..StartOpts::default()
            },
        );
        assert!(
            implicit.get("_limits").is_none(),
            "the client's own default must not suppress server seeding"
        );
        // The flat key still rides for loops that read it directly.
        assert!(implicit.get("max_iterations").is_none());

        let explicit = build_input_data(
            "go",
            &StartOpts {
                max_iterations: 12,
                max_iterations_explicit: true,
                ..StartOpts::default()
            },
        );
        assert_eq!(explicit["_limits"]["max_iterations"], json!(12));
        assert_eq!(
            explicit["_limits"].as_object().map(|m| m.len()),
            Some(1),
            "just the declared budget"
        );
    }

    #[test]
    fn tool_policy_rides_runtime_composed_with_provider_model() {
        let opts = StartOpts {
            provider: "lmstudio".into(),
            model: "qwen3-4b".into(),
            tool_policy: crate::tool_policy::RunToolPolicy {
                auto_approve_tools: vec!["read_file".into(), "list_files".into()],
                require_approval_tools: vec!["fetch_url".into()],
            },
            ..StartOpts::default()
        };
        let input = build_input_data("go", &opts);
        // Composed into the SAME _runtime map as provider/model, never
        // clobbering them.
        assert_eq!(input["_runtime"]["provider"], json!("lmstudio"));
        assert_eq!(input["_runtime"]["model"], json!("qwen3-4b"));
        assert_eq!(
            input["_runtime"]["tool_policy"]["auto_approve_tools"],
            json!(["read_file", "list_files"])
        );
        assert_eq!(
            input["_runtime"]["tool_policy"]["require_approval_tools"],
            json!(["fetch_url"])
        );
    }

    #[test]
    fn fully_loaded_start_opts_serialize_every_surface_without_clobbering() {
        // Cycle-3 audit (item 3): three lanes write into ONE input_data —
        // provider/model (+ _runtime), the tier lane's tool_policy (into
        // the SAME _runtime map), and the goal lane's goal/max_cycles —
        // beside workspace scope, messages, tools, skills. This test loads
        // EVERY field at once and asserts each surface serialized intact:
        // additive composition, no lane clobbers a sibling's keys.
        let opts = StartOpts {
            provider: "lmstudio".into(),
            model: "qwen3-4b".into(),
            gating_mode: "auto".into(),
            reasoning: "high".into(),
            workspace_root: Some("/tmp/proj".into()),
            workspace_mode: Some("workspace_or_allowed".into()),
            workspace_allowed: vec!["/srv/data".into()],
            max_iterations: 20,
            max_iterations_explicit: true,
            system: "be brief".into(),
            system_prompt_extra: "Project instructions: run cargo fmt.".into(),
            review_mode: Some(true),
            review_capable: true,
            review_max_rounds: 3,
            messages: vec![
                ("user".into(), "hi".into()),
                ("assistant".into(), "yo".into()),
            ],
            tools: Some(vec!["read_file".into(), "write_file".into()]),
            skills: vec!["coredoc".into()],
            goal: Some(("make the suite green".into(), 8)),
            tool_policy: crate::tool_policy::RunToolPolicy {
                auto_approve_tools: vec!["read_file".into()],
                require_approval_tools: vec!["fetch_url".into()],
            },
            context_window: 262_144,
            attachments: vec![json!({"$artifact": "a1", "filename": "report.pdf"})],
            prompt_cache: Some(false),
        };
        let input = build_input_data("make the suite green", &opts);
        // Top-level surfaces.
        assert_eq!(input["prompt"], json!("make the suite green"));
        assert_eq!(input["use_session_history"], json!(true));
        assert_eq!(input["max_iterations"], json!(20));
        assert_eq!(input["system"], json!("be brief"));
        assert_eq!(input["provider"], json!("lmstudio"));
        assert_eq!(input["model"], json!("qwen3-4b"));
        assert_eq!(input["tools"], json!(["read_file", "write_file"]));
        assert_eq!(input["skills"], json!(["coredoc"]));
        assert_eq!(input["goal"], json!("make the suite green"));
        assert_eq!(input["max_cycles"], json!(8));
        // Workspace scope.
        assert_eq!(input["workspace_root"], json!("/tmp/proj"));
        assert_eq!(
            input["workspace_access_mode"],
            json!("workspace_or_allowed")
        );
        assert_eq!(input["workspace_allowed_paths"], json!(["/srv/data"]));
        // Client conversation context + the fold-in flag.
        assert_eq!(input["use_context"], json!(true));
        assert_eq!(
            input["context"]["messages"],
            json!([{"role": "user", "content": "hi"},
                   {"role": "assistant", "content": "yo"}])
        );
        assert_eq!(input["context"]["task"], json!("make the suite green"));
        // Attachments ride context.attachments as WHOLE refs, beside
        // messages/task without clobbering either.
        assert_eq!(
            input["context"]["attachments"],
            json!([{"$artifact": "a1", "filename": "report.pdf"}])
        );
        // The ONE _runtime map: provider/model AND tool_policy coexist.
        let runtime = input["_runtime"]
            .as_object()
            .expect("_runtime is one object");
        assert_eq!(runtime["provider"], json!("lmstudio"));
        assert_eq!(runtime["model"], json!("qwen3-4b"));
        // Reasoning rides the EXISTING wire key: `thinking` (the UI
        // word is "reasoning"; one wire spelling, contract v1).
        assert_eq!(runtime["thinking"], json!("high"));
        // Gating rides TOP-LEVEL input_data (a workflow input pin), not
        // _runtime; only "auto" is sent.
        assert_eq!(input["gating_mode"], json!("auto"));
        assert!(runtime.get("gating_mode").is_none());
        assert_eq!(
            runtime["tool_policy"]["auto_approve_tools"],
            json!(["read_file"])
        );
        assert_eq!(
            runtime["tool_policy"]["require_approval_tools"],
            json!(["fetch_url"])
        );
        // Project instructions APPEND to the workflow's own system prompt.
        assert_eq!(
            runtime["system_prompt_extra"],
            json!("Project instructions: run cargo fmt.")
        );
        // Verifier-before-conclude rides beside them.
        assert_eq!(runtime["review_mode"], json!(true));
        assert_eq!(runtime["review_max_rounds"], json!(3));
        // Prompt-cache posture rides the SAME map (`--no-prompt-cache`).
        assert_eq!(runtime["prompt_cache"], json!(false));
        assert_eq!(
            runtime.len(),
            8,
            "exactly provider + model + thinking + system_prompt_extra + review_mode + review_max_rounds + tool_policy + prompt_cache — a new _runtime writer must extend this test"
        );
        // The declared window rides its own namespace, never _runtime —
        // beside the iteration budget, which must reach the resolver here or
        // the runtime's 20-iteration default wins.
        assert_eq!(input["_limits"]["max_tokens"], json!(262_144));
        assert_eq!(input["_limits"]["max_iterations"], json!(20));
    }

    #[test]
    fn prompt_cache_states_a_posture_only_when_the_operator_declared_one() {
        // Absent by default: every pre-existing caller keeps server truth
        // (the gateway seeds `_runtime.prompt_cache = {"enabled": true}`),
        // so no `_runtime` key is created on its own account.
        let bare = build_input_data("go", &StartOpts::default());
        assert!(bare.get("_runtime").is_none());

        // `--no-prompt-cache` -> Some(false) -> the literal `false` the
        // runtime reads as "derive no key at all"
        // (effect_handlers.py `_maybe_inject_prompt_cache_key`).
        let off = build_input_data(
            "go",
            &StartOpts {
                prompt_cache: Some(false),
                ..Default::default()
            },
        );
        assert_eq!(off["_runtime"]["prompt_cache"], json!(false));
        assert_eq!(
            off["_runtime"].as_object().expect("one map").len(),
            1,
            "the posture is enough to create _runtime and carries nothing else"
        );

        // An explicit ON posture is expressible too and rides the same key.
        let on = build_input_data(
            "go",
            &StartOpts {
                prompt_cache: Some(true),
                provider: "mlx".into(),
                ..Default::default()
            },
        );
        assert_eq!(on["_runtime"]["prompt_cache"], json!(true));
        assert_eq!(on["_runtime"]["provider"], json!("mlx"));
    }

    #[test]
    fn project_context_rides_runtime_and_stays_absent_when_empty() {
        // Absent by default: a repo with no AGENTS.md keeps byte-parity with
        // every pre-existing caller (no _runtime key at all).
        let bare = build_input_data("go", &StartOpts::default());
        assert!(bare.get("_runtime").is_none());

        // Present alone: it is enough to create `_runtime` on its own — the
        // key does not depend on provider/model being set.
        let only_ctx = StartOpts {
            system_prompt_extra: "Project instructions (from AGENTS.md — follow them):\nrule"
                .into(),
            ..StartOpts::default()
        };
        let input = build_input_data("go", &only_ctx);
        assert!(input["_runtime"]["system_prompt_extra"]
            .as_str()
            .expect("rides _runtime")
            .contains("rule"));
        // NOT a top-level key and NOT the `system` pin: `system` REPLACES a
        // workflow's prompt, `system_prompt_extra` APPENDS to it. Sending
        // project conventions as `system` would silently delete the
        // bundle's own persona and gate wording.
        assert!(input.get("system_prompt_extra").is_none());
        assert!(input.get("system").is_none());

        // Whitespace-only is absent, not an empty injected block.
        let blank = StartOpts {
            system_prompt_extra: "   \n ".into(),
            ..StartOpts::default()
        };
        assert!(build_input_data("go", &blank).get("_runtime").is_none());
    }

    #[test]
    fn review_mode_rides_runtime_with_an_explicit_posture_only() {
        // Untouched = absent = server truth (which is review OFF). A caller
        // that never opted in must keep byte-parity.
        let bare = build_input_data("go", &StartOpts::default());
        assert!(bare.get("_runtime").is_none());

        // Enabled with a budget.
        let on = StartOpts {
            review_mode: Some(true),
            review_capable: true,
            review_max_rounds: 3,
            ..StartOpts::default()
        };
        let input = build_input_data("go", &on);
        assert_eq!(input["_runtime"]["review_mode"], json!(true));
        assert_eq!(input["_runtime"]["review_max_rounds"], json!(3));

        // Explicitly DISABLED is a real posture and must ride: it pins the
        // run against a future server-side default flip.
        let off = StartOpts {
            review_mode: Some(false),
            review_capable: true,
            review_max_rounds: 3,
            ..StartOpts::default()
        };
        let input = build_input_data("go", &off);
        assert_eq!(input["_runtime"]["review_mode"], json!(false));
        // No budget alongside a disabled verifier — it would describe a loop
        // that never runs.
        assert!(input["_runtime"].get("review_max_rounds").is_none());

        // A review-INCAPABLE workflow (memact) states no posture at all:
        // `MemActAgent` deprecation-warns on these kwargs and the Python client
        // withholds them for that agent kind. Sending them would make the
        // server warn about a posture it ignores.
        let memact = StartOpts {
            review_mode: Some(true),
            review_capable: false,
            review_max_rounds: 3,
            ..StartOpts::default()
        };
        let input = build_input_data("go", &memact);
        assert!(
            input
                .get("_runtime")
                .and_then(|r| r.get("review_mode"))
                .is_none(),
            "memact must receive no review posture"
        );
        assert!(input
            .get("_runtime")
            .and_then(|r| r.get("review_max_rounds"))
            .is_none());

        // Enabled with no budget leaves the loop's own default (1) alone.
        let no_budget = StartOpts {
            review_mode: Some(true),
            review_capable: true,
            review_max_rounds: 0,
            ..StartOpts::default()
        };
        let input = build_input_data("go", &no_budget);
        assert_eq!(input["_runtime"]["review_mode"], json!(true));
        assert!(input["_runtime"].get("review_max_rounds").is_none());
    }

    #[test]
    fn attachments_key_absent_when_empty() {
        let input = build_input_data("go", &StartOpts::default());
        assert!(input["context"].get("attachments").is_none());
    }

    #[test]
    fn tool_policy_absent_when_empty() {
        // No provider/model + empty policy → no _runtime at all.
        let bare = build_input_data("go", &StartOpts::default());
        assert!(bare.get("_runtime").is_none());
        // A policy with ONLY an auto list omits the require key (and vice
        // versa) — an empty side never rides the wire.
        let auto_only = StartOpts {
            tool_policy: crate::tool_policy::RunToolPolicy {
                auto_approve_tools: vec!["read_file".into()],
                require_approval_tools: Vec::new(),
            },
            ..StartOpts::default()
        };
        let input = build_input_data("go", &auto_only);
        assert_eq!(
            input["_runtime"]["tool_policy"]["auto_approve_tools"],
            json!(["read_file"])
        );
        assert!(input["_runtime"]["tool_policy"]
            .get("require_approval_tools")
            .is_none());
    }
}
