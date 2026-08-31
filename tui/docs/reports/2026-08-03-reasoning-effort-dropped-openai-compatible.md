# `reasoning_effort` never reaches the wire on `openai-compatible` — R-Type bench fairness defect

Date: 2026-08-03
Investigator seat: abstractcode (adversary F)
Status of every claim below: **CONFIRMED** unless explicitly marked PLAUSIBLE or UNKNOWN.

---

## 1. Headline

The 24-cell R-Type benchmark (`untracked/rtype-bench/`) was believed to have two unfair
arms (`abstractcode-basic`, `abstractcode-coder`) running at the relay default
(`reasoning_effort` absent → `none`) while the other six ran at `medium`.

That is wrong, and it is wrong in the more damaging direction.

**Five of the eight arms ran with reasoning off.** Every arm that reaches gpt-5.4 through
abstractcore's `openai-compatible` provider — that is `tui-basic`, `tui-coder`,
`tui-multi`, `abstractcode-basic` and `abstractcode-coder` — sent **no `reasoning_effort`
field at all**, and the relay forwarded `reasoning: null` to OpenAI for every one of them.

Only `opencode` and `pi` actually ran at medium on the relay. `codex` ran at medium on its
own ChatGPT-subscription backend (documented, intended, not via the relay).

The bench's own route verification says `CONFIRMED gpt-5.4/medium (run store)` for the
three `tui-*` arms. That verification is reading **intent, not effect** — see §5.

---

## 2. How this was verified (wire-level, not config-level)

The airelays relay writes a structured JSONL log per hour under
`~/.airelays/logs/YYYY/MM/DD-HH.log`. Two phases matter:

- `inbound_request` — the **verbatim HTTP request body the client sent**, plus request
  headers including `user-agent`.
- `upstream_request` — **what the relay forwarded to OpenAI**, where the effort appears as
  `reasoning: {"effort": "..."}`.

This is a genuine wire observation on both sides of the relay, not a config read. It is
also immune to the failure mode that made an earlier claim in this campaign inert (a
config object re-serialised with the old value): the log records bytes that were actually
transmitted.

Calibration, confirmed by direct `curl` against the relay on 2026-08-03:

| inbound `reasoning_effort` | upstream `reasoning` |
|---|---|
| absent | `null` |
| `"medium"` | `{"effort": "medium"}` |
| `"none"` | `{"effort": "none"}` |

So an absent field is **not** silently defaulted to medium upstream — it genuinely means
reasoning off. `GET /v1/models` independently reports for `gpt-5.4`:
`{"parameter":"reasoning_effort","modes":["none","low","medium","high","xhigh"],"default":"none"}`.

User-agent identifies the client unambiguously:

| user-agent | client |
|---|---|
| `python-httpx/0.28.1` | abstractcore `openai_compatible_provider` (gateway arms **and** abstractcode local loop) |
| `opencode/1.18.10 …` | opencode |
| `OpenAI/JS 6.26.0` | pi |
| `OpenAI/Python 1.109.1` | abstractcore `openai_provider` (see §6) |

### 2.1 The original bench window

All `inbound_request` POSTs joined to their `upstream_request`, window
`2026-08-02T21:06:54Z .. 22:25:00Z` (first tui cell start → last pi cell):

```
    n  client ua              inbound model  inbound reasoning_effort  upstream reasoning
   43  OpenAI/JS              gpt-5.4        medium                    {"effort": "medium"}
    6  Python-urllib/3.12     gpt-5.4        None                      null
    3  opencode/1.18.10       gpt-5.4        None                      null
  121  opencode/1.18.10       gpt-5.4        medium                    {"effort": "medium"}
  444  python-httpx/0.28.1    gpt-5.4        None                      null
```

**444 requests from abstractcore, every one with reasoning off.** Sampling those requests
at 21:06:54–21:12:00 shows the gateway workflow system prompts
(`## MY PERSONA You are an autonomous ReAct agent …`, `# Coding task …`,
`FIRST BUILD on a fresh branch …`), i.e. these are the `tui-basic` / `tui-coder` /
`tui-multi` cells, not background noise.

Widening to the whole of August so far (2026-08-01 → 2026-08-03), **zero** `python-httpx`
requests have ever carried `reasoning_effort` (4025 requests to gpt-5.4, all absent). The
`opencode` and `OpenAI/JS` clients carry it routinely.

