---
name: abstractcode
description: Operate AbstractCode terminal and browser clients of AbstractGateway. Use for connecting, continuing durable coding sessions, inspecting tool permissions and workspaces, asking scoped assistant questions, or configuring terminal voice and dictation.
license: MIT
metadata:
  package-version: "unreleased"
---

# Use AbstractCode

AbstractCode has a Rust terminal client (`abstractcode`) and browser client
(`@abstractframework/code`). The coding workflow executes on AbstractGateway;
clients observe its ledger and submit commands. Closing a client does not cancel
the durable run. There is no standalone local-agent mode.

## Connect and inspect before running

1. Install the released terminal with `cargo install abstractcode`, or follow
   the repository's contributor instructions for unreleased source checkouts.
2. Use `abstractcode --help` for flags and `abstractcode doctor` to diagnose
   connectivity before starting work. `abstractcode --caps` reports rendering
   capabilities. A reachable Gateway and its authentication are required.
3. `abstractcode login` verifies and persists credentials. URL precedence is
   explicit flag, environment alias, saved connection, local Gateway pointer,
   then loopback default. Keep bearer tokens out of transcripts and source.
4. Start `abstractcode`. Choose the workflow, model and permitted workspaces
   before submitting work. A local client directory is not automatically a
   remote Gateway workspace.
5. Use `/sessions` to inspect or continue a durable conversation. Refresh keeps
   the previous Gateway summary visible; partial lists and unknown metrics are
   labelled. `/new` starts a separate session. Archive is reversible.

## Controls and permissions

The header's Tools menu groups Tools, Skills and MCP. `/tools` opens tool choices:
Off excludes a tool, Ask requires approval and Auto skips its approval. Category
choices affect currently listed eligible tools. Server-unavailable tools cannot
be enabled from the client. Changes apply to future runs and do not rewrite an
active run. Keep the user's approved scope; do not choose Auto just to bypass a
pause. Workflow and Gateway ceilings still apply. `/permissions` opens the
existing broader approval controls; `/workspace` inspects workspace policy.

`/settings`, `/workflow`, `/model` and `/help` expose the other supported panels.
Outside clicks dismiss ordinary modals; approval steps remain protected. Wheel
scrolling, arrow keys and explicit buttons are available in the Tools list.

## Questions beside the coding task

The assistant drawer has independent Docs and This run scopes. Docs answers from
AbstractCode documentation. This run receives an immutable snapshot of the
currently loaded conversation when each question is sent. It cannot inspect
newer/unloaded records or steer, approve or execute tools on the coding run.
These scopes preserve separate drafts/history. `/docs` opens Docs directly.
Use the coding composer for task instructions, not a side question that should
have no execution authority. There is no `/btw` command in this implementation.

## Voice

`/voice` configures the speaking voice from the Gateway catalog, read-aloud,
recognition route/language and this computer's microphone/speaker. It saves
client preferences, not Gateway-wide defaults. Audio recording/playback uses
AbstractVoice on this computer; synthesis and transcription use Gateway routes.

- Ctrl+P reads/pauses/resumes the latest answer; Esc stops speech.
- Read aloud narrates new reasoning-cycle prose and the answer in order, not
  restored history or the reasoning channel.
- On an empty prompt, hold Space to record a voice message and release to send
  automatically (the existing default). `/voice auto-send off` or Voice messages
  → Review before sending puts its transcription into the editable prompt instead.
- Ctrl+R is dictation into the prompt; it never directly sends a message.
- `/voice space off` disables the held-Space gesture; ordinary typed spaces remain
  text. Taps are discarded. Recording/transcription failures show an error rather
  than sending an empty request.

## Automation and limits

`abstractcode exec "<prompt>" [OPTIONS]` is the headless one-shot surface. It may
execute tools on the Gateway: use it only within the user's authorized scope.
Choose a workflow explicitly when no default exists. Exit codes are 0 completed,
1 failed, 2 usage/configuration, 124 timeout, 130 cancelled. See `--help` and the
CLI reference for workspace, permissions, timeout and streaming options.

Client tool choices are local preferences; they are not a shared policy record.
This run questions report only observed context and may require follow-up after
loading more history. Terminal graphics depend on capabilities; voice requires
an installed host audio backend and configured Gateway routes. Windows end-to-end
support is not verified in the terminal reference. No companion/avatar observer
is enabled; that design remains proposed.

References: [terminal API](../../tui/docs/api.md), [setup](../../docs/getting-started.md),
[architecture](../../docs/architecture.md), [troubleshooting](../../tui/docs/troubleshooting.md).
