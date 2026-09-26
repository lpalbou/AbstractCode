//! A run's workspace on the GATEWAY host (§W): where it is, what is in it,
//! and whether this terminal is on the same machine. PURE — parsing, path
//! joining and the "is this local?" rules; the worker does the HTTP and
//! `ui::files` draws.
//!
//! Two facts decide every action here, and they come from different
//! places on purpose:
//! - `caller_is_this_machine` is the GATEWAY's verdict about this caller
//!   (it sees the peer address; we cannot).
//! - whether the gateway URL is loopback is THIS client's own fact.
//!
//! Opening a folder with the desktop's file manager needs both (and the
//! gateway's `open_supported`, which already includes "caller is admin").
//! Everywhere else the path is SHOWN, absolute, with the words "on gateway
//! host <name>", because a path on another machine is only useful when the
//! reader knows which machine.

use serde_json::Value;

/// `GET /runs/{rid}/workspace`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceInfo {
    /// Absolute path on the gateway host.
    pub workspace_root: String,
    /// `session` | `run` | `launch_folder`.
    pub kind: String,
    pub session_id: String,
    pub exists: bool,
    pub hostname: String,
    pub caller_is_this_machine: bool,
    pub open_supported: bool,
}

/// One row of `GET /runs/{rid}/workspace/files`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceEntry {
    pub name: String,
    /// Relative to the workspace root.
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: Option<u64>,
}

/// `GET /runs/{rid}/workspace/files` — one folder.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceListing {
    /// The folder listed, relative ("" = the root).
    pub path: String,
    pub entries: Vec<WorkspaceEntry>,
    /// The gateway cut the list (its limit) — shown, never hidden.
    pub truncated: bool,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Parse `GET /runs/{rid}/workspace`. A response missing a field the
/// contract names is an ERROR — never read as "empty" or "not this machine"
/// (review S6).
pub fn workspace_info_from(v: &Value) -> Result<WorkspaceInfo, String> {
    let bad = |what: &str| format!("unexpected /workspace response: missing {what}");
    let root = v
        .get("workspace_root")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .ok_or_else(|| bad("workspace_root"))?;
    let host = v
        .get("host")
        .filter(|h| h.is_object())
        .ok_or_else(|| bad("host"))?;
    let flag = |obj: &Value, k: &str| obj.get(k).and_then(Value::as_bool).ok_or_else(|| bad(k));
    Ok(WorkspaceInfo {
        workspace_root: root.to_string(),
        kind: s(v, "kind"),
        session_id: s(v, "session_id"),
        exists: flag(v, "exists")?,
        hostname: s(host, "hostname"),
        caller_is_this_machine: flag(host, "caller_is_this_machine")?,
        open_supported: flag(v, "open_supported")?,
    })
}

