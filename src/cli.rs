//! Command line: argument parsing, `login`, `doctor`, and shared options.

use crate::config;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Verifier rounds when review is on — 3, the value the Python
/// `abstractcode` client has always used (`react_shell.py:252`).
pub const DEFAULT_REVIEW_ROUNDS: u32 = 3;

/// `#[WARNING:TIMEOUT]` — `exec`'s wall-clock safeguard, 7200s (2h).
///
/// Source of the number: ADR-0014 §2 sets 7200s as the per-effect
/// orchestrated default and states these defaults "do not impose any maximum
/// duration on a workflow/run". ADR-0027 §2 forbids low defaults on
/// correctness-critical paths and prefers "no client-side timeout … or a very
/// high safeguard (e.g., 2h)"; §3 allows timeouts only as explicit,
/// documented, auditable safeguards.
///
/// This replaced a 900s default that abandoned any genuinely complex build
/// after 15 minutes. `--timeout 0` disables the safeguard outright, for runs
/// that should never be interrupted by this client.
pub const DEFAULT_EXEC_TIMEOUT_SECS: u64 = 7200;

/// Verifier-before-conclude is ON unless the operator says otherwise.
///
/// Aimed at the premature-completion gap, with a precise parity claim —
/// the loose version of it does not survive reading the Python source.
///
/// abstractcode has TWO agent paths. Its NATIVE loops (`--agent react`,
/// `--agent codeact`) are constructed with `review_mode=True,
/// review_max_rounds=3` (`react_shell.py:251-252`, `agents/react.py:189-194`).
/// Its `WorkflowAgent` path — the one that runs gateway BUNDLES, i.e. the
/// closest analogue to this whole client — passes no review kwargs at all
/// (`workflow_agent.py:1192-1200`). So this is parity with abstractcode's
/// native-loop default, EXTENDED to the bundles this client runs; it is not a
/// behaviour abstractcode exhibits when it runs the same bundle.
///
/// Costs one extra verifier LLM call per candidate final answer. Reach is
/// limited: see `run_input::StartOpts::review_mode`. Not sent for memact,
/// which has no review nodes (matching `react_shell.py:775-779`).
pub const DEFAULT_REVIEW_MODE: bool = true;

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
    /// `--no-project-context` — do NOT inject the workspace's `AGENTS.md`
    /// into the agent's system prompt. Injection is the default (parity with
    /// the Python `abstractcode` client); this opts a scripted run out when
    /// it needs a byte-exact prompt independent of the repo's files.
    pub no_project_context: bool,
    /// `--review` / `--no-review` — verifier-before-conclude posture
    /// (`_runtime.review_mode`). `None` = the client default (ON, matching
    /// the Python `abstractcode` client); `Some(false)` pins it off.
    pub review: Option<bool>,
    /// `--review-rounds <N>` — verifier round budget (default 3, as
    /// abstractcode uses). 0 leaves the loop's own default.
    pub review_rounds: u32,
    pub workspace_mode: Option<String>,
    pub theme: Option<String>,
    pub max_iterations: u32,
    /// `--max-iterations` was given on THIS command line (vs the built-in
    /// default). Only an explicit budget rides `_limits` — see `run_input.rs`.
    pub max_iterations_explicit: bool,
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
    /// `exec --param KEY=VALUE` — extra `input_data` keys for workflow input
    /// pins (repeatable). Deterministic orchestration needs parameterizable
    /// pins: the ralph loop's `verify_command`/`max_steps_per_cycle` are
    /// declared start pins the client previously had no way to reach (the
    /// Python client has had `--param` all along). Values parse as JSON
    /// scalars when they look like one (numbers, booleans), else ride as
    /// strings. Reserved keys the client owns (prompt, workspace_root, …)
    /// are refused at parse rather than silently clobbered.
    pub params: Vec<(String, String)>,
    pub timeout_secs: u64,
    /// `--no-prompt-cache` — opt this run OUT of the runtime's prompt-cache
    /// prepare/reuse lane (`_runtime.prompt_cache = false`). Absent = the
    /// gateway/runtime default (on), byte-parity for every existing caller.
    /// Exists so a single gateway can serve an A/B measurement: the cached
    /// and uncached lanes differ only by this key.
    pub no_prompt_cache: bool,
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
  --workflow <B[:F]>      agent workflow bundle[:flow] (default: saved, else
                          coding-agent:coder — the verified coding loop)
  --provider <NAME>       provider override (default: gateway defaults)
  --model <NAME>          model override
  --workspace <PATH>      workspace root for tools (default: current directory)
  --no-workspace          do not send a workspace root
  --no-project-context    do not inject the workspace AGENTS.md into the
                          agent system prompt (injected by default)
  --no-prompt-cache       opt this run out of the runtime prompt cache
                          (_runtime.prompt_cache=false); default: server truth
  --review / --no-review  verifier-before-conclude: before accepting a
                          tool-call-free response as final, a strict
                          verifier re-reads the transcript and can force
                          more tool calls (default: on; /review toggles)
  --review-rounds <N>     verifier round budget (default 3)
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
  --param <K=V>           exec: extra input_data key for a workflow input pin
                          (repeatable; numbers/booleans parse, else string —
                          e.g. --param verify_command='node --check game.js'
                          --param max_steps_per_cycle=16)
  --timeout <SECS>        exec: wall-clock safeguard, 0 = none (default: 7200 = 2h;
                          ADR-0014/0027 — a complex agentic run may take hours,
                          so this never doubles as a performance knob)
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
        // Parity with abstractcode's ReAct default (`react_shell.py:252`).
        review_rounds: DEFAULT_REVIEW_ROUNDS,
        replay_turns: crate::runner::REHYDRATE_DEFAULT_TURNS,
        // #[WARNING:TIMEOUT] exec wall-clock safeguard. ADR-0027 §2 forbids
        // low defaults on correctness-critical paths and prefers none or a
        // very high safeguard; ADR-0014 sets 7200s per effect and states that
        // defaults impose NO maximum duration on a run. The old 900s default
        // gave up on any genuinely complex build — a coding agent can work for
        // an hour — and the abandoned run kept burning tokens server-side.
        // `--timeout 0` disables the safeguard entirely.
        timeout_secs: DEFAULT_EXEC_TIMEOUT_SECS,
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
            "--no-project-context" => args.no_project_context = true,
            "--no-prompt-cache" => args.no_prompt_cache = true,
            "--review" => args.review = Some(true),
            "--no-review" => args.review = Some(false),
            "--review-rounds" => {
                args.review_rounds = take(a)?
                    .parse()
                    .map_err(|_| "--review-rounds needs a number".to_string())?
            }
            "--workspace-mode" => args.workspace_mode = Some(take(a)?),
            "--theme" => args.theme = Some(take(a)?),
            "--max-iterations" => {
                let v = take(a)?;
                args.max_iterations = v
                    .parse::<u32>()
                    .map_err(|_| format!("--max-iterations: not a number: {v}"))?;
                args.max_iterations_explicit = true;
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
            "--param" => {
                let v = take(a)?;
                let (k, val) = v
                    .split_once('=')
                    .ok_or_else(|| format!("--param: expected KEY=VALUE, got {v}"))?;
                let k = k.trim();
                const RESERVED: [&str; 8] = [
                    "prompt",
                    "workspace_root",
                    "workspace_access_mode",
                    "gating_mode",
                    "provider",
                    "model",
                    "use_session_history",
                    "max_iterations",
                ];
                if k.is_empty() {
                    return Err("--param: empty key".into());
                }
                if RESERVED.contains(&k) {
                    return Err(format!(
                        "--param: '{k}' is client-owned (set it with its dedicated flag)"
                    ));
                }
                args.params.push((k.to_string(), val.to_string()));
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
        assert_eq!(
            args.timeout_secs, 7200,
            "ADR-0014 §2 / ADR-0027 §2: the exec safeguard is the framework's \
             2h default, never a low performance knob — a complex agentic run \
             can legitimately take hours"
        );

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

    /// ADR-0027 §2/§3 contract for the one wall-clock cap this binary owns.
    /// Pinned because the failure mode is invisible: a lowered default does
    /// not break any test, it just starts abandoning long runs.
    #[test]
    fn exec_timeout_is_a_high_explicit_safeguard_and_zero_disables_it() {
        let with_cap =
            parse(&["exec".into(), "go".into(), "--timeout".into(), "30".into()]).unwrap();
        assert_eq!(with_cap.timeout_secs, 30, "an explicit cap is honored");

        // 0 is the documented "never interrupt this run" spelling. It must
        // survive parsing as 0 — `exec` widens it to a ~10-year deadline. The
        // trap this guards: exec's old `.max(10)` clamp would have turned
        // "unlimited" into the shortest cap in the program.
        let uncapped =
            parse(&["exec".into(), "go".into(), "--timeout".into(), "0".into()]).unwrap();
        assert_eq!(uncapped.timeout_secs, 0, "0 reaches exec as 0 = no cap");

        assert!(
            usage().contains("0 = none"),
            "the uncapped spelling must be discoverable in --help"
        );
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
