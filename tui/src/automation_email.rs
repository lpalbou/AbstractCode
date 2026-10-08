//! Email in automations (R17.2): the kit's `EMAIL_TEXT` wording and its
//! email rules (`ui-kit/src/automations/panel_core.ts`, framework backlog
//! 0992 WP6), in the terminal. Pure: no HTTP, no signals.
//!
//! - the "When an email arrives" trigger (`email.received@1`): typed
//!   filters, a check interval (1 h for a model by default, never under
//!   60 s) and a max batch — [`EmailTriggerForm`], [`email_trigger_config_from`];
//! - the Mailbox section: "Email result" (`notify.channels` gets `email`)
//!   and the recipients (only me, or me and these addresses) —
//!   [`notify_for`], [`allowed_recipients_from`];
//! - both are offered only when `GET /api/gateway/me/email` says the
//!   account can be used now ([`EmailStatus::usable`], the kit's
//!   `emailUsable`); otherwise the kit's notice "Connect a mailbox first —
//!   open My email" (+ the administrator's cause).
//!
//! [`TEXT`] is the kit's `EMAIL_TEXT` table byte for byte: the kit reads it
//! from `automation_controls.json` → `email`, which this crate vendors
//! byte-identically as `assets/automation_controls.json` (the root
//! `scripts/check_identity_sync.py` fails on drift).
//! `tests::wording_table_matches_the_kit` diffs [`TEXT`] against that copy;
//! `scripts/check_email_wording.py` diffs kit ↔ vendored ↔ terminal.

use serde_json::{json, Map, Value};

// ---------------------------------------------------------------------------
// The wording table (the kit's EMAIL_TEXT, verbatim)
// ---------------------------------------------------------------------------

pub const TRIGGER_SOURCE: &str = "email.received@1";
pub const TRIGGER_LABEL: &str = "When an email arrives";
pub const NOT_SET_UP: &str = "Connect a mailbox first — open My email";
pub const OPEN_MY_EMAIL: &str = "open My email";
pub const FILTERS_LEGEND: &str = "Only mail that matches (all optional)";
pub const FROM_IN: &str = "From these addresses";
pub const FROM_DOMAIN_IN: &str = "From these domains";
pub const TO_IN: &str = "Sent to these addresses";
pub const SUBJECT_CONTAINS: &str = "Subject contains";
pub const HAS_ATTACHMENT: &str = "Attachments";
pub const HAS_ATTACHMENT_ANY: &str = "any";
pub const HAS_ATTACHMENT_YES: &str = "only with attachments";
pub const HAS_ATTACHMENT_NO: &str = "only without attachments";
pub const LIST_HINT: &str =
    "Separate entries with commas or new lines. Exact addresses and domains only (no patterns).";
pub const EVERY_LABEL: &str = "Check for new mail every";
pub const INTERVAL_RULE: &str = "An automation that runs a model on new mail checks once an hour by default; one that needs no model checks every 60 s. The shortest interval is 60 s.";
pub const MAX_BATCH_LABEL: &str = "At most this many emails per run";
pub const MAX_BATCH_HINT: &str = "The rest wait for the next run. Each email is read once by this automation; mail that arrived before it was created, or while it is paused, is not processed.";
pub const UNTRUSTED_HINT: &str = "Incoming mail is data, never instructions: the automation acts only on its task. Link-opening tools (fetch_url, browser_probe) always ask for your approval.";
pub const NOTIFY_LABEL: &str = "Email result";
pub const NOTIFY_HINT: &str = "Email each completed run’s result to the selected recipients.";
pub const RECIPIENTS_LEGEND: &str = "Recipients";
pub const RECIPIENTS_SELF: &str = "Only me (default)";
pub const RECIPIENTS_LIST: &str = "Me and these addresses";
pub const RECIPIENTS_HINT: &str = "Separate multiple addresses with commas.";

