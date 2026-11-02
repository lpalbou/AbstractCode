---
name: abstractcode
description: Operate AbstractCode terminal and browser clients of AbstractGateway. Use for connecting, continuing durable coding sessions, inspecting tool permissions and workspaces, asking scoped assistant questions, or configuring terminal voice and dictation.
license: MIT
metadata:
  package-version: "terminal 0.13.1 / web 0.14.0"
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

Documentation and Companion are separate drawers with independent histories and
drafts. Documentation receives AbstractCode documentation plus recent user requests
and assistant replies from the main conversation. Companion receives an immutable snapshot of the
currently loaded conversation when each question is sent: the newest whole turns up
to 50,000 estimated tokens, with a note counting any older messages left out. It cannot inspect
newer/unloaded records or steer, approve or execute tools on the coding run.
`/docs` opens Documentation directly; the avatar or account menu opens Companion.
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
Completed canonical answers render outside folded cycles. Terminal reconnect paths
drain their ledger tails and reconcile run output; historical replay does not
automatically speak restored replies.

Companion questions report only observed context and may require follow-up after
loading more history. Terminal graphics depend on capabilities; voice requires
an installed host audio backend and configured Gateway routes. Windows end-to-end
support is not verified in the terminal reference. Companion visuals and automatic comments are opt-in and off by default.

References: [terminal API](https://raw.githubusercontent.com/lpalbou/AbstractCode/main/tui/docs/api.md), [setup](https://raw.githubusercontent.com/lpalbou/AbstractCode/main/docs/getting-started.md),
[architecture](https://raw.githubusercontent.com/lpalbou/AbstractCode/main/docs/architecture.md), [troubleshooting](https://raw.githubusercontent.com/lpalbou/AbstractCode/main/tui/docs/troubleshooting.md).

## Optional companion

Use account menu → Companion settings, or the Companion drawer's ⚙ action. Enable
Halo, Hover, Vortex, Eclipse, Eye or Rover visuals, choose reduced motion, and optionally enable comments or side
read-aloud. Click the avatar (compact ◉ on small terminals) to open Companion, separately from Documentation.
Position selects Top right or Bottom right within the transcript, clearing the
composer/audio rows. Empty artwork cells preserve underlying text; occupied cells
replace that cell's glyph. It does not narrow the transcript. Account → Companion works
with the avatar disabled. Ready breathes at2fps, Waiting pulses at6fps, active
phases animate at up to12fps; reduced motion stops clocks.
Appearance opens a silent, large preview: select any of the six styles, hold any of seven
phases, Auto cycle or Pause motion. The active design opens selected/focused. Apply avatar
saves appearance and closes the preview; Enter on a focused design does the same,
while clicking a design previews it. Cancel/Esc
discards preview changes. No inference or microphone access occurs in this preview.
Comments require new visible conversation content, allow one in-flight request and
at least 60 seconds between attempts in this TUI. They have no tool/steering/approval
authority. Shared multi-client cooldown is not implemented. Restored history does
not itself trigger a comment.

The drawer's ●/■ action records/stops a question for that side draft; ↑ sends it.
This is separate from Ctrl+R dictation into the coding composer. Speaking voice
can be chosen independently while preserving main device and volume settings;
automatic speech shares one ordered queue and respects opt-out/cancellation.
Preferences are local; entity voice/appearance remains Gateway-owned.
During current speech, bottom-meter and avatar ‖/▶ buttons pause/resume that audio
without starting another reply. Per-message playback controls remain available.

Ready eyes follow terminal cursor movement and presses; they center after two seconds idle.
Other phases/reduced motion ignore tracking. Error faces show red X eyes. Shared
geometry/catalog belongs to the independent AbstractAvatar (`abstractavatar`) generator;
web hosts use AbstractUIC `ActivityAvatar` instead of copying geometry.

Live tool/run failures hold the avatar Error cue for at least1s even if the run
recovers or begins speaking. Repeated failures restart the hold; history replay
does not flash Error, and session changes/hiding cancel the transient cue.

A new submitted coding message interrupts current/preparing/paused speech and
discards old queued narration, including when guiding an active/starting run.
Typing alone does not stop speech. Current-audio Pause/Resume sits immediately
beside the bottom realtime histogram as well as in the companion action row.

Session boundaries (`/new`, `/clear`, switching conversations) stop the current
speech and discard queued narration; late chunks cannot resume in the new session.

## Selected content in the terminal

Click mapped assistant text, a table cell or code, then choose Explain, Use as
context or Change. Review the quoted content in the existing coding draft before
sending normally; selection and staging grant no send, tool or approval authority.
Use unit actions to switch word/sentence/paragraph/cell/code scope, or drag a span.
Table context retains headers and code context retains language/lines. Finish a
slash-command draft before adding context. Esc/Cancel preserves the coding draft;
Esc also stops current voice work. Custom user/tool/JSON cards use screen-copy
fallback and do not provide semantic actions. See
[terminal selection](https://raw.githubusercontent.com/lpalbou/AbstractCode/main/tui/docs/api.md#selected-content).

## Optional avatar conversation

In the browser, choose **Avatar** for public subtitles and **Conversation** to restore the
ordinary trace. Start the next task in Avatar mode to request actor-authored public updates;
changing a legacy run's view does not create missing updates. Type or speak during work to
guide it; selecting **Reply to this** fixes a public brève as context. Accepted guidance is
not proof that the assistant adopted the suggestion. Answer the exact displayed question
and use explicit permission controls for approval. See [web usage](https://raw.githubusercontent.com/lpalbou/AbstractCode/main/docs/web.md).

Avatar requests use plain actions/targets and the exact batch’s public explanation.
“Review details” returns to the technical view without granting permission. Tool syntax,
argument JSON and standing grants belong there. Missing reasons remain explicit.
