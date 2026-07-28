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
    pub system: String,
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
}

pub fn build_input_data(prompt: &str, opts: &StartOpts) -> Value {
    let mut input = json!({
        "prompt": prompt,
        "context": { "task": prompt },
        "use_session_history": true,
        "max_iterations": if opts.max_iterations == 0 { 50 } else { opts.max_iterations },
    });
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
    if !runtime.is_empty() {
        input["_runtime"] = Value::Object(runtime);
    }
    // Declared context window (CTX-0): the runtime's canonical `_limits`
    // namespace, top-level (root run vars; subflows inherit parent
    // `_limits`). `max_tokens` = total context window (ADR-0008). Sent
    // only when the operator declared one — absence keeps server truth.
    if opts.context_window > 0 {
        input["_limits"] = json!({ "max_tokens": opts.context_window });
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
        assert_eq!(input["max_iterations"], json!(50));
        assert!(
            input.get("provider").is_none(),
            "empty provider stays absent"
        );
        assert!(input.get("_runtime").is_none());
        assert!(
            input.get("_limits").is_none(),
            "no declared window = no _limits key (server truth owns limits)"
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
        assert_eq!(input["max_iterations"], json!(20));
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
        assert!(
            input["_limits"].as_object().map(|m| m.len()) == Some(1),
            "exactly the window declaration — no fabricated sibling limits"
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
            system: "be brief".into(),
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
        assert_eq!(
            runtime.len(),
            4,
            "exactly provider + model + thinking + tool_policy — a new _runtime writer must extend this test"
        );
        // The declared window rides its own namespace, never _runtime.
        assert_eq!(input["_limits"]["max_tokens"], json!(262_144));
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
