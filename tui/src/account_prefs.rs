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
//!
//! Round 18 — the account's SPOKEN LANGUAGE rides on the same answer: the
//! `spoken_language` block `{value, label, help, choices}` ("auto" or a
//! code; the list is the gateway's, i.e. AbstractVoice's). The terminal
//! keeps no list and no copy; a pick is ONE PUT `{"spoken_language": v}`.
//! A missing block is said with the web's sentence ([`SPOKEN_LANGUAGE_MISSING`]).

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

/// The sentence when the answer lacks the block (the kit's
/// `SPOKEN_LANGUAGE_MISSING`, verbatim).
pub const SPOKEN_LANGUAGE_MISSING: &str =
    "The gateway's account preferences answer has no spoken_language block.";

/// The account's spoken language as the gateway serves it (the kit's
/// `SpokenLanguagePreference`): `value` = "auto" or a code, never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpokenLanguagePref {
    pub value: String,
    pub label: String,
    pub help: String,
    /// `(value, label)` in the gateway's order ("auto" first).
    pub choices: Vec<(String, String)>,
}

impl SpokenLanguagePref {
    /// The current choice's served label ("Auto (detected)", "French");
    /// the bare value when the gateway does not list it.
    pub fn current_label(&self) -> String {
        self.choices
            .iter()
            .find(|(v, _)| *v == self.value)
            .map(|(_, l)| l.clone())
            .unwrap_or_else(|| self.value.clone())
    }

    /// The line beside the dictation control: "Spoken language: <label>".
    pub fn line(&self) -> String {
        format!("Spoken language: {}", self.current_label())
    }
}

/// The `spoken_language` block, checked like the web's
/// `accountSpokenLanguage`: a missing block or field is said, never guessed.
pub fn spoken_language(answer: &Value) -> Result<SpokenLanguagePref, String> {
    let b = answer.get("spoken_language").filter(|b| b.is_object());
    let parsed = b.and_then(|b| {
        let value = b
            .get("value")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())?;
        let label = b.get("label").and_then(Value::as_str)?;
        let help = b.get("help").and_then(Value::as_str)?;
        let choices = b.get("choices").and_then(Value::as_array)?;
        Some(SpokenLanguagePref {
            value: value.to_string(),
            label: label.to_string(),
            help: help.to_string(),
            choices: choices
                .iter()
                .filter_map(|c| {
                    let v = c.get("value").and_then(Value::as_str)?;
                    let l = c.get("label").and_then(Value::as_str).unwrap_or(v);
                    Some((v.to_string(), l.to_string()))
                })
                .collect(),
        })
    });
    parsed.ok_or_else(|| SPOKEN_LANGUAGE_MISSING.to_string())
}

/// The PUT body of a spoken-language pick ("auto" = the engine detects it).
pub fn put_spoken_language(value: &str) -> Value {
    json!({"spoken_language": value})
}

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

/// Everything the Workflow panel (and the Voice panel's Spoken language
/// row) needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub state: State,
    /// A PUT is in flight (the row is busy).
    pub busy: bool,
    /// "Saved." / "Not saved. <sentence>" under the row (`true` = error).
    pub note: Option<(bool, String)>,
    /// The migration settled: remove the old device choice (UI thread).
    pub clear_device: bool,
    /// The account's spoken language from the same GET (round 18). `Err("")`
    /// = not read yet; `Err(sentence)` = the gateway did not serve it (an
    /// older gateway: [`SPOKEN_LANGUAGE_MISSING`]) or could not be read.
    pub spoken_language: Result<SpokenLanguagePref, String>,
    /// A spoken-language PUT is in flight.
    pub spoken_busy: bool,
    /// "Saved." / "Not saved. <sentence>" under the Spoken language row.
    pub spoken_note: Option<(bool, String)>,
}

impl Default for View {
    fn default() -> Self {
        View {
            state: State::Unknown,
            busy: false,
            note: None,
            clear_device: false,
            spoken_language: Err(String::new()),
            spoken_busy: false,
            spoken_note: None,
        }
    }
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

    /// A change answered: the new row on success; on a refusal the shown
    /// value stays (no optimistic flip) and the note says why.
    pub fn apply_save(&mut self, out: Result<Row, String>) {
        let note = change_note(&out.as_ref().map(|_| ()).map_err(Clone::clone));
        if let Ok(row) = out {
            self.state = State::Ok(row);
        }
        self.busy = false;
        self.note = Some(note);
    }

