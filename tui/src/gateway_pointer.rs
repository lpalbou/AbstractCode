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
//!   is a regular file (not a symlink) owned by the current user and
//!   writable by nobody else (no group/world write bit);
//! - the checks run on the OPENED file (POSIX: `O_NOFOLLOW | O_NONBLOCK`,
//!   then `fstat`) and the bytes are read from that same descriptor, so the
//!   file cannot be swapped between the check and the read, and a FIFO in
//!   its place cannot hang the client;
//! - a pointer is a few hundred bytes of configuration: a file over
//!   [`MAX_POINTER_BYTES`] is refused, never read (a config-file bound, not
//!   a model-input cap);
//! - anything else is ignored with ONE visible warning; a missing file is
//!   silent.
//!
//! Precedence (`config::resolve_gateway_url`): the launch flag, then the
//! legacy environment, then the saved login (except a saved
//! `http://127.0.0.1:8080`, the old built-in default, which the pointer
//! replaces), then the pointer, then `http://127.0.0.1:8080`.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub const POINTER_SCHEMA: u64 = 1;

/// The largest pointer file this reader opens (64 KiB). The real file is a
/// few hundred bytes; anything this large is not a pointer.
pub const MAX_POINTER_BYTES: u64 = 64 * 1024;

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
    // SAFETY: getuid(2) takes no arguments, cannot fail, and has no side effects.
    unsafe { libc::getuid() }
}

/// Why the pointer could not be opened.
enum OpenError {
    Missing,
    Symlink,
    Other(std::io::Error),
}

/// Open without following a final symlink and without blocking (a FIFO).
#[cfg(unix)]
fn open_pointer(path: &Path) -> Result<File, OpenError> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|e| match e.raw_os_error() {
            _ if e.kind() == std::io::ErrorKind::NotFound => OpenError::Missing,
            // ELOOP (Linux, macOS) / EMLINK (FreeBSD): the final component is a symlink.
            Some(code) if code == libc::ELOOP || code == libc::EMLINK => OpenError::Symlink,
            _ => OpenError::Other(e),
        })
}

/// Windows has no O_NOFOLLOW through std: refuse a symlink before opening.
#[cfg(not(unix))]
fn open_pointer(path: &Path) -> Result<File, OpenError> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(OpenError::Missing),
        Err(e) => return Err(OpenError::Other(e)),
        Ok(m) if m.file_type().is_symlink() => return Err(OpenError::Symlink),
        Ok(_) => {}
    }
    File::open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => OpenError::Missing,
        _ => OpenError::Other(e),
    })
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
    let mut file = match open_pointer(path) {
        Ok(f) => f,
        Err(OpenError::Missing) => return Pointer::Missing,
        Err(OpenError::Symlink) => return refused("it is a symbolic link".into()),
        Err(OpenError::Other(e)) => return refused(format!("cannot read it ({e})")),
    };
    // Every check below reads the OPENED file (fstat), never the path again.
    let meta = match file.metadata() {
        Ok(m) => m,
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
        if meta.mode() & 0o022 != 0 {
            return refused(format!(
                "other users can write it (mode {:o})",
                meta.mode() & 0o777
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = uid;
    // Read from the same descriptor, at most one byte past the bound: an
    // oversized file (or one that grew after the fstat) is refused, and
    // nothing past the bound is ever read.
    let mut raw = String::new();
    if let Err(e) = (&mut file)
        .take(MAX_POINTER_BYTES + 1)
        .read_to_string(&mut raw)
    {
        return refused(format!("cannot read it ({e})"));
    }
    if raw.len() as u64 > MAX_POINTER_BYTES {
        return refused(format!(
            "it is larger than the {} KiB a pointer file may be",
            MAX_POINTER_BYTES / 1024
        ));
    }
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
