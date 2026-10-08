//! The account's default workflow for AbstractCode (R17.1, the terminal
//! counterpart of R14-W2's Code web row).
//!
//! The gateway keeps the workflow NEW conversations start on, per account
//! and per app: `GET/PUT /api/gateway/accounts/me/preferences`,
//! `default_workflow` `{"abstractcode.agent.v1": null | "bundle:flow" |
//! "catalog:bundle:flow"}`, null = the gateway's per-app default. The same
//! value the Assistant, the Code web and the console show.
//!
//! Wording and rules are the Code web's (`web/src/workspace/
//! account_workflow_default.tsx`, `account_preferences.ts`):
//! - the row is "Default for new conversations"; its first choice is the
//!   gateway's `gateway_default_label` verbatim ("Gateway default
//!   (<name>)"), then `choices[].label`; a saved value the gateway no longer
//!   lists reads "<value> (no longer runs)";
//! - a change is ONE PUT at once; "Saved." or "Not saved. <sentence>";
//! - a broken choice shows the gateway's `reason`, else the help sentence;
//! - the choice made on THIS computer before the gateway kept it (the old
//!   device preference) is uploaded once when the account has none, then
//!   removed here; a refusal (400) removes it too, a network failure keeps
//!   it for the next start;
//! - a gateway without the route (404, older than 0.13.1) keeps the old
//!   behaviour: the choice stays on this computer (`/workflow` saves it).

use serde_json::{json, Value};

/// The app interface this terminal runs (the web's `CODE_AGENT_INTERFACE`).
pub const INTERFACE: &str = "abstractcode.agent.v1";
/// The route (under `/api/gateway`).
pub const PATH: &str = "/accounts/me/preferences";

/// The row's label, verbatim from the Code web.
pub const LABEL: &str = "Default for new conversations";
/// The help sentence under the row, verbatim from the Code web.
pub const HELP: &str = "Kept by the gateway for your account: the Assistant and every browser use the same choice. This conversation's workflow is picked above.";
/// The panel's one sentence on a gateway without the route (older than
/// 0.13.1; the console's `p` says the same of its Preferences).
pub const UNSUPPORTED: &str = "This gateway does not keep a default for your account (it needs a newer gateway): /workflow saves your choice on this computer, as before.";
pub const SAVED: &str = "Saved.";
pub const NOT_SAVED: &str = "Not saved.";

/// One choice of the row: the account value, its label, the bundle and
/// flow it runs (for the terminal's own workflow list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
    pub workflow_id: String,
    pub bundle_id: String,
    pub flow_id: String,
}

/// The app's row of the gateway answer (the web's `AccountWorkflowRow`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The account's own choice; `None` = the gateway default.
    pub value: Option<String>,
    /// "default" | "set" | "broken".
    pub state: String,
    pub reason: Option<String>,
    pub gateway_default_label: String,
    pub choices: Vec<Choice>,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default()
}

/// The app's row, checked like the web: a missing row or field is said,
/// never guessed (the web's sentences, verbatim).
pub fn row(answer: &Value) -> Result<Row, String> {
    let ok = answer.get("apps").is_some_and(Value::is_array)
        && answer
            .pointer("/preferences/default_workflow")
            .is_some_and(Value::is_object);
    if !ok {
        return Err(
            "The gateway's account preferences answer has no apps or default_workflow.".into(),
        );
    }
    let apps = answer["apps"].as_array().cloned().unwrap_or_default();
    let Some(r) = apps
        .iter()
        .find(|r| r.get("interface").and_then(Value::as_str) == Some(INTERFACE))
    else {
        return Err(format!(
            "The gateway's account preferences have no row for {INTERFACE}."
        ));
    };
    let label = r.get("gateway_default_label").and_then(Value::as_str);
    let choices = r.get("choices").and_then(Value::as_array);
    let (Some(label), Some(choices)) = (label, choices) else {
        return Err(format!(
            "The gateway's account preferences row for {INTERFACE} has no gateway_default_label or choices."
        ));
    };
    Ok(Row {
        value: r
            .get("value")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .map(str::to_string),
        state: r
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("default")
            .to_string(),
        reason: r
            .get("reason")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .map(str::to_string),
        gateway_default_label: label.to_string(),
        choices: choices
            .iter()
            .filter(|c| c.is_object())
            .map(|c| {
                let value = c
                    .get("value")
                    .map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| v.to_string())
                    })
                    .unwrap_or_default();
                let label = c
                    .get("label")
                    .or_else(|| c.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| value.clone());
                Choice {
                    value,
                    label,
                    workflow_id: s(c, "workflow_id"),
                    bundle_id: s(c, "bundle_id"),
                    flow_id: s(c, "flow_id"),
                }
            })
            .collect(),
    })
}