    /// The account's spoken language when the gateway served it.
    pub fn spoken(&self) -> Option<&SpokenLanguagePref> {
        self.spoken_language.as_ref().ok()
    }

    /// A spoken-language pick answered (the PUT's whole answer, parsed): the
    /// new block on success; on a refusal the shown value stays and the note
    /// says why ("Not saved. <the gateway's sentence>").
    pub fn apply_spoken_save(&mut self, out: Result<SpokenLanguagePref, String>) {
        let note = change_note(&out.as_ref().map(|_| ()).map_err(Clone::clone));
        if let Ok(block) = out {
            self.spoken_language = Ok(block);
        }
        self.spoken_busy = false;
        self.spoken_note = Some(note);
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
    const PUT_FR: &str = include_str!("../tests/fixtures/account_prefs/put_spoken_fr.json");

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
    fn a_refusal_keeps_the_shown_value_a_success_shows_the_new_one() {
        let mut v = View {
            state: State::Ok(row(&parse(GET)).unwrap()),
            busy: true,
            ..View::default()
        };
        v.apply_save(Err("workflow bundle 'x' is not on this gateway.".into()));
        assert_eq!(v.row().unwrap().value, None, "no optimistic flip");
        assert!(!v.busy);
        assert_eq!(
            v.note,
            Some((
                true,
                "Not saved. workflow bundle 'x' is not on this gateway.".into()
            ))
        );
        v.busy = true;
        v.apply_save(row(&parse(PUT_OK)));
        assert_eq!(
            v.row().unwrap().value.as_deref(),
            Some("coding-agent:coder")
        );
        assert_eq!(v.note, Some((false, "Saved.".into())));
        assert!(!v.busy);
    }

    #[test]
    fn the_spoken_language_block_is_read_from_the_recorded_answer() {
        let b = spoken_language(&parse(GET)).unwrap();
        assert_eq!(b.value, "auto");
        assert_eq!(b.label, "Spoken language");
        assert_eq!(b.help, "The language spoken to the microphone. Auto lets the speech engine detect it; naming it skips detection, so short phrases and mixed-language speech transcribe reliably and a little faster.");
        assert_eq!(
            b.choices[0],
            ("auto".to_string(), "Auto (detected)".to_string())
        );
        assert!(b
            .choices
            .contains(&("fr".to_string(), "French".to_string())));
        assert_eq!(b.current_label(), "Auto (detected)");
        assert_eq!(b.line(), "Spoken language: Auto (detected)");
        let fr = spoken_language(&parse(PUT_FR)).unwrap();
        assert_eq!(fr.value, "fr");
        assert_eq!(fr.line(), "Spoken language: French");
    }

    #[test]
    fn a_missing_spoken_language_block_is_said_never_guessed() {
        let mut v = parse(GET);
        v.as_object_mut().unwrap().remove("spoken_language");
        assert_eq!(
            spoken_language(&v).unwrap_err(),
            "The gateway's account preferences answer has no spoken_language block."
        );
        let mut v = parse(GET);
        v["spoken_language"]
            .as_object_mut()
            .unwrap()
            .remove("choices");
        assert_eq!(spoken_language(&v).unwrap_err(), SPOKEN_LANGUAGE_MISSING);
        // The workflow row still reads: the two blocks are independent.
        assert!(row(&v).is_ok());
    }

    #[test]
    fn a_spoken_language_pick_is_one_put_body_and_its_note() {
        assert_eq!(put_spoken_language("fr"), json!({"spoken_language": "fr"}));
        assert_eq!(
            put_spoken_language("auto"),
            json!({"spoken_language": "auto"})
        );
        let mut v = View {
            spoken_language: spoken_language(&parse(GET)),
            spoken_busy: true,
            ..View::default()
        };
        v.apply_spoken_save(Err(
            "spoken_language = 'xx' refused: not a language the speech engines support.".into(),
        ));
        assert_eq!(v.spoken().unwrap().value, "auto", "no optimistic flip");
        assert!(!v.spoken_busy);
        assert_eq!(v.spoken_note, Some((true, "Not saved. spoken_language = 'xx' refused: not a language the speech engines support.".into())));
        v.apply_spoken_save(spoken_language(&parse(PUT_FR)));
        assert_eq!(v.spoken().unwrap().value, "fr");
        assert_eq!(v.spoken_note, Some((false, "Saved.".into())));
        assert_eq!(v.note, None, "the workflow row's note is untouched");
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
