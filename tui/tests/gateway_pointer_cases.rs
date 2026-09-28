//! The local gateway pointer (`~/.abstractframework/gateway.json`) against the
//! shared case table every reader checks (`tests/fixtures/gateway_pointer`,
//! byte-identical copies of the ui-kit's `scripts/fixtures/gateway_pointer`),
//! and the one URL precedence. ONE test mutates the process environment, so
//! nothing here races on it.

use std::path::{Path, PathBuf};

use serde_json::Value;

use abstractcode::config::{resolve_gateway_url_in, write_login, UrlOrigin};
use abstractcode::gateway_pointer::{
    pointer_path, read_pointer, read_pointer_as, Pointer, MAX_POINTER_BYTES,
};

const DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/gateway_pointer"
);

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("acode-pointer-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn place(home: &Path, fixture: &str) -> PathBuf {
    let path = pointer_path(home);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::copy(format!("{DIR}/{fixture}"), &path).unwrap();
    path
}

#[test]
fn every_shared_case_and_the_precedence() {
    // Hermetic: no URL from the environment, the login store in scratch.
    for name in [
        "ABSTRACTCODE_GATEWAY_URL",
        "ABSTRACTFLOW_GATEWAY_URL",
        "ABSTRACTGATEWAY_URL",
    ] {
        std::env::remove_var(name);
    }
    let store_dir = scratch("store");
    let store = store_dir.join("gateway.json");
    std::env::set_var("ABSTRACTCODE_GATEWAY_CONNECTION_FILE", &store);

    let cases: Value =
        serde_json::from_slice(&std::fs::read(format!("{DIR}/cases.json")).unwrap()).unwrap();
    let cases = cases["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 5);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let home = scratch(name);
        if let Some(file) = case["file"].as_str() {
            place(&home, file);
        }
        let r = resolve_gateway_url_in(None, &home);
        assert_eq!(r.value, case["expect"].as_str().unwrap(), "case {name}");
        assert_eq!(
            r.warning.is_some(),
            case["warn"].as_bool().unwrap(),
            "case {name}: {:?}",
            r.warning
        );
        if case["file"].is_string() && !case["warn"].as_bool().unwrap() {
            assert!(
                r.source.starts_with("gateway pointer"),
                "case {name}: {}",
                r.source
            );
        }
    }

    // Precedence: flag > env > saved login (the old 8080 gives way) > pointer > 8080.
    let home = scratch("precedence");
    place(&home, "valid.json");
    let save = |url: &str| std::fs::write(&store, format!("{{\"base_url\": \"{url}\"}}")).unwrap();
    save("http://127.0.0.1:8080");
    assert_eq!(
        resolve_gateway_url_in(None, &home).value,
        "http://127.0.0.1:8081",
        "a saved 8080 gives way to the pointer"
    );
    save("http://gw.example:9000");
    let r = resolve_gateway_url_in(None, &home);
    assert_eq!(
        (r.value.as_str(), r.source.starts_with("login")),
        ("http://gw.example:9000", true),
        "a saved login wins"
    );
    std::env::set_var("ABSTRACTGATEWAY_URL", "http://127.0.0.1:7000");
    assert_eq!(
        resolve_gateway_url_in(None, &home).value,
        "http://127.0.0.1:7000",
        "the environment wins"
    );
    assert_eq!(
        resolve_gateway_url_in(Some("http://127.0.0.1:6000/"), &home).value,
        "http://127.0.0.1:6000",
        "the flag wins"
    );
    std::env::remove_var("ABSTRACTGATEWAY_URL");
    // A refused pointer keeps a saved 8080 (and says why once).
    let bad = scratch("bad");
    place(&bad, "non_loopback.json");
    save("http://127.0.0.1:8080");
    let r = resolve_gateway_url_in(None, &bad);
    assert_eq!(r.value, "http://127.0.0.1:8080");
    assert!(
        r.warning
            .as_deref()
            .unwrap()
            .contains("not on this computer"),
        "{:?}",
        r.warning
    );

    // `abstractcode login` saves a URL only when the user gave one: a login
    // resolved through the pointer saves none, so the client keeps
    // following the gateway when it moves to another port.
    let _ = std::fs::remove_file(&store);
    let moving = scratch("moving");
    let ptr = place(&moving, "valid.json");
    let r = resolve_gateway_url_in(None, &moving);
    assert_eq!(r.origin, UrlOrigin::Pointer);
    assert_eq!(r.url_to_save(), None, "a pointer URL is never saved");
    write_login(r.url_to_save(), Some("tok")).unwrap();
    let saved: Value = serde_json::from_slice(&std::fs::read(&store).unwrap()).unwrap();
    assert!(saved.get("base_url").is_none(), "{saved}");
    assert_eq!(saved["token"], "tok");
    let moved = std::fs::read_to_string(&ptr)
        .unwrap()
        .replace("127.0.0.1:8081", "127.0.0.1:18999");
    std::fs::write(&ptr, moved).unwrap();
    assert_eq!(
        resolve_gateway_url_in(None, &moving).value,
        "http://127.0.0.1:18999",
        "after login the client still follows the pointer"
    );
    // The default is never saved either; the flag, the legacy env alias and
    // an already-saved login are.
    let _ = std::fs::remove_file(&store);
    let r = resolve_gateway_url_in(None, &scratch("nothing"));
    assert_eq!((r.origin, r.url_to_save()), (UrlOrigin::Default, None));
    let r = resolve_gateway_url_in(Some("http://gw.example:9000"), &moving);
    assert_eq!(r.url_to_save(), Some("http://gw.example:9000"));
    std::env::set_var("ABSTRACTGATEWAY_URL", "http://127.0.0.1:7000");
    let r = resolve_gateway_url_in(None, &moving);
    assert_eq!(r.url_to_save(), Some("http://127.0.0.1:7000"));
    std::env::remove_var("ABSTRACTGATEWAY_URL");
    write_login(Some("http://gw.example:9000"), Some("tok")).unwrap();
    let r = resolve_gateway_url_in(None, &moving);
    assert_eq!(
        (r.origin, r.url_to_save()),
        (UrlOrigin::Login, Some("http://gw.example:9000"))
    );
    std::env::remove_var("ABSTRACTCODE_GATEWAY_CONNECTION_FILE");
}