/// The kit's `EMAIL_TEXT`, in its order (key, text).
pub const TEXT: &[(&str, &str)] = &[
    ("trigger_source", TRIGGER_SOURCE),
    ("trigger_label", TRIGGER_LABEL),
    ("not_set_up", NOT_SET_UP),
    ("open_my_email", OPEN_MY_EMAIL),
    ("filters_legend", FILTERS_LEGEND),
    ("from_in", FROM_IN),
    ("from_domain_in", FROM_DOMAIN_IN),
    ("to_in", TO_IN),
    ("subject_contains", SUBJECT_CONTAINS),
    ("has_attachment", HAS_ATTACHMENT),
    ("has_attachment_any", HAS_ATTACHMENT_ANY),
    ("has_attachment_yes", HAS_ATTACHMENT_YES),
    ("has_attachment_no", HAS_ATTACHMENT_NO),
    ("list_hint", LIST_HINT),
    ("every_label", EVERY_LABEL),
    ("interval_rule", INTERVAL_RULE),
    ("max_batch_label", MAX_BATCH_LABEL),
    ("max_batch_hint", MAX_BATCH_HINT),
    ("untrusted_hint", UNTRUSTED_HINT),
    ("notify_label", NOTIFY_LABEL),
    ("notify_hint", NOTIFY_HINT),
    ("recipients_legend", RECIPIENTS_LEGEND),
    ("recipients_self", RECIPIENTS_SELF),
    ("recipients_list", RECIPIENTS_LIST),
    ("recipients_hint", RECIPIENTS_HINT),
];

/// The kit's line under the Email result switch (AfEmailOptionsFields).
pub fn notify_help() -> String {
    format!("{NOTIFY_HINT} Turn on Email result to choose yourself or other email addresses.")
}

// ---------------------------------------------------------------------------
// Constants (panel_core.ts; the runtime's caps)
// ---------------------------------------------------------------------------

pub const SOURCE_ID: &str = "email.received";
pub const SOURCE_VERSION: u64 = 1;
pub const DEFAULT_EVERY_MODEL: &str = "1h";
pub const DEFAULT_EVERY_NO_MODEL: &str = "60s";
pub const MIN_EVERY_SECONDS: u64 = 60;
pub const DEFAULT_MAX_BATCH: u64 = 100;
pub const MAX_BATCH: u64 = 1000;
pub const MAX_FILTER_ENTRIES: usize = 200;
pub const MAX_SUBJECT_CONTAINS: usize = 200;
pub const MAX_ALLOWED_RECIPIENTS: usize = 50;

pub fn is_email_trigger(source_id: &str, source_version: u64) -> bool {
    source_id == SOURCE_ID && source_version == SOURCE_VERSION
}

// ---------------------------------------------------------------------------
// The account's email status (`GET /api/gateway/me/email`)
// ---------------------------------------------------------------------------

/// What the dialog reads from `GET /api/gateway/me/email`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmailStatus {
    /// `effective_enabled`: connected, the user's switch on, allowed by the
    /// administrator.
    pub effective_enabled: bool,
    /// `admin_disabled.cause` ("" when email was not turned off by an admin).
    pub admin_cause: String,
}

impl EmailStatus {
    pub fn parse(v: &Value) -> Result<EmailStatus, String> {
        if !v.is_object() {
            return Err("GET /api/gateway/me/email did not answer with an object".into());
        }
        Ok(EmailStatus {
            effective_enabled: v.get("effective_enabled") == Some(&json!(true)),
            admin_cause: v
                .pointer("/admin_disabled/cause")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })
    }

    /// The kit's `emailUsable`: unknown (not read / the call failed) is NOT usable.
    pub fn usable(status: Option<&EmailStatus>) -> bool {
        status.is_some_and(|s| s.effective_enabled)
    }
}

