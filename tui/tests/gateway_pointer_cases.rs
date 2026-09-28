//! The local gateway pointer (`~/.abstractframework/gateway.json`) against the
//! shared case table every reader checks (`tests/fixtures/gateway_pointer`,
//! byte-identical copies of the ui-kit's `scripts/fixtures/gateway_pointer`),
//! and the one URL precedence. ONE test mutates the process environment, so
//! nothing here races on it.

use std::path::{Path, PathBuf};

use serde_json::Value;

use abstractcode::config::resolve_gateway_url_in;
use abstractcode::gateway_pointer::{pointer_path, read_pointer, read_pointer_as, Pointer};

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
                assert!(warning.contains("not a regular file"), "{warning}")
            }
            other => panic!("a symlink must be refused: {other:?}"),
        }
    }
    assert_eq!(
        read_pointer(&scratch("none").join("gateway.json")),
        Pointer::Missing
    );
}
