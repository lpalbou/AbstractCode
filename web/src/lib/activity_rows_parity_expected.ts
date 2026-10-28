// The intentional differences between the Activity rows built from the replay view and the ones
// built from the ledger, for every AbstractRuntime 0.15.0 conformance case (fixtures: the case's
// ledger `records.json` and the view AbstractGateway 0.17.0 serves for it, `view.json`). Any other
// difference fails `activity_rows_view.test.ts`; a difference that disappears fails it too (update
// this table). Rows read `run | kind | title | detail | status | statusLabel`.

/** Why a row differs. */
export const REASONS = {
  TIMER: "The view keeps a timer's `until` only while it waits and never its `resume_to_node`: a finished timer reads \"wait · timer\" (runtime backlog 0860 territory).",
  CHILD_OUTPUT: "A child run's output (its stop reason, iterations, answer) is not in the view; only the root's final message is. The row is there, without that detail.",
  DELEGATION_CLOSED: "The ledger fold leaves a delegation \"waiting for subflow\" when the run was cancelled or ended without a resume record; the view closes it (runtime rule: a later step or a terminal record ends a wait). A fix, not a loss.",
  EXECUTION_RULE: "One activity per node execution (runtime `replay/executions.py`, AbstractFlow's executionIdFor): an Agent's structured-output model call joins its node's execution instead of being a separate step. Fewer, merged rows.",
  SLIM: "The ledger fold cannot resolve a `$slim` pointer outside the window it read; the view resolves it, so the row names the tool and its target.",
  PAYLOAD: "A step's raw effect payload (here `model_residency`) is not in the view: the row has its title, not the payload preview.",
  SNAPSHOT_FINAL: "The run's end comes from its snapshot when its ledger has no completion record: the view shows \"run finished\" with the final answer, the ledger fold shows nothing.",
  LLM_CALLS: "\"N tool calls\" on a model step is read from the tool step that follows it in the view; this synthetic ledger's llm result lists no calls.",
} as const;

export type Reason = keyof typeof REASONS;
export type ParityExpectation = {
  ledgerOnly: [string, Reason][];
  viewOnly: [string, Reason][];
  groups?: { ledger: string; view: string; why: Reason };
};

