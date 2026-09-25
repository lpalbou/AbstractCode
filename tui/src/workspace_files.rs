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

pub fn workspace_info_from(v: &Value) -> WorkspaceInfo {
    let host = v.get("host").cloned().unwrap_or(Value::Null);
    WorkspaceInfo {
        workspace_root: s(v, "workspace_root"),
        kind: s(v, "kind"),
        session_id: s(v, "session_id"),
        exists: v.get("exists").and_then(Value::as_bool).unwrap_or(false),
        hostname: s(&host, "hostname"),
        caller_is_this_machine: host
            .get("caller_is_this_machine")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        open_supported: v
            .get("open_supported")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

/// Folders first, then files, each by name (case-insensitive).
pub fn workspace_listing_from(v: &Value) -> WorkspaceListing {
    let mut entries: Vec<WorkspaceEntry> = v
        .get("entries")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    let name = s(r, "name");
                    if name.is_empty() {
                        return None;
                    }
                    let path = match s(r, "path") {
                        p if p.is_empty() => name.clone(),
                        p => p,
                    };
                    Some(WorkspaceEntry {
                        is_dir: s(r, "type") == "dir",
                        size_bytes: r.get("size_bytes").and_then(Value::as_u64),
                        name,
                        path,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    WorkspaceListing {
        path: s(v, "path"),
        entries,
        truncated: v.get("truncated").and_then(Value::as_bool).unwrap_or(false),
    }
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

/// Whether a gateway base URL points at THIS machine by address
/// (`localhost`, `127.0.0.0/8`, `::1`).
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
    } else {
        authority.split(':').next().unwrap_or("")
    };
    let host = host.to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") || host == "::1" {
        return true;
    }
    host.parse::<std::net::Ipv4Addr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
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

/// The workspace root the TUI sends with a run.
///
/// Decision (mission C2, §W): a LOOPBACK gateway gets this terminal's cwd,
/// as always — both sides see the same disk. A REMOTE gateway does NOT get
/// the local cwd implicitly: that path names a folder on this laptop, and
/// on the gateway host it is either missing or, worse, a different folder
/// of the same name that the agent would create or write into. The gateway
/// then works in its own session folder, which `/files` shows. An explicit
/// `--workspace <path>` is always sent (the operator may be naming a path on
/// the gateway host). Returns `(root, notice)`; the notice says what
/// happened, once, at boot.
pub fn launch_workspace_root(
    no_workspace: bool,
    explicit: Option<&str>,
    cwd: Option<&str>,
    gateway_base_url: &str,
) -> (Option<String>, Option<String>) {
    if no_workspace {
        return (None, None);
    }
    if let Some(p) = explicit.map(str::trim).filter(|p| !p.is_empty()) {
        return (Some(p.to_string()), None);
    }
    if is_loopback_url(gateway_base_url) {
        return (cwd.map(str::to_string), None);
    }
    (
        None,
        Some(
            "this gateway is on another machine, so your local folder is not sent as the \
             workspace (it would name a path on the gateway host) — the agent works in a \
             gateway-side session folder; /files shows it. --workspace <path> names a folder \
             on the gateway host."
                .to_string(),
        ),
    )
}

/// The platform's "open this folder" program, or `None` where there is none.
pub fn opener_program() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("open")
    } else if cfg!(target_os = "windows") {
        Some("explorer")
    } else if cfg!(unix) {
        Some("xdg-open")
    } else {
        None
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
        }));
        assert_eq!(info.workspace_root, "/srv/gw/workspaces/session-abc");
        assert_eq!(info.hostname, "studio");
        assert!(info.exists && !info.caller_is_this_machine && !info.open_supported);

        let l = workspace_listing_from(&json!({
            "path": "src", "truncated": true,
            "entries": [
                {"name": "b.rs", "path": "src/b.rs", "type": "file", "size_bytes": 12, "mtime": 1.0},
                {"name": "Zeta", "path": "src/Zeta", "type": "dir"},
                {"name": "a.rs", "path": "src/a.rs", "type": "file", "size_bytes": 3},
                {"name": ""}
            ]
        }));
        assert!(l.truncated);
        let names: Vec<&str> = l.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            ["Zeta", "a.rs", "b.rs"],
            "folders first, then by name"
        );
        assert_eq!(l.entries[2].size_bytes, Some(12));
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
            "http://user@localhost:1",
        ] {
            assert!(is_loopback_url(u), "{u}");
        }
        for u in [
            "http://192.168.1.4:8080",
            "https://gw.example.com",
            "http://10.0.0.1",
            "http://localhost.example.com",
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

    #[test]
    fn a_remote_gateway_does_not_get_the_local_cwd() {
        let (root, note) =
            launch_workspace_root(false, None, Some("/Users/me/proj"), "http://127.0.0.1:8080");
        assert_eq!(root.as_deref(), Some("/Users/me/proj"));
        assert!(note.is_none());
        let (root, note) = launch_workspace_root(
            false,
            None,
            Some("/Users/me/proj"),
            "https://gw.example.com",
        );
        assert_eq!(root, None);
        assert!(note.unwrap().contains("/files"));
        let (root, note) = launch_workspace_root(
            false,
            Some("/srv/proj"),
            Some("/Users/me/proj"),
            "https://gw.example.com",
        );
        assert_eq!(
            root.as_deref(),
            Some("/srv/proj"),
            "explicit is always sent"
        );
        assert!(note.is_none());
        assert_eq!(
            launch_workspace_root(true, Some("/x"), Some("/y"), "http://127.0.0.1"),
            (None, None)
        );
    }
}