/// The kit's `AfEmailSetupNotice` as one line: "" when email is usable,
/// else "Connect a mailbox first — open My email" (+ " (<cause>)").
pub fn setup_notice(status: Option<&EmailStatus>) -> String {
    if EmailStatus::usable(status) {
        return String::new();
    }
    match status
        .map(|s| s.admin_cause.as_str())
        .filter(|c| !c.is_empty())
    {
        Some(cause) => format!("{NOT_SET_UP} ({cause})"),
        None => NOT_SET_UP.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Typed lists (addresses, domains)
// ---------------------------------------------------------------------------

/// The kit's `parseEntryList`: split on commas, semicolons and white space,
/// trimmed, lower-cased, de-duplicated (order kept).
pub fn parse_entry_list(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        let e = raw.trim().to_lowercase();
        if !e.is_empty() && !out.contains(&e) {
            out.push(e);
        }
    }
    out
}

fn address_forbidden(c: char) -> bool {
    c.is_whitespace() || "<>,;:\"()[]".contains(c)
}

/// `name@example.test` — the runtime's plain-address rule.
pub fn is_plain_address(value: &str) -> bool {
    let a = value.trim().to_lowercase();
    let Some(at) = a.find('@') else { return false };
    if at == 0 || at == a.len() - 1 {
        return false;
    }
    !a[at + 1..].contains('@') && !a.chars().any(address_forbidden)
}

/// `example.test` — the runtime's domain rule (no "@", a dot, no pattern characters).
pub fn is_plain_domain(value: &str) -> bool {
    let d = value.trim().to_lowercase();
    !d.is_empty()
        && !d.contains('@')
        && d.contains('.')
        && !d.starts_with('.')
        && !d.ends_with('.')
        && !d
            .chars()
            .any(|c| address_forbidden(c) || c == '/' || c == '*')
}

fn list_field(
    text: &str,
    label: &str,
    ok: fn(&str) -> bool,
    kind: &str,
    errors: &mut Vec<String>,
) -> Option<Vec<String>> {
    let items = parse_entry_list(text);
    if items.is_empty() {
        return None;
    }
    let bad: Vec<&str> = items
        .iter()
        .map(String::as_str)
        .filter(|v| !ok(v))
        .collect();
    if !bad.is_empty() {
        errors.push(format!(
            "{label}: {} {} {kind}.",
            bad.join(", "),
            if bad.len() == 1 { "is not" } else { "are not" }
        ));
    }
    if items.len() > MAX_FILTER_ENTRIES {
        errors.push(format!("{label}: at most {MAX_FILTER_ENTRIES} entries."));
    }
    Some(items)
}

// ---------------------------------------------------------------------------
// The "When an email arrives" fields
// ---------------------------------------------------------------------------

/// `any` | `yes` | `no`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Attachments {
    #[default]
    Any,
    Yes,
    No,
}

impl Attachments {
    pub fn label(self) -> &'static str {
        match self {
            Attachments::Any => HAS_ATTACHMENT_ANY,
            Attachments::Yes => HAS_ATTACHMENT_YES,
            Attachments::No => HAS_ATTACHMENT_NO,
        }
    }
    pub fn next(self) -> Attachments {
        match self {
            Attachments::Any => Attachments::Yes,
            Attachments::Yes => Attachments::No,
            Attachments::No => Attachments::Any,
        }
    }
}

/// The trigger fields, as typed. `every` / `max_batch` empty = the default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailTriggerForm {
    /// Does the target run a model on new mail? (the default interval: 1 h vs 60 s)
    pub uses_model: bool,
    /// "90m", "2h", "1d"; empty = the default for `uses_model`.
    pub every: String,
    /// A whole number; empty = 100.
    pub max_batch: String,
    pub from_in: String,
    pub from_domain_in: String,
    pub to_in: String,
    pub subject_contains: String,
    pub has_attachment: Attachments,
}

impl Default for EmailTriggerForm {
    fn default() -> Self {
        EmailTriggerForm {
            uses_model: true,
            every: String::new(),
            max_batch: String::new(),
            from_in: String::new(),
            from_domain_in: String::new(),
            to_in: String::new(),
            subject_contains: String::new(),
            has_attachment: Attachments::Any,
        }
    }
}

/// "1h" when the target runs a model, else "60s".
pub fn default_every(uses_model: bool) -> &'static str {
    if uses_model {
        DEFAULT_EVERY_MODEL
    } else {
        DEFAULT_EVERY_NO_MODEL
    }
}

/// `(amount, unit)` from "90m" / "2 h" / "1d" (minutes, hours, days — the
/// form's units); `None` when it is not a whole number of at least 1.
fn typed_interval(text: &str) -> Option<String> {
    let t: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let unit = t.chars().last()?;
    if !matches!(unit, 'm' | 'h' | 'd') {
        return None;
    }
    let digits = &t[..t.len() - 1];
    let n: u64 = digits.parse().ok()?;
    (n >= 1).then(|| format!("{n}{unit}"))
}