/// Parse `GET /runs/{rid}/workspace/files`, folders first, then files, each
/// by name. A missing `entries`/`truncated`, or an entry without
/// `name`/`path`/`type`, is an ERROR (review S6) — a malformed answer must
/// never render as an empty folder.
pub fn workspace_listing_from(v: &Value) -> Result<WorkspaceListing, String> {
    let bad = |what: &str| format!("unexpected /workspace/files response: missing {what}");
    let rows = v
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| bad("entries"))?;
    let truncated = v
        .get("truncated")
        .and_then(Value::as_bool)
        .ok_or_else(|| bad("truncated"))?;
    let mut entries = Vec::with_capacity(rows.len());
    for (i, r) in rows.iter().enumerate() {
        let field = |k: &str| {
            r.get(k)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .ok_or_else(|| bad(&format!("entries[{i}].{k}")))
        };
        let kind = field("type")?;
        if kind != "file" && kind != "dir" {
            return Err(format!(
                "unexpected /workspace/files response: entries[{i}].type is {kind:?}"
            ));
        }
        entries.push(WorkspaceEntry {
            name: field("name")?,
            path: field("path")?,
            is_dir: kind == "dir",
            size_bytes: r.get("size_bytes").and_then(Value::as_u64),
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(WorkspaceListing {
        path: s(v, "path"),
        entries,
        truncated,
    })
}

/// The absolute path of a relative workspace path ON THE GATEWAY HOST,
/// spelled with that host's separator (a Windows gateway's root uses `\`).
pub fn absolute_path(root: &str, rel: &str) -> String {
    let rel = rel.trim_matches(|c| c == '/' || c == '\\');
    if rel.is_empty() {
        return root.to_string();
    }
    let sep = if root.contains('\\') && !root.contains('/') {
        '\\'
    } else {
        '/'
    };
    let rel = if sep == '\\' {
        rel.replace('/', "\\")
    } else {
        rel.to_string()
    };
    if root.ends_with(sep) {
        format!("{root}{rel}")
    } else {
        format!("{root}{sep}{rel}")
    }
}

/// The parent of a relative folder ("" for the root's children).
pub fn parent_dir(rel: &str) -> String {
    let t = rel.trim_matches('/');
    match t.rfind('/') {
        Some(i) => t[..i].to_string(),
        None => String::new(),
    }
}

/// Whether a gateway base URL points at THIS machine by address:
/// `localhost` (and `*.localhost`), all of `127.0.0.0/8`, and IPv6 loopback
/// in every spelling (`::1`, `[::1]`, `0:0:0:0:0:0:0:1`, and the
/// IPv4-mapped `::ffff:127.x.y.z`).
pub fn is_loopback_url(base_url: &str) -> bool {
    let rest = base_url
        .trim()
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(base_url.trim());
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    let host = if let Some(stripped) = authority.strip_prefix('[') {
        stripped.split(']').next().unwrap_or("")
    } else if authority.matches(':').count() > 1 {
        // A bare (unbracketed) IPv6 literal: no port can be split off.
        authority
    } else {
        authority.split(':').next().unwrap_or("")
    };
    let host = host.to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback(),
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
        Err(_) => false,
    }
}

/// The desktop "open folder" action is offered ONLY when the gateway says
/// it supports it for this caller (admin), says the caller is on its
/// machine, AND this client reaches it over loopback (contract W +
/// amendment 4). Otherwise the path is shown for copying.
pub fn can_open_locally(info: &WorkspaceInfo, gateway_base_url: &str) -> bool {
    info.open_supported && info.caller_is_this_machine && is_loopback_url(gateway_base_url)
}

/// Why "open folder" is not offered — the words the modal shows.
pub fn open_refusal(info: &WorkspaceInfo, gateway_base_url: &str) -> Option<String> {
    if can_open_locally(info, gateway_base_url) {
        return None;
    }
    let host = if info.hostname.is_empty() {
        "the gateway host".to_string()
    } else {
        format!("gateway host {}", info.hostname)
    };
    Some(
        if !is_loopback_url(gateway_base_url) || !info.caller_is_this_machine {
            format!("the folder is on {host}, not this machine — c copies its path")
        } else {
            "opening folders needs an admin sign-in on this gateway — c copies the path".to_string()
        },
    )
}

/// `send_local_workspace` (a stored preference, `prefs.json`): whether this
/// terminal's folder is sent as the run's workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SendLocalWorkspace {
    /// Only when the gateway is on this machine (default).
    #[default]
    Auto,
    /// Always — for a gateway that sees the same path (a shared mount).
    Always,
    /// Never — the agent always works in the gateway's session folder.
    Never,
}

