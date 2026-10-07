//! Workspaces (round 11 model, R14.4): the kit WorkspaceChooser's rules and
//! words, in the terminal. Pure: no HTTP, no signals.
//!
//! ONE workspace model, three levels the terminal shows (the kit's fourth,
//! the gateway's eligible set, is the console's):
//!
//! - `Session` — this conversation's workspaces, stored by the gateway on
//!   the session (`GET/PUT /sessions/{id}/workspaces`); every app opening
//!   the conversation sees the same choice. `{configured:false}` = "Use my
//!   default" (the account default applies).
//! - `Account` — "My default workspaces" (`GET/PUT /workspace/policy/me`):
//!   what this account's conversations start from. `{configured:false}` =
//!   "Follow the gateway policy".
//! - `Run` — an automation definition (`target.input_data.workspace`): the
//!   same rows, nothing PUT here; the host keeps the value (`None` = "Use my
//!   default") and shows the gateway's dry run (`POST
//!   /workspace/effective/me {workspace}`) as the effective workspaces.
//!
//! The GATEWAY decides everything: no path check, no clamp, no cap
//! computation lives here. Caps come from the gateway's
//! `effective.folders[].cap`; the effective line and the "Gateway:" line
//! are the gateway's own strings, shown verbatim. A refusal is the
//! gateway's sentence + "Not saved.".
//!
//! [`TEXT`] is the kit's wording table (`WORKSPACE_CHOOSER_TEXT`, ui-kit
//! 0.8.5) byte for byte — diffed against the vendored kit block in
//! `tests/fixtures/workspaces/kit_workspace_chooser_text.ts`.

use serde_json::{json, Map, Value};

// ---------------------------------------------------------------------------
// The wording table (the kit's, verbatim)
// ---------------------------------------------------------------------------

pub const TITLE: &str = "Workspaces";
pub const GATEWAY_TITLE: &str = "Eligible workspaces";
pub const GATEWAY_HELP: &str =
    "The workspaces accounts may choose from, and the most each one allows.";
pub const ACCOUNT_HELP: &str = "The workspaces this account's agents use, among the eligible ones.";
pub const SESSION_HELP: &str = "The workspaces this conversation uses, among the eligible ones.";
pub const RUN_HELP: &str = "The workspaces this run uses, among the eligible ones.";
pub const GATEWAY_PREFIX: &str = "Gateway:";
pub const POSTURE_LABEL: &str = "Workspaces agents may use";
pub const POSTURE_ALLOWED_ONLY: &str = "Deny everything, allow listed workspaces";
pub const POSTURE_ALLOWED_ONLY_HELP: &str = "Agents may only work in the listed workspaces.";
pub const POSTURE_ANY_EXCEPT_DENIED: &str = "Allow everything, refuse listed workspaces";
pub const POSTURE_ANY_EXCEPT_DENIED_HELP: &str =
    "Agents may work in any workspace except the refused ones.";
pub const ACCESS_LABEL: &str = "Permission";
pub const ACCESS_READ: &str = "Read-only";
pub const ACCESS_READ_WRITE: &str = "Read & write";
pub const ACCESS_DENIED: &str = "Refused";
pub const CAP_READ_ONLY: &str = "The gateway allows this workspace read-only";
pub const CAP_REFUSED: &str = "The gateway refuses this workspace";
pub const EVERYTHING_ELSE: &str = "Everything else";
pub const ALLOWED_TITLE: &str = "Allowed workspaces";
pub const DENIED_TITLE: &str = "Refused workspaces";
pub const BUILTIN_REFUSED: &str = "Always refused: the gateway's own data and credentials";
pub const EMPTY_ALLOWED: &str = "No workspace is listed: agents only use their private workspace.";
pub const PRIVATE_NOTE: &str =
    "The private workspace of each run is always available, read & write.";
pub const ADD_PLACEHOLDER: &str = "Add a workspace path";
pub const ADD: &str = "Add";
pub const CHOOSE: &str = "Choose…";
pub const REMOVE: &str = "Remove";
pub const FOLLOW_GATEWAY: &str = "Follow the gateway policy";
pub const FOLLOW_GATEWAY_HELP: &str = "On: this account gets exactly what the gateway allows.";
pub const USE_DEFAULT: &str = "Use my default";
pub const USE_DEFAULT_HELP: &str = "On: the account's default workspaces apply.";
pub const LOCKED: &str = "These workspaces can be seen here but not changed.";
pub const LOADING: &str = "Loading…";
pub const SAVED: &str = "Saved";
pub const NOT_SAVED: &str = "Not saved.";

