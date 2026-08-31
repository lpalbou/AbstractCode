# Workflow-driven UI events (contract)

AbstractCode can translate **durable** workflow `emit_event` effects into host UX updates.

Start here: [`docs/getting-started.md`](getting-started.md).

This is designed so:
- the event is recorded in the **ledger** (durable, replayable),
- different hosts (TUI, web) can render the same “UX hint” deterministically.

Related:
- Workflows overview: [`docs/workflows.md`](workflows.md)
- Architecture: [`docs/architecture.md`](architecture.md)

## Event namespace + backward compatibility

Canonical namespace: `abstract.*`

Deprecated alias: `abstractcode.*` is accepted and normalized to `abstract.*`.

## Supported events

### 1) `abstract.status`

Purpose: update the host’s status/spinner text.

Accepted payload shapes:
- string: `"Indexing repo…"`
- object: `{ "text": "Indexing repo…", "duration": 2 }`

`duration` is seconds:
- `null`/missing: host default
- `-1`: sticky (do not auto-clear)
- `> 0`: auto-clear after that many seconds (best-effort)

Evidence:

### 2) `abstract.message`

Purpose: show a notification/message block in the host.

Accepted payload shapes:
- string: `"Done."`
- object:
  - required: `text`
  - optional: `level` (`info|success|warning|error`), `title`, `meta` (object)

Evidence:

### 3) `abstract.tool_execution`

Purpose: render a “tool call” UX block **without** requiring actual tool execution.

Accepted payload shapes:
- a single tool call object
- a list of tool call objects

Supported tool call object shapes (best-effort normalization):
- Preferred normalized:
  - `{ "name": "read_file", "arguments": { "path": "README.md" }, "call_id": "..." }`
- OpenAI-ish:
  - `{ "id": "...", "type": "function", "function": { "name": "read_file", "arguments": "{\"path\":\"README.md\"}" } }`

### 4) `abstract.tool_result`

Purpose: render a “tool result” UX block.

Accepted payload shapes:
- a single tool result object
- a list of tool result objects

Supported tool result object fields (best-effort):
- `name`/`tool`/`tool_name` (string)
- `success` (boolean)
- `output`/`result`/`content`/`text` (any; rendered as string)

## Operational notes

- Events are only surfaced after the corresponding ledger record is written as `status == completed`.
- Hosts must treat these events as **UX-only** (they must not affect correctness).

