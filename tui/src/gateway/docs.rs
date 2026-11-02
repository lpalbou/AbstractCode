//! Documentation requests use the same authenticated Gateway transport as runs.
use super::{GatewayClient, GwError, GwResult};
use serde_json::Value;
impl GatewayClient {
    pub fn docs_corpus(&self) -> GwResult<String> {
        let v = self.get_json("/docs/corpus?app=code")?;
        let text = v.get("text").and_then(Value::as_str).unwrap_or("");
        if text.trim().is_empty() {
            return Err(GwError::transport(
                "The Gateway returned no AbstractCode documentation; no question was sent.",
            ));
        }
        Ok(text.into())
    }
    pub fn start_docs_question(
        &self,
        session: &str,
        question: &str,
        docs: &str,
    ) -> GwResult<String> {
        if docs.trim().is_empty() || question.trim().is_empty() || session.trim().is_empty() {
            return Err(GwError::transport("Documentation, a question and a docs conversation are required; no question was sent."));
        }
        let v = self.post_json(
            "/runs/start",
            &crate::docs_chat::start_body(session, question, docs),
        )?;
        Ok(super::started_run_from_response(&v)?.run_id)
    }
}