/// The kit's `WORKSPACE_CHOOSER_TEXT`, key for key, in the kit's order.
pub const TEXT: &[(&str, &str)] = &[
    ("title", TITLE),
    ("gatewayTitle", GATEWAY_TITLE),
    ("gatewayHelp", GATEWAY_HELP),
    ("accountHelp", ACCOUNT_HELP),
    ("sessionHelp", SESSION_HELP),
    ("runHelp", RUN_HELP),
    ("gatewayPrefix", GATEWAY_PREFIX),
    ("postureLabel", POSTURE_LABEL),
    ("postureAllowedOnly", POSTURE_ALLOWED_ONLY),
    ("postureAllowedOnlyHelp", POSTURE_ALLOWED_ONLY_HELP),
    ("postureAnyExceptDenied", POSTURE_ANY_EXCEPT_DENIED),
    ("postureAnyExceptDeniedHelp", POSTURE_ANY_EXCEPT_DENIED_HELP),
    ("accessLabel", ACCESS_LABEL),
    ("accessRead", ACCESS_READ),
    ("accessReadWrite", ACCESS_READ_WRITE),
    ("accessDenied", ACCESS_DENIED),
    ("capReadOnly", CAP_READ_ONLY),
    ("capRefused", CAP_REFUSED),
    ("everythingElse", EVERYTHING_ELSE),
    ("allowedTitle", ALLOWED_TITLE),
    ("deniedTitle", DENIED_TITLE),
    ("builtinRefused", BUILTIN_REFUSED),
    ("emptyAllowed", EMPTY_ALLOWED),
    ("privateNote", PRIVATE_NOTE),
    ("addPlaceholder", ADD_PLACEHOLDER),
    ("add", ADD),
    ("choose", CHOOSE),
    ("remove", REMOVE),
    ("followGateway", FOLLOW_GATEWAY),
    ("followGatewayHelp", FOLLOW_GATEWAY_HELP),
    ("useDefault", USE_DEFAULT),
    ("useDefaultHelp", USE_DEFAULT_HELP),
    ("locked", LOCKED),
    ("loading", LOADING),
    ("saved", SAVED),
    ("notSaved", NOT_SAVED),
];

/// The Code web's own sentences around the chooser (workspace_folders.tsx).
pub const DISCONNECTED: &str = "Connect to your gateway to change workspaces.";
pub const MY_DEFAULT_WORKSPACES: &str = "My default workspaces";
/// The load failure prefix (web `Could not read the workspaces: <error>`).
pub fn load_error(reason: &str) -> String {
    format!("Could not read the workspaces: {reason}")
}

const OLD_MODEL: &str =
    "The gateway answered with an older workspace model (it needs the round-11 workspace API).";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Read-only, Read & write, or Refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Rw,
    Ro,
    Deny,
}

impl Mode {
    /// The kit's order of a row's segmented control.
    pub const ALL: [Mode; 3] = [Mode::Rw, Mode::Ro, Mode::Deny];

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Rw => "rw",
            Mode::Ro => "ro",
            Mode::Deny => "deny",
        }
    }

    pub fn parse(v: &str) -> Option<Mode> {
        match v {
            "rw" => Some(Mode::Rw),
            "ro" => Some(Mode::Ro),
            "deny" => Some(Mode::Deny),
            _ => None,
        }
    }

    /// Read-only | Read & write | Refused.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Ro => ACCESS_READ,
            Mode::Deny => ACCESS_DENIED,
            Mode::Rw => ACCESS_READ_WRITE,
        }
    }

    fn order(self) -> u8 {
        match self {
            Mode::Deny => 0,
            Mode::Ro => 1,
            Mode::Rw => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Posture {
    AllowedOnly,
    AnyExceptDenied,
}

impl Posture {
    pub const ALL: [Posture; 2] = [Posture::AllowedOnly, Posture::AnyExceptDenied];

    pub fn as_str(self) -> &'static str {
        match self {
            Posture::AllowedOnly => "allowed_only",
            Posture::AnyExceptDenied => "any_except_denied",
        }
    }

    pub fn parse(v: &str) -> Option<Posture> {
        match v {
            "allowed_only" => Some(Posture::AllowedOnly),
            "any_except_denied" => Some(Posture::AnyExceptDenied),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Posture::AnyExceptDenied => POSTURE_ANY_EXCEPT_DENIED,
            Posture::AllowedOnly => POSTURE_ALLOWED_ONLY,
        }
    }

    pub fn help(self) -> &'static str {
        match self {
            Posture::AnyExceptDenied => POSTURE_ANY_EXCEPT_DENIED_HELP,
            Posture::AllowedOnly => POSTURE_ALLOWED_ONLY_HELP,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Account,
    Session,
    Run,
}

impl Level {
    pub fn help(self) -> &'static str {
        match self {
            Level::Account => ACCOUNT_HELP,
            Level::Session => SESSION_HELP,
            Level::Run => RUN_HELP,
        }
    }

    /// "Follow the gateway policy" (account) / "Use my default" (session, run).
    pub fn follow_label(self) -> &'static str {
        match self {
            Level::Account => FOLLOW_GATEWAY,
            _ => USE_DEFAULT,
        }
    }

    pub fn follow_help(self) -> &'static str {
        match self {
            Level::Account => FOLLOW_GATEWAY_HELP,
            _ => USE_DEFAULT_HELP,
        }
    }
}

