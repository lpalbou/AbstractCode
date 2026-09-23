//! Canonical MTP request intent; discovery and execution remain host-owned.
use serde_json::{json, Value};

pub fn normalize(value: &Value) -> Option<Value> {
    if value == &Value::Bool(false) || value.get("mode").and_then(Value::as_str) == Some("off") {
        return Some(Value::Bool(false));
    }
    if value.get("mode").and_then(Value::as_str) == Some("native_mtp") {
        if let Some(n) = value
            .get("num_draft_tokens")
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
        {
            return Some(
                json!({"mode":"native_mtp","num_draft_tokens":n,"require_acceleration":true}),
            );
        }
    }
    None
}

pub fn parse(input: &str) -> Result<Option<Value>, String> {
    match input.trim().to_ascii_lowercase().as_str() {
        "inherit" | "default" | "clear" => Ok(None),
        "off" => Ok(Some(Value::Bool(false))),
        text => match text.parse::<u64>() {
            Ok(n) if n > 0 => Ok(Some(
                json!({"mode":"native_mtp","num_draft_tokens":n,"require_acceleration":true}),
            )),
            _ => Err("MTP takes inherit | off | a positive integer depth".into()),
        },
    }
}

pub fn label(value: Option<&Value>) -> String {
    match value {
        None => "Inherit".into(),
        Some(Value::Bool(false)) => "Off".into(),
        Some(value) => format!(
            "Depth {}",
            value.get("num_draft_tokens").unwrap_or(&Value::Null)
        ),
    }
}

pub struct Row {
    pub label: String,
    pub value: Option<Value>,
    pub selectable: bool,
}

pub fn rows(payload: Option<&Value>, saved: Option<&Value>) -> Vec<Row> {
    let caps = payload.and_then(|p| p.pointer("/execution/speculation"));
    let default = caps
        .and_then(|c| c.get("effective_default").or_else(|| c.get("default")))
        .and_then(normalize);
    let inherited = default
        .as_ref()
        .map(|v| format!("Inherit ({})", label(Some(v))))
        .unwrap_or_else(|| "Inherit (Core / Gateway default)".into());
    let mut rows = vec![
        Row {
            label: inherited,
            value: None,
            selectable: true,
        },
        Row {
            label: "Off".into(),
            value: Some(Value::Bool(false)),
            selectable: true,
        },
    ];
    if caps
        .and_then(|c| c.get("supported"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        if let Some(depths) = caps
            .and_then(|c| c.get("supported_depths"))
            .and_then(Value::as_array)
        {
            for n in depths.iter().filter_map(Value::as_u64).filter(|n| *n > 0) {
                let value =
                    json!({"mode":"native_mtp","num_draft_tokens":n,"require_acceleration":true});
                if !rows.iter().any(|r| r.value.as_ref() == Some(&value)) {
                    rows.push(Row {
                        label: format!("Depth {n}"),
                        value: Some(value),
                        selectable: true,
                    });
                }
            }
        }
    }
    if let Some(saved) = saved {
        if !rows.iter().any(|r| r.value.as_ref() == Some(saved)) {
            rows.push(Row {
                label: format!("{} (saved; unavailable)", label(Some(saved))),
                value: Some(saved.clone()),
                selectable: false,
            });
        }
    }
    let note = caps
        .and_then(|c| c.get("reason"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            if caps.is_none() {
                Some("MTP capability unknown; no depth is assumed".into())
            } else if caps
                .and_then(|c| c.get("requires_reload"))
                .and_then(Value::as_bool)
                == Some(true)
            {
                Some("Model reload required".into())
            } else if caps.and_then(|c| c.get("ready")).and_then(Value::as_bool) != Some(true) {
                Some("Loaded-instance MTP readiness is not confirmed".into())
            } else {
                None
            }
        });
    if let Some(note) = note {
        rows.push(Row {
            label: note,
            value: None,
            selectable: false,
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intent_preserves_off_and_strict_depth() {
        assert_eq!(parse("off").unwrap(), Some(json!(false)));
        assert_eq!(parse("inherit").unwrap(), None);
        assert_eq!(
            parse("3").unwrap(),
            Some(json!({"mode":"native_mtp","num_draft_tokens":3,"require_acceleration":true}))
        );
        for bad in ["0", "-1", "2.5", "on"] {
            assert!(parse(bad).is_err());
        }
    }
    #[test]
    fn unknown_does_not_fabricate_depths() {
        assert_eq!(rows(None, None).iter().filter(|r| r.selectable).count(), 2);
        let payload = json!({"execution":{"speculation":{"supported":true,"ready":false,"supported_depths":[2,3,4,5],"requires_reload":true}}});
        assert_eq!(
            rows(Some(&payload), None)
                .iter()
                .filter(|r| r.selectable)
                .count(),
            6
        );
        assert!(rows(Some(&payload), None)
            .iter()
            .any(|r| r.label.contains("reload")));
    }
}