/// The kit's `emailTriggerConfigFrom`: explicit `uses_model`, `every`,
/// `max_batch`; `filter` only when set — or the reasons it is invalid.
pub fn email_trigger_config_from(form: &EmailTriggerForm) -> (Map<String, Value>, Vec<String>) {
    let mut errors = Vec::new();
    let mut config = Map::new();
    config.insert("uses_model".into(), json!(form.uses_model));
    if form.every.trim().is_empty() {
        config.insert("every".into(), json!(default_every(form.uses_model)));
    } else {
        match typed_interval(&form.every) {
            Some(every) => {
                config.insert("every".into(), json!(every));
            }
            None => errors.push("The check interval must be a whole number of at least 1.".into()),
        }
    }
    if let Some(every) = config.get("every").and_then(Value::as_str) {
        if crate::automations::duration_seconds(every).unwrap_or(0) < MIN_EVERY_SECONDS {
            errors.push("The check interval is at least 60 seconds.".into());
        }
    }
    if form.max_batch.trim().is_empty() {
        config.insert("max_batch".into(), json!(DEFAULT_MAX_BATCH));
    } else {
        match form.max_batch.trim().parse::<u64>() {
            Ok(n) if (1..=MAX_BATCH).contains(&n) => {
                config.insert("max_batch".into(), json!(n));
            }
            _ => errors.push(format!(
                "At most this many emails per run: a whole number from 1 to {MAX_BATCH}."
            )),
        }
    }
    let mut filter = Map::new();
    if let Some(v) = list_field(
        &form.from_in,
        FROM_IN,
        is_plain_address,
        "an email address",
        &mut errors,
    ) {
        filter.insert("from_in".into(), json!(v));
    }
    if let Some(v) = list_field(
        &form.from_domain_in,
        FROM_DOMAIN_IN,
        is_plain_domain,
        "a domain like example.com",
        &mut errors,
    ) {
        filter.insert("from_domain_in".into(), json!(v));
    }
    if let Some(v) = list_field(
        &form.to_in,
        TO_IN,
        is_plain_address,
        "an email address",
        &mut errors,
    ) {
        filter.insert("to_in".into(), json!(v));
    }
    let subject = form.subject_contains.trim();
    if !subject.is_empty() {
        if subject.encode_utf16().count() > MAX_SUBJECT_CONTAINS || subject.contains(['\r', '\n']) {
            errors.push(format!(
                "{SUBJECT_CONTAINS}: one line of at most {MAX_SUBJECT_CONTAINS} characters."
            ));
        } else {
            filter.insert("subject_contains".into(), json!(subject));
        }
    }
    match form.has_attachment {
        Attachments::Yes => {
            filter.insert("has_attachment".into(), json!(true));
        }
        Attachments::No => {
            filter.insert("has_attachment".into(), json!(false));
        }
        Attachments::Any => {}
    }
    if !filter.is_empty() {
        config.insert("filter".into(), Value::Object(filter));
    }
    (config, errors)
}

fn strs(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The kit's `emailTriggerLabel`: "when an email arrives · from a@x.test ·
/// subject contains “invoice” · checked every hour · up to 100 per run".
pub fn email_trigger_label(config: &Map<String, Value>) -> String {
    let f = config.get("filter").cloned().unwrap_or(Value::Null);
    let mut parts = vec!["when an email arrives".to_string()];
    let mut from = strs(f.get("from_in"));
    from.extend(strs(f.get("from_domain_in")));
    if !from.is_empty() {
        parts.push(format!("from {}", from.join(", ")));
    }
    let to = strs(f.get("to_in"));
    if !to.is_empty() {
        parts.push(format!("to {}", to.join(", ")));
    }
    if let Some(s) = f
        .get("subject_contains")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("subject contains “{s}”"));
    }
    match f.get("has_attachment") {
        Some(Value::Bool(true)) => parts.push("with attachments".into()),
        Some(Value::Bool(false)) => parts.push("without attachments".into()),
        _ => {}
    }
    let every = match config.get("every").and_then(Value::as_str) {
        Some(e) => e.to_string(),
        None => default_every(config.get("uses_model") != Some(&json!(false))).to_string(),
    };
    parts.push(format!(
        "checked {}",
        crate::automations::interval_label(&every)
    ));
    let batch = config
        .get("max_batch")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MAX_BATCH);
    parts.push(format!("up to {batch} per run"));
    parts.join(" · ")
}

// ---------------------------------------------------------------------------
// The Mailbox section ("Email result", recipients)
// ---------------------------------------------------------------------------

/// The recipients as the form holds them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecipientsForm {
    /// false = "Only me (default)"; true = "Me and these addresses".
    pub list: bool,
    pub addresses: String,
}

/// The kit's `emailAllowedRecipientsFrom`: `["self"]` for only me,
/// `["self", ...addresses]` for me and these addresses (at least one).
pub fn allowed_recipients_from(form: &RecipientsForm) -> (Vec<String>, Vec<String>) {
    if !form.list {
        return (vec!["self".into()], Vec::new());
    }
    let mut errors = Vec::new();
    let items: Vec<String> = parse_entry_list(&form.addresses)
        .into_iter()
        .filter(|a| a != "self")
        .collect();
    if items.is_empty() {
        errors
            .push("Name at least one address the automation may email, or choose Only me.".into());
    }
    let bad: Vec<&str> = items
        .iter()
        .map(String::as_str)
        .filter(|a| !is_plain_address(a))
        .collect();
    if !bad.is_empty() {
        errors.push(format!(
            "{RECIPIENTS_LIST}: {} {}.",
            bad.join(", "),
            if bad.len() == 1 {
                "is not an email address"
            } else {
                "are not email addresses"
            }
        ));
    }
    if items.len() + 1 > MAX_ALLOWED_RECIPIENTS {
        errors.push(format!("At most {} addresses.", MAX_ALLOWED_RECIPIENTS - 1));
    }
    let mut out = vec!["self".to_string()];
    out.extend(items);
    (out, errors)
}

