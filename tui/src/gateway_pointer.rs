//! The local gateway pointer: `~/.abstractframework/gateway.json`.
//!
//! The gateway's `serve` and the installer write where this computer's
//! installed gateway listens: `{"schema": 1, "url", "port", "data_dir",
//! "updated_at", "written_by"}` (no token, no liveness). Every client reads
//! it the same way (the shared case table is the ui-kit's
//! `scripts/fixtures/gateway_pointer/cases.json`, vendored byte-identical
//! under `tests/fixtures/gateway_pointer`):
//!
//! - believed only when `schema` is 1, the url is http(s) on `127.0.0.1`,
//!   `[::1]` or `localhost` with nothing after the port, and (POSIX) the file
//!   is a regular file owned by the current user;
//! - anything else is ignored with ONE visible warning; a missing file is
//!   silent.
//!
//! Precedence (`config::resolve_gateway_url`): the launch flag, then the
//! legacy environment, then the saved login (except a saved
//! `http://127.0.0.1:8080`, the old built-in default, which the pointer
//! replaces), then the pointer, then `http://127.0.0.1:8080`.

use std::path::{Path, PathBuf};

use serde_json::Value;

pub const POINTER_SCHEMA: u64 = 1;

/// `<home>/.abstractframework/gateway.json`.
pub fn pointer_path(home: &Path) -> PathBuf {
    home.join(".abstractframework").join("gateway.json")
}

/// What reading the pointer found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pointer {
    /// The gateway's URL (`scheme://host[:port]`).
    Found { url: String },
    /// No pointer file (silent).
    Missing,
    /// A pointer file this reader refuses; `warning` is shown once.
    Refused { warning: String },
}

#[cfg(unix)]
fn current_uid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    // SAFETY: getuid(2) takes no arguments, cannot fail, and has no side effects.
    unsafe { getuid() }
}

/// Read and check the pointer at `path`.
pub fn read_pointer(path: &Path) -> Pointer {
    #[cfg(unix)]
    let uid = current_uid();
    #[cfg(not(unix))]
    let uid = 0;
    read_pointer_as(path, uid)
}

/// `read_pointer` for a given current uid (tests use a foreign one).
pub fn read_pointer_as(path: &Path, uid: u32) -> Pointer {
    let refused = |reason: String| Pointer::Refused {
        warning: format!("Ignoring the gateway pointer {}: {reason}.", path.display()),
    };
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Pointer::Missing,
        Err(e) => return refused(format!("cannot read it ({e})")),
    };
    if !meta.file_type().is_file() {
        return refused("it is not a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != uid {
            return refused("it belongs to another user".into());
        }
    }
    #[cfg(not(unix))]
    let _ = uid;
    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(e) => return refused(format!("cannot read it ({e})")),
    };
    let data: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return refused(format!("it is not valid JSON ({e})")),
    };
    let Some(obj) = data.as_object() else {
        return refused("it is not a JSON object".into());
    };
    // JSON numbers compare by value (1 and 1.0 are the same schema), as every reader does.
    if obj.get("schema").and_then(Value::as_f64) != Some(POINTER_SCHEMA as f64) {
        let got = obj.get("schema").cloned().unwrap_or(Value::Null);
        return refused(format!(
            "unknown schema {got} (this reader knows {POINTER_SCHEMA})"
        ));
    }
    let url = obj.get("url").and_then(Value::as_str).unwrap_or("");
    match loopback_origin(url) {
        Ok(origin) => Pointer::Found { url: origin },
        Err(why) => refused(format!("url {url:?} {why}")),
    }
}

/// `scheme://host[:port]` when `url` is http(s) on 127.0.0.1, [::1] or
/// localhost with nothing after the port (a bare trailing "/" is allowed).
fn loopback_origin(url: &str) -> Result<String, &'static str> {
    let url = url.trim();
    let (scheme, rest) = if let Some(r) = url.strip_prefix("http://") {
        ("http", r)
    } else if let Some(r) = url.strip_prefix("https://") {
        ("https", r)
    } else {
        return Err("is not an http(s) URL");
    };
    let (authority, tail) = match rest.find(['/', '?', '#']) {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    if !(tail.is_empty() || tail == "/") || authority.contains('@') {
        return Err("must be scheme://host:port only");
    }
    let (host, port) = if let Some(after) = authority.strip_prefix('[') {
        let end = after.find(']').ok_or("is not a URL")?;
        (format!("[{}]", &after[..end]), &after[end + 1..])
    } else {
        match authority.find(':') {
            Some(i) => (authority[..i].to_string(), &authority[i..]),
            None => (authority.to_string(), ""),
        }
    };
    let host = host.to_ascii_lowercase();
    if !matches!(host.as_str(), "127.0.0.1" | "[::1]" | "localhost") {
        return Err("is not on this computer (127.0.0.1, ::1 or localhost only)");
    }
    let port = match port.strip_prefix(':') {
        None if port.is_empty() => String::new(),
        Some(p)
            if !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n)) =>
        {
            format!(":{}", p.trim_start_matches('0'))
        }
        _ => return Err("has an invalid port"),
    };
    Ok(format!("{scheme}://{host}{port}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_loopback_only_and_bare() {
        assert_eq!(
            loopback_origin("http://127.0.0.1:8081"),
            Ok("http://127.0.0.1:8081".into())
        );
        assert_eq!(
            loopback_origin("http://127.0.0.1:8081/"),
            Ok("http://127.0.0.1:8081".into())
        );
        assert_eq!(
            loopback_origin("https://LOCALHOST:9000"),
            Ok("https://localhost:9000".into())
        );
        assert_eq!(
            loopback_origin("http://[::1]:8080"),
            Ok("http://[::1]:8080".into())
        );
        assert!(loopback_origin("http://192.168.1.20:8081").is_err());
        assert!(loopback_origin("http://127.0.0.1.evil.example:8081").is_err());
        assert!(loopback_origin("http://127.0.0.1:8081/api").is_err());
        assert!(loopback_origin("http://u:p@127.0.0.1:8081").is_err());
        assert!(loopback_origin("ftp://127.0.0.1:21").is_err());
        assert!(loopback_origin("http://127.0.0.1:99999").is_err());
    }
}
