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
    pub workspace_root: Option<String>,
    pub workspace_mode: Option<String>,
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
    if let Some(tools) = opts.tools.as_ref() {
        input["tools"] = json!(tools);
    }
    if !opts.skills.is_empty() {
        input["skills"] = json!(opts.skills);
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
    if !runtime.is_empty() {
        input["_runtime"] = Value::Object(runtime);
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
        assert!(input.get("workspace_root").is_none());
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
            workspace_root: Some("/tmp/proj".into()),
            workspace_mode: Some("all_except_ignored".into()),
            max_iterations: 20,
            ..StartOpts::default()
        };
        let input = build_input_data("do it", &opts);
        assert_eq!(input["provider"], json!("lmstudio"));
        assert_eq!(input["_runtime"]["model"], json!("qwen3-4b"));
        assert_eq!(input["workspace_root"], json!("/tmp/proj"));
        assert_eq!(input["workspace_access_mode"], json!("all_except_ignored"));
        assert_eq!(input["max_iterations"], json!(20));
    }
}
