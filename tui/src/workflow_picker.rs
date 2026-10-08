//! The kit's workflow picker core (`ui-kit/src/workflow_picker_core.ts`),
//! in the terminal: `/schedule`'s "What" picker (R17.2). Pure.
//!
//! `GET /api/gateway/bundles?executable_for=abstractcode.agent.v1` lists
//! exactly what the signed-in person may run with this app; the parser FAILS
//! LOUDLY on a gateway that does not honour that contract (no
//! `executable_for` echo, an item without `owner`/`shipped`, an entrypoint
//! that does not declare the interface) — such a gateway would hand the app
//! workflows it cannot run. The rows: "Gateway default" first (detail = what
//! it resolves to, or why it cannot), then Shared, then Mine (name, then the
//! newest version first).

use serde_json::{json, Value};

pub const DEFAULT_VALUE: &str = "@default";
pub const EMPTY: &str = "No workflows available for this app — ask your admin.";
const UPDATE: &str = "update the gateway";

/// One runnable choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// `bundle_id@bundle_version:flow_id`.
    pub value: String,
    pub bundle_id: String,
    pub bundle_version: String,
    pub flow_id: String,
    pub name: String,
    pub description: String,
    /// "shared" (owner gateway) | "mine" (owner user).
    pub group: &'static str,
}

/// What "Gateway default" resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayDefault {
    Ok {
        name: String,
        bundle_id: String,
        bundle_version: String,
        flow_id: String,
    },
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Executable {
    pub entries: Vec<Entry>,
    pub gateway_default: GatewayDefault,
}

/// `bundles?executable_for=<interface>` (the kit's `executableWorkflowsPath`).
pub fn path(interface_id: &str) -> String {
    format!(
        "/api/gateway/bundles?executable_for={}",
        crate::gateway::url_encode(interface_id)
    )
}

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("")
        .to_string()
}

fn default_from(body: &Value, interface_id: &str) -> GatewayDefault {
    let Some(defaults) = body
        .get("default_agent_workflows")
        .filter(|d| d.is_object())
    else {
        return GatewayDefault::Unavailable(
            "the gateway does not report a default workflow".into(),
        );
    };
    let Some(row) = defaults.get(interface_id).filter(|r| r.is_object()) else {
        let why = text(body.pointer(&format!(
            "/default_agent_workflows_unavailable/{}/reason",
            interface_id.replace('~', "~0").replace('/', "~1")
        )));
        return GatewayDefault::Unavailable(if why.is_empty() {
            format!("no default workflow for {interface_id}")
        } else {
            why
        });
    };
    let bundle_id = text(row.get("bundle_id"));
    let flow_id = text(row.get("flow_id"));
    if bundle_id.is_empty() || flow_id.is_empty() {
        return GatewayDefault::Unavailable(
            "the gateway's default workflow is missing its bundle or flow".into(),
        );
    }
    let name = text(row.get("name"));
    GatewayDefault::Ok {
        name: if name.is_empty() {
            flow_id.clone()
        } else {
            name
        },
        bundle_id,
        bundle_version: text(row.get("bundle_version")),
        flow_id,
    }
}

