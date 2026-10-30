//! State and request contract for the Gateway-owned docs assistant.
use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct DocsChat {
    pub session_id: String,
    pub operation: u64,
    pub messages: Vec<(String, String)>,
    pub busy: bool,
    pub run_id: Option<String>,
    pub error: String,
    pub corpus: Option<String>,
}
impl Default for DocsChat {
    fn default() -> Self {
        Self {
            session_id: format!("code-docs-assistant:{}", crate::config::mint_session_id()),
            operation: 0,
            messages: Vec::new(),
            busy: false,
            run_id: None,
            error: String::new(),
            corpus: None,
        }
    }
}
impl DocsChat {
    pub fn matches(&self, session: &str, operation: u64) -> bool {
        self.session_id == session && self.operation == operation
    }
}

/// Same workflow and purpose as panel-chat's docsQaStartBody.
pub fn start_body(session: &str, question: &str, docs: &str) -> Value {
    json!({"registry_scope":"tenant_catalog", "bundle_id":"docs-qa", "bundle_version":"0.1.1",
        "flow_id":"docsqa001", "session_id":session, "kind":"docs",
        "input_data":{"prompt":question,"docs":docs,"app":"AbstractCode","use_session_history":true}})
}