**Completeness check — no alternative channel.** Effort could in principle have been
conveyed some other way (abstractcore does inject a Harmony-style `Reasoning: <level>`
system-prompt line for GPT-OSS-class models, `base.py:1510-1547`). It was not. Across all
**489** `python-httpx` requests in the bench window:

- the union of every non-message top-level payload key is
  `['max_completion_tokens','model','prompt_cache_key','response_format','stream',
  'temperature','tool_choice','top_p']` — `reasoning_effort` never appears;
- **0** requests contain a `Reasoning: minimal|low|medium|high|xhigh` line in any system or
  developer message.

gpt-5.4 is not a Harmony model, so that path is correctly inactive — leaving no channel at
all. The five relay arms ran at reasoning off.

### 2.2 Live reproduction, 2026-08-03

`abstractcode exec --workflow basic-agent --provider endpoint:airelay --model gpt-5.4
--reasoning medium` (the exact `tui-basic` invocation shape), run `8ea55553-…`:

- Gateway run store `/Users/albou/tmp/abstractframework/runtime/run_8ea55553-….json`
  records `vars._runtime.thinking = "medium"`.
- Relay `inbound_request` for that run: top-level keys
  `['model','prompt_cache_key','stream','temperature','tool_choice','top_p']` —
  **no `reasoning_effort`**.
- Relay `upstream_request`: `reasoning: null`.

`abstractcode exec --agent basic-agent --provider endpoint:airelay --model gpt-5.4
--reasoning medium` (the `abstractcode-basic` invocation shape) reproduces identically,
and additionally prints the abstractcore warning to stderr:

```
openai_compatible_provider.py:825: RuntimeWarning: thinking='medium' requested but provider
'openai-compatible' does not implement a thinking control mapping for model 'gpt-5.4';
no control was applied and the model/server default thinking behavior remains in effect.
```

(Read that file:line as Python's `stacklevel` attribution, not the defect site: line 825 is
the innocent `generate()` → `generate_with_telemetry()` forwarder. The warning text is
raised at `base.py:1710-1717`; the value is discarded at
`openai_compatible_provider.py:473`, per §3.)

---

### 2.3 Blast radius — this is not one benchmark

Scanning the **entire relay history** (`~/.airelays/logs`, 628 hourly files, 2026-07-03 →
2026-08-03) for gpt-5.4 requests from `python-httpx`:

```
   6546  gpt-5.4    reasoning_effort = None
      0  gpt-5.4    reasoning_effort = any value
```

**Not once, ever.** Reasoning control has never reached gpt-5.4 through this path in the
month the relay has been logging.

One honest caveat so this is not read as broader than it is: `python-httpx` requests for
**`gpt-5.4-mini`** *do* carry `reasoning_effort` (34,658 at medium). Those are **not**
abstractcore — the payload shape is different (`['max_tokens','model','reasoning_effort',
'temperature']`, no `stream`, no `top_p`, whereas abstractcore's openai-compatible builder
always sends `stream`, `temperature` and `top_p`), and the content is an unrelated
CV-screening application that happens to use raw httpx too. Same UA, different client.

Re-running the same history scan filtered on abstractcore's actual payload signature
(`model` + `stream` + `temperature` + `top_p` all present) removes that ambiguity
completely:

```
abstractcore openai-compatible signature, 2026-07-03 .. 2026-08-03 (628 log files)

   6547  gpt-5.4        reasoning_effort = None
   1579  gpt-5.6-sol    reasoning_effort = None
    708  gpt-5.4-mini   reasoning_effort = None
    170  gpt-5.5        reasoning_effort = None
     16  claude:*, codex-auto-review …  reasoning_effort = None
      1  gpt-5.4        reasoning_effort = medium
      ------------------------------------------------------------
   9021  TOTAL,  of which exactly 1 carried reasoning_effort
```

That single exception is timestamped `2026-08-03T17:16:03Z` — it is the **fix simulation
from §4.1 of this report**, run minutes before writing this. In a month of logs and 9,021
requests, the only time abstractcore's `openai-compatible` provider has ever put a
reasoning effort on this wire is when the two-part fix was patched in by hand.