#[test]
fn only_a_regular_file_of_the_current_user_is_believed() {
    let home = scratch("owner");
    let path = place(&home, "valid.json");
    assert_eq!(
        read_pointer(&path),
        Pointer::Found {
            url: "http://127.0.0.1:8081".into()
        }
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mine = std::fs::metadata(&path).unwrap().uid();
        match read_pointer_as(&path, mine + 1) {
            Pointer::Refused { warning } => {
                assert!(warning.contains("belongs to another user"), "{warning}")
            }
            other => panic!("a foreign owner must be refused: {other:?}"),
        }
        let link_home = scratch("link");
        let link = pointer_path(&link_home);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&path, &link).unwrap();
        match read_pointer(&link) {
            Pointer::Refused { warning } => {
                assert!(warning.contains("symbolic link"), "{warning}")
            }
            other => panic!("a symlink must be refused: {other:?}"),
        }
    }
    assert_eq!(
        read_pointer(&scratch("none").join("gateway.json")),
        Pointer::Missing
    );
}

// The mode case is LOCAL: the canonical kit fixtures
// (abstractuic ui-kit/scripts/fixtures/gateway_pointer, checksum-synced into
// tests/fixtures/gateway_pointer) have no mode case yet, and the kit's reader
// (app-server gateway_pointer.js, v0.1.14) refuses `mode & 0o022` the same way.
#[cfg(unix)]
#[test]
fn a_pointer_other_users_can_write_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let home = scratch("mode");
    let path = place(&home, "valid.json");
    for (mode, believed) in [(0o600, true), (0o644, true), (0o664, false), (0o646, false)] {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        match read_pointer(&path) {
            Pointer::Found { url } if believed => assert_eq!(url, "http://127.0.0.1:8081"),
            Pointer::Refused { warning } if !believed => assert!(
                warning.contains(&format!("other users can write it (mode {mode:o})")),
                "{warning}"
            ),
            other => panic!("mode {mode:o}: {other:?}"),
        }
    }
}

#[test]
fn an_oversized_pointer_is_refused() {
    let home = scratch("size");
    let path = place(&home, "valid.json");
    // Still a valid pointer JSON, padded past the bound with whitespace.
    let valid = std::fs::read_to_string(&path).unwrap();
    let pad = " ".repeat(MAX_POINTER_BYTES as usize + 1 - valid.len());
    std::fs::write(&path, format!("{valid}{pad}")).unwrap();
    match read_pointer(&path) {
        Pointer::Refused { warning } => assert!(
            warning.contains("larger than the 64 KiB a pointer file may be"),
            "{warning}"
        ),
        other => panic!("an oversized pointer must be refused: {other:?}"),
    }
    // Exactly at the bound is still read.
    let pad = " ".repeat(MAX_POINTER_BYTES as usize - valid.len());
    std::fs::write(&path, format!("{valid}{pad}")).unwrap();
    assert_eq!(
        read_pointer(&path),
        Pointer::Found {
            url: "http://127.0.0.1:8081".into()
        }
    );
}

/// A FIFO in the pointer's place must not hang the client: the open is
/// non-blocking and the fstat refuses it. Run on a thread with a deadline
/// so a regression FAILS instead of hanging the suite.
#[cfg(unix)]
#[test]
fn a_fifo_in_the_pointers_place_never_blocks() {
    use std::os::unix::ffi::OsStrExt;
    let home = scratch("fifo");
    let path = pointer_path(&home);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: a valid NUL-terminated path; mkfifo has no other preconditions.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo");
    let (tx, rx) = std::sync::mpsc::channel();
    let p = path.clone();
    std::thread::spawn(move || {
        let _ = tx.send(read_pointer(&p));
    });
    match rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Pointer::Refused { warning }) => {
            assert!(warning.contains("not a regular file"), "{warning}")
        }
        Ok(other) => panic!("a FIFO must be refused: {other:?}"),
        Err(_) => {
            // Unblock the stuck reader so the process can exit.
            let _ = std::fs::OpenOptions::new().write(true).open(&path);
            panic!("reading a FIFO pointer blocked (no O_NONBLOCK)");
        }
    }
}
