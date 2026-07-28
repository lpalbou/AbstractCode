//! Connection + preference resolution.
//!
//! Connection precedence (one resolution point, mirrored from the Python
//! `abstractcode` CLI so both clients share one mental model AND one login
//! store): explicit flag > env > login store > default. Env beats the store
//! deliberately (unix convention); `abstractcode-tui doctor` prints WHICH
//! source won, which is the antidote to silently-swapped principals from a
//! stale export.

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

pub const DEFAULT_GATEWAY_URL: &str = "http://127.0.0.1:8080";

/// A resolved value plus the human label of the source that provided it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub value: String,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct Connection {
    pub base_url: String,
    pub token: Option<String>,
}

fn trimmed_env(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(v) => {
            let t = v.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        }
        Err(_) => None,
    }
}

/// The login store shared with the Python `abstractcode` CLI
/// (`~/.abstractcode/gateway.json`, written by `abstractcode login` or
/// `abstractcode-tui login`).
pub fn login_store_path() -> PathBuf {
    if let Some(p) = trimmed_env("ABSTRACTCODE_GATEWAY_CONNECTION_FILE") {
        return PathBuf::from(p);
    }
    home_dir().join(".abstractcode").join("gateway.json")
}

/// Preferences owned by this app (theme, workflow, model, session).
pub fn prefs_path() -> PathBuf {
    if let Some(p) = trimmed_env("ABSTRACTCODE_TUI_PREFS_FILE") {
        return PathBuf::from(p);
    }
    home_dir().join(".abstractcode-tui").join("prefs.json")
}

pub fn home_dir() -> PathBuf {
    if let Some(h) = trimmed_env("HOME") {
        return PathBuf::from(h);
    }
    #[cfg(windows)]
    if let Some(h) = trimmed_env("USERPROFILE") {
        return PathBuf::from(h);
    }
    PathBuf::from(".")
}

fn read_json_file(path: &PathBuf) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&raw).ok()
}

