//! Where preferences are read from, and the one-way migration from the
//! location this client used before it was renamed to `abstractcode`.
//!
//! These live in their own integration binary on purpose. They mutate `HOME`
//! and the prefs env overrides, which are process-wide; inside the lib test
//! binary they raced anything else resolving a path from `HOME` (notably
//! `entities::roster_cache_path`, which derives from `prefs_path().parent()`).
//! An integration test gets its own process, so the blast radius is this file,
//! and the mutex below serializes the tests within it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use abstractcode::config::{legacy_prefs_path, prefs_path, Prefs};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard(Vec<(&'static str, Option<String>)>);

impl EnvGuard {
    fn set(vars: &[(&'static str, Option<&str>)]) -> EnvGuard {
        let saved = vars
            .iter()
            .map(|(k, _)| (*k, std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
        EnvGuard(saved)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in &self.0 {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("acode-prefsmig-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".abstractcode-tui")).expect("mkdir legacy");
    fs::create_dir_all(dir.join(".abstractcode")).expect("mkdir current");
    dir
}

fn write_legacy(home: &Path, theme: &str) {
    fs::write(
        home.join(".abstractcode-tui").join("prefs.json"),
        format!(r#"{{"theme":"{theme}","tool_approval":{{"accepted_tier":"all"}}}}"#),
    )
    .expect("write legacy");
}

#[test]
fn an_explicit_prefs_override_never_falls_back_to_the_legacy_file() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = scratch("override");
    write_legacy(&home, "abstract-aurora");
    let fresh = home.join("fresh").join("prefs.json");

    let _guard = EnvGuard::set(&[
        ("HOME", Some(home.to_str().unwrap())),
        ("ABSTRACTCODE_PREFS_FILE", Some(fresh.to_str().unwrap())),
        ("ABSTRACTCODE_TUI_PREFS_FILE", None),
    ]);

    // The override names a file that does not exist yet. Reading the operator's
    // legacy file here would defeat the isolation the override exists to give:
    // the conformance harnesses point it at a fresh path precisely so a run
    // starts from defaults, and the legacy file carries a saved
    // `tool_approval.accepted_tier` that would auto-approve tools in a run the
    // harness believes is clean.
    assert_eq!(legacy_prefs_path(), None);
    assert_eq!(prefs_path(), fresh);
    assert!(Prefs::load().theme.is_none());

    let _ = fs::remove_dir_all(home);
}

#[test]
fn the_legacy_env_override_is_also_honored_exactly() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = scratch("legacy-override");
    write_legacy(&home, "abstract-aurora");
    let fresh = home.join("fresh2").join("prefs.json");

    let _guard = EnvGuard::set(&[
        ("HOME", Some(home.to_str().unwrap())),
        ("ABSTRACTCODE_PREFS_FILE", None),
        ("ABSTRACTCODE_TUI_PREFS_FILE", Some(fresh.to_str().unwrap())),
    ]);

    assert_eq!(legacy_prefs_path(), None);
    assert_eq!(prefs_path(), fresh);

    let _ = fs::remove_dir_all(home);
}

#[test]
fn prefs_from_before_the_rename_are_read_once_and_saved_forward() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = scratch("migrate");
    write_legacy(&home, "abstract-aurora");
    let current = home.join(".abstractcode").join("prefs.json");

    let _guard = EnvGuard::set(&[
        ("HOME", Some(home.to_str().unwrap())),
        ("ABSTRACTCODE_PREFS_FILE", None),
        ("ABSTRACTCODE_TUI_PREFS_FILE", None),
    ]);

    assert_eq!(
        legacy_prefs_path(),
        Some(home.join(".abstractcode-tui").join("prefs.json"))
    );

    let prefs = Prefs::load();
    assert_eq!(prefs.theme.as_deref(), Some("abstract-aurora"));
    // Loaded from the old file, but pointed at the new one.
    assert_eq!(prefs.path.as_ref(), Some(&current));

    prefs.save().expect("save");
    assert!(current.exists(), "the first save migrates the file forward");
    // Once the current file exists the old one is no longer consulted.
    assert_eq!(legacy_prefs_path(), None);
    assert_eq!(Prefs::load().theme.as_deref(), Some("abstract-aurora"));

    let _ = fs::remove_dir_all(home);
}

#[test]
fn the_current_prefs_file_wins_when_both_exist() {
    let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let home = scratch("both");
    write_legacy(&home, "abstract-aurora");
    fs::write(
        home.join(".abstractcode").join("prefs.json"),
        r#"{"theme":"nord"}"#,
    )
    .expect("write current");

    let _guard = EnvGuard::set(&[
        ("HOME", Some(home.to_str().unwrap())),
        ("ABSTRACTCODE_PREFS_FILE", None),
        ("ABSTRACTCODE_TUI_PREFS_FILE", None),
    ]);

    assert_eq!(legacy_prefs_path(), None);
    assert_eq!(Prefs::load().theme.as_deref(), Some("nord"));

    let _ = fs::remove_dir_all(home);
}
