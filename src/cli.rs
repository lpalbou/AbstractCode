//! Command line: argument parsing, `login`, `doctor`, and shared options.

use crate::config;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Default)]
pub struct Args {
    pub subcommand: Option<String>,
    pub prompt: Option<String>,
    pub gateway: Option<String>,
    pub token: Option<String>,
    pub session: Option<String>,
    pub workflow: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub workspace: Option<String>,
    pub no_workspace: bool,
    pub workspace_mode: Option<String>,
    pub theme: Option<String>,
    pub max_iterations: u32,
    /// Prior turns replayed in full detail at boot (0 disables).
    pub replay_turns: usize,
    pub approve_all: bool,
    pub timeout_secs: u64,
    pub show_caps: bool,
    pub show_help: bool,
    pub show_version: bool,
}

pub fn usage() -> String {
    format!(
        r#"abstractcode-tui {VERSION} — AbstractCode on AbstractTUI (gateway client)

USAGE:
  abstractcode-tui [OPTIONS]                    launch the TUI
  abstractcode-tui exec "<prompt>" [OPTIONS]    headless one-shot run (prints events)
  abstractcode-tui login [OPTIONS]              verify + persist gateway credentials
  abstractcode-tui doctor [OPTIONS]             diagnose the gateway connection
  abstractcode-tui --caps                       print the terminal capability report

OPTIONS:
  --gateway <URL>         gateway base url (default: login store or http://127.0.0.1:8080)
  --token <TOKEN>         bearer token (default: env or login store)
  --session <ID>          durable session id (default: last used / minted)
  --workflow <B[:F]>      agent workflow bundle[:flow] (default: saved or basic-agent)
  --provider <NAME>       provider override (default: gateway defaults)
  --model <NAME>          model override
  --workspace <PATH>      workspace root for tools (default: current directory)
  --no-workspace          do not send a workspace root
  --workspace-mode <M>    workspace access mode (e.g. all_except_ignored)
  --theme <ID>            start theme (26 built-in; /theme lists them)
  --max-iterations <N>    agent iteration budget (default: 50)
  --replay-turns <N>      prior turns replayed in full at boot (default: 20; 0 disables)
  --approve-all           exec: auto-approve tool calls (default: deny)
  --timeout <SECS>        exec: give up after SECS (default: 900)
  -h, --help              this help
  -V, --version           version

ENVIRONMENT:
  ABSTRACTCODE_GATEWAY_URL / ABSTRACTFLOW_GATEWAY_URL / ABSTRACTGATEWAY_URL
      gateway url (first set wins; beats the login store)
  ABSTRACTCODE_GATEWAY_TOKEN / ABSTRACTGATEWAY_AUTH_TOKEN / ABSTRACTFLOW_GATEWAY_AUTH_TOKEN
      bearer token
  ABSTRACTCODE_GATEWAY_CONNECTION_FILE   login store path (default ~/.abstractcode/gateway.json)
  ABSTRACTCODE_TUI_PREFS_FILE            preferences path (default ~/.abstractcode-tui/prefs.json)
  ABSTRACTTUI_THEME                      start theme

`login` takes credentials from flags/env (it never prompts) and persists them
to the store shared with the Python CLI: ~/.abstractcode/gateway.json.
"#
    )
}

pub fn parse(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        max_iterations: 50,
        replay_turns: crate::runner::REHYDRATE_DEFAULT_TURNS,
        timeout_secs: 900,
        ..Args::default()
    };
    let mut i = 0;
    let mut positionals: Vec<String> = Vec::new();
    while i < argv.len() {
        let a = argv[i].as_str();
        let mut take = |name: &str| -> Result<String, String> {
            i += 1;
            argv.get(i)
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match a {
            "-h" | "--help" => args.show_help = true,
            "-V" | "--version" => args.show_version = true,
            "--caps" => args.show_caps = true,
            "--gateway" | "--gateway-url" => args.gateway = Some(take(a)?),
            "--token" => args.token = Some(take(a)?),
            "--session" => args.session = Some(take(a)?),
            "--workflow" | "--agent" => args.workflow = Some(take(a)?),
            "--provider" => args.provider = Some(take(a)?),
            "--model" => args.model = Some(take(a)?),
            "--workspace" => args.workspace = Some(take(a)?),
            "--no-workspace" => args.no_workspace = true,
            "--workspace-mode" => args.workspace_mode = Some(take(a)?),
            "--theme" => args.theme = Some(take(a)?),
            "--max-iterations" => {
                let v = take(a)?;
                args.max_iterations = v
                    .parse::<u32>()
                    .map_err(|_| format!("--max-iterations: not a number: {v}"))?;
            }
            "--replay-turns" => {
                let v = take(a)?;
                args.replay_turns = v
                    .parse::<usize>()
                    .map_err(|_| format!("--replay-turns: not a number: {v}"))?
                    .min(100);
            }
            "--approve-all" | "--auto-approve" => args.approve_all = true,
            "--timeout" => {
                let v = take(a)?;
                args.timeout_secs = v
                    .parse::<u64>()
                    .map_err(|_| format!("--timeout: not a number: {v}"))?;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option {other} (see --help)"));
            }
            other => positionals.push(other.to_string()),
        }
        i += 1;
    }
    let mut positionals = positionals.into_iter();
    if let Some(first) = positionals.next() {
        match first.as_str() {
            "exec" | "login" | "doctor" => {
                args.subcommand = Some(first);
                args.prompt = positionals.next();
            }
            other => {
                return Err(format!(
                    "unknown subcommand {other} (expected exec, login, or doctor)"
                ));
            }
        }
    }
    if positionals.next().is_some() {
        return Err("too many positional arguments".into());
    }
    Ok(args)
}

/// Split `bundle[:flow]` into (bundle, flow?).
pub fn split_workflow_ref(raw: &str) -> (String, Option<String>) {
    match raw.split_once(':') {
        Some((b, f)) if !f.trim().is_empty() => (b.trim().to_string(), Some(f.trim().to_string())),
        _ => (raw.trim().to_string(), None),
    }
}

// ---------------------------------------------------------------------------
// login / doctor
// ---------------------------------------------------------------------------

pub fn login(args: &Args) -> i32 {
    let url = config::resolve_gateway_url(args.gateway.as_deref());
    let (token, token_source) = config::resolve_gateway_token(args.token.as_deref());
    let client = crate::gateway::GatewayClient::new(&url.value, token.as_deref());
    match client.ping() {
        Ok(_) => match config::write_login(&url.value, token.as_deref()) {
            Ok(path) => {
                println!("✓ authenticated against {} (ping ok)", url.value);
                println!("✓ saved to {} (0600)", path.display());
                if url.source.starts_with("env") || token_source.starts_with("env") {
                    println!("  note: env vars override the saved login when set (doctor shows which source wins).");
                }
                0
            }
            Err(e) => {
                eprintln!("✗ verified but could not save login: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("✗ gateway at {} refused: {e}", url.value);
            eprintln!("  Nothing saved. Is the gateway running? Is the token right?");
            if e.status.is_none() {
                2
            } else {
                1
            }
        }
    }
}

pub fn doctor(args: &Args) -> i32 {
    let url = config::resolve_gateway_url(args.gateway.as_deref());
    let (token, token_source) = config::resolve_gateway_token(args.token.as_deref());
    println!("abstractcode-tui ⇄ gateway doctor");
    println!("  URL:   {}   (source: {})", url.value, url.source);
    println!(
        "  Token: {}   (source: {})",
        if token.is_some() { "present" } else { "none" },
        token_source
    );

    // 1) Reachability (no auth): any HTTP response = server up.
    let anon = crate::gateway::GatewayClient::new(&url.value, None);
    let reachable = match anon.ping() {
        Ok(_) => {
            println!("  [1/3] reachability   ✓ server up (no auth required)");
            true
        }
        Err(e) if e.status.is_some() => {
            println!("  [1/3] reachability   ✓ server up (auth enforced)");
            true
        }
        Err(e) => {
            println!("  [1/3] reachability   ✗ {e}");
            false
        }
    };
    if !reachable {
        println!(
            "Result: UNREACHABLE — start one with `abstractgateway serve` or fix the URL (login)."
        );
        return 2;
    }

    // 2) Authentication.
    let client = crate::gateway::GatewayClient::new(&url.value, token.as_deref());
    let auth_ok = match client.ping() {
        Ok(_) => {
            println!("  [2/3] authentication ✓ ping ok");
            true
        }
        Err(e) => {
            println!("  [2/3] authentication ✗ {e} — run `abstractcode-tui login`");
            false
        }
    };

    // 3) Agent workflow catalog.
    let mut catalog_ok = false;
    if auth_ok {
        match client.list_bundles() {
            Ok(v) => {
                let flows = crate::runner::agent_workflows_from_bundles(&v);
                // An empty agent catalog is DEGRADED, not healthy: the TUI
                // cannot start a run without an agent.v1 entrypoint.
                catalog_ok = !flows.is_empty();
                let names: Vec<String> = flows
                    .iter()
                    .map(|w| format!("{}:{}", w.bundle_id, w.flow_id))
                    .collect();
                if flows.is_empty() {
                    println!("  [3/3] catalog        ✗ no agent workflows (abstractcode.agent.v1) — install the basic-agent bundle on the gateway");
                } else {
                    println!(
                        "  [3/3] catalog        ✓ {} agent workflow(s): {}",
                        flows.len(),
                        names.join(", ")
                    );
                }
            }
            Err(e) => println!("  [3/3] catalog        ✗ {e}"),
        }
    } else {
        println!("  [3/3] catalog        - skipped (authentication failed)");
    }

    let healthy = auth_ok && catalog_ok;
    println!("Result: {}", if healthy { "HEALTHY" } else { "DEGRADED" });
    if healthy {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_defaults_and_flags() {
        let args = parse(&[]).unwrap();
        assert!(args.subcommand.is_none());
        assert_eq!(args.max_iterations, 50);

        let argv: Vec<String> = ["exec", "do things", "--approve-all", "--model", "m1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let args = parse(&argv).unwrap();
        assert_eq!(args.subcommand.as_deref(), Some("exec"));
        assert_eq!(args.prompt.as_deref(), Some("do things"));
        assert!(args.approve_all);
        assert_eq!(args.model.as_deref(), Some("m1"));
    }

    #[test]
    fn parse_rejects_unknown() {
        assert!(parse(&["--frobnicate".to_string()]).is_err());
        assert!(parse(&["frobnicate".to_string()]).is_err());
        assert!(parse(&["--model".to_string()]).is_err());
    }

    #[test]
    fn workflow_ref_split() {
        assert_eq!(
            split_workflow_ref("coding-agent:coder"),
            ("coding-agent".into(), Some("coder".into()))
        );
        assert_eq!(
            split_workflow_ref("basic-agent"),
            ("basic-agent".into(), None)
        );
    }
}