/// The kit's `emailRecipientsFormFrom`: absent = only me.
pub fn recipients_form_from(list: &[String]) -> RecipientsForm {
    let extra: Vec<&str> = list
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "self")
        .collect();
    if extra.is_empty() {
        RecipientsForm::default()
    } else {
        RecipientsForm {
            list: true,
            addresses: extra.join(", "),
        }
    }
}

/// "Only me" / "Me and boss@example.test".
pub fn recipients_label(list: &[String]) -> String {
    let extra: Vec<&str> = list
        .iter()
        .map(String::as_str)
        .filter(|a| *a != "self")
        .collect();
    if extra.is_empty() {
        "Only me".into()
    } else {
        format!("Me and {}", extra.join(", "))
    }
}

/// The kit's `notifyFor`: channels `["console", "email"]` when on, the
/// default `["console"]` when off; `recipients` only beyond "self".
pub fn notify_for(email_me: bool, recipients: &[String]) -> Value {
    let mut out = json!({"channels": if email_me { json!(["console", "email"]) } else { json!(["console"]) }});
    if recipients.iter().any(|r| r != "self") {
        out["recipients"] = json!(recipients);
    }
    out
}

/// `notify.channels` has "email".
pub fn notify_emails(notify: &Value) -> bool {
    notify
        .get("channels")
        .and_then(Value::as_array)
        .is_some_and(|c| c.iter().any(|x| x == "email"))
}