fn store_string(store: &Option<Value>, key: &str) -> Option<String> {
    let v = store.as_ref()?.get(key)?.as_str()?.trim().to_string();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// Resolve the gateway base URL -> (url, source label).
pub fn resolve_gateway_url(explicit: Option<&str>) -> Resolved {
    if let Some(e) = explicit {
        let t = e.trim().trim_end_matches('/');
        if !t.is_empty() {
            return Resolved {
                value: t.to_string(),
                source: "flag".into(),
            };
        }
    }
    for name in [
        "ABSTRACTCODE_GATEWAY_URL",
        "ABSTRACTFLOW_GATEWAY_URL",
        "ABSTRACTGATEWAY_URL",
    ] {
        if let Some(v) = trimmed_env(name) {
            return Resolved {
                value: v.trim_end_matches('/').to_string(),
                source: format!("env {name}"),
            };
        }
    }
    let store = read_json_file(&login_store_path());
    if let Some(v) = store_string(&store, "base_url") {
        return Resolved {
            value: v.trim_end_matches('/').to_string(),
            source: format!("login ({})", login_store_path().display()),
        };
    }
    Resolved {
        value: DEFAULT_GATEWAY_URL.into(),
        source: "default".into(),
    }
}

/// Resolve the gateway auth token -> (token or None, source label).
pub fn resolve_gateway_token(explicit: Option<&str>) -> (Option<String>, String) {
    if let Some(e) = explicit {
        let t = e.trim();
        if !t.is_empty() {
            return (Some(t.to_string()), "flag".into());
        }
    }
    for name in [
        "ABSTRACTCODE_GATEWAY_TOKEN",
        "ABSTRACTGATEWAY_AUTH_TOKEN",
        "ABSTRACTFLOW_GATEWAY_AUTH_TOKEN",
    ] {
        if let Some(v) = trimmed_env(name) {
            return (Some(v), format!("env {name}"));
        }
    }
    let store = read_json_file(&login_store_path());
    if let Some(v) = store_string(&store, "token") {
        return (Some(v), format!("login ({})", login_store_path().display()));
    }
    (None, "none".into())
}

pub fn resolve_connection(url_flag: Option<&str>, token_flag: Option<&str>) -> Connection {
    let url = resolve_gateway_url(url_flag);
    let (token, _token_source) = resolve_gateway_token(token_flag);
    Connection {
        base_url: url.value,
        token,
    }
}

/// Persist a verified login to the shared store (0600: the token is a
/// credential at rest).
pub fn write_login(base_url: &str, token: Option<&str>) -> std::io::Result<PathBuf> {
    let path = login_store_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let payload = json!({
        "base_url": base_url.trim().trim_end_matches('/'),
        "token": token.map(|t| t.trim()).filter(|t| !t.is_empty()),
        "verified_at": now_iso_utc(),
    });
    fs::write(
        &path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        ),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(path)
}

pub fn now_iso_utc() -> String {
    // Seconds-precision ISO timestamp without pulling a time crate: derive
    // the civil date from the unix epoch (proleptic Gregorian).
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    let rem = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Howard Hinnant's `civil_from_days` (public domain algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---------------------------------------------------------------------------
// Preferences (theme, workflow, provider/model, session, tools, skills)
// ---------------------------------------------------------------------------

/// One remembered session for the `/sessions` picker.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionEntry {
    pub id: String,
    /// First prompt of the session (its human name in the picker).
    pub label: String,
    /// ISO timestamp of the last time this session was active.
    pub last_used: String,
}

pub const RECENT_SESSIONS_MAX: usize = 15;

/// A session's remembered tools-modal configuration (operator ask
/// 2026-07-23: "those preferences should be sticky per session"). Mirrors
/// the `session_queues`/`session_goals` slot pattern — one slot per
/// session id, recency-ordered, capped. A session with no slot inherits
/// the global baseline (the top-level `tool_*`/`disabled_tools` fields)
/// on first use and then diverges independently.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionToolPrefs {
    /// The run's exact disabled tool names (empty = untouched = the
    /// workflow's own tool set decides).
    pub disabled_tools: Vec<String>,
    /// Per-tool approval pins `(name, "auto"|"ask")`.
    pub tool_overrides: Vec<(String, String)>,
    /// The accepted approval tier for this session.
    pub accepted_tier: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Prefs {
    pub theme: Option<String>,
    pub bundle_id: Option<String>,
    pub flow_id: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Reasoning effort override (first-citizen directive). Persisted as
    /// the TRIPLE (reasoning + the provider/model it was chosen under):
    /// a saved effort applies only under its own route — on load, a
    /// provider/model mismatch DROPS the reasoning (the coupling rule:
    /// "selecting a model enables picking the effort"; a stale effort
    /// riding onto a different model is the fabricated-selection class).
    pub reasoning: Option<String>,
    pub reasoning_provider: Option<String>,
    pub reasoning_model: Option<String>,
    pub session_id: Option<String>,
    pub workspace_mode: Option<String>,
    /// Extra allowlisted root directories sent as
    /// `workspace_allowed_paths` with every run (used by the gateway in
    /// `workspace_or_allowed` mode; server policy may clamp them).
    pub workspace_allowed: Vec<String>,
    pub show_details: Option<bool>,
    /// PERSISTED permissions level (`tool_approval.accepted_tier` — the
    /// at-rest key deliberately keeps the pre-consolidation spelling: it
    /// is documented hand-editable for headless, and renaming the JSON
    /// key would break existing files for zero gain). Batches whose
    /// every call classifies at-or-below it auto-approve. One of
    /// "read" | "write" | "all"; empty/unknown reads as "read" (the
    /// strictest — see `tool_policy::Tier::parse_or_default`). Survives
    /// restarts BY DESIGN: a graded posture, not a blanket (the
    /// session-scoped /auto blanket is deleted — c5028).
    pub tool_accepted_tier: String,
    /// Per-tool pins (`tool_approval.overrides`): name → "auto" | "ask".
    /// A pin beats the tier in both directions (an explicit user act).
    pub tool_overrides: Vec<(String, String)>,
    /// Gateway tools the user switched OFF (`/tools`). A disabled-list (not
    /// an enabled-list) so newly published gateway tools default to ON.
    pub disabled_tools: Vec<String>,
    /// Gateway skills attached to every run (`/skills` -> input_data.skills).
    pub skills: Vec<String>,
    /// Recently used sessions, newest first (the `/sessions` picker).
    pub recent_sessions: Vec<SessionEntry>,
    /// Per-session prompt-queue stash, most-recently-written first
    /// (`session_queues`): the `/queue` FIFO persists keyed by session id
    /// (the `touch_session` slot pattern) and RESTORES PAUSED — it never
    /// auto-starts on any restore. Empty queues drop their slot; slots cap
    /// at `RECENT_SESSIONS_MAX`.
    pub session_queues: Vec<(String, Vec<String>)>,
    /// Per-session active `/goal`, most-recently-written first:
    /// `(session_id, (goal_text, run_id))`. The run id lets a restart
    /// restore `finish_on_root_only` when it reattaches to a live goal
    /// run; the text labels the strip + `/goal` status.
    pub session_goals: Vec<(String, (String, String))>,
    /// Per-session tools-modal config, most-recently-written first
    /// (`session_tool_prefs`): the disabled set + approval pins + tier
    /// persist keyed by session id (same slot discipline as queues/goals),
    /// so each session remembers its own tool activation independently.
    pub session_tool_prefs: Vec<(String, SessionToolPrefs)>,
    /// `/goal` cycle budget sent as `input_data.max_cycles`
    /// (`goal_max_cycles`; 0 = unset, resolved to the default 8).
    pub goal_max_cycles: u32,
    /// OPERATOR-DECLARED model context window in tokens (CTX-0). 0 =
    /// not declared. Set by `/context <tokens>` (persisted) or
    /// `--max-tokens` (session only); drives the `ctx N/M (P%)` meter
    /// and rides runs as `_limits.max_tokens`. Always source-labeled
    /// "declared" — never a client-shipped capability table (the
    /// 2026-07-17 fabricated-selection class is the hard line).
    pub context_window: u64,
    /// Where this Prefs persists. `None` = EPHEMERAL: `save()` is a no-op.
    /// Default-constructed prefs never touch the filesystem — a test
    /// harness building a UiCtx cannot pollute the operator's real file
    /// (live incident 2026-07-21: `cargo test` overwrote the user's saved
    /// theme/model through the default path).
    pub path: Option<PathBuf>,
}

impl Prefs {
    pub fn load() -> Prefs {
        Prefs::load_from(prefs_path())
    }

    pub fn load_from(path: PathBuf) -> Prefs {
        let v = read_json_file(&path).unwrap_or(Value::Null);
        let s = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(str::to_string)
        };
        let string_list = |k: &str| -> Vec<String> {
            v.get(k)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|x| !x.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default()
        };
        let mut seen_ids = std::collections::HashSet::new();
        let sessions = v
            .get("recent_sessions")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        let id = e.get("id")?.as_str()?.trim().to_string();
                        // First occurrence wins; duplicates (hand-edits,
                        // merge bugs) must not survive into the picker.
                        if id.is_empty() || !seen_ids.insert(id.clone()) {
                            return None;
                        }
                        Some(SessionEntry {
                            id,
                            label: e
                                .get("label")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .trim()
                                .to_string(),
                            last_used: e
                                .get("last_used")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .trim()
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        // tool_approval: {accepted_tier, overrides:{name: "auto"|"ask"}}.
        // Unknown tier spellings normalize to the STRICTEST ("read") at
        // load — a typo in a hand-edited prefs.json must never widen
        // what auto-runs. Unknown override decisions are dropped.
        let approval = v.get("tool_approval");
        let tool_accepted_tier = approval
            .and_then(|a| a.get("accepted_tier"))
            .and_then(Value::as_str)
            .map(|raw| {
                crate::tool_policy::Tier::parse_or_default(raw)
                    .label()
                    .to_string()
            })
            .unwrap_or_default();
        let tool_overrides: Vec<(String, String)> = approval
            .and_then(|a| a.get("overrides"))
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .filter_map(|(name, decision)| {
                        let d = decision.as_str()?.trim().to_lowercase();
                        let name = name.trim();
                        if name.is_empty() || !matches!(d.as_str(), "auto" | "ask") {
                            return None;
                        }
                        Some((name.to_string(), d))
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Per-session queue stash: `[{"id": sid, "prompts": [...]}, ...]`
        // (a LIST, not an object — recency order is load-bearing and
        // serde_json objects sort keys). Blank ids/prompts drop at load.
        let mut seen_queue_ids = std::collections::HashSet::new();
        let session_queues: Vec<(String, Vec<String>)> = v
            .get("session_queues")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        let id = e.get("id")?.as_str()?.trim().to_string();
                        if id.is_empty() || !seen_queue_ids.insert(id.clone()) {
                            return None;
                        }
                        let prompts: Vec<String> = e
                            .get("prompts")?
                            .as_array()?
                            .iter()
                            .filter_map(Value::as_str)
                            .filter(|p| !p.trim().is_empty())
                            .map(str::to_string)
                            .collect();
                        if prompts.is_empty() {
                            return None; // an empty stash carries nothing
                        }
                        Some((id, prompts))
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Per-session goals: `[{"id": sid, "text": ..., "run_id": ...}]`.
        let mut seen_goal_ids = std::collections::HashSet::new();
        let session_goals: Vec<(String, (String, String))> = v
            .get("session_goals")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        let id = e.get("id")?.as_str()?.trim().to_string();
                        let text = e.get("text")?.as_str()?.trim().to_string();
                        if id.is_empty() || text.is_empty() || !seen_goal_ids.insert(id.clone()) {
                            return None;
                        }
                        let run_id = e
                            .get("run_id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string();
                        Some((id, (text, run_id)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Per-session tool prefs: `[{"id": sid, "disabled_tools": [...],
        // "overrides": {name: decision}, "accepted_tier": "..."}]`.
        let mut seen_tool_ids = std::collections::HashSet::new();
        let session_tool_prefs: Vec<(String, SessionToolPrefs)> = v
            .get("session_tool_prefs")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        let id = e.get("id")?.as_str()?.trim().to_string();
                        if id.is_empty() || !seen_tool_ids.insert(id.clone()) {
                            return None;
                        }
                        let disabled_tools: Vec<String> = e
                            .get("disabled_tools")
                            .and_then(Value::as_array)
                            .map(|xs| {
                                xs.iter()
                                    .filter_map(Value::as_str)
                                    .map(str::to_string)
                                    .collect()
                            })
                            .unwrap_or_default();
                        let tool_overrides: Vec<(String, String)> = e
                            .get("overrides")
                            .and_then(Value::as_object)
                            .map(|m| {
                                m.iter()
                                    .filter_map(|(k, val)| {
                                        Some((k.clone(), val.as_str()?.to_string()))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let accepted_tier = e
                            .get("accepted_tier")
                            .and_then(Value::as_str)
                            .map(|s| {
                                crate::tool_policy::Tier::parse_or_default(s)
                                    .label()
                                    .to_string()
                            })
                            .unwrap_or_default();
                        Some((
                            id,
                            SessionToolPrefs {
                                disabled_tools,
                                tool_overrides,
                                accepted_tier,
                            },
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Prefs {
            theme: s("theme"),
            bundle_id: s("bundle_id"),
            flow_id: s("flow_id"),
            provider: s("provider"),
            model: s("model"),
            reasoning: s("reasoning"),
            reasoning_provider: s("reasoning_provider"),
            reasoning_model: s("reasoning_model"),
            session_id: s("session_id"),
            workspace_mode: s("workspace_mode"),
            workspace_allowed: string_list("workspace_allowed"),
            show_details: v.get("show_details").and_then(Value::as_bool),
            tool_accepted_tier,
            tool_overrides,
            disabled_tools: string_list("disabled_tools"),
            skills: string_list("skills"),
            session_tool_prefs,
            recent_sessions: sessions,
            session_queues,
            session_goals,
            goal_max_cycles: v
                .get("goal_max_cycles")
                .and_then(Value::as_u64)
                .map(|n| n.min(u32::MAX as u64) as u32)
                .unwrap_or(0),
            // Same range rule as the declaration surfaces (`/context`,
            // `--max-tokens` → `parse_token_count`: 1..=1e12): a hand-
            // edited out-of-range value reads as UNSET, matching this
            // file's load posture (malformed fails toward defaults) —
            // it must not declare a window the command surface refuses
            // (cycle-2 review P2-G).
            context_window: v
                .get("context_window")
                .and_then(Value::as_u64)
                .filter(|n| (1..=1_000_000_000_000).contains(n))
                .unwrap_or(0),
            path: Some(path),
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(path) = self.path.as_ref() else {
            return Ok(()); // ephemeral prefs (tests, default-constructed)
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let payload = json!({
            "theme": self.theme,
            "bundle_id": self.bundle_id,
            "flow_id": self.flow_id,
            "provider": self.provider,
            "model": self.model,
            "reasoning": self.reasoning,
            "reasoning_provider": self.reasoning_provider,
            "reasoning_model": self.reasoning_model,
            "session_id": self.session_id,
            "workspace_mode": self.workspace_mode,
            "workspace_allowed": self.workspace_allowed,
            "show_details": self.show_details,
            // Always written normalized + legible: headless users edit
            // this by hand (config-first for exec runs).
            "tool_approval": {
                "accepted_tier": crate::tool_policy::Tier::parse_or_default(
                    &self.tool_accepted_tier
                )
                .label(),
                "overrides": self
                    .tool_overrides
                    .iter()
                    .map(|(name, decision)| (name.clone(), json!(decision)))
                    .collect::<serde_json::Map<String, Value>>(),
            },
            "disabled_tools": self.disabled_tools,
            "skills": self.skills,
            "recent_sessions": self.recent_sessions.iter().map(|e| json!({
                "id": e.id,
                "label": e.label,
                "last_used": e.last_used,
            })).collect::<Vec<_>>(),
            // Queue stash + goals persist as LISTS (recency order matters;
            // JSON objects would sort keys at rest).
            "session_queues": self.session_queues.iter().map(|(id, prompts)| json!({
                "id": id,
                "prompts": prompts,
            })).collect::<Vec<_>>(),
            "session_goals": self.session_goals.iter().map(|(id, (text, run_id))| json!({
                "id": id,
                "text": text,
                "run_id": run_id,
            })).collect::<Vec<_>>(),
            "session_tool_prefs": self.session_tool_prefs.iter().map(|(id, tp)| json!({
                "id": id,
                "disabled_tools": tp.disabled_tools,
                "overrides": tp
                    .tool_overrides
                    .iter()
                    .map(|(name, decision)| (name.clone(), json!(decision)))
                    .collect::<serde_json::Map<String, Value>>(),
                "accepted_tier": crate::tool_policy::Tier::parse_or_default(&tp.accepted_tier).label(),
            })).collect::<Vec<_>>(),
            "goal_max_cycles": self.goal_max_cycles,
            "context_window": self.context_window,
        });
        let body = format!(
            "{}\n",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
        // Atomic replace: a direct overwrite can be torn by a crash or a
        // concurrent instance; a torn read parses as null -> all defaults ->
        // a fresh session id overwrites the operator's continuity.
        let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
        fs::write(&tmp, body)?;
        fs::rename(&tmp, path)
    }

    /// Record a session as just-used: upsert to the front, keep the newest
    /// label when one exists (the first prompt names a session for life),
    /// cap the list.
    pub fn touch_session(&mut self, id: &str, label: Option<&str>) {
        let id = id.trim();
        if id.is_empty() {
            return;
        }
        let mut entry = SessionEntry {
            id: id.to_string(),
            label: String::new(),
            last_used: now_iso_utc(),
        };
        if let Some(pos) = self.recent_sessions.iter().position(|e| e.id == id) {
            entry.label = self.recent_sessions.remove(pos).label;
        }
        if let Some(l) = label {
            let l = l.trim();
            if !l.is_empty() && entry.label.is_empty() {
                entry.label = l.chars().take(60).collect();
            }
        }
        self.recent_sessions.insert(0, entry);
        self.recent_sessions.truncate(RECENT_SESSIONS_MAX);
    }

    /// The stashed prompt queue for a session (empty = no stash).
    pub fn session_queue(&self, id: &str) -> Vec<String> {
        self.session_queues
            .iter()
            .find(|(sid, _)| sid == id)
            .map(|(_, q)| q.clone())
            .unwrap_or_default()
    }

    /// Write-through slot for a session's queue: upsert to front (the
    /// `touch_session` pattern), remove the slot when the queue empties,
    /// cap slot count so abandoned sessions never grow the file forever.
    pub fn set_session_queue(&mut self, id: &str, prompts: &[String]) {
        let id = id.trim();
        if id.is_empty() {
            return;
        }
        self.session_queues.retain(|(sid, _)| sid != id);
        if !prompts.is_empty() {
            self.session_queues
                .insert(0, (id.to_string(), prompts.to_vec()));
            self.session_queues.truncate(RECENT_SESSIONS_MAX);
        }
    }

    /// The persisted goal for a session: `(text, run_id)`.
    pub fn session_goal(&self, id: &str) -> Option<(String, String)> {
        self.session_goals
            .iter()
            .find(|(sid, _)| sid == id)
            .map(|(_, g)| g.clone())
    }

    /// Set/clear a session's goal slot (same slot discipline as queues).
    pub fn set_session_goal(&mut self, id: &str, goal: Option<(String, String)>) {
        let id = id.trim();
        if id.is_empty() {
            return;
        }
        self.session_goals.retain(|(sid, _)| sid != id);
        if let Some((text, run_id)) = goal {
            if !text.trim().is_empty() {
                self.session_goals
                    .insert(0, (id.to_string(), (text, run_id)));
                self.session_goals.truncate(RECENT_SESSIONS_MAX);
            }
        }
    }

    /// A session's remembered tools-modal config (`None` = never touched;
    /// the caller seeds from the global baseline).
    pub fn session_tool_prefs(&self, id: &str) -> Option<SessionToolPrefs> {
        self.session_tool_prefs
            .iter()
            .find(|(sid, _)| sid == id)
            .map(|(_, tp)| tp.clone())
    }

    /// Write-through slot for a session's tools-modal config (same slot
    /// discipline as queues/goals: upsert to front, cap slot count). An
    /// all-empty/default slot still persists — an explicit "all off with
    /// tier read" is a real per-session choice, distinct from "never
    /// touched" (no slot).
    pub fn set_session_tool_prefs(&mut self, id: &str, tp: &SessionToolPrefs) {
        let id = id.trim();
        if id.is_empty() {
            return;
        }
        self.session_tool_prefs.retain(|(sid, _)| sid != id);
        self.session_tool_prefs
            .insert(0, (id.to_string(), tp.clone()));
        self.session_tool_prefs.truncate(RECENT_SESSIONS_MAX);
    }

    /// The `/goal` cycle budget: the persisted value, else the default 8
    /// (the plan's pref default — hand-editable as `goal_max_cycles`).
    pub fn goal_cycles(&self) -> u32 {
        if self.goal_max_cycles == 0 {
            8
        } else {
            self.goal_max_cycles
        }
    }
}

/// Parse a human token count: `262144`, `262k`/`262K`, `1m`/`1M` (and
/// `1.5m`). Returns `None` for anything else — `/context` refuses loudly
/// instead of guessing. Suffix math is decimal (k = 1 000) because that
/// is how model windows are spoken ("128k", "262k" = 262 144 is the
/// binary spelling the OPERATOR types digits for if they mean it).
pub fn parse_token_count(raw: &str) -> Option<u64> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    let (num, mult) = match t.chars().last() {
        Some('k') | Some('K') => (&t[..t.len() - 1], 1_000f64),
        Some('m') | Some('M') => (&t[..t.len() - 1], 1_000_000f64),
        _ => (t, 1f64),
    };
    let n: f64 = num.trim().replace('_', "").parse().ok()?;
    if !n.is_finite() || n <= 0.0 {
        return None;
    }
    let v = (n * mult).round();
    if !(1.0..=1e12).contains(&v) {
        return None;
    }
    Some(v as u64)
}

/// Reasoning effort ladder accepted on the wire (contract v1, plan v13):
/// `none|minimal|low|medium|high|xhigh` + the passthroughs `auto`/`on`.
/// Parse-time validation refuses anything else loudly — an unknown value
/// must die at the flag, never ride to a provider ValueError mid-run.
pub const REASONING_LEVELS: &[&str] = &[
    "none", "minimal", "low", "medium", "high", "xhigh", "auto", "on",
];

pub fn valid_reasoning_level(v: &str) -> bool {
    REASONING_LEVELS.contains(&v.trim().to_ascii_lowercase().as_str())
}

/// The pair-coupled reasoning load: a persisted effort applies ONLY under
/// the provider/model it was saved with (coupling rule — a model change
/// resets the override). Returns "" when absent or mismatched.
pub fn coupled_reasoning(prefs: &Prefs, provider: &str, model: &str) -> String {
    let r = prefs.reasoning.clone().unwrap_or_default();
    if r.trim().is_empty() {
        return String::new();
    }
    let rp = prefs.reasoning_provider.clone().unwrap_or_default();
    let rm = prefs.reasoning_model.clone().unwrap_or_default();
    if rp == provider && rm == model {
        r.trim().to_ascii_lowercase()
    } else {
        String::new()
    }
}

/// Mint a session id: `acode-<12 hex>` hashed from time + pid + ASLR noise
/// (uniqueness, not cryptographic randomness — session ids are not secrets).
pub fn mint_session_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let addr = {
        let x = 0u8;
        std::ptr::addr_of!(x) as usize as u128
    };
    let mut h: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    for byte in nanos
        .to_le_bytes()
        .iter()
        .chain(pid.to_le_bytes().iter())
        .chain(addr.to_le_bytes().iter())
    {
        h ^= *byte as u128;
        h = h.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
    }
    format!("acode-{:012x}", (h >> 32) as u64 & 0xffff_ffff_ffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_ladder_and_pair_coupled_load() {
        // Contract v1 ladder + passthroughs; junk refused.
        for v in [
            "none", "minimal", "low", "medium", "high", "xhigh", "auto", "on", " High ",
        ] {
            assert!(valid_reasoning_level(v), "{v:?} is legal");
        }
        assert!(!valid_reasoning_level("turbo"));
        assert!(!valid_reasoning_level(""));
        // Pair coupling: the persisted effort applies ONLY under the
        // route it was saved with (a route change resets it).
        let p = Prefs {
            reasoning: Some("high".into()),
            reasoning_provider: Some("lmstudio".into()),
            reasoning_model: Some("qwen3-4b".into()),
            ..Default::default()
        };
        assert_eq!(coupled_reasoning(&p, "lmstudio", "qwen3-4b"), "high");
        assert_eq!(coupled_reasoning(&p, "lmstudio", "other-model"), "");
        assert_eq!(coupled_reasoning(&p, "ollama", "qwen3-4b"), "");
        assert_eq!(coupled_reasoning(&Prefs::default(), "a", "b"), "");
    }

    #[test]
    fn session_ids_are_unique_enough() {
        let a = mint_session_id();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = mint_session_id();
        assert_ne!(a, b);
        assert!(a.starts_with("acode-"));
        assert_eq!(a.len(), "acode-".len() + 12);
    }

    #[test]
    fn iso_timestamp_shape() {
        let ts = now_iso_utc();
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
    }

    #[test]
    fn civil_from_days_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }

    #[test]
    fn default_prefs_are_ephemeral_and_never_touch_disk() {
        // REGRESSION (live incident 2026-07-21): the headless test harness
        // built UiCtx with Prefs::default(); save() wrote to the OPERATOR'S
        // real prefs file, overwriting their theme + model with fixture
        // values ("qwen-a"). Default prefs must be path-less no-ops.
        let p = Prefs {
            model: Some("qwen-a".into()),
            ..Prefs::default()
        };
        assert!(p.path.is_none());
        p.save().expect("ephemeral save is an Ok no-op");
    }

    #[test]
    fn prefs_round_trip_with_capability_fields() {
        let dir = std::env::temp_dir().join(format!("acode-prefs-test-{}", std::process::id()));
        let path = dir.join("prefs.json");
        let mut p = Prefs {
            theme: Some("nord".into()),
            disabled_tools: vec!["fetch_url".into()],
            skills: vec!["coredoc".into()],
            path: Some(path.clone()),
            ..Prefs::default()
        };
        p.touch_session("acode-aaa", Some("first prompt of session"));
        p.touch_session("acode-bbb", None);
        p.touch_session("acode-aaa", Some("late label must not overwrite"));
        // Per-session tool prefs (operator ask): each session's slot
        // round-trips independently.
        p.set_session_tool_prefs(
            "acode-aaa",
            &SessionToolPrefs {
                disabled_tools: vec!["camera_open".into(), "camera_close".into()],
                tool_overrides: vec![("write_file".into(), "ask".into())],
                accepted_tier: "write".into(),
            },
        );
        p.set_session_tool_prefs(
            "acode-bbb",
            &SessionToolPrefs {
                disabled_tools: vec![],
                tool_overrides: vec![],
                accepted_tier: "read".into(),
            },
        );
        p.save().expect("save to temp");
        let loaded = Prefs::load_from(path);
        assert_eq!(loaded.theme.as_deref(), Some("nord"));
        // Each session's tool slot survives independently.
        let aaa = loaded
            .session_tool_prefs("acode-aaa")
            .expect("aaa slot round-trips");
        assert_eq!(
            aaa.disabled_tools,
            vec!["camera_open".to_string(), "camera_close".to_string()]
        );
        assert_eq!(
            aaa.tool_overrides,
            vec![("write_file".to_string(), "ask".to_string())]
        );
        assert_eq!(aaa.accepted_tier, "write");
        let bbb = loaded
            .session_tool_prefs("acode-bbb")
            .expect("bbb slot round-trips (empty disabled is a real choice)");
        assert!(bbb.disabled_tools.is_empty());
        assert_eq!(bbb.accepted_tier, "read");
        assert!(
            loaded.session_tool_prefs("acode-never").is_none(),
            "an untouched session has no slot"
        );
        assert_eq!(loaded.disabled_tools, vec!["fetch_url".to_string()]);
        assert_eq!(loaded.skills, vec!["coredoc".to_string()]);
        // Newest first; the FIRST label sticks (a session is named once).
        assert_eq!(loaded.recent_sessions[0].id, "acode-aaa");
        assert_eq!(loaded.recent_sessions[0].label, "first prompt of session");
        assert_eq!(loaded.recent_sessions[1].id, "acode-bbb");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn tool_approval_and_workspace_allowed_round_trip() {
        let dir = std::env::temp_dir().join(format!("acode-prefs-tier-{}", std::process::id()));
        let path = dir.join("prefs.json");
        let p = Prefs {
            tool_accepted_tier: "write".into(),
            tool_overrides: vec![
                ("fetch_url".into(), "auto".into()),
                ("read_file".into(), "ask".into()),
            ],
            workspace_allowed: vec!["/srv/data".into(), "/opt/shared".into()],
            path: Some(path.clone()),
            ..Prefs::default()
        };
        p.save().expect("save");
        let loaded = Prefs::load_from(path.clone());
        assert_eq!(loaded.tool_accepted_tier, "write");
        assert!(loaded
            .tool_overrides
            .contains(&("fetch_url".to_string(), "auto".to_string())));
        assert!(loaded
            .tool_overrides
            .contains(&("read_file".to_string(), "ask".to_string())));
        assert_eq!(
            loaded.workspace_allowed,
            vec!["/srv/data".to_string(), "/opt/shared".to_string()]
        );

        // A hand-edited unknown tier + junk overrides normalize at load:
        // tier falls to the STRICTEST ("read"), junk decisions drop.
        fs::write(
            &path,
            r#"{"tool_approval": {"accepted_tier": "yolo",
                 "overrides": {"write_file": "always", "edit_file": "ask", "": "auto"}}}"#,
        )
        .expect("write raw");
        let loaded = Prefs::load_from(path);
        assert_eq!(loaded.tool_accepted_tier, "read");
        assert_eq!(
            loaded.tool_overrides,
            vec![("edit_file".to_string(), "ask".to_string())]
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn prefs_round_trip_with_every_field_populated() {
        // Cycle-3 audit (item 4): six agents grew this schema across two
        // cycles — ONE test now loads every field at once and proves the
        // full save→load round trip. A new pref field must extend this.
        let dir = std::env::temp_dir().join(format!("acode-prefs-full-{}", std::process::id()));
        let path = dir.join("prefs.json");
        let mut p = Prefs {
            theme: Some("nord".into()),
            bundle_id: Some("basic-agent".into()),
            flow_id: Some("81795ea9".into()),
            provider: Some("lmstudio".into()),
            model: Some("qwen3-4b".into()),
            reasoning: Some("high".into()),
            reasoning_provider: Some("lmstudio".into()),
            reasoning_model: Some("qwen3-4b".into()),
            session_id: Some("acode-full".into()),
            workspace_mode: Some("workspace_or_allowed".into()),
            workspace_allowed: vec!["/srv/data".into(), "/opt/shared".into()],
            show_details: Some(false),
            tool_accepted_tier: "write".into(),
            tool_overrides: vec![("fetch_url".into(), "auto".into())],
            disabled_tools: vec!["fetch_url".into()],
            skills: vec!["coredoc".into()],
            recent_sessions: Vec::new(),
            session_queues: Vec::new(),
            session_goals: Vec::new(),
            session_tool_prefs: Vec::new(),
            goal_max_cycles: 12,
            context_window: 262_144,
            path: Some(path.clone()),
        };
        p.touch_session("acode-full", Some("first prompt"));
        p.set_session_queue("acode-full", &["queued one".into(), "queued two".into()]);
        p.set_session_goal("acode-full", Some(("ship it".into(), "run-9".into())));
        p.save().expect("save");

        // Human-readable at rest: pretty-printed JSON, the documented
        // hand-editable surfaces present under their documented names.
        let raw = fs::read_to_string(&path).expect("read raw");
        assert!(raw.contains("\n  \"tool_approval\""), "pretty + named keys");
        for key in [
            "\"theme\"",
            "\"bundle_id\"",
            "\"flow_id\"",
            "\"provider\"",
            "\"model\"",
            "\"session_id\"",
            "\"workspace_mode\"",
            "\"workspace_allowed\"",
            "\"show_details\"",
            "\"accepted_tier\"",
            "\"overrides\"",
            "\"disabled_tools\"",
            "\"skills\"",
            "\"recent_sessions\"",
            "\"session_queues\"",
            "\"session_goals\"",
            "\"goal_max_cycles\"",
            "\"context_window\"",
        ] {
            assert!(raw.contains(key), "{key} at rest:\n{raw}");
        }

        let l = Prefs::load_from(path);
        assert_eq!(l.theme.as_deref(), Some("nord"));
        assert_eq!(l.bundle_id.as_deref(), Some("basic-agent"));
        assert_eq!(l.flow_id.as_deref(), Some("81795ea9"));
        assert_eq!(l.provider.as_deref(), Some("lmstudio"));
        assert_eq!(l.model.as_deref(), Some("qwen3-4b"));
        assert_eq!(l.session_id.as_deref(), Some("acode-full"));
        assert_eq!(l.workspace_mode.as_deref(), Some("workspace_or_allowed"));
        assert_eq!(
            l.workspace_allowed,
            vec!["/srv/data".to_string(), "/opt/shared".to_string()]
        );
        assert_eq!(l.show_details, Some(false));
        assert_eq!(l.tool_accepted_tier, "write");
        assert_eq!(
            l.tool_overrides,
            vec![("fetch_url".to_string(), "auto".to_string())]
        );
        assert_eq!(l.disabled_tools, vec!["fetch_url".to_string()]);
        assert_eq!(l.skills, vec!["coredoc".to_string()]);
        assert_eq!(l.recent_sessions[0].id, "acode-full");
        assert_eq!(l.recent_sessions[0].label, "first prompt");
        assert_eq!(
            l.session_queue("acode-full"),
            vec!["queued one".to_string(), "queued two".to_string()]
        );
        assert_eq!(
            l.session_goal("acode-full"),
            Some(("ship it".to_string(), "run-9".to_string()))
        );
        assert_eq!(l.goal_max_cycles, 12);
        assert_eq!(l.context_window, 262_144);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn prefs_load_tolerates_missing_and_malformed_fields() {
        // Cycle-3 audit (item 4): every field wrong-TYPED at once must
        // fail toward defaults — never a panic, never a widened posture.
        let dir = std::env::temp_dir().join(format!("acode-prefs-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("prefs.json");
        fs::write(
            &path,
            r#"{
                "theme": 7,
                "bundle_id": null,
                "provider": ["not", "a", "string"],
                "workspace_mode": {},
                "workspace_allowed": "not-a-list",
                "show_details": "yes",
                "tool_approval": "not-an-object",
                "disabled_tools": [1, 2, {"x": 3}],
                "skills": 42,
                "recent_sessions": {"not": "a list"},
                "session_queues": [{"id": "a"}, {"id": "b", "prompts": "not-a-list"},
                                    {"id": "c", "prompts": ["kept"]}],
                "session_goals": [{"id": "a", "text": 9}, {"text": "no id"},
                                   {"id": "g", "text": "kept goal", "run_id": 4}],
                "goal_max_cycles": "eight",
                "context_window": "lots"
            }"#,
        )
        .expect("write junk");
        let l = Prefs::load_from(path.clone());
        assert_eq!(l.theme, None);
        assert_eq!(l.bundle_id, None);
        assert_eq!(l.provider, None);
        assert_eq!(l.workspace_mode, None);
        assert!(l.workspace_allowed.is_empty());
        assert_eq!(l.show_details, None);
        assert_eq!(
            l.tool_accepted_tier, "",
            "unset tier stays the strictest-reading default"
        );
        assert!(l.tool_overrides.is_empty());
        assert!(l.disabled_tools.is_empty());
        assert!(l.skills.is_empty());
        assert!(l.recent_sessions.is_empty());
        // Malformed queue slots drop; the well-formed one survives.
        assert!(l.session_queue("a").is_empty());
        assert!(l.session_queue("b").is_empty());
        assert_eq!(l.session_queue("c"), vec!["kept".to_string()]);
        // Malformed goal slots drop; a wrong-typed run_id degrades to "".
        assert!(l.session_goal("a").is_none());
        assert_eq!(
            l.session_goal("g"),
            Some(("kept goal".to_string(), String::new()))
        );
        assert_eq!(l.goal_max_cycles, 0);
        assert_eq!(l.goal_cycles(), 8, "unset budget resolves to the default");
        assert_eq!(l.context_window, 0, "wrong-typed window reads as unset");

        // Out-of-range declarations read as unset too (P2-G): the load
        // must honor the same 1..=1e12 range the declaration surfaces
        // (`/context`, `--max-tokens`) enforce — a hand-edited 1e18
        // otherwise rendered "ctx —/1000000000000.0M tk" in the footer.
        fs::write(
            &path,
            r#"{"context_window": 1000000000000000000, "theme": "nord"}"#,
        )
        .expect("write out-of-range");
        let l = Prefs::load_from(path.clone());
        assert_eq!(l.context_window, 0, "out-of-range window reads as unset");
        assert_eq!(l.theme.as_deref(), Some("nord"), "siblings unaffected");
        fs::write(&path, r#"{"context_window": 262144}"#).expect("write in-range");
        assert_eq!(Prefs::load_from(path.clone()).context_window, 262_144);

        // Non-object roots (a hand-edit gone wrong) read as all-defaults.
        for junk in ["[1,2,3]", "\"just a string\"", "not json at all", ""] {
            fs::write(&path, junk).expect("write junk root");
            let l = Prefs::load_from(path.clone());
            assert_eq!(l.theme, None, "junk root {junk:?} reads as defaults");
            assert!(l.session_queues.is_empty());
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn token_count_parsing() {
        assert_eq!(parse_token_count("262144"), Some(262_144));
        assert_eq!(parse_token_count("262k"), Some(262_000));
        assert_eq!(parse_token_count("128K"), Some(128_000));
        assert_eq!(parse_token_count("1m"), Some(1_000_000));
        assert_eq!(parse_token_count("1.5M"), Some(1_500_000));
        assert_eq!(parse_token_count(" 32_768 "), Some(32_768));
        for bad in ["", "abc", "-5", "0", "12kb", "1e99m", "nan"] {
            assert_eq!(parse_token_count(bad), None, "{bad:?} must refuse");
        }
    }

    #[test]
    fn recent_sessions_are_capped() {
        let mut p = Prefs::default();
        for i in 0..30 {
            p.touch_session(&format!("acode-{i:03}"), None);
        }
        assert_eq!(p.recent_sessions.len(), RECENT_SESSIONS_MAX);
        assert_eq!(p.recent_sessions[0].id, "acode-029");
    }

    #[test]
    fn session_queue_slots_round_trip_upsert_and_gc() {
        let dir = std::env::temp_dir().join(format!("acode-prefs-queue-{}", std::process::id()));
        let path = dir.join("prefs.json");
        let mut p = Prefs {
            path: Some(path.clone()),
            ..Prefs::default()
        };
        p.set_session_queue("acode-a", &["one".into(), "two".into()]);
        p.set_session_queue("acode-b", &["b-task".into()]);
        // Rewriting a slot moves it to the front (recency order).
        p.set_session_queue("acode-a", &["one".into(), "two".into(), "three".into()]);
        assert_eq!(p.session_queues[0].0, "acode-a");
        p.save().expect("save");
        let loaded = Prefs::load_from(path.clone());
        assert_eq!(
            loaded.session_queue("acode-a"),
            vec!["one".to_string(), "two".to_string(), "three".to_string()],
            "queue stash round-trips in order"
        );
        assert_eq!(loaded.session_queue("acode-b"), vec!["b-task".to_string()]);
        assert!(loaded.session_queue("acode-ghost").is_empty());

        // Emptying a queue removes its slot (no dead stash clutter).
        let mut loaded = loaded;
        loaded.set_session_queue("acode-b", &[]);
        loaded.save().expect("save");
        let reloaded = Prefs::load_from(path);
        assert!(reloaded.session_queue("acode-b").is_empty());
        assert_eq!(
            reloaded.session_queues.len(),
            1,
            "empty stash slots are removed at rest"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn session_queue_slots_are_capped() {
        let mut p = Prefs::default();
        for i in 0..30 {
            p.set_session_queue(&format!("acode-{i:03}"), &[format!("task {i}")]);
        }
        assert_eq!(p.session_queues.len(), RECENT_SESSIONS_MAX);
        assert_eq!(p.session_queues[0].0, "acode-029", "newest first");
    }

    #[test]
    fn session_goal_slots_round_trip_and_clear() {
        let dir = std::env::temp_dir().join(format!("acode-prefs-goal-{}", std::process::id()));
        let path = dir.join("prefs.json");
        let mut p = Prefs {
            goal_max_cycles: 12,
            path: Some(path.clone()),
            ..Prefs::default()
        };
        p.set_session_goal("acode-a", Some(("ship the release".into(), String::new())));
        // Binding the run id rewrites the same slot.
        p.set_session_goal("acode-a", Some(("ship the release".into(), "run-9".into())));
        p.save().expect("save");
        let loaded = Prefs::load_from(path.clone());
        assert_eq!(
            loaded.session_goal("acode-a"),
            Some(("ship the release".to_string(), "run-9".to_string()))
        );
        assert_eq!(loaded.goal_max_cycles, 12);
        assert_eq!(loaded.goal_cycles(), 12);
        assert_eq!(
            Prefs::default().goal_cycles(),
            8,
            "unset budget resolves to the plan default"
        );
        // Clearing removes the slot at rest.
        let mut loaded = loaded;
        loaded.set_session_goal("acode-a", None);
        loaded.save().expect("save");
        assert!(Prefs::load_from(path).session_goal("acode-a").is_none());
        let _ = fs::remove_dir_all(dir);
    }
}