export const EXPECTED_DIFFERENCES: Record<string, ParityExpectation> = {
  a2_delegated_approval_completed: {
    ledgerOnly: [
      ["c298 | wait | wait · delay 0 s | until 00:23:17 · resumes at end | completed | completed", "TIMER"],
      ["c298 | run | subflow finished | state txt-7702e579893f · post delay 0 · success true | completed | completed", "CHILD_OUTPUT"],
      ["7480 | run | subflow finished | txt-1313194b4bc8 · 5 iterations | completed | completed", "CHILD_OUTPUT"],
      ["7bae | wait | wait · delay 0 s | until 00:24:07 · resumes at end | completed | completed", "TIMER"],
      ["7bae | run | subflow finished | state txt-0bfffb0038e0 · post delay 0 · success true | completed | completed", "CHILD_OUTPUT"],
    ],
    viewOnly: [
      ["c298 | wait | wait · timer |  | completed | completed", "TIMER"],
      ["c298 | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
      ["7480 | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
      ["7bae | wait | wait · timer |  | completed | completed", "TIMER"],
      ["7bae | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
    ],
  },
  a_delegated_child_approval: {
    ledgerOnly: [
      ["c298 | wait | wait · delay 0 s | until 00:23:17 · resumes at end | completed | completed", "TIMER"],
      ["c298 | run | subflow finished | state txt-7702e579893f · post delay 0 · success true | completed | completed", "CHILD_OUTPUT"],
    ],
    viewOnly: [
      ["c298 | wait | wait · timer |  | completed | completed", "TIMER"],
      ["c298 | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
    ],
  },
  b_timer_without_resume: {
    ledgerOnly: [
      ["3283 | wait | wait · delay 4 s | until 03:32:05 · resumes at node-3 | completed | completed", "TIMER"],
    ],
    viewOnly: [
      ["3283 | wait | wait · delay 4 s | until 03:32:05 | completed | completed", "TIMER"],
    ],
  },
  c_parallel_tool_batch: {
    ledgerOnly: [
      ["7582 | wait | wait · until 20:04:02 | resumes at end | completed | completed", "TIMER"],
      ["7582 | run | subflow finished | state txt-7702e579893f · post delay 0 · success true | completed | completed", "CHILD_OUTPUT"],
      ["324f | run | subflow finished | txt-1313194b4bc8 · 2 iterations | completed | completed", "CHILD_OUTPUT"],
      ["9aa0 | wait | wait · delay 3 s | until 20:04:50 · resumes at end | completed | completed", "TIMER"],
      ["9aa0 | run | subflow finished | state txt-0bfffb0038e0 · post delay 3 · success true | completed | completed", "CHILD_OUTPUT"],
    ],
    viewOnly: [
      ["7582 | wait | wait · timer |  | completed | completed", "TIMER"],
      ["7582 | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
      ["324f | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
      ["9aa0 | wait | wait · delay 3 s | until 20:04:50 | completed | completed", "TIMER"],
      ["9aa0 | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
    ],
  },
  d_cancellation: {
    ledgerOnly: [
      ["edfc | wait | wait · until 19:23:07 | resumes at end | completed | completed", "TIMER"],
      ["edfc | run | subflow finished | state txt-7702e579893f · post delay 0 · success true | completed | completed", "CHILD_OUTPUT"],
      ["fdd8 | subflow | subflow · basic-agent | node-2 · run e14a0f3c | waiting | waiting for subflow", "DELEGATION_CLOSED"],
    ],
    viewOnly: [
      ["edfc | wait | wait · timer |  | completed | completed", "TIMER"],
      ["edfc | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
      ["fdd8 | subflow | subflow · basic-agent | node-2 · run e14a0f3c | completed | completed", "DELEGATION_CLOSED"],
    ],
    groups: { ledger: "Start: Waiting for subflow, Step 1: cancelled", view: "Start: Done, Step 1: cancelled", why: "DELEGATION_CLOSED" },
  },
  e2_structured_repeated_node: {
    ledgerOnly: [
      ["44ed | run | subflow finished | txt-1313194b4bc8 · 3 iterations | completed | completed", "CHILD_OUTPUT"],
      ["67d4 | llm | llm · endpoint:airelay · gpt-5.6-terra | 8.6 s | completed | completed", "EXECUTION_RULE"],
      ["67d4 | llm | llm · endpoint:airelay · gpt-5.6-terra | 17 s | completed | completed", "EXECUTION_RULE"],
    ],
    viewOnly: [
      ["44ed | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
    ],
    groups: { ledger: "Step 1: Done, Step 2: Done, Step 3: Done, Step 4: Done, Step 5: Done, Step 6: Done", view: "Step 1: Done, Step 2: Done, Step 3: Done, Step 4: Done", why: "EXECUTION_RULE" },
  },
  e_structured_output_agents: {
    ledgerOnly: [
      ["4df7 | run | subflow finished | txt-1313194b4bc8 · 1 iteration | completed | completed", "CHILD_OUTPUT"],
      ["7385 | llm | llm · lmstudio · ornith-1.0-35b | 74.1 s | completed | completed", "EXECUTION_RULE"],
      ["bf50 | run | subflow finished | txt-1313194b4bc8 · 1 iteration | completed | completed", "CHILD_OUTPUT"],
      ["7385 | llm | llm · lmstudio · ornith-1.0-35b | 42 s | completed | completed", "EXECUTION_RULE"],
      ["7385 | subflow | subflow · co-scientist_0_1_0_co-scientist_rank | run 41fe165e | waiting | waiting for subflow", "DELEGATION_CLOSED"],
    ],
    viewOnly: [
      ["4df7 | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
      ["bf50 | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
      ["7385 | subflow | subflow · co-scientist_0_1_0_co-scientist_rank | run 41fe165e | completed | completed", "DELEGATION_CLOSED"],
    ],
    groups: { ledger: "Start: Done, Step 1: Done, Step 2: Done, Step 3: Done, Step 4: Waiting for subflow", view: "Start: Done, Step 1: Done, Step 2: Done", why: "EXECUTION_RULE" },
  },
  f_windowed_slim: {
    ledgerOnly: [
      ["7480 | tools | tools · tool call |  | waiting | approval needed", "SLIM"],
    ],
    viewOnly: [
      ["7480 | tools | tools · write_file | file path txt-0103dd11b7b3 | waiting | approval needed", "SLIM"],
    ],
  },
  g_ask_user: {
    ledgerOnly: [
      ["73c7 | step | model residency | type model_residency · operation txt-39ec261f0d84 · task txt-d79d9213c3f6 · provider lmstudio · model qwen/qw… | completed | completed", "PAYLOAD"],
      ["73c7 | step | model residency | type model_residency · operation txt-39ec261f0d84 · task txt-0369b2b12ff4 · provider omnivoice · resume to no… | completed | completed", "PAYLOAD"],
    ],
    viewOnly: [
      ["73c7 | step | model residency |  | completed | completed", "PAYLOAD"],
      ["73c7 | step | model residency |  | completed | completed", "PAYLOAD"],
      ["73c7 | run | run finished | txt-7ddd2d759a50 | completed | completed", "SNAPSHOT_FINAL"],
    ],
  },
  h_denied_approval: {
    ledgerOnly: [
      ["1e54 | run | subflow finished | 2 iterations | completed | completed", "CHILD_OUTPUT"],
    ],
    viewOnly: [
      ["1e54 | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
    ],
  },
  i_wait_kinds: {
    ledgerOnly: [
    ],
    viewOnly: [
    ],
  },
  j_event_answered_then_pause: {
    ledgerOnly: [
    ],
    viewOnly: [
    ],
  },
  k_nested_delegation: {
    ledgerOnly: [
      ["c3c3 | run | subflow finished | grandchild done | completed | completed", "CHILD_OUTPUT"],
      ["c1c1 | run | subflow finished | a done | completed | completed", "CHILD_OUTPUT"],
      ["c2c2 | run | subflow finished | b done | completed | completed", "CHILD_OUTPUT"],
    ],
    viewOnly: [
      ["c2c2 | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
      ["c1c1 | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
      ["c3c3 | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
    ],
  },
  l_answer_then_completion: {
    ledgerOnly: [
    ],
    viewOnly: [
    ],
  },
  m_session_tool_policy: {
    ledgerOnly: [
      ["e1e1 | llm | llm · lmstudio · m | 20 tokens in · 5 out · 1 s | completed | completed", "LLM_CALLS"],
      ["e1e1 | run | subflow finished | Fetched; listing was refused. | completed | completed", "CHILD_OUTPUT"],
    ],
    viewOnly: [
      ["e1e1 | llm | llm · lmstudio · m | 20 tokens in · 5 out · 1 s · 2 tool calls | completed | completed", "LLM_CALLS"],
      ["e1e1 | run | subflow finished |  | completed | completed", "CHILD_OUTPUT"],
    ],
  },
};