/// `notify.recipients` (absent = only me).
pub fn notify_recipients(notify: &Value) -> Vec<String> {
    strs(notify.get("recipients"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONNECTED: &str = include_str!("../tests/fixtures/schedule/me_email_connected.json");
    const NOT_CONNECTED: &str =
        include_str!("../tests/fixtures/schedule/me_email_not_connected.json");
    const ADMIN_OFF: &str = include_str!("../tests/fixtures/schedule/me_email_admin_disabled.json");

    fn v(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    /// The vendored kit table (`assets/automation_controls.json` → `email`),
    /// as (key, text) pairs in the file's order.
    fn kit_pairs() -> Vec<(String, String)> {
        let src = crate::automations::AUTOMATION_CONTROLS_JSON;
        let start = src.find("\"email\": {").expect("the email block");
        let block = &src[start + "\"email\": {".len()..];
        let end = block.find('}').expect("the email block end");
        block[..end]
            .lines()
            .filter_map(|l| {
                let l = l.trim().trim_end_matches(',');
                let (k, rest) = l.split_once("\": ")?;
                let key: String = serde_json::from_str(&format!("{k}\"")).ok()?;
                let value: String = serde_json::from_str(rest).ok()?;
                Some((key, value))
            })
            .collect()
    }

    #[test]
    fn wording_table_matches_the_kit() {
        let kit = kit_pairs();
        assert_eq!(kit.len(), 25, "the kit's EMAIL_TEXT has 25 keys");
        let ours: Vec<(String, String)> = TEXT
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let diffs: Vec<String> = kit
            .iter()
            .zip(ours.iter())
            .filter(|(a, b)| a != b)
            .map(|(a, b)| format!("kit {a:?} vs terminal {b:?}"))
            .collect();
        assert!(diffs.is_empty(), "wording diffs: {diffs:#?}");
        assert_eq!(kit, ours);
        // The same table, read as JSON (byte-equal strings, no escaping slip).
        let spec: Value =
            serde_json::from_str(crate::automations::AUTOMATION_CONTROLS_JSON).unwrap();
        for (k, text) in TEXT {
            assert_eq!(spec["email"][k].as_str(), Some(*text), "{k}");
        }
    }

    #[test]
    fn email_status_gates_on_effective_enabled() {
        let on = EmailStatus::parse(&v(CONNECTED)).unwrap();
        assert!(EmailStatus::usable(Some(&on)));
        assert_eq!(setup_notice(Some(&on)), "");
        let off = EmailStatus::parse(&v(NOT_CONNECTED)).unwrap();
        assert!(!EmailStatus::usable(Some(&off)));
        assert_eq!(setup_notice(Some(&off)), NOT_SET_UP);
        let admin = EmailStatus::parse(&v(ADMIN_OFF)).unwrap();
        assert!(!EmailStatus::usable(Some(&admin)));
        assert_eq!(
            setup_notice(Some(&admin)),
            "Connect a mailbox first — open My email (Your admin turned mailboxes off for your account.)"
        );
        // Unknown (not read, the call failed) is not usable.
        assert!(!EmailStatus::usable(None));
        assert_eq!(setup_notice(None), NOT_SET_UP);
    }

    #[test]
    fn trigger_config_is_the_kits() {
        let (config, errors) = email_trigger_config_from(&EmailTriggerForm::default());
        assert!(errors.is_empty());
        assert_eq!(
            Value::Object(config.clone()),
            json!({"uses_model": true, "every": "1h", "max_batch": 100})
        );
        assert_eq!(
            email_trigger_label(&config),
            "when an email arrives · checked every hour · up to 100 per run"
        );
        let form = EmailTriggerForm {
            every: "90m".into(),
            max_batch: "20".into(),
            from_in: "Boss@Example.test, a@x.test; a@x.test".into(),
            from_domain_in: "x.test".into(),
            subject_contains: "invoice".into(),
            has_attachment: Attachments::Yes,
            ..EmailTriggerForm::default()
        };
        let (config, errors) = email_trigger_config_from(&form);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            Value::Object(config.clone()),
            json!({"uses_model": true, "every": "90m", "max_batch": 20, "filter": {
                "from_in": ["boss@example.test", "a@x.test"], "from_domain_in": ["x.test"],
                "subject_contains": "invoice", "has_attachment": true}})
        );
        assert_eq!(
            email_trigger_label(&config),
            "when an email arrives · from boss@example.test, a@x.test, x.test · subject contains “invoice” · with attachments · checked every 90 minutes · up to 20 per run"
        );
        let bad = EmailTriggerForm {
            every: "0m".into(),
            max_batch: "5000".into(),
            from_in: "nobody".into(),
            from_domain_in: "*.x.test".into(),
            ..EmailTriggerForm::default()
        };
        let (_, errors) = email_trigger_config_from(&bad);
        assert_eq!(
            errors,
            vec![
                "The check interval must be a whole number of at least 1.".to_string(),
                "At most this many emails per run: a whole number from 1 to 1000.".into(),
                "From these addresses: nobody is not an email address.".into(),
                "From these domains: *.x.test is not a domain like example.com.".into(),
            ]
        );
    }

    #[test]
    fn recipients_and_notify_are_the_kits() {
        assert_eq!(
            allowed_recipients_from(&RecipientsForm::default()),
            (vec!["self".to_string()], vec![])
        );
        let (list, errors) = allowed_recipients_from(&RecipientsForm {
            list: true,
            addresses: "boss@example.test, Team@Example.test".into(),
        });
        assert!(errors.is_empty());
        assert_eq!(list, vec!["self", "boss@example.test", "team@example.test"]);
        assert_eq!(
            notify_for(true, &list),
            json!({"channels": ["console", "email"], "recipients": ["self", "boss@example.test", "team@example.test"]})
        );
        assert_eq!(
            notify_for(true, &["self".to_string()]),
            json!({"channels": ["console", "email"]})
        );
        assert_eq!(
            notify_for(false, &["self".to_string()]),
            json!({"channels": ["console"]})
        );
        let (_, errors) = allowed_recipients_from(&RecipientsForm {
            list: true,
            addresses: "".into(),
        });
        assert_eq!(
            errors,
            vec!["Name at least one address the automation may email, or choose Only me."]
        );
        let (_, errors) = allowed_recipients_from(&RecipientsForm {
            list: true,
            addresses: "x, y@z.test".into(),
        });
        assert_eq!(
            errors,
            vec!["Me and these addresses: x is not an email address."]
        );
        assert_eq!(
            recipients_label(&list),
            "Me and boss@example.test, team@example.test"
        );
        assert_eq!(recipients_label(&[]), "Only me");
        assert_eq!(
            recipients_form_from(&list).addresses,
            "boss@example.test, team@example.test"
        );
        assert!(notify_emails(&json!({"channels": ["console", "email"]})));
        assert!(!notify_emails(&json!({"channels": ["console"]})));
    }
}