/// The PUT body of one change (`None` = back to the gateway default).
pub fn put_body(value: Option<&str>) -> Value {
    json!({"default_workflow": {INTERFACE: value}})
}

impl Row {
    /// The options as the web's select lists them: the gateway default
    /// (`None`), a saved value the gateway no longer lists, then every
    /// choice in the gateway's order.
    pub fn options(&self) -> Vec<(Option<String>, String)> {
        let mut out = vec![(None, self.gateway_default_label.clone())];
        if let Some(v) = &self.value {
            if !self.choices.iter().any(|c| &c.value == v) {
                out.push((Some(v.clone()), format!("{v} (no longer runs)")));
            }
        }
        out.extend(
            self.choices
                .iter()
                .map(|c| (Some(c.value.clone()), c.label.clone())),
        );
        out
    }

    /// The label of the current option.
    pub fn current_label(&self) -> String {
        self.options()
            .into_iter()
            .find(|(v, _)| *v == self.value)
            .map(|(_, l)| l)
            .unwrap_or_else(|| self.gateway_default_label.clone())
    }

    /// The line under the row: a broken choice's reason, else the help.
    pub fn note(&self) -> String {
        match (&self.state[..], &self.reason) {
            ("broken", Some(r)) => r.clone(),
            _ => HELP.to_string(),
        }
    }

    /// The `(bundle, flow)` the account's choice runs, `None` = the
    /// gateway default (also for a value the gateway no longer lists: the
    /// gateway says why in `reason`, like the web's `selectionFromAccountValue`).
    pub fn workflow(&self) -> Option<(String, String)> {
        let v = self.value.as_deref()?;
        self.choices
            .iter()
            .find(|c| c.value == v && !c.bundle_id.is_empty() && !c.flow_id.is_empty())
            .map(|c| (c.bundle_id.clone(), c.flow_id.clone()))
    }
}

/// The account value of the old device choice (`bundle:flow`, version-less
/// like the web's `accountValueFromSelection`; the terminal's choices are
/// the gateway's own registry, no prefix). `None` for the gateway default.
pub fn device_value(bundle: Option<&str>, flow: Option<&str>) -> Option<String> {
    let bundle = bundle?.split('@').next()?.trim();
    let flow = flow?.trim();
    (!bundle.is_empty() && !flow.is_empty()).then(|| format!("{bundle}:{flow}"))
}

/// The row's state in the terminal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum State {
    /// Not asked yet.
    #[default]
    Unknown,
    Loading,
    /// 404: a gateway older than 0.13.1 — the choice stays on this computer.
    Unsupported,
    Error(String),
    Ok(Row),
}

/// Everything the Workflow panel needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct View {
    pub state: State,
    /// A PUT is in flight (the row is busy).
    pub busy: bool,
    /// "Saved." / "Not saved. <sentence>" under the row (`true` = error).
    pub note: Option<(bool, String)>,
    /// The migration settled: remove the old device choice (UI thread).
    pub clear_device: bool,
}

impl View {
    /// The gateway keeps the default (the device choice is ignored).
    pub fn managed(&self) -> bool {
        matches!(self.state, State::Ok(_))
    }

    pub fn row(&self) -> Option<&Row> {
        match &self.state {
            State::Ok(r) => Some(r),
            _ => None,
        }
    }
}