Meanwhile **18 live benchmark output directories** under `untracked/` record
`"reasoning": "medium"` in their provenance. Every one of them that ran gateway or
abstractcode arms in this window has the same defect, whether or not its provenance
mentions it. `untracked/rtype-bench/` is simply the one under scrutiny. The others were
not audited here.

## 3. Root cause — one function, one line

**Owning package: `abstractcore`** (version 2.13.38, editable install at
`/Users/albou/tmp/abstractframework/abstractcore`).

Both abstractcode paths and all three tui paths converge on the same drop site.

`abstractcore/abstractcore/providers/openai_compatible_provider.py:472-473`

```python
if not surfaces.budget_template_kwarg or not template_kwargs_supported:
    return kwargs, ThinkingControlHandling()      # <-- line 473: the drop
```

Confirmed by `sys.settrace` line-tracing of a real call
(`_apply_provider_thinking_kwargs(enabled=True, level="medium", kwargs={})` on a live
gpt-5.4 provider instance): executed lines are
`391, 394, 395, 397, 398, 399, 398, 400, 397, 406, 413, 472, 473, RETURN@473`.

Why it falls through:

1. `_thinking_control_surfaces()` for gpt-5.4 returns **all-None**
   (`ThinkingControlSurfaces(prompt_disable_token=None, template_kwarg=None,
   assistant_prefill_disable=None, budget_template_kwarg=None,
   low_effort_template_kwarg=None, request_param=None)`) — gpt-5.4 declares no
   `thinking_control` block in `assets/model_capabilities.json`, and architecture `gpt`
   declares none in `assets/architecture_formats.json`.
2. `surfaces.template_kwarg` is None → the boolean-template branch at line 413 is skipped.
3. `surfaces.budget_template_kwarg` is None → line 472 short-circuits and line 473 returns
   the kwargs untouched with `ThinkingControlHandling()` (both flags False).

The hook has **no `reasoning_levels → reasoning_effort` branch at all**, even though
`_model_reasoning_levels()` on this very instance returns
`['none','low','medium','high','xhigh']`.

Contrast `abstractcore/abstractcore/providers/openai_provider.py:97-149`, which implements
exactly that mapping and terminates at line 148 with
`new_kwargs["reasoning_effort"] = effort`. The two providers disagree, and the endpoint
profile `airelay` is declared `provider_family: "openai-compatible"`, so the bench gets
the one without the mapping.

### 3.1 On the `request_param` hypothesis — partly REFUTED

The starting hypothesis was that `architectures/thinking_controls.py` already models a
`request_param: "reasoning_effort"` surface and the provider simply fails to consume it.

- CONFIRMED: `reasoning_effort` is implemented in `openai_provider.py` and absent from
  `openai_compatible_provider.py`; the bench routes via the latter.
- REFUTED as the operative mechanism: **no code anywhere in abstractcore reads
  `surfaces.request_param`** (grep across `abstractcore/**/*.py` finds it only inside
  `thinking_controls.py` itself), and **gpt-5.4 does not declare it** in assets. The module
  docstring is explicit that the field is *"informational; recorded so the control surface
  is declared even before a provider consumes it."*

So declaring `request_param` in assets would fix nothing on its own. The fix has to be a
provider-side mapping.

### 3.2 Per-path attribution

