Recorded route answers (R14-W4, 2026-10-07) from a hermetic scratch gateway at
origin/main (abstractgateway 2a28b90 = 0.13.0, abstractruntime 640ca24 = 0.9.0,
abstractcore 055b3f6 = 2.25.0), signed in as the non-admin user `alice`.
Gateway policy: Allow everything, refuse listed workspaces (rw) with
/Users/ada/home/work (rw), /Users/ada/home/Pictures (ro cap), /Users/ada/home/Desktop (refused).
Paths rewritten (scratch root -> /Users/ada/home, /srv/gateway).

- session_get_default.json       GET  /api/gateway/sessions/s-w4-one/workspaces (nothing stored: configured false)
- session_put_configured.json    PUT  the same {configured, allowed_only, Pictures ro, work/project rw} -> 200
- session_put_above_cap.json     PUT  Pictures rw -> 400 workspace_refused
- session_put_refused_path.json  PUT  + Desktop ro -> 400 workspace_refused
- session_get_configured.json    GET  after the PUT
- effective_session.json         GET  /api/gateway/workspace/effective/me?session=s-w4-one
- dryrun_default.json            POST /api/gateway/workspace/effective/me {workspace: null}
- dryrun_payload.json            POST the same {workspace: {allowed_only, work ro}}
- dryrun_refused.json            POST the same {workspace: {Pictures rw}} -> 400
- account_get.json               GET  /api/gateway/workspace/policy/me
- discovery_tools_command_sandbox.json          GET  /api/gateway/discovery/tools (command_sandbox + sandboxed/sandbox rows)
- kit_workspace_chooser_text.ts  the kit wording table (see its header)
- sandbox_ledger.json            ledger records with output.sandbox (copied from the Code web fixture web/src/workspace/fixtures/sandbox_ledger.json, R13-W4)
