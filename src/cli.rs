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
    /// `--resume` — reopen the LAST session instead of starting fresh
    /// (operator ruling 2026-07-26: launch starts a NEW session by
    /// default; continuity is explicit, via this flag, `--session`, or
    /// the in-app `/sessions` picker).
    pub resume: bool,
    pub workflow: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub workspace: Option<String>,
    pub no_workspace: bool,
    pub workspace_mode: Option<String>,
    pub theme: Option<String>,
    pub max_iterations: u32,
    /// Operator-declared model context window in tokens (CTX-0);
    /// 0 = not declared. Session-scoped; `/context` persists.
    pub max_tokens: u64,
    /// Prior turns replayed in full detail at boot (0 disables).
    pub replay_turns: usize,
    /// `--reasoning <none|minimal|low|medium|high|xhigh|auto>` — reasoning
    /// effort override for the session route (first-citizen directive).
    /// Validated at parse; empty = gateway default.
    pub reasoning: Option<String>,
    /// `--ungated` — run a gating-capable workflow (the multi-agent
    /// coder) unattended, skipping its human-approval pauses
    /// (`gating_mode=auto`). REFUSED unless `--permissions` is also set
    /// on the same command line: ungated + unattended is exactly when an
    /// unwatched tool could run, so the operator must choose the tool
    /// posture explicitly (never a silent default).
    pub ungated: bool,
    /// `--permissions <read|write|all>` — the tool-permission level for
    /// this invocation (validated at parse; c5028 consolidation). `None`
    /// = the persisted prefs level applies.
    pub permissions: Option<String>,
    /// `--require-approval <name[,name]>` — per-tool ask pins for this
    /// invocation (repeatable, accumulated). In headless exec an
    /// ask-pinned tool DENIES (no interactive user); the TUI prompts.
    pub require_approval: Vec<String>,
    /// `exec --attach <path>` — files uploaded to the gateway before the
    /// run starts, riding as `context.attachments` (repeatable). Any
    /// failure exits 1 BEFORE `runs/start` — nothing spent.
    pub attach: Vec<String>,
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
  --session <ID>          durable session id (default: a fresh session)
  --resume                reopen the last session (also: --continue)
  --reasoning <LEVEL>     reasoning effort: none|minimal|low|medium|high|xhigh|auto
  --ungated               run a gating-capable workflow unattended (skips its
                          human approval pauses); requires --permissions
  --workflow <B[:F]>      agent workflow bundle[:flow] (default: saved or basic-agent)
  --provider <NAME>       provider override (default: gateway defaults)
  --model <NAME>          model override
  --workspace <PATH>      workspace root for tools (default: current directory)
  --no-workspace          do not send a workspace root
  --workspace-mode <M>    workspace access mode: workspace_only |
                          workspace_or_allowed | all_except_ignored
                          (default: server-managed; /workspace edits + persists)
  --theme <ID>            start theme (26 built-in; /theme lists them)
  --max-iterations <N>    agent iteration budget (default: 50)
  --max-tokens <N>        declare the model's context window in tokens
                          (e.g. 262144 or 262k) — drives the ctx N/M (%)
                          meter and rides runs as _limits.max_tokens;
                          /context <tokens> sets + persists it
                          (aliases: --context, --context-window)
  --replay-turns <N>      prior turns replayed in full at boot (default: 20; 0 disables)
  --permissions <LEVEL>   tool permissions for this invocation: read | write | all
                          (all = every tool auto-approves; per-tool 'ask' pins and
                          gateway-disabled tools still gate)
  --require-approval <T>  gate these tools regardless of level (comma-separated,
                          repeatable); headless exec DENIES them, the TUI prompts
  --attach <PATH>         exec: attach a file to the prompt (repeatable; uploads
                          to the gateway before the run starts, exits 1 on failure)
  --timeout <SECS>        exec: give up after SECS (default: 900)
  -h, --help              this help
  -V, --version           version