/// One listed workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub path: String,
    pub mode: Mode,
}

/// The policy read at one level (`answer.policy`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// false = following the level above.
    pub configured: bool,
    pub posture: Posture,
    /// The mode of everything not listed (ro | rw).
    pub default_mode: Mode,
    pub folders: Vec<Rule>,
}

/// One effective row (server-computed, with the gateway's cap).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveRow {
    pub path: String,
    pub mode: Mode,
    pub cap: Mode,
    pub source: String,
}

/// What applies (server-computed). `summary` and `gateway_summary` are shown verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    pub posture: Posture,
    /// `None` under "Deny everything, allow listed workspaces".
    pub default_mode: Option<Mode>,
    pub folders: Vec<EffectiveRow>,
    pub summary: String,
    pub gateway_summary: String,
}

/// Everything the chooser renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub policy: Policy,
    pub effective: Effective,
    /// `Some(false)` = shown, not changeable.
    pub can_edit: Option<bool>,
}

/// The run level's value: the definition's `workspace` (`None` = "Use my default").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunValue {
    pub posture: Posture,
    pub default_mode: Mode,
    pub folders: Vec<Rule>,
}

impl RunValue {
    pub fn to_json(&self) -> Value {
        json!({
            "posture": self.posture.as_str(),
            "default_mode": self.default_mode.as_str(),
            "folders": rules_json(&self.folders),
        })
    }
}

fn rules_json(rules: &[Rule]) -> Value {
    Value::Array(
        rules
            .iter()
            .map(|r| json!({"path": r.path, "mode": r.mode.as_str()}))
            .collect(),
    )
}

/// The value's JSON (`null` = "Use my default") — the dry-run body's `workspace`.
pub fn run_value_json(value: Option<&RunValue>) -> Value {
    value.map_or(Value::Null, RunValue::to_json)
}

// ---------------------------------------------------------------------------
// Parsing the gateway's answers (fails loudly on an older model)
// ---------------------------------------------------------------------------

fn access(v: Option<&Value>) -> Option<Mode> {
    v.and_then(Value::as_str)
        .and_then(Mode::parse)
        .filter(|m| *m != Mode::Deny)
}

fn rules(v: Option<&Value>) -> Option<Vec<Rule>> {
    v?.as_array()?
        .iter()
        .map(|r| {
            Some(Rule {
                path: r.get("path")?.as_str()?.to_string(),
                mode: Mode::parse(r.get("mode")?.as_str()?)?,
            })
        })
        .collect()
}

/// An answer's `policy` (account / session level), checked.
pub fn as_policy(answer: &Value) -> Result<Policy, String> {
    let empty = Map::new();
    let p = answer
        .get("policy")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    if p.contains_key("shared_workspace") || answer.get("shared_workspace").is_some() {
        return Err(OLD_MODEL.into());
    }
    let posture = p
        .get("posture")
        .and_then(Value::as_str)
        .and_then(Posture::parse);
    let default_mode = access(p.get("default_mode"));
    let folders = rules(p.get("folders"));
    let configured = p.get("configured").and_then(Value::as_bool);
    match (posture, default_mode, folders, configured) {
        (Some(posture), Some(default_mode), Some(folders), Some(configured)) => Ok(Policy {
            configured,
            posture,
            default_mode,
            folders,
        }),
        _ => Err(format!(
            "The gateway answered without a workspace policy (policy.posture, default_mode, folders, configured). {OLD_MODEL}"
        )),
    }
}