/// The kit's `parseExecutableWorkflows` (per-interface mode).
pub fn parse(envelope: &Value, interface_id: &str) -> Result<Executable, String> {
    if !envelope.is_object() {
        return Err("The gateway's workflow list is not a JSON object.".into());
    }
    let echoed = text(envelope.get("executable_for"));
    if echoed != interface_id {
        return Err(if echoed.is_empty() {
            format!("This gateway does not filter workflows per app (no executable_for in its answer): {UPDATE}.")
        } else {
            format!("The gateway listed workflows for {echoed}, not {interface_id}.")
        });
    }
    let Some(items) = envelope.get("items").and_then(Value::as_array) else {
        return Err("The gateway's workflow list has no items.".into());
    };
    let mut entries = Vec::new();
    for item in items {
        let bundle_id = text(item.get("bundle_id"));
        if !item.is_object() || bundle_id.is_empty() {
            return Err("The gateway listed a workflow without a bundle id.".into());
        }
        let group = match text(item.pointer("/owner/kind")).as_str() {
            "gateway" => "shared",
            "user" => "mine",
            _ => {
                return Err(format!(
                    "The gateway did not say who owns {bundle_id} (owner missing): {UPDATE}."
                ))
            }
        };
        if !item.get("shipped").is_some_and(Value::is_boolean) {
            return Err(format!(
                "The gateway did not say whether {bundle_id} ships with it (shipped missing): {UPDATE}."
            ));
        }
        let bundle_version = text(item.get("bundle_version"));
        for ep in item
            .get("entrypoints")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let flow_id = text(ep.get("flow_id"));
            if !ep.is_object() || flow_id.is_empty() {
                continue;
            }
            let declares = ep
                .get("interfaces")
                .and_then(Value::as_array)
                .is_some_and(|a| {
                    a.iter()
                        .any(|i| i.as_str().map(str::trim) == Some(interface_id))
                });
            if !declares {
                return Err(format!(
                    "The gateway offered {bundle_id}:{flow_id}, which does not declare {interface_id}: {UPDATE}."
                ));
            }
            let value = {
                let w = text(ep.get("workflow_id"));
                if w.is_empty() {
                    format!(
                        "{bundle_id}{}:{flow_id}",
                        if bundle_version.is_empty() {
                            String::new()
                        } else {
                            format!("@{bundle_version}")
                        }
                    )
                } else {
                    w
                }
            };
            let name = [
                text(ep.get("name")),
                text(item.pointer("/metadata/name")),
                bundle_id.clone(),
            ]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or_default();
            let description = [text(ep.get("description")), text(item.get("description"))]
                .into_iter()
                .find(|s| !s.is_empty())
                .unwrap_or_default();
            entries.push(Entry {
                value,
                bundle_id: bundle_id.clone(),
                bundle_version: bundle_version.clone(),
                flow_id,
                name,
                description,
                group,
            });
        }
    }
    Ok(Executable {
        entries,
        gateway_default: default_from(envelope, interface_id),
    })
}

/// Numeric-aware descending version order (`localeCompare(…, {numeric})`).
fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |s: &str| -> Vec<(u64, String)> {
        s.split(['.', '-'])
            .map(|p| (p.parse::<u64>().unwrap_or(0), p.to_string()))
            .collect()
    };
    parts(a).cmp(&parts(b))
}

/// Shared first, then Mine; inside a group: name, then the newest version.
pub fn ordered(entries: &[Entry]) -> Vec<Entry> {
    let mut out = Vec::new();
    for group in ["shared", "mine"] {
        let mut g: Vec<Entry> = entries
            .iter()
            .filter(|e| e.group == group)
            .cloned()
            .collect();
        g.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| version_cmp(&b.bundle_version, &a.bundle_version))
                .then_with(|| a.value.cmp(&b.value))
        });
        out.extend(g);
    }
    out
}

/// The default entry's detail: what it resolves to, or why it cannot.
pub fn gateway_default_detail(d: &GatewayDefault) -> String {
    match d {
        GatewayDefault::Unavailable(reason) => reason.clone(),
        GatewayDefault::Ok {
            name,
            bundle_version,
            ..
        } => {
            if bundle_version.is_empty() {
                name.clone()
            } else {
                format!("{name} @{bundle_version}")
            }
        }
    }
}

/// One option: `None` entry = "Gateway default".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub detail: String,
    pub group: Option<&'static str>,
    pub entry: Option<Entry>,
}

/// Every option in display order: "Gateway default", then Shared, then Mine.
pub fn rows(data: &Executable) -> Vec<Row> {
    let mut out = vec![Row {
        name: "Gateway default".into(),
        detail: gateway_default_detail(&data.gateway_default),
        group: None,
        entry: None,
    }];
    for e in ordered(&data.entries) {
        out.push(Row {
            name: e.name.clone(),
            detail: if e.bundle_version.is_empty() {
                String::new()
            } else {
                format!("@{}", e.bundle_version)
            },
            group: Some(e.group),
            entry: Some(e),
        });
    }
    out
}

/// The automation target of a row (the kit's AutomationWorkflowPicker).
pub fn target_of(row: &Row, interface_id: &str) -> Value {
    match &row.entry {
        None => json!({"flow_id": DEFAULT_VALUE, "interface": interface_id}),
        Some(e) => {
            json!({"bundle_ref": format!("{}@{}", e.bundle_id, e.bundle_version), "flow_id": e.flow_id})
        }
    }
}