impl SendLocalWorkspace {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "auto" | "" => Some(Self::Auto),
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            _ => None,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

/// Whether this terminal's folder is sent as the workspace root.
///
/// Decision (mission C2 + review S4/S5): the gateway's own same-machine
/// verdict wins whenever this client has one (`GET /runs/{rid}/workspace`
/// → `host.caller_is_this_machine`, learned from the first run's workspace);
/// before any run there is none, so a loopback URL counts as this machine.
/// A path from this laptop sent to another host names a folder that is
/// missing there — or a different one the agent would write into. The
/// stored preference overrides: `always` (shared mounts), `never`. An
/// explicit `--workspace` is always sent.
pub fn sends_local_workspace(
    pref: SendLocalWorkspace,
    explicit: bool,
    gateway_base_url: &str,
    gateway_says_same_machine: Option<bool>,
) -> bool {
    if explicit {
        return true;
    }
    match pref {
        SendLocalWorkspace::Always => true,
        SendLocalWorkspace::Never => false,
        SendLocalWorkspace::Auto => {
            gateway_says_same_machine.unwrap_or_else(|| is_loopback_url(gateway_base_url))
        }
    }
}

/// The one-time notice for a withheld folder.
pub const REMOTE_WORKSPACE_NOTICE: &str =
    "this gateway is on another machine, so your local folder is not sent as the workspace \
     (it would name a path on the gateway host) — the agent works in a gateway-side session \
     folder; /files shows it. --workspace <path> names a folder on the gateway host; \
     /workspace send always sends your folder (shared mounts).";

/// The launch-time candidate root (`--workspace`, else the cwd; `None` with
/// `--no-workspace`) and whether it was explicit.
pub fn launch_workspace_candidate(
    no_workspace: bool,
    explicit: Option<&str>,
    cwd: Option<&str>,
) -> (Option<String>, bool) {
    if no_workspace {
        return (None, false);
    }
    if let Some(p) = explicit.map(str::trim).filter(|p| !p.is_empty()) {
        return (Some(p.to_string()), true);
    }
    (cwd.map(str::to_string), false)
}

/// Bundle-like folders a desktop opener would LAUNCH rather than show.
const LAUNCHABLE_SUFFIXES: &[&str] = &[".app", ".bundle", ".framework", ".pkg"];

/// The command that REVEALS the workspace root in the desktop file manager
/// (review S1): never a launch. `root` must be the gateway's
/// `workspace_root` string verbatim — never a path assembled here. Refused:
/// a relative path, any `..` component, a bundle-like suffix (macOS `open`
/// on `x.app` runs it). macOS `open -R` (reveal in Finder), Linux
/// `xdg-open`, Windows `explorer /select,`.
pub fn reveal_command(root: &str) -> Result<(&'static str, Vec<String>), String> {
    let trimmed = root.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Err("the gateway reported no workspace folder".into());
    }
    let absolute = trimmed.starts_with('/')
        || trimmed.as_bytes().get(1) == Some(&b':')
        || trimmed.starts_with("\\\\");
    if !absolute {
        return Err(format!("not opening {root}: not an absolute path"));
    }
    if trimmed.split(['/', '\\']).any(|seg| seg == "..") {
        return Err(format!("not opening {root}: the path contains '..'"));
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(ext) = LAUNCHABLE_SUFFIXES.iter().find(|e| lower.ends_with(*e)) {
        return Err(format!(
            "not opening {root}: a {ext} folder would be launched, not shown — c copies the path"
        ));
    }
    if cfg!(target_os = "macos") {
        Ok(("open", vec!["-R".into(), trimmed.to_string()]))
    } else if cfg!(target_os = "windows") {
        Ok(("explorer", vec![format!("/select,{trimmed}")]))
    } else if cfg!(unix) {
        Ok(("xdg-open", vec![trimmed.to_string()]))
    } else {
        Err(format!("no folder opener on this platform — {root}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn info_and_listing_parse_the_contract_shapes() {
        let info = workspace_info_from(&json!({
            "workspace_root": "/srv/gw/workspaces/session-abc", "kind": "session",
            "session_id": "s1", "exists": true,
            "host": {"hostname": "studio", "caller_is_this_machine": false},
            "open_supported": false
        }))
        .unwrap();
        assert_eq!(info.workspace_root, "/srv/gw/workspaces/session-abc");
        assert_eq!(info.hostname, "studio");
        assert!(info.exists && !info.caller_is_this_machine && !info.open_supported);

        let l = workspace_listing_from(&json!({
            "path": "src", "truncated": true,
            "entries": [
                {"name": "b.rs", "path": "src/b.rs", "type": "file", "size_bytes": 12, "mtime": 1.0},
                {"name": "Zeta", "path": "src/Zeta", "type": "dir"},
                {"name": "a.rs", "path": "src/a.rs", "type": "file", "size_bytes": 3}
            ]
        }))
        .unwrap();
        assert!(l.truncated);
        let names: Vec<&str> = l.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["Zeta", "a.rs", "b.rs"],
            "folders first, then by name"
        );
        assert_eq!(l.entries[2].size_bytes, Some(12));
    }

    /// Review S6: a malformed answer is an error, never "empty folder" or
    /// "not this machine".
    #[test]
    fn malformed_answers_are_errors() {
        let e = workspace_listing_from(&json!({"path": ""})).unwrap_err();
        assert!(e.contains("missing entries"), "{e}");
        let e = workspace_listing_from(&json!({"entries": []})).unwrap_err();
        assert!(e.contains("missing truncated"), "{e}");
        let e = workspace_listing_from(&json!({"truncated": false, "entries": [{"name": "a"}]}))
            .unwrap_err();
        assert!(e.contains("entries[0]"), "{e}");
        let e = workspace_listing_from(
            &json!({"truncated": false, "entries": [{"name": "a", "path": "a", "type": "link"}]}),
        )
        .unwrap_err();
        assert!(e.contains("type"), "{e}");
        assert!(
            workspace_listing_from(&json!({"truncated": false, "entries": []}))
                .unwrap()
                .entries
                .is_empty()
        );
        let full = json!({"workspace_root": "/w", "exists": true, "open_supported": false,
                          "host": {"hostname": "h", "caller_is_this_machine": true}});
        assert!(workspace_info_from(&full).is_ok());
        for k in ["workspace_root", "exists", "open_supported", "host"] {
            let mut v = full.clone();
            v.as_object_mut().unwrap().remove(k);
            assert!(workspace_info_from(&v).is_err(), "missing {k} must fail");
        }
        let mut v = full.clone();
        v["host"]
            .as_object_mut()
            .unwrap()
            .remove("caller_is_this_machine");
        assert!(workspace_info_from(&v)
            .unwrap_err()
            .contains("caller_is_this_machine"));
    }

    #[test]
    fn absolute_paths_use_the_gateway_hosts_separator() {
        assert_eq!(absolute_path("/w/s", ""), "/w/s");
        assert_eq!(absolute_path("/w/s", "src/a.rs"), "/w/s/src/a.rs");
        assert_eq!(absolute_path("/w/s/", "/a"), "/w/s/a");
        assert_eq!(absolute_path(r"C:\gw\s", "src/a.rs"), r"C:\gw\s\src\a.rs");
        assert_eq!(parent_dir("src/deep/x"), "src/deep");
        assert_eq!(parent_dir("src"), "");
    }

    #[test]
    fn loopback_detection() {
        for u in [
            "http://127.0.0.1:8080",
            "http://localhost:8080/",
            "https://127.9.9.9",
            "http://[::1]:8080",
            "http://[0:0:0:0:0:0:0:1]:8080",
            "http://[::ffff:127.0.0.1]:8080",
            "http://::1",
            "http://user@localhost:1",
        ] {
            assert!(is_loopback_url(u), "{u}");
        }
        for u in [
            "http://192.168.1.4:8080",
            "https://gw.example.com",
            "http://10.0.0.1",
            "http://localhost.example.com",
            "http://[::ffff:10.0.0.1]:80",
            "http://[fe80::1]:80",
        ] {
            assert!(!is_loopback_url(u), "{u}");
        }
    }

    #[test]
    fn open_needs_all_three_facts() {
        let mut info = WorkspaceInfo {
            workspace_root: "/w".into(),
            hostname: "studio".into(),
            caller_is_this_machine: true,
            open_supported: true,
            ..Default::default()
        };
        assert!(can_open_locally(&info, "http://127.0.0.1:8080"));
        assert!(open_refusal(&info, "http://127.0.0.1:8080").is_none());
        assert!(!can_open_locally(&info, "http://192.168.1.4:8080"));
        assert!(open_refusal(&info, "http://192.168.1.4:8080")
            .unwrap()
            .contains("gateway host studio"));
        info.open_supported = false;
        assert!(open_refusal(&info, "http://127.0.0.1:8080")
            .unwrap()
            .contains("admin"));
        info.open_supported = true;
        info.caller_is_this_machine = false;
        assert!(!can_open_locally(&info, "http://127.0.0.1:8080"));
    }

    /// Review S1: `o` reveals the ROOT and never launches: `..` and
    /// bundle-like folders are refused, macOS uses `open -R`.
    #[test]
    fn reveal_never_launches() {
        for bad in [
            "/w/tool.app",
            "/w/tool.APP/",
            "/w/x.bundle",
            "/w/x.framework",
            "/w/setup.pkg",
            "/w/../etc",
            "relative/path",
            "",
        ] {
            assert!(reveal_command(bad).is_err(), "{bad:?} must be refused");
        }
        let (program, args) = reveal_command("/srv/gw/workspaces/session-1").unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(program, "open");
            assert_eq!(args, vec!["-R", "/srv/gw/workspaces/session-1"]);
        } else if cfg!(target_os = "windows") {
            assert_eq!(program, "explorer");
        } else {
            assert_eq!(program, "xdg-open");
            assert_eq!(args, vec!["/srv/gw/workspaces/session-1"]);
        }
    }

    /// Review S4/S5: the gateway's verdict wins when known; before it, a
    /// loopback URL counts as this machine; the stored preference overrides.
    #[test]
    fn workspace_send_decision() {
        use SendLocalWorkspace::*;
        let lan = "http://192.168.1.4:8080";
        let lo = "http://127.0.0.1:8080";
        assert!(sends_local_workspace(Auto, false, lo, None));
        assert!(!sends_local_workspace(Auto, false, lan, None));
        assert!(
            sends_local_workspace(Auto, false, lan, Some(true)),
            "gateway says same host"
        );
        assert!(
            !sends_local_workspace(Auto, false, lo, Some(false)),
            "gateway says otherwise"
        );
        assert!(sends_local_workspace(Always, false, lan, Some(false)));
        assert!(!sends_local_workspace(Never, false, lo, Some(true)));
        assert!(
            sends_local_workspace(Never, true, lan, None),
            "explicit is always sent"
        );
        assert_eq!(SendLocalWorkspace::parse("ALWAYS"), Some(Always));
        assert_eq!(SendLocalWorkspace::parse("sometimes"), None);
        assert_eq!(
            launch_workspace_candidate(false, None, Some("/me")),
            (Some("/me".into()), false)
        );
        assert_eq!(
            launch_workspace_candidate(false, Some("/srv"), Some("/me")),
            (Some("/srv".into()), true)
        );
        assert_eq!(
            launch_workspace_candidate(true, Some("/x"), Some("/y")),
            (None, false)
        );
    }
}