| arm | path | where the value is lost |
|---|---|---|
| `abstractcode-basic` | **local in-process abstractcore loop** (CONFIRMED twice: the live probe created no gateway run record at all, and the abstractcore RuntimeWarning surfaces directly on the abstractcode process's own stderr — it would appear in the gateway's log, not the client's, on a gateway route) | `openai_compatible_provider.py:473` |
| `abstractcode-coder` | gateway (`coding-agent:coding-agent`) → abstractruntime → abstractcore | `openai_compatible_provider.py:473` |
| `tui-basic` / `tui-coder` / `tui-multi` | gateway → abstractruntime → abstractcore | `openai_compatible_provider.py:473` |

A note on reading the run store, because it is easy to mis-join: one TUI cell writes
several records. The live probe produced `basic-agent@0.0.4:81795ea9` (`thinking=medium`,
the top-level run the harness reads), `visual_react_agent_basic-agent_0_0_4`
(`thinking=medium`), two `basic-agent@0.0.4:15f19f7f` sub-runs (`thinking=None`) and a
`__session_memory__` record — all under one session. The `…:15f19f7f` records with
`thinking=None` are **delegated sub-runs of the TUI cell**, not separate abstractcode
cells. Effort intent does not inherit uniformly into them, which is a second, smaller
issue worth a look once the wire defect is fixed.

**abstractruntime is not at fault.** The gateway run record for the live reproduction
proves the runtime handed the value to abstractcore correctly and that abstractcore
refused it:

```
…/steps[0]/effect/payload/params/thinking                       = "medium"
…/_runtime_observability/llm_generate_kwargs/params/thinking    = "medium"
…/result/metadata/thinking_requested                            = "medium"
…/result/metadata/thinking_level_requested                      = "medium"
…/result/metadata/thinking_supported_levels                     = ["none","low","medium","high","xhigh"]
…/result/metadata/thinking_supports_control                     = true
…/result/metadata/thinking_handled_enable_disable               = false   <-- dropped
…/result/metadata/thinking_handled_level                        = false   <-- dropped
```

Separately and PLAUSIBLE (not chased to ground, because it is moot while §3 stands): the
abstractcode client's own gateway lane appears not to propagate the preference into
`_runtime.thinking` for `--agent coder` the way the TUI does. `abstractcode/reasoning.py`
`wire_value()` is only consumed at `abstractcode/react_shell.py:12954-12968`, inside
`_sync_tool_prompt_settings_to_run`, which is additionally gated on
`not self._run_thread_active()`. Even if that gate were wrong, fixing it alone would not
put `reasoning_effort` on the wire.

---

## 4. Requests to other packages

### 4.1 Request to the `abstractcore` seat — ready to send

> **Subject: `openai-compatible` provider silently drops `thinking=<level>` for models
> that declare `reasoning_levels` (gpt-5.4 on an OpenAI-compatible relay)**
>
> **Severity: high.** It has already invalidated a 24-cell cross-client benchmark: five of
> eight arms ran with reasoning off while believing they ran at medium.
>
> **Root cause (CONFIRMED, line-traced).**
> `abstractcore/providers/openai_compatible_provider.py:472-473`. The provider's
> `_apply_provider_thinking_kwargs` hook only handles asset-declared *chat-template*
> surfaces — `surfaces.template_kwarg` (line 413) and `surfaces.budget_template_kwarg`
> (line 472). When a model declares neither, line 473 returns the kwargs untouched with
> `ThinkingControlHandling()` and the requested effort is discarded. There is no branch
> that maps a requested level to the OpenAI-standard `reasoning_effort` request field,
> even when `self._model_reasoning_levels()` is non-empty.
>
> `gpt-5.4` hits this exactly: `assets/model_capabilities.json` gives it
> `"reasoning_levels": ["none","low","medium","high","xhigh"]` and
> `"thinking_support": true`, but no `thinking_control` block, and architecture `gpt` does
> not supply one either. `_thinking_control_surfaces()` therefore returns all-None.
>
> `abstractcore/providers/openai_provider.py:97-149` already implements the correct
> mapping and ends at line 148 with `new_kwargs["reasoning_effort"] = effort`. The two
> providers disagree about a field that is part of the OpenAI Chat Completions API, not an
> OpenAI-hosted-endpoint extension — so any OpenAI-compatible relay or gateway fronting a
> reasoning model loses effort control today.
>
> **Note this is not a silent failure in the ADR-0001 sense** — `base.py:1700-1717` does
> emit two RuntimeWarnings. But warnings are invisible to a gateway host: the gateway
> records `_runtime.thinking = "medium"` in its run store and reports the route as
> verified, while the wire carries nothing. The honest metadata
> (`thinking_handled_level: false`) is present in the response metadata but is not what
> hosts key on.
>
> **Proposed change — and please note it is TWO parts, because one part alone is inert.**
>
> The obvious one-line fix does **not** work, and I verified that on the wire rather than
> assuming it. Patching only the hook to set `kwargs["reasoning_effort"] = level` changes
> nothing: the request still leaves without the field, and upstream still gets
> `reasoning: null`. The reason is that `_generate_internal` builds the payload from an
> **explicit allowlist** (`openai_compatible_provider.py:943-1007` and again at
> `1460-1520`) — `model`, `messages`, `stream`, `temperature`, `top_p`, conditionally
> `max_tokens`/`top_k`/`stream_options`/`prompt_cache_key`/`tools`/`tool_choice`/
> `frequency_penalty`/`presence_penalty`/`repetition_penalty`/`seed`/`response_format`.
> Arbitrary kwargs are never copied through. (Independently confirmed: passing
> `reasoning_effort="medium"` straight into `generate()` is also silently dropped.)
>
> So:
>
> **Part 1 — map it.** In `_apply_provider_thinking_kwargs`, before the template-surface
> branches, add a `reasoning_effort` mapping gated on `self._model_reasoning_levels()`
> being non-empty, mirroring `openai_provider._apply_provider_thinking_kwargs`, returning
> `ThinkingControlHandling(handled_enable_disable=True, handled_level=True)`.
>
> **Part 2 — emit it.** Copy it into the payload. `_mutate_payload`
> (`openai_compatible_provider.py:307`) is the clean single point: it is called by **both**
> payload builders (lines 1012 and 1525), so one change covers the sync and streaming
> paths. Adding it to the two allowlists separately works too but duplicates the logic.
>
> **This combination is CONFIRMED on the wire, not proposed.** Simulating exactly those two
> changes on a live gpt-5.4 provider against the relay (monkeypatched in a throwaway
> process — I did not edit your package):
>
> ```
> client user-agent       : python-httpx/0.28.1
> INBOUND top-level keys  : ['max_completion_tokens','model','reasoning_effort','stream','temperature','top_p']
> INBOUND reasoning_effort: 'medium'
> UPSTREAM reasoning      : {"effort": "medium"}
> ```
>
> versus the same call with only Part 1 applied:
>
> ```
> INBOUND top-level keys  : ['max_completion_tokens','model','stream','temperature','top_p']
> INBOUND reasoning_effort: None
> UPSTREAM reasoning      : null
> ```
>
> Two design points for your judgement, not mine:
> - Whether to gate on `_model_reasoning_levels()` alone or additionally require an
>   asset-declared `thinking_control.request_param: "reasoning_effort"`. The typed surface
>   already exists in `architectures/thinking_controls.py` but **is currently read by no
>   code anywhere in the package** — its docstring calls it "informational". If you prefer
>   the asset-gated route, gpt-5.x (and every other `reasoning_levels` model) will need the
>   declaration added, and the levels-only path should stay as a fallback so existing
>   assets do not silently stay broken.
> - Whether unknown/strict third-party OpenAI-compatible servers might reject an unexpected
>   `reasoning_effort`. The conservative gate is "only send it when the model's own
>   capability entry declares `reasoning_levels`", which is exactly the condition that is
>   already false for non-reasoning models.
>
> **Validation, wire-level (this is how I verified the defect; the same method proves a
> fix).** With the airelays relay running on `http://127.0.0.1:8317/v1`:
>
> 1. `python -c "from abstractcore.providers.openai_compatible_provider import
>    OpenAICompatibleProvider; p=OpenAICompatibleProvider(model='gpt-5.4',
>    base_url='http://127.0.0.1:8317/v1', api_key='bench');
>    p.generate('MARK reply ok', thinking='medium', max_tokens=16)"`
> 2. Read the newest `~/.airelays/logs/YYYY/MM/DD-HH.log`, find the `inbound_request`
>    record containing `MARK`, and assert `body.json.reasoning_effort == "medium"`.
> 3. Join on `request_id` to the `upstream_request` record and assert
>    `body.json.reasoning == {"effort": "medium"}`.
>
>    Today step 2 yields `None` and step 3 yields `null`.
>
> 4. Unit-level regressions worth pinning — **both**, since either alone passes while the
>    feature stays broken:
>    - on a gpt-5.4 provider instance,
>      `_apply_provider_thinking_kwargs(enabled=True, level="medium", kwargs={})` must
>      return `({'reasoning_effort': 'medium'}, ThinkingControlHandling(True, True))`.
>      Today it returns `({}, ThinkingControlHandling(False, False))`.
>    - and the built **payload** must contain `reasoning_effort`. A hook-level assertion
>      alone would have gone green on the inert fix above. Assert on the payload, or on the
>      captured request body.
>
> 5. Please also keep a negative case: a model with no `reasoning_levels` must still send
>    no `reasoning_effort`.
>
> **Blast radius if fixed:** every abstractruntime/abstractgateway host and every
> abstractcode/abstractcode arm pointed at an OpenAI-compatible reasoning endpoint
> starts honouring the reasoning dial. Payload bytes change for those routes, so warm
> prompt-cache prefixes for them will be invalidated once.

### 4.2 Request to the `abstractgateway` / `abstractruntime` seats — lower priority

> The run store records `vars._runtime.thinking` (the **request**) in a place hosts treat
> as route evidence, while the **outcome** — `result.metadata.thinking_handled_level` and
> `thinking_handled_enable_disable`, both `false` here — is buried in per-step metadata.
> A run whose requested effort was not honoured currently looks, at the top level,
> identical to one that was.
>
> Suggestion: surface an effective/handled reasoning field at run level (e.g.
> `_runtime.thinking_effective`, or a `thinking_handled` boolean) so a host can tell
> "asked for medium" from "ran at medium". No abstractcore change makes this unnecessary;
> the next provider without a mapping reintroduces the same blind spot.

---

## 5. Defect in this repo's own harness (`abstractcode`, ours to fix)

`scripts/bench_clients.py:512-520`:

```python
elif str(r.wire_model) == MODEL and str(r.wire_thinking) == REASONING:
    r.route_verified = f"CONFIRMED {r.wire_model}/{r.wire_thinking} (run store)"
```

`r.wire_thinking` is read from `_runtime.thinking` in the gateway run store. That is the
**requested** effort. The harness prints `CONFIRMED gpt-5.4/medium (run store)` for a run
whose HTTP request carried no `reasoning_effort` at all. The field name `wire_thinking` is
itself the bug: nothing about it is the wire.

Recommended, once abstractcore is fixed: verify route from the relay's `inbound_request`
log (join client user-agent + time window, assert `reasoning_effort`), or at minimum read
`result.metadata.thinking_handled_level` from the run record and refuse to print
`CONFIRMED` when it is `false`.

Not changed in this pass — the harness is operator-owned surface and the fix should land
together with a decision about re-running.

Second, smaller evidence-hygiene point in the same file: `bench_clients.py:632` writes
`"api_key_used": False` into the provenance as a **hardcoded literal**, not a measurement.
The claim happens to be true — the relay is subscription-backed (`owned_by:
airelays-openai-subscription`, upstream `chatgpt.com/backend-api/codex`), `OPENAI_API_KEY`
was verified unset for every child process in this investigation, and the relay runs
`require_bearer_auth = false` — but a constant cannot evidence itself. If the operator
relies on that field, it should assert the environment rather than declare it.

---

## 6. Is there a benchmark-side route to medium today, without editing another package?

Candidates tested on the wire. **Three refuted, one works but is not clean.**

| candidate | result |
|---|---|
| `thinking="medium"` via abstractcore `openai-compatible` | **REFUTED.** No `reasoning_effort` inbound; `reasoning: null` upstream. This is the defect itself. |
| raw `reasoning_effort="medium"` generate kwarg | **REFUTED.** Silently dropped — the payload builder emits a fixed key set; inbound keys were `['max_completion_tokens','model','stream','temperature','top_p']`. |
| `extra_body={"reasoning_effort":"medium"}` | **REFUTED.** Lands as a literal nested top-level JSON field `"extra_body": {...}` (`_mutate_payload`, line 316-322), which the relay ignores. Upstream `reasoning: null`. |
| model-id suffix `gpt-5.4:medium`, the way `pi` declares it | **REFUTED.** The relay does not parse the suffix — it forwards the literal model id and upstream rejects it: `"The 'gpt-5.4:medium' model is not supported when using Codex with a ChatGPT account."` Upstream `reasoning: null`. (pi translates the suffix client-side into a real `reasoning_effort` field; confirmed from pi's `OpenAI/JS` inbound bodies, which carry `model: "gpt-5.4"` + `reasoning_effort: "medium"`.) |
| route the endpoint through `provider_family: "openai"` instead of `"openai-compatible"` | **WORKS on the wire, but see below.** |

The last one is real. `OpenAIProvider(model="gpt-5.4",
base_url="http://127.0.0.1:8317/v1", api_key="bench")` with `thinking="medium"` produces,
confirmed in the relay log:

```
client user-agent       : OpenAI/Python 1.109.1
INBOUND reasoning_effort: 'medium'
UPSTREAM reasoning      : {"effort": "medium"}
```

No API key is involved: the relay runs `require_bearer_auth = false`, and `"bench"` is the
same literal placeholder `opencode` and `pi` already carry because their schemas demand the
field. `OPENAI_API_KEY` was verified unset throughout.

**Why it was not used for a re-run anyway** — three independent reasons:

1. **It changes more than the reasoning dial.** The two providers build different payloads.
   `openai-compatible` sends `prompt_cache_key`, `temperature`, `top_p`, `tool_choice`;
   `openai` sends `frequency_penalty`, `presence_penalty`, `max_completion_tokens` and no
   `prompt_cache_key`. Swapping provider class to fix effort introduces a second confound —
   notably the loss of prompt-cache keying — into cells meant to be compared against 18
   others built with the first payload shape.
2. **Applying it to the abstractcode arms alone would invert the unfairness, not remove
   it.** Since `tui-basic`/`tui-coder`/`tui-multi` are equally affected (§2), giving only
   abstractcode medium would make it the sole relay arm with reasoning on. That is exactly
   the "tuning so a particular arm wins" the charter forbids, pointed the other way.
3. **Using it requires a config write I am not authorised to make.** The bench passes
   `--provider endpoint:airelay`, resolved from
   `~/.abstractcore/config/abstractcore.json`, where the profile is declared
   `"provider_family": "openai-compatible"`. Getting the bench onto the `openai` family
   means adding or editing a profile in that existing file. Under the operator's
   constraint issued mid-task (no deletions, no overwrites of existing files, no git
   mutations), rewriting that file is out of scope, and I did not do it.

---

## 7. What was NOT done, and why

**The 6 abstractcode cells were not re-run.** The charter's own stop rule applies: a
re-run at the wrong effort is worse than no re-run. Re-running only `abstractcode-basic`
and `abstractcode-coder` at a verified medium — while `tui-basic`, `tui-coder` and
`tui-multi` remain at reasoning off — would produce a table that looks like a fair
comparison and is not one, and it would flatter abstractcode specifically.

The honest unit of re-run, once abstractcore ships the fix, is **all 15 relay-routed
cells** (`tui-basic` ×3, `tui-coder` ×3, `tui-multi` ×3, `abstractcode-basic` ×3,
`abstractcode-coder` ×3). `opencode` ×3 and `pi` ×3 already ran at medium and are
comparable as they stand. `codex` ×3 remains on its own subscription backend with its own
documented asymmetry.

**Blocked on:** the `abstractcore` seat, §4.1.

### 7.1 The measured scores that stand, for reference

No "new" column exists, because nothing was re-run. Recorded here so the numbers the
re-run would have been compared against are in one place. Every one of these cells except
`opencode` and `pi` ran with reasoning off.

| cell | SCORE | TIER0 | effort actually on the wire |
|---|---|---|---|
| abstractcode-basic-1 | 0.3889 | PASS | **none** |
| abstractcode-basic-2 | 0.5593 | PASS | **none** |
| abstractcode-basic-3 | 0.5507 | PASS | **none** |
| abstractcode-coder-1 | 0.5850 | PASS | **none** |
| abstractcode-coder-2 | 0.0000 | **FAIL** | **none** |
| abstractcode-coder-3 | 0.5850 | PASS | **none** |
| tui-basic-1 / -2 / -3 | 0.5530 / 0.5741 / 0.5920 | PASS | **none** |
| tui-coder-1 / -2 / -3 | 0.5850 / 0.5850 / 0.5850 | PASS | **none** |
| tui-multi-1 / -2 / -3 | 0.5772 / 0.5641 / 0.6000 | PASS | **none** |
| opencode-1 / -2 / -3 | 0.4040 / 0.5929 / 0.5897 | PASS | medium |
| pi-1 / -2 / -3 | 0.5933 / 0.5867 / 0.5741 | PASS | medium |
| codex-1 / -2 / -3 | 0.5891 / 0.5821 / 0.5786 | PASS | medium (own subscription, not the relay) |

Worth noting for whoever designs the re-run: the two arms that *did* run at medium do not
stand out — `pi` (0.5741–0.5933) and `opencode` (0.4040–0.5929) sit inside the same band as
the reasoning-off arms (0.5507–0.6000, excluding the two outliers). That is **not**
evidence that effort does not matter — the spread is small, n=3 per arm, and the scorer
caveat in §7a applies. It does mean nobody should expect the re-run to reorder the table,
and it is a reason to size the re-run for a real effect rather than to assume one.

## 8. Separate finding: the scorer is NOT frozen at the version that produced `scores.json`

The charter states the rubric is frozen. Three *different* scorer hashes are in play, and
they are all hashes of the same file pair — `scripts/rtype_review_score.py` +
`scripts/zelda_review_score.py`, via `sha_of()`, which is what both
`rtype_review_score.py:2258` and `run_fixture_suite.py:336` compute:

| where | recorded hash |
|---|---|
| `untracked/rtype-bench/scores.json` → `scorer_sha256` (the version that produced the 24 measured scores) | `815c0d42…` |
| `tests/rtype_fixtures/baseline.json` → `rubric_sha256` (the version the fixture baseline was captured against) | `f1df6e18…` |
| working tree today | `ab7fe7e3…` |

Both files are modified in the working tree (`git status`: ` M scripts/rtype_review_score.py`,
` M tests/rtype_fixtures/baseline.json`). **Not by this investigation** — their mtimes are
17:28:17 and 17:38:35 local, roughly ninety minutes before this session's first probe, and
nothing in this pass wrote to `scripts/` or `tests/`. Flagged, not touched.

**The fixture suite passes and reports zero drift**, which bounds the concern usefully:

```
SUITE PASS — every attack scored below every control by at least 0.25,
             and no attack cleared 0.5
MARGIN  worst control 0.9260 - best attack 0.4406 = +0.4854   (floor 0.25)
BASELINE DRIFT   every fixture  +0.0000
```

So the `f1df6e18… → ab7fe7e3…` change is behaviour-preserving on all ten fixtures — the
hash moved, the rubric's judgements did not. The remaining question is the *first* hop,
`815c0d42… → f1df6e18…`, which the fixture baseline does not cover, because
`815c0d42…` is the version that actually produced the 24 measured scores in
`scores.json`.

That is directly testable and does not require re-running anything: re-score the existing
products with today's scorer and diff against `scores.json`.

**Status: that re-score was launched and had not finished when this report was written**
(24 products, each rendered under Playwright, `--jobs 4`). It reads the products and writes
only to a scratch path — nothing under `untracked/rtype-bench/` is touched. Command, for
whoever picks it up:

```
python3 scripts/rtype_review_score.py --root untracked/rtype-bench --jobs 4 \
        --out <somewhere-outside-the-bench-dir>.json
```

then diff `rows[cell].SCORE` against `untracked/rtype-bench/scores.json`. A non-zero delta
on any cell means the 24-cell table is not self-consistent with today's instrument and
must be fully re-scored before any new cell is compared against it. Marked **UNKNOWN**
until that lands — not assumed to be zero.

Either way the discipline for a re-run stands: re-score **all** cells with one pinned
scorer version and record it. The `scorer_sha256` field already exists to catch exactly
this — it just has to be checked rather than only written.

## 9. Known-unknowns

- **UNKNOWN:** the magnitude of the quality effect. Nothing here measures how much
  reasoning=medium would change R-Type scores; it establishes only that the setting never
  took effect.
- **UNKNOWN:** whether `abstractcode --agent coder` propagates the preference into
  `_runtime.thinking` at all (§3.2). Moot until abstractcore is fixed, then worth
  re-checking before any re-run.
- **UNKNOWN:** attribution of the 6 `Python-urllib/3.12` requests in the bench window
  (gpt-5.4, `reasoning_effort` absent). Low volume; not any measured arm's main traffic.
- **Observed, not investigated:** MLX (`mlx-community/Qwen3-*`, `unsloth/Qwen3-*`) and TTS
  workflow runs were executing on the same machine during the 2026-08-02 bench window
  (run store timestamps 21:43–22:09). Whether that contended for resources with the
  measured cells is UNKNOWN.