/// The note of a change: "Saved." or "Not saved. <sentence>" (the web's).
pub fn change_note(out: &Result<(), String>) -> (bool, String) {
    match out {
        Ok(()) => (false, SAVED.to_string()),
        Err(sentence) => {
            let s = sentence.trim();
            if s.is_empty() {
                (true, NOT_SAVED.to_string())
            } else {
                (true, format!("{NOT_SAVED} {s}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GET: &str = include_str!("../tests/fixtures/account_prefs/get_default.json");
    const PUT_OK: &str = include_str!("../tests/fixtures/account_prefs/put_coder.json");

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn the_row_reads_the_recorded_gateway_answer() {
        let r = row(&parse(GET)).unwrap();
        assert_eq!(r.value, None);
        assert_eq!(r.state, "default");
        assert_eq!(r.gateway_default_label, "Gateway default (Basic agent)");
        assert!(r.choices.iter().any(|c| c.value == "coding-agent:coder"
            && c.label == "Coding agent (chat)"
            && c.bundle_id == "coding-agent"
            && c.flow_id == "coder"));
        // The gateway default first, verbatim; then the choices in order.
        let opts = r.options();
        assert_eq!(opts[0], (None, "Gateway default (Basic agent)".to_string()));
        assert_eq!(opts.len(), r.choices.len() + 1);
        assert_eq!(r.current_label(), "Gateway default (Basic agent)");
        assert_eq!(r.workflow(), None);
        assert_eq!(r.note(), HELP);
    }

    #[test]
    fn a_set_value_names_its_choice_and_runs_its_workflow() {
        let r = row(&parse(PUT_OK)).unwrap();
        assert_eq!(r.value.as_deref(), Some("coding-agent:coder"));
        assert_eq!(r.current_label(), "Coding agent (chat)");
        assert_eq!(
            r.workflow(),
            Some(("coding-agent".to_string(), "coder".to_string()))
        );
    }

    #[test]
    fn a_value_no_longer_listed_says_so_and_a_broken_row_shows_the_reason() {
        let mut v = parse(GET);
        v["apps"][0]["value"] = json!("gone:flow");
        v["apps"][0]["state"] = json!("broken");
        v["apps"][0]["reason"] = json!("The workflow gone:flow is not on this gateway.");
        let r = row(&v).unwrap();
        assert_eq!(
            r.options()[1],
            (
                Some("gone:flow".into()),
                "gone:flow (no longer runs)".into()
            )
        );
        assert_eq!(r.current_label(), "gone:flow (no longer runs)");
        assert_eq!(r.note(), "The workflow gone:flow is not on this gateway.");
        assert_eq!(r.workflow(), None, "the gateway default runs");
    }

    #[test]
    fn a_missing_field_is_said_with_the_webs_sentence() {
        assert_eq!(
            row(&json!({"ok": true})).unwrap_err(),
            "The gateway's account preferences answer has no apps or default_workflow."
        );
        let mut v = parse(GET);
        v["apps"] = json!([]);
        assert_eq!(
            row(&v).unwrap_err(),
            "The gateway's account preferences have no row for abstractcode.agent.v1."
        );
        let mut v = parse(GET);
        v["apps"][0]
            .as_object_mut()
            .unwrap()
            .remove("gateway_default_label");
        assert!(row(&v)
            .unwrap_err()
            .contains("has no gateway_default_label or choices"));
    }

    #[test]
    fn the_body_and_the_notes_are_the_webs() {
        assert_eq!(
            put_body(Some("coding-agent:coder")),
            json!({"default_workflow": {"abstractcode.agent.v1": "coding-agent:coder"}})
        );
        assert_eq!(
            put_body(None),
            json!({"default_workflow": {"abstractcode.agent.v1": null}})
        );
        assert_eq!(change_note(&Ok(())), (false, "Saved.".into()));
        assert_eq!(
            change_note(&Err("workflow bundle 'x' is not on this gateway.".into())),
            (
                true,
                "Not saved. workflow bundle 'x' is not on this gateway.".into()
            )
        );
        assert_eq!(LABEL, "Default for new conversations");
    }

    #[test]
    fn the_device_value_is_version_less() {
        assert_eq!(
            device_value(Some("coding-agent@0.2.8"), Some("coder")).as_deref(),
            Some("coding-agent:coder")
        );
        assert_eq!(device_value(None, None), None);
        assert_eq!(device_value(Some("b"), None), None);
    }
}