/// An effective answer (or an answer's `effective`), checked.
pub fn as_effective(v: &Value) -> Result<Effective, String> {
    if v.get("shared_workspace").is_some() {
        return Err(OLD_MODEL.into());
    }
    let missing = || {
        format!(
            "The gateway answered without the effective workspaces (posture, default_mode, folders with cap, summary, gateway_summary). {OLD_MODEL}"
        )
    };
    let posture = v
        .get("posture")
        .and_then(Value::as_str)
        .and_then(Posture::parse)
        .ok_or_else(missing)?;
    let default_mode = match v.get("default_mode") {
        Some(Value::Null) if posture == Posture::AllowedOnly => None,
        other => Some(access(other).ok_or_else(missing)?),
    };
    let folders = v
        .get("folders")
        .and_then(Value::as_array)
        .ok_or_else(missing)?
        .iter()
        .map(|f| {
            Some(EffectiveRow {
                path: f.get("path")?.as_str()?.to_string(),
                mode: Mode::parse(f.get("mode")?.as_str()?)?,
                cap: Mode::parse(f.get("cap")?.as_str()?)?,
                source: f
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(missing)?;
    let summary = v
        .get("summary")
        .and_then(Value::as_str)
        .ok_or_else(missing)?;
    let gateway_summary = v
        .get("gateway_summary")
        .and_then(Value::as_str)
        .ok_or_else(missing)?;
    Ok(Effective {
        posture,
        default_mode,
        folders,
        summary: summary.to_string(),
        gateway_summary: gateway_summary.to_string(),
    })
}

/// A level's GET/PUT answer → the chooser state.
pub fn as_state(answer: &Value) -> Result<State, String> {
    let policy = as_policy(answer)?;
    let effective = as_effective(answer.get("effective").unwrap_or(&Value::Null))?;
    Ok(State {
        policy,
        effective,
        can_edit: answer.get("can_edit").and_then(Value::as_bool),
    })
}

/// A stored definition's `input_data.workspace`: a payload (it has `folders`)
/// or `None` — absent, or the gateway's `{configured:false}` = "Use my default".
pub fn run_value_from(input_data: &Value) -> Option<RunValue> {
    let w = input_data.get("workspace")?;
    let folders = rules(w.get("folders"))?;
    Some(RunValue {
        posture: w
            .get("posture")
            .and_then(Value::as_str)
            .and_then(Posture::parse)
            .unwrap_or(Posture::AllowedOnly),
        default_mode: access(w.get("default_mode")).unwrap_or(Mode::Rw),
        folders,
    })
}

/// The run level as a stored-level state (value `None` = following the account default).
pub fn run_state(value: Option<&RunValue>, effective: &Effective) -> State {
    let policy = match value {
        Some(v) => Policy {
            configured: true,
            posture: v.posture,
            default_mode: v.default_mode,
            folders: v.folders.clone(),
        },
        None => Policy {
            configured: false,
            posture: effective.posture,
            default_mode: effective.default_mode.unwrap_or(Mode::Rw),
            folders: Vec::new(),
        },
    };
    State {
        policy,
        effective: effective.clone(),
        can_edit: None,
    }
}

// ---------------------------------------------------------------------------
// The view (kit `workspaceChooserView`)
// ---------------------------------------------------------------------------

/// One row as shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: String,
    pub mode: Mode,
    /// The gateway's cap for this path (`effective.folders[].cap`).
    pub cap: Option<Mode>,
    /// Modes that can be picked here.
    pub allowed: Vec<Mode>,
    /// Why a mode cannot be picked (the kit tooltip).
    pub reasons: Vec<(Mode, &'static str)>,
    pub editable: bool,
}

impl Row {
    pub fn reason(&self, mode: Mode) -> Option<&'static str> {
        self.reasons
            .iter()
            .find(|(m, _)| *m == mode)
            .map(|(_, r)| *r)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub level: Level,
    pub posture: Posture,
    /// configured:false (following the level above).
    pub following: bool,
    pub rows: Vec<Row>,
    /// Posture b: (the mode of everything not listed, editable).
    pub everything_else: Option<(Mode, bool)>,
    pub can_add: bool,
    /// "Gateway: <gateway_summary>".
    pub gateway_line: String,
    /// The effective line, verbatim.
    pub summary: String,
    pub locked: bool,
}

impl View {
    pub fn allowed(&self) -> Vec<&Row> {
        self.rows.iter().filter(|r| r.mode != Mode::Deny).collect()
    }

    pub fn refused(&self) -> Vec<&Row> {
        self.rows.iter().filter(|r| r.mode == Mode::Deny).collect()
    }
}

/// The modes at or below the gateway's cap, and why the others are not offered.
pub fn modes_under_cap(cap: Option<Mode>) -> (Vec<Mode>, Vec<(Mode, &'static str)>) {
    let Some(cap) = cap else {
        return (Mode::ALL.to_vec(), Vec::new());
    };
    let reason = if cap == Mode::Deny {
        CAP_REFUSED
    } else {
        CAP_READ_ONLY
    };
    let allowed: Vec<Mode> = Mode::ALL
        .into_iter()
        .filter(|m| m.order() <= cap.order())
        .collect();
    let reasons = Mode::ALL
        .into_iter()
        .filter(|m| !allowed.contains(m))
        .map(|m| (m, reason))
        .collect();
    (allowed, reasons)
}

/// The rows and lines the chooser shows for one level.
pub fn view(level: Level, state: &State) -> View {
    let policy = &state.policy;
    let eff = &state.effective;
    let following = !policy.configured;
    let locked = state.can_edit == Some(false);
    let cap_of = |path: &str| eff.folders.iter().find(|f| f.path == path).map(|f| f.cap);
    let source: Vec<Rule> = if following {
        eff.folders
            .iter()
            .map(|f| Rule {
                path: f.path.clone(),
                mode: f.mode,
            })
            .collect()
    } else {
        policy.folders.clone()
    };
    let posture = if following {
        eff.posture
    } else {
        policy.posture
    };
    let default_mode = if following {
        eff.default_mode.unwrap_or(policy.default_mode)
    } else {
        policy.default_mode
    };
    let editable = !following && !locked;
    let rows = source
        .into_iter()
        .map(|r| {
            let cap = cap_of(&r.path);
            let (allowed, reasons) = modes_under_cap(cap);
            Row {
                path: r.path,
                mode: r.mode,
                cap,
                allowed,
                reasons,
                editable,
            }
        })
        .collect();
    View {
        level,
        posture,
        following,
        rows,
        everything_else: (posture == Posture::AnyExceptDenied).then_some((default_mode, editable)),
        can_add: editable,
        gateway_line: format!("{GATEWAY_PREFIX} {}", eff.gateway_summary),
        summary: eff.summary.clone(),
        locked,
    }
}

/// The mode a newly added workspace starts with: refused under posture b;
/// Read-only under a (never above a cap).
pub fn new_row_mode(posture: Posture) -> Mode {
    match posture {
        Posture::AnyExceptDenied => Mode::Deny,
        Posture::AllowedOnly => Mode::Ro,
    }
}

// ---------------------------------------------------------------------------
// Payloads: the ONE body every change sends (a full replacement)
// ---------------------------------------------------------------------------

fn body(posture: Posture, default_mode: Mode, folders: &[Rule]) -> Value {
    json!({
        "configured": true,
        "posture": posture.as_str(),
        "default_mode": default_mode.as_str(),
        "folders": rules_json(folders),
    })
}

/// Change one row's mode.
pub fn mode_payload(policy: &Policy, path: &str, mode: Mode) -> Value {
    let folders: Vec<Rule> = policy
        .folders
        .iter()
        .map(|r| {
            if r.path == path {
                Rule {
                    path: path.to_string(),
                    mode,
                }
            } else {
                r.clone()
            }
        })
        .collect();
    body(policy.posture, policy.default_mode, &folders)
}

/// Change the posture (rows kept).
pub fn posture_payload(policy: &Policy, posture: Posture) -> Value {
    body(posture, policy.default_mode, &policy.folders)
}

/// Change the mode of everything else (posture b).
pub fn default_mode_payload(policy: &Policy, mode: Mode) -> Value {
    body(policy.posture, mode, &policy.folders)
}

/// Add a workspace (trimmed; the gateway checks the path).
pub fn add_payload(policy: &Policy, path: &str) -> Value {
    let mut folders = policy.folders.clone();
    folders.push(Rule {
        path: path.trim().to_string(),
        mode: new_row_mode(policy.posture),
    });
    body(policy.posture, policy.default_mode, &folders)
}

/// Remove a workspace.
pub fn remove_payload(policy: &Policy, path: &str) -> Value {
    let folders: Vec<Rule> = policy
        .folders
        .iter()
        .filter(|r| r.path != path)
        .cloned()
        .collect();
    body(policy.posture, policy.default_mode, &folders)
}

/// The follow switch: ON = `{configured:false}`; OFF = start from what
/// applies now (the gateway's effective answer, verbatim) as this level's rows.
pub fn follow_payload(state: &State, follow: bool) -> Value {
    if follow {
        return json!({"configured": false});
    }
    let e = &state.effective;
    let folders: Vec<Rule> = e
        .folders
        .iter()
        .map(|f| Rule {
            path: f.path.clone(),
            mode: f.mode,
        })
        .collect();
    body(
        e.posture,
        e.default_mode.unwrap_or(state.policy.default_mode),
        &folders,
    )
}

/// A payload as the run level's next value (`{configured:false}` → `None`).
pub fn payload_run_value(payload: &Value) -> Option<RunValue> {
    if payload.get("configured").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    run_value_from(&json!({"workspace": payload}))
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

/// The gateway's sentence from an error body: `{detail: {message}}`,
/// `{detail: "<sentence>"}` or `{message}`.
pub fn error_sentence(body: &Value) -> Option<String> {
    let d = body.get("detail");
    d.and_then(|d| d.get("message"))
        .and_then(Value::as_str)
        .or_else(|| d.and_then(Value::as_str))
        .or_else(|| body.get("message").and_then(Value::as_str))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// A refusal for the row ("<gateway sentence> Not saved.").
pub fn refusal(sentence: &str) -> String {
    let s = sentence.trim();
    let s = if s.is_empty() {
        "The gateway refused the change."
    } else {
        s
    };
    let stop = if s.ends_with(['.', '!', '?']) {
        ""
    } else {
        "."
    };
    format!("{s}{stop} {NOT_SAVED}")
}

/// "Workspaces: <summary>" — an automation's one line (the summary is the gateway's).
pub fn workspaces_line(summary: &str) -> String {
    format!("{TITLE}: {summary}")
}

// ---------------------------------------------------------------------------
// What the gateway lane posts (store.workspaces)
// ---------------------------------------------------------------------------

/// The status shown under one control: "Saved" or "<sentence> Not saved.".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// Which chooser ("session:<id>", "account", "run").
    pub scope: String,
    /// Which control ("follow", "posture", "everything-else", "add", a path).
    pub key: String,
    pub text: String,
    pub error: bool,
}

/// The chooser data the terminal holds. Written only by posted closures of
/// the `gateway::workspaces` lane and by the UI thread.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WsData {
    /// `GET /sessions/{id}/workspaces`: (session id, state or load error).
    pub session: Option<(String, Result<State, String>)>,
    /// `GET /workspace/policy/me`.
    pub account: Option<Result<State, String>>,
    /// The run level's dry runs: (value JSON, effective or load error),
    /// the newest last (a few kept: a saved change and the definition that
    /// follows it read the same answer without a second request).
    pub runs: Vec<(String, Result<Effective, String>)>,
    /// The run level's value while a NEW automation is being made
    /// (`None` = "Use my default").
    pub draft: Option<RunValue>,
    /// A change in flight: (scope, key).
    pub busy: Option<(String, String)>,
    pub status: Option<Status>,
    /// Bumped when the account default changed (the session view reloads).
    pub account_tick: u64,
    /// Loads in flight (`session:<id>`, `account`, `run:<value JSON>`).
    pub loading: Vec<String>,
}

impl WsData {
    /// The dry run of a value (by its JSON), if read.
    pub fn run_for(&self, key: &str) -> Option<&Result<Effective, String>> {
        self.runs
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, r)| r)
    }

    /// Keep a dry run (the newest few).
    pub fn put_run(&mut self, key: String, answer: Result<Effective, String>) {
        self.runs.retain(|(k, _)| *k != key);
        self.runs.push((key, answer));
        if self.runs.len() > 4 {
            self.runs.remove(0);
        }
    }

    /// The status line of `key` in `scope`, if any.
    pub fn status_of(&self, scope: &str, key: &str) -> Option<&Status> {
        self.status
            .as_ref()
            .filter(|s| s.scope == scope && s.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KIT: &str = include_str!("../tests/fixtures/workspaces/kit_workspace_chooser_text.ts");
    const SESSION_DEFAULT: &str =
        include_str!("../tests/fixtures/workspaces/session_get_default.json");
    const SESSION_CONFIGURED: &str =
        include_str!("../tests/fixtures/workspaces/session_get_configured.json");
    const ABOVE_CAP: &str = include_str!("../tests/fixtures/workspaces/session_put_above_cap.json");
    const OUTSIDE: &str =
        include_str!("../tests/fixtures/workspaces/session_put_refused_path.json");
    const DRY_PAYLOAD: &str = include_str!("../tests/fixtures/workspaces/dryrun_payload.json");
    const DRY_DEFAULT: &str = include_str!("../tests/fixtures/workspaces/dryrun_default.json");
    const ACCOUNT: &str = include_str!("../tests/fixtures/workspaces/account_get.json");

    fn v(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    /// The kit block parsed as (key, value) pairs, in order.
    fn kit_pairs() -> Vec<(String, String)> {
        let start = KIT.find("WORKSPACE_CHOOSER_TEXT = {").expect("kit block");
        let block = &KIT[start..];
        let end = block.find("} as const;").expect("kit block end");
        block[..end]
            .lines()
            .skip(1)
            .filter_map(|l| {
                let l = l.trim();
                let (k, rest) = l.split_once(':')?;
                let rest = rest.trim().strip_suffix(',').unwrap_or(rest.trim());
                let value: String = serde_json::from_str(rest).ok()?;
                Some((k.trim().to_string(), value))
            })
            .collect()
    }

    #[test]
    fn wording_table_matches_the_kit() {
        let kit = kit_pairs();
        assert_eq!(kit.len(), 36, "the kit table has 36 keys");
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
        // Vocabulary: workspaces, never folders; no shared workspace.
        for (_, text) in TEXT {
            assert!(!text.to_lowercase().contains("folder"), "{text}");
            assert!(!text.to_lowercase().contains("shared"), "{text}");
        }
    }

    #[test]
    fn a_session_following_the_default_shows_what_applies_read_only() {
        let s = as_state(&v(SESSION_DEFAULT)).unwrap();
        assert!(!s.policy.configured);
        let view = view(Level::Session, &s);
        assert!(view.following);
        assert!(!view.can_add);
        assert_eq!(view.rows.len(), 3);
        assert!(view.rows.iter().all(|r| !r.editable));
        assert!(view
            .gateway_line
            .starts_with("Gateway: Allow everything, refuse listed workspaces (rw) · "));
        assert_eq!(view.summary, s.effective.summary);
        assert_eq!(view.everything_else, Some((Mode::Rw, false)));
    }

    #[test]
    fn a_configured_session_has_editable_rows_with_the_gateway_caps() {
        let s = as_state(&v(SESSION_CONFIGURED)).unwrap();
        let view = view(Level::Session, &s);
        assert!(!view.following);
        assert_eq!(view.posture, Posture::AllowedOnly);
        assert!(view.everything_else.is_none());
        let pictures = view
            .rows
            .iter()
            .find(|r| r.path.ends_with("/Pictures"))
            .unwrap();
        assert_eq!(pictures.cap, Some(Mode::Ro));
        assert_eq!(pictures.allowed, vec![Mode::Ro, Mode::Deny]);
        assert_eq!(pictures.reason(Mode::Rw), Some(CAP_READ_ONLY));
        let project = view
            .rows
            .iter()
            .find(|r| r.path.ends_with("/project"))
            .unwrap();
        assert_eq!(project.allowed, Mode::ALL.to_vec());
        assert!(view
            .summary
            .starts_with("Deny everything, allow listed workspaces · "));
    }

    #[test]
    fn payloads_are_full_replacements() {
        let s = as_state(&v(SESSION_CONFIGURED)).unwrap();
        let p = mode_payload(&s.policy, &s.policy.folders[1].path, Mode::Ro);
        assert_eq!(p["configured"], json!(true));
        assert_eq!(p["posture"], json!("allowed_only"));
        assert_eq!(p["folders"].as_array().unwrap().len(), 2);
        assert_eq!(p["folders"][1]["mode"], json!("ro"));
        let added = add_payload(&s.policy, "  /tmp/x  ");
        assert_eq!(added["folders"][2], json!({"path": "/tmp/x", "mode": "ro"}));
        let removed = remove_payload(&s.policy, &s.policy.folders[0].path);
        assert_eq!(removed["folders"].as_array().unwrap().len(), 1);
        let posture = posture_payload(&s.policy, Posture::AnyExceptDenied);
        assert_eq!(posture["posture"], json!("any_except_denied"));
        let added_b = add_payload(
            &Policy {
                posture: Posture::AnyExceptDenied,
                ..s.policy.clone()
            },
            "/tmp/y",
        );
        assert_eq!(added_b["folders"][2]["mode"], json!("deny"));
        assert_eq!(follow_payload(&s, true), json!({"configured": false}));
        let d = as_state(&v(SESSION_DEFAULT)).unwrap();
        let off = follow_payload(&d, false);
        assert_eq!(off["configured"], json!(true));
        assert_eq!(off["posture"], json!("any_except_denied"));
        assert_eq!(off["folders"].as_array().unwrap().len(), 3);
        assert_eq!(payload_run_value(&json!({"configured": false})), None);
        assert_eq!(payload_run_value(&off).unwrap().folders.len(), 3);
    }

    #[test]
    fn refusals_are_the_gateway_sentence_then_not_saved() {
        let above = error_sentence(&v(ABOVE_CAP)).unwrap();
        assert!(above.starts_with("The gateway allows this workspace read-only: "));
        assert!(refusal(&above).ends_with(". Not saved."));
        assert!(!refusal(&above).contains(".. "));
        let outside = error_sentence(&v(OUTSIDE)).unwrap();
        assert!(outside.contains("is outside the workspaces the gateway allows"));
        assert_eq!(refusal("No"), "No. Not saved.");
        assert_eq!(refusal(""), "The gateway refused the change. Not saved.");
        assert_eq!(
            error_sentence(&json!({"detail": "Only admins."})).unwrap(),
            "Only admins."
        );
    }

    #[test]
    fn the_run_level_reads_the_dry_run() {
        let e = as_effective(&v(DRY_PAYLOAD)).unwrap();
        assert_eq!(e.posture, Posture::AllowedOnly);
        let value = RunValue {
            posture: Posture::AllowedOnly,
            default_mode: Mode::Rw,
            folders: vec![Rule {
                path: e.folders[0].path.clone(),
                mode: Mode::Ro,
            }],
        };
        let state = run_state(Some(&value), &e);
        let view = view(Level::Run, &state);
        assert!(!view.following);
        assert_eq!(view.rows.len(), 1);
        let d = as_effective(&v(DRY_DEFAULT)).unwrap();
        let following = view_of_default(&d);
        assert!(following.following);
        assert_eq!(run_value_json(None), Value::Null);
        assert_eq!(
            run_value_from(&json!({"workspace": {"configured": false}})),
            None,
            "the gateway's follow marker is Use my default"
        );
        assert_eq!(
            run_value_from(&json!({"workspace": value.to_json()})),
            Some(value)
        );
    }

    fn view_of_default(e: &Effective) -> View {
        view(Level::Run, &run_state(None, e))
    }

    #[test]
    fn the_account_level_follows_the_gateway() {
        let s = as_state(&v(ACCOUNT)).unwrap();
        assert!(!s.policy.configured);
        assert_eq!(Level::Account.follow_label(), FOLLOW_GATEWAY);
        assert_eq!(Level::Session.follow_label(), USE_DEFAULT);
    }

    #[test]
    fn an_older_model_fails_loudly() {
        let old = json!({"policy": {"shared_workspace": "/x", "posture": "allowed_only"}});
        assert!(as_policy(&old)
            .unwrap_err()
            .contains("older workspace model"));
        let no_cap = json!({"posture": "allowed_only", "default_mode": null,
            "folders": [{"path": "/a", "mode": "ro"}], "summary": "s", "gateway_summary": "g"});
        assert!(as_effective(&no_cap).is_err());
        let no_summary = json!({"posture": "allowed_only", "default_mode": null, "folders": [], "gateway_summary": "g"});
        assert!(as_effective(&no_summary).is_err());
    }

    #[test]
    fn caps_disable_the_modes_above_them() {
        assert_eq!(modes_under_cap(None).0, Mode::ALL.to_vec());
        let (allowed, reasons) = modes_under_cap(Some(Mode::Deny));
        assert_eq!(allowed, vec![Mode::Deny]);
        assert_eq!(
            reasons,
            vec![(Mode::Rw, CAP_REFUSED), (Mode::Ro, CAP_REFUSED)]
        );
    }
}