/// The workflow whose input schema a row runs: (bundle, version, flow), or
/// the reason the default cannot be resolved.
pub fn schema_key(row: &Row, data: &Executable) -> Result<(String, String, String), String> {
    match (&row.entry, &data.gateway_default) {
        (Some(e), _) => Ok((
            e.bundle_id.clone(),
            e.bundle_version.clone(),
            e.flow_id.clone(),
        )),
        (
            None,
            GatewayDefault::Ok {
                bundle_id,
                bundle_version,
                flow_id,
                ..
            },
        ) => Ok((bundle_id.clone(), bundle_version.clone(), flow_id.clone())),
        (None, GatewayDefault::Unavailable(reason)) => Err(format!("Gateway default: {reason}.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str =
        include_str!("../tests/fixtures/schedule/bundles_executable_code_agent.json");
    const IFACE: &str = "abstractcode.agent.v1";

    #[test]
    fn gateway_default_first_then_shared_by_name() {
        let data = parse(&serde_json::from_str(LIST).unwrap(), IFACE).unwrap();
        let rows = rows(&data);
        assert_eq!(rows[0].name, "Gateway default");
        assert_eq!(rows[0].detail, "Basic agent @0.0.5");
        assert!(rows[0].entry.is_none());
        let names: Vec<&str> = rows[1..].iter().map(|r| r.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        assert_eq!(names, sorted);
        assert_eq!(rows.len(), 1 + data.entries.len());
        let basic = rows.iter().find(|r| r.name == "Basic agent").unwrap();
        assert_eq!(
            target_of(basic, IFACE),
            json!({"bundle_ref": "basic-agent@0.0.5", "flow_id": "81795ea9"})
        );
        assert_eq!(
            target_of(&rows[0], IFACE),
            json!({"flow_id": "@default", "interface": IFACE})
        );
        assert_eq!(
            schema_key(&rows[0], &data).unwrap(),
            ("basic-agent".into(), "0.0.5".into(), "81795ea9".into())
        );
    }

    #[test]
    fn a_gateway_that_ignores_the_contract_fails_loudly() {
        let mut v: Value = serde_json::from_str(LIST).unwrap();
        let mut no_echo = v.clone();
        no_echo.as_object_mut().unwrap().remove("executable_for");
        assert!(parse(&no_echo, IFACE)
            .unwrap_err()
            .contains("does not filter workflows per app"));
        v["items"][0]["entrypoints"][0]["interfaces"] = json!(["other.v1"]);
        assert!(parse(&v, IFACE)
            .unwrap_err()
            .contains("which does not declare"));
        let mut owner: Value = serde_json::from_str(LIST).unwrap();
        owner["items"][0].as_object_mut().unwrap().remove("owner");
        assert!(parse(&owner, IFACE).unwrap_err().contains("owner missing"));
    }

    #[test]
    fn the_routes_are_pinned() {
        assert_eq!(
            path(IFACE),
            "/api/gateway/bundles?executable_for=abstractcode.agent.v1"
        );
        assert_eq!(
            crate::schedule_input::input_schema_path("basic-agent", "81795ea9", "0.0.5"),
            "/api/gateway/bundles/basic-agent/flows/81795ea9/input_schema?bundle_version=0.0.5"
        );
        assert_eq!(
            crate::schedule_input::flow_source_path("basic-agent", "81795ea9", "0.0.5"),
            "/api/gateway/bundles/basic-agent/flows/81795ea9?bundle_version=0.0.5"
        );
        let r = crate::gateway::automations::email_status_request();
        assert_eq!((r.method, r.path.as_str()), ("GET", "/api/gateway/me/email"));
    }

    #[test]
    fn unavailable_default_names_its_reason() {
        let mut v: Value = serde_json::from_str(LIST).unwrap();
        v.as_object_mut().unwrap().remove("default_agent_workflows");
        let data = parse(&v, IFACE).unwrap();
        assert_eq!(
            rows(&data)[0].detail,
            "the gateway does not report a default workflow"
        );
        assert!(schema_key(&rows(&data)[0], &data).is_err());
    }
}
