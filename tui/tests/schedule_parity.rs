//! `/schedule` body parity with the Code web (R17.2).
//!
//! `tests/fixtures/schedule/web_schedule_bodies.json` is written by
//! `tui/scripts/web_schedule_body.ts`, which runs the web's own
//! `buildWorkflowInput` / `schemaDefaults` / `validateWorkflowInputs` /
//! `withAutomationWorkspace` and the kit's `buildCreateRequest` /
//! `withAutomationTools` / `automationToolSelection` for a few conversations.
//! Each case is rebuilt here through the terminal's own path
//! (`schedule_view::create_body`, the function "Create automation" calls) and
//! the two bodies must be byte-equal once their keys are sorted.

use serde_json::{json, Value};

use abstractcode::automation_email::{Attachments, EmailTriggerForm, RecipientsForm};
use abstractcode::automations::{CreateForm, When};
use abstractcode::schedule_input::{normalize_input_schema, Conversation};
use abstractcode::ui::schedule_view::{create_body, first_tools, Draft, Picked};

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/schedule");

fn read(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(format!("{FIX}/{name}")).unwrap()).unwrap()
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The neutral case → the terminal's dialog state.
fn draft_of(case: &Value) -> (Draft, Value, bool) {
    let schema = normalize_input_schema(&read(case["schema"].as_str().unwrap())).unwrap();
    let c = &case["conversation"];
    let d = &case["dialog"];
    let conv = Conversation {
        provider: s(&c["provider"]),
        model: s(&c["model"]),
        reasoning: s(&c["reasoning"]),
        speculation: None,
        stream: match c["stream"].as_str() {
            Some("on") => Some(true),
            Some("off") => Some(false),
            _ => None,
        },
        max_iterations: c["maxIterations"].as_u64().unwrap_or(0),
        max_tokens: c["maxTokens"].as_u64().unwrap_or(0),
        tools: c["customizedTools"]
            .as_array()
            .map(|_| strings(&c["customizedTools"])),
        skills: strings(&c["skills"]),
    };
    let when = match d["when"]["kind"].as_str().unwrap() {
        "every" => When::Every {
            amount: d["when"]["amount"].to_string(),
            unit: s(&d["when"]["unit"]).chars().next().unwrap(),
        },
        "once" => When::Once {
            at: s(&d["when"]["at"]),
        },
        _ => When::Email,
    };
    let e = &d["email"];
    let email = if e.is_object() {
        EmailTriggerForm {
            uses_model: true,
            every: if e["every"].is_object() {
                format!("{}{}", e["every"]["amount"], s(&e["every"]["unit"]))
            } else {
                String::new()
            },
            max_batch: e["maxBatch"]
                .as_u64()
                .map(|n| n.to_string())
                .unwrap_or_default(),
            from_in: s(&e["fromIn"]),
            from_domain_in: s(&e["fromDomainIn"]),
            to_in: s(&e["toIn"]),
            subject_contains: s(&e["subjectContains"]),
            has_attachment: match e["hasAttachment"].as_str() {
                Some("yes") => Attachments::Yes,
                Some("no") => Attachments::No,
                _ => Attachments::Any,
            },
        }
    } else {
        EmailTriggerForm::default()
    };
    let picked = case
        .get("picked")
        .filter(|p| p.is_object())
        .map(|p| Picked {
            label: "picked".into(),
            target: p.clone(),
            schema: Ok(("picked".into(), "0".into(), "f".into())),
        });
    let mut draft = Draft {
        opened: Box::default(),
        form: CreateForm {
            prompt: s(&d["prompt"]),
            when,
            email,
            context: s(&d["context"]),
            growing_max_tokens: d["growingMaxTokens"].to_string(),
            tool_approval: s(&d["toolApproval"]),
            tools: None,
            notify_email: d["notifyEmail"].as_bool().unwrap(),
            recipients: RecipientsForm {
                list: d["recipients"]["mode"] == "list",
                addresses: s(&d["recipients"]["addresses"]),
            },
            title: s(&d["title"]),
            start_at: s(&d["startAt"]),
            count: d["count"]
                .as_u64()
                .map(|n| n.to_string())
                .unwrap_or_default(),
            until: s(&d["until"]),
            workspace: abstractcode::workspaces::run_value_from(
                &json!({"workspace": d["workspace"]}),
            ),
        },
        conv_target: Some(case["conversationTarget"].clone()),
        conv_label: String::new(),
        conv_schema: Some(("conv".into(), "0".into(), "f".into())),
        picked,
        conv,
        tools_set: true,
        rule: Default::default(),
    };
    draft.form.tools = match &d["tools"] {
        Value::String(t) if t == "initial" => first_tools(&draft, Some(&schema)),
        Value::Null => None,
        list => Some(strings(list)),
    };
    let usable = d["emailUsable"].as_bool().unwrap();
    (draft, schema, usable)
}

#[test]
fn schedule_body_equals_the_web() {
    let all = read("web_schedule_bodies.json");
    let cases = all.as_object().unwrap();
    assert_eq!(cases.len(), 3, "inherit, custom, picked");
    for (name, entry) in cases {
        let (draft, schema, usable) = draft_of(&entry["case"]);
        let ours = create_body(&draft, Ok(&schema), usable, "rid-parity")
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        // serde_json maps are key-sorted: the pretty strings are the
        // canonical (sorted) JSON of each side.
        let ours = serde_json::to_string_pretty(&ours).unwrap();
        let web = serde_json::to_string_pretty(&entry["body"]).unwrap();
        assert_eq!(
            ours, web,
            "{name}: the terminal's POST body differs from the web's"
        );
    }
}

#[test]
fn the_web_fixture_pins_the_facts_the_adversary_named() {
    let all = read("web_schedule_bodies.json");
    let inherit = &all["inherit"]["body"]["target"]["input_data"];
    // Schema defaults ride; the served tools default is the first selection.
    assert_eq!(inherit["temperature"], json!(0.7));
    assert_eq!(inherit["tools"], inherit["_runtime"]["allowed_tools"]);
    assert_eq!(inherit["tools"].as_array().unwrap().len(), 9);
    assert!(inherit.get("workspace_root").is_none());
    assert!(inherit["_runtime"].get("tool_policy").is_none());
    let custom = &all["custom"]["body"];
    assert_eq!(custom["trigger"]["source_id"], json!("email.received"));
    assert_eq!(
        custom["notify"],
        json!({"channels": ["console", "email"], "recipients": ["self", "boss@example.test"]})
    );
    // A picked workflow starts from "Use workflow default tools".
    let picked = &all["picked"]["body"]["target"]["input_data"];
    assert!(picked.get("tools").is_none());
    assert_eq!(
        all["picked"]["body"]["target"]["bundle_ref"],
        json!("react-agent@0.1.0")
    );
}
