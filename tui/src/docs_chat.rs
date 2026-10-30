//! State and request contract for the Gateway-owned docs assistant.
use serde_json::{json, Value};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChatScope {
    #[default]
    Docs,
    Run,
}
impl ChatScope {
    pub fn chat(self, store: crate::store::Store) -> abstracttui::reactive::Signal<DocsChat> {
        match self {
            Self::Docs => store.docs_chat,
            Self::Run => store.run_chat,
        }
    }
    pub fn draft(self, store: crate::store::Store) -> abstracttui::reactive::Signal<String> {
        match self {
            Self::Docs => store.docs_draft,
            Self::Run => store.run_question_draft,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DocsChat {
    pub session_id: String,
    pub operation: u64,
    pub messages: Vec<(String, String)>,
    pub busy: bool,
    pub run_id: Option<String>,
    pub error: String,
    pub corpus: Option<String>,
    pub source_session: String,
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
            source_session: String::new(),
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

/// Cached documentation and fresh conversation are kept separate.
#[derive(Clone, Debug, Default)]
pub struct QuestionReference {
    pub corpus: Option<String>,
    pub conversation: String,
}
