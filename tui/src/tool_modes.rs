//! Three-state editing over the existing allowlist and approval pins.
use crate::{
    store::ToolInfo,
    tool_policy::{self, Tier},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    Ask,
    Auto,
}
impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Ask => "Ask",
            Self::Auto => "Auto",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Ask,
            Self::Ask => Self::Auto,
            Self::Auto => Self::Off,
        }
    }
}

pub fn effective(
    tool: &ToolInfo,
    disabled: &[String],
    pins: &[(String, String)],
    tier: &str,
) -> Mode {
    if tool.served_disabled || disabled.contains(&tool.name) {
        return Mode::Off;
    }
    let classes = vec![tool_policy::ToolClass::from(tool)];
    if tool_policy::call_auto_approves(&tool.name, Tier::parse_or_default(tier), pins, &classes) {
        Mode::Auto
    } else {
        Mode::Ask
    }
}

/// Apply to the current inventory only. A server-disabled member is never grantable.
pub fn set(
    tools: &[ToolInfo],
    names: &[String],
    mode: Mode,
    disabled: &mut Vec<String>,
    pins: &mut Vec<(String, String)>,
) -> usize {
    disabled.retain(|n| tools.iter().any(|t| &t.name == n));
    let mut changed = 0;
    for tool in tools
        .iter()
        .filter(|t| !t.served_disabled && names.contains(&t.name))
    {
        disabled.retain(|n| n != &tool.name);
        pins.retain(|(n, _)| n != &tool.name);
        match mode {
            Mode::Off => disabled.push(tool.name.clone()),
            Mode::Ask => pins.push((tool.name.clone(), "ask".into())),
            Mode::Auto => pins.push((tool.name.clone(), "auto".into())),
        }
        changed += 1;
    }
    disabled.sort();
    disabled.dedup();
    pins.sort_by(|a, b| a.0.cmp(&b.0));
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn category_modes_preserve_server_gates_other_categories_and_wire_approval() {
        let tools = vec![
            ToolInfo {
                name: "read_file".into(),
                toolset: "files".into(),
                ..Default::default()
            },
            ToolInfo {
                name: "write_file".into(),
                toolset: "files".into(),
                ..Default::default()
            },
            ToolInfo {
                name: "blocked".into(),
                toolset: "files".into(),
                served_disabled: true,
                ..Default::default()
            },
            ToolInfo {
                name: "web_search".into(),
                toolset: "web".into(),
                ..Default::default()
            },
        ];
        let names = tools
            .iter()
            .filter(|t| t.toolset == "files")
            .map(|t| t.name.clone())
            .collect::<Vec<_>>();
        let classes = tools
            .iter()
            .map(tool_policy::ToolClass::from)
            .collect::<Vec<_>>();
        let mut disabled = vec!["web_search".into()];
        let mut pins = Vec::new();
        for mode in [Mode::Ask, Mode::Auto, Mode::Off] {
            assert_eq!(set(&tools, &names, mode, &mut disabled, &mut pins), 2);
            assert!(disabled.contains(&"web_search".into()));
            assert!(!pins.iter().any(|(n, _)| n == "blocked"));
            assert!(!disabled.contains(&"blocked".into()));
            assert_eq!(effective(&tools[0], &disabled, &pins, "all"), mode);
            assert_eq!(effective(&tools[1], &disabled, &pins, "read"), mode);
            assert_eq!(
                effective(&tools[2], &[], &[("blocked".into(), "auto".into())], "all"),
                Mode::Off
            );
            let policy = tool_policy::expand_run_policy(&classes, "read", &pins);
            let opts = crate::run_input::StartOpts {
                tools: Some(
                    tools
                        .iter()
                        .filter(|t| !t.served_disabled && !disabled.contains(&t.name))
                        .map(|t| t.name.clone())
                        .collect(),
                ),
                tool_policy: policy,
                ..Default::default()
            };
            let body = crate::run_input::build_input_data("test", &opts);
            let allowed = body["tools"].as_array().unwrap();
            assert!(!allowed.iter().any(|n| n == "blocked" || n == "web_search"));
            match mode {
                Mode::Off => assert!(allowed.is_empty()),
                Mode::Ask => assert!(body["_runtime"]["tool_policy"]["require_approval_tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|n| n == "read_file")),
                Mode::Auto => assert!(body["_runtime"]["tool_policy"]["auto_approve_tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|n| n == "write_file")),
            }
        }
    }
}