CONFIG (prefs.json — the TUI writes it; headless `exec` reads the SAME file):
  tool_approval.accepted_tier   read | write | all — tool batches at-or-below
                                auto-approve; above it the TUI prompts and
                                exec DENIES (naming the rule). /permissions.
  tool_approval.overrides       {{"tool_name": "auto"|"ask"}} per-tool pins.
  workspace_mode                access mode sent with runs (/workspace).
  workspace_allowed             extra allowlisted roots sent as
                                workspace_allowed_paths (/workspace).
  context_window                operator-declared model context window in
                                tokens (/context; 0 = undeclared) — drives
                                the footer's ctx used/window (%) meter.
  The gateway enforces workspace policy server-side; it may clamp client
  paths to operator-controlled roots.

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
            "--resume" | "--continue" => args.resume = true,
            "--ungated" | "--no-gate" | "--auto" => args.ungated = true,
            "--reasoning" | "--thinking" => {
                let v = take(a)?.trim().to_ascii_lowercase();
                if !crate::config::valid_reasoning_level(&v) {
                    return Err(format!(
                        "--reasoning takes none|minimal|low|medium|high|xhigh|auto (got {v:?})"
                    ));
                }
                args.reasoning = Some(v);
            }
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
            "--max-tokens" | "--context" | "--context-window" => {
                let v = take(a)?;
                args.max_tokens = crate::config::parse_token_count(&v)
                    .ok_or_else(|| format!("{a}: not a token count: {v} (try 262144 or 262k)"))?;
            }
            "--replay-turns" => {
                let v = take(a)?;
                args.replay_turns = v
                    .parse::<usize>()
                    .map_err(|_| format!("--replay-turns: not a number: {v}"))?
                    .min(100);
            }
            "--permissions" | "--permission" => {
                let v = take(a)?;
                if crate::tool_policy::Tier::parse(&v).is_none() {
                    return Err(format!(
                        "--permissions: unknown level {v:?} — expected read, write, or all"
                    ));
                }
                args.permissions = Some(v);
            }
            "--require-approval" => {
                let v = take(a)?;
                args.require_approval.extend(
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                );
            }
            "--attach" => args.attach.push(take(a)?),
            // Removed spellings TEACH the replacement (hard error, never
            // a silent alias: the semantics changed — the level persists
            // pins, where the old flag silently bypassed them).
            "--approve-all" | "--auto-approve" => {
                return Err(
                    "--approve-all was removed — use --permissions all (per-tool 'ask' \
                     pins still gate; --require-approval <names> adds gates)"
                        .to_string(),
                );
            }
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
                let flows = crate::discovery::agent_workflows_from_bundles(&v);
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
        assert_eq!(args.max_tokens, 0, "window undeclared by default");

        let argv: Vec<String> = ["exec", "do things", "--permissions", "all", "--model", "m1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let args = parse(&argv).unwrap();
        assert_eq!(args.subcommand.as_deref(), Some("exec"));
        assert_eq!(args.prompt.as_deref(), Some("do things"));
        assert_eq!(args.permissions.as_deref(), Some("all"));
        assert_eq!(args.model.as_deref(), Some("m1"));
    }

    #[test]
    fn permissions_flag_validates_and_removed_spellings_teach() {
        // Valid levels parse; garbage refuses AT PARSE with the three
        // levels named (never a silent misconfiguration at run time).
        let ok = parse(&[
            "exec".into(),
            "x".into(),
            "--permissions".into(),
            "write".into(),
        ])
        .unwrap();
        assert_eq!(ok.permissions.as_deref(), Some("write"));
        let err = parse(&[
            "exec".into(),
            "x".into(),
            "--permissions".into(),
            "yolo".into(),
        ])
        .unwrap_err();
        assert!(
            err.contains("read, write, or all"),
            "refusal teaches the levels: {err}"
        );
        // --require-approval accumulates across repeats + comma lists.
        let args = parse(&[
            "exec".into(),
            "x".into(),
            "--require-approval".into(),
            "write_file,execute_command".into(),
            "--require-approval".into(),
            "fetch_url".into(),
        ])
        .unwrap();
        assert_eq!(
            args.require_approval,
            vec!["write_file", "execute_command", "fetch_url"]
        );
        // The removed flag TEACHES its replacement (hard error — the
        // semantics changed: pins gate even at all, where the old flag
        // silently bypassed them).
        let err = parse(&["exec".into(), "x".into(), "--approve-all".into()]).unwrap_err();
        assert!(
            err.contains("--permissions all"),
            "removed spelling teaches: {err}"
        );
    }

    #[test]
    fn ungated_flag_parses_all_spellings_and_defaults_off() {
        assert!(!parse(&[]).unwrap().ungated, "gated is the default");
        for flag in ["--ungated", "--no-gate", "--auto"] {
            assert!(
                parse(&[flag.to_string()]).unwrap().ungated,
                "{flag} arms ungated"
            );
        }
    }

    #[test]
    fn resume_flag_parses_both_spellings_and_defaults_off() {
        // Operator ruling 2026-07-26: launch = fresh session; --resume
        // (or --continue) is the explicit continuity act.
        let a = parse(&[]).unwrap();
        assert!(!a.resume, "fresh session is the default posture");
        for flag in ["--resume", "--continue"] {
            let a = parse(&[flag.to_string()]).unwrap();
            assert!(a.resume, "{flag} arms resume");
        }
    }

    #[test]
    fn parse_rejects_unknown() {
        assert!(parse(&["--frobnicate".to_string()]).is_err());
        assert!(parse(&["frobnicate".to_string()]).is_err());
        assert!(parse(&["--model".to_string()]).is_err());
    }

    #[test]
    fn parse_max_tokens_accepts_counts_and_refuses_junk() {
        let argv: Vec<String> = ["--max-tokens", "262k"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(parse(&argv).unwrap().max_tokens, 262_000);
        let argv: Vec<String> = ["--max-tokens", "262144"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(parse(&argv).unwrap().max_tokens, 262_144);
        let argv: Vec<String> = ["--max-tokens", "lots"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(parse(&argv).is_err(), "junk refuses loudly");
        // Aliases: the /context spelling and the prefs-key spelling both
        // land on the same declaration; errors name the flag AS TYPED.
        let argv: Vec<String> = ["--context-window", "128k"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(parse(&argv).unwrap().max_tokens, 128_000);
        let argv: Vec<String> = ["--context", "32768"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(parse(&argv).unwrap().max_tokens, 32_768);
        let argv: Vec<String> = ["--context-window", "lots"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let err = parse(&argv).unwrap_err();
        assert!(
            err.contains("--context-window"),
            "the error names the flag as typed: {err}"
        );
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
