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

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Prefs {
    pub theme: Option<String>,
    pub bundle_id: Option<String>,
    pub flow_id: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub session_id: Option<String>,
    pub workspace_mode: Option<String>,
    pub show_details: Option<bool>,
    /// Gateway tools the user switched OFF (`/tools`). A disabled-list (not
    /// an enabled-list) so newly published gateway tools default to ON.
    pub disabled_tools: Vec<String>,
    /// Gateway skills attached to every run (`/skills` -> input_data.skills).
    pub skills: Vec<String>,
    /// Recently used sessions, newest first (the `/sessions` picker).
    pub recent_sessions: Vec<SessionEntry>,
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
        Prefs {
            theme: s("theme"),
            bundle_id: s("bundle_id"),
            flow_id: s("flow_id"),
            provider: s("provider"),
            model: s("model"),
            session_id: s("session_id"),
            workspace_mode: s("workspace_mode"),
            show_details: v.get("show_details").and_then(Value::as_bool),
            disabled_tools: string_list("disabled_tools"),
            skills: string_list("skills"),
            recent_sessions: sessions,
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
            "session_id": self.session_id,
            "workspace_mode": self.workspace_mode,
            "show_details": self.show_details,
            "disabled_tools": self.disabled_tools,
            "skills": self.skills,
            "recent_sessions": self.recent_sessions.iter().map(|e| json!({
                "id": e.id,
                "label": e.label,
                "last_used": e.last_used,
            })).collect::<Vec<_>>(),
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
        p.save().expect("save to temp");
        let loaded = Prefs::load_from(path);
        assert_eq!(loaded.theme.as_deref(), Some("nord"));
        assert_eq!(loaded.disabled_tools, vec!["fetch_url".to_string()]);
        assert_eq!(loaded.skills, vec!["coredoc".to_string()]);
        // Newest first; the FIRST label sticks (a session is named once).
        assert_eq!(loaded.recent_sessions[0].id, "acode-aaa");
        assert_eq!(loaded.recent_sessions[0].label, "first prompt of session");
        assert_eq!(loaded.recent_sessions[1].id, "acode-bbb");
        let _ = fs::remove_dir_all(dir);
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
}
