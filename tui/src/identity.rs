//! Who made this app, and where to find it — the About screen's facts.
//!
//! The AbstractFramework root owns ONE canonical descriptor
//! (`identity/abstractframework.json`); every app vendors a BYTE-IDENTICAL
//! copy so an installed binary stays self-contained. This crate's copy is
//! `assets/abstractframework_identity.json`, compiled in with
//! `include_str!`; the root `scripts/check_identity_sync.py` fails when it
//! drifts. Nothing here is typed by hand: names, links, author, licence and
//! contact all come from the descriptor, the version from Cargo.toml.

use serde_json::Value;

/// The vendored descriptor, verbatim.
pub const IDENTITY_JSON: &str = include_str!("../assets/abstractframework_identity.json");

/// This app's key in the descriptor's `apps` map.
pub const APP_ID: &str = "abstractcode";

/// One app's identity, resolved against the framework's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdentity {
    pub app_id: String,
    pub name: String,
    pub version: String,
    pub framework_name: String,
    pub framework_website: String,
    pub author: String,
    pub years: String,
    pub license: String,
    pub copyright: String,
    pub contact_email: String,
    pub website: String,
    pub repo: String,
    pub docs: String,
    pub issues: String,
    pub feedback: String,
}

fn descriptor() -> Value {
    // A malformed vendored copy is a build-time defect, caught by the tests
    // below and by the root sync check — never a runtime condition.
    serde_json::from_str(IDENTITY_JSON).expect("vendored identity descriptor is valid JSON")
}

/// `app_id`'s identity at `version`. `None` when the descriptor does not
/// list the app (a descriptor/app mismatch — the tests pin ours).
pub fn app_identity(app_id: &str, version: &str) -> Option<AppIdentity> {
    let d = descriptor();
    let fw = d.get("framework")?;
    let app = d.get("apps")?.get(app_id)?;
    let s = |v: &Value, k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    Some(AppIdentity {
        app_id: app_id.to_string(),
        name: s(app, "name"),
        version: version.to_string(),
        framework_name: s(fw, "name"),
        framework_website: s(fw, "website"),
        author: s(fw, "author"),
        years: s(fw, "years"),
        license: s(fw, "license"),
        copyright: s(fw, "copyright"),
        contact_email: s(fw, "contact_email"),
        website: s(app, "website"),
        repo: s(app, "repo"),
        docs: s(app, "docs"),
        issues: s(app, "issues"),
        feedback: s(app, "feedback"),
    })
}

/// AbstractCode's own identity (this crate's version).
pub fn this_app() -> AppIdentity {
    app_identity(APP_ID, env!("CARGO_PKG_VERSION"))
        .expect("the vendored descriptor lists abstractcode")
}

/// The About screen as `(label, value)` rows, in display order. An empty
/// label is a free-standing line. `gateway` = `(version, base_url)` rows
/// the caller knows; nothing is shown for what it does not know.
pub fn about_rows(id: &AppIdentity, gateway: &[(String, String)]) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = vec![
        (String::new(), format!("{} {}", id.name, id.version)),
        (
            String::new(),
            format!("Part of {} — {}", id.framework_name, id.framework_website),
        ),
        (
            String::new(),
            format!("Author: {} ({})", id.author, id.years),
        ),
        (String::new(), id.copyright.clone()),
        ("Website".into(), id.website.clone()),
        ("Source".into(), id.repo.clone()),
        ("Documentation".into(), id.docs.clone()),
        ("Report an issue".into(), id.issues.clone()),
        ("Give feedback".into(), id.feedback.clone()),
        ("Contact".into(), id.contact_email.clone()),
    ];
    rows.extend(gateway.iter().cloned());
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vendored_descriptor_names_this_app() {
        let id = this_app();
        assert_eq!(id.name, "AbstractCode");
        assert_eq!(id.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(id.framework_name, "AbstractFramework");
        assert_eq!(id.framework_website, "https://abstractframework.ai");
        assert_eq!(id.author, "Laurent-Philippe Albou, PhD");
        assert_eq!(id.years, "2023-2026");
        assert!(id.copyright.contains("MIT"));
        for link in [&id.website, &id.repo, &id.docs, &id.issues, &id.feedback] {
            assert!(link.starts_with("https://"), "{link}");
        }
        assert!(id.contact_email.contains('@'));
        assert!(app_identity("no-such-app", "0").is_none());
    }

    #[test]
    fn about_rows_carry_every_required_line() {
        let id = this_app();
        let rows = about_rows(&id, &[("Gateway".into(), "AbstractGateway 0.4.4".into())]);
        let text: Vec<String> = rows
            .iter()
            .map(|(k, v)| {
                if k.is_empty() {
                    v.clone()
                } else {
                    format!("{k}: {v}")
                }
            })
            .collect();
        let all = text.join("\n");
        for needle in [
            &format!("AbstractCode {}", env!("CARGO_PKG_VERSION")),
            "Part of AbstractFramework — https://abstractframework.ai",
            "Author: Laurent-Philippe Albou, PhD (2023-2026)",
            "© 2023-2026 Laurent-Philippe Albou, PhD. Released under the MIT License.",
            "Website: https://abstractframework.ai/code",
            "Source: https://github.com/lpalbou/AbstractCode",
            "Documentation: ",
            "Report an issue: ",
            "Give feedback: ",
            "Contact: contact@abstractframework.ai",
            "Gateway: AbstractGateway 0.4.4",
        ] {
            assert!(all.contains(needle), "missing {needle:?} in\n{all}");
        }
    }

    /// Cargo.toml's homepage/documentation mirror the descriptor.
    #[test]
    fn manifest_links_match_the_descriptor() {
        let manifest = include_str!("../Cargo.toml");
        let id = this_app();
        assert!(manifest.contains(&format!("homepage = \"{}\"", id.website)));
        assert!(manifest.contains(&format!("documentation = \"{}\"", id.docs)));
        assert!(manifest.contains("\"/assets/abstractframework_identity.json\""));
    }
}
