import { describe, expect, it } from "vitest";

import {
  buildWorkflowInput,
  normalizeSessionSummaries,
  normalizeToolCatalog,
  normalizeWorkflowCatalog,
  normalizeWorkspacePolicy,
  resolveRestoredWorkflow,
  type WorkflowDefinition,
} from "./catalog";

const agentWorkflow: WorkflowDefinition = {
  id: "private:code@1:agent",
  workflowId: "code@1:agent",
  bundleId: "code",
  bundleVersion: "1",
  flowId: "agent",
  name: "Agent",
  description: "",
  interfaces: ["abstractcode.agent.v1"],
  registryScope: "private",
};

describe("normalizeWorkflowCatalog", () => {
  it("keeps every runnable interface and qualifies private and catalog identities", () => {
    const privateBundles = {
      default_bundle_id: "demo",
      bundles: [
        {
          bundle_id: "demo",
          bundle_version: "1.0.0",
          default_entrypoint: "agent",
          entrypoints: [
            {
              flow_id: "agent",
              workflow_id: "demo@1.0.0:agent",
              name: "Code agent",
              interfaces: ["abstractcode.agent.v1"],
            },
            {
              flow_id: "report",
              workflow_id: "demo@1.0.0:report",
              name: "Report",
              interfaces: ["acme.report.v2"],
              input_schema: { type: "object", required: ["topic"] },
            },
            { flow_id: "old", name: "Old", deprecated: true, interfaces: [] },
          ],
        },
      ],
    };
    const catalog = {
      items: [
        {
          bundle_id: "demo",
          bundle_version: "2.0.0",
          actions: { can_run: true },
          entrypoints: [
            {
              flow_id: "report",
              name: "Report v2",
              interfaces: ["acme.report.v2"],
            },
          ],
        },
        {
          bundle_id: "forbidden",
          actions: { can_run: false },
          entrypoints: [{ flow_id: "hidden", interfaces: [] }],
        },
      ],
    };

    const workflows = normalizeWorkflowCatalog(privateBundles, catalog);

    expect(workflows.map((item) => item.id)).toEqual([
      "private:demo@1.0.0:agent",
      "private:demo@1.0.0:report",
      "tenant_catalog:demo@2.0.0:report",
    ]);
    expect(workflows[0]).toMatchObject({
      bundleVersion: "1.0.0",
      registryScope: "private",
      isDefault: true,
    });
    expect(workflows[1].inputSchema).toEqual({
      type: "object",
      required: ["topic"],
    });
  });

  it("only filters interfaces when explicitly requested", () => {
    const bundles = [
      {
        bundle_id: "demo",
        entrypoints: [
          { flow_id: "one", interfaces: ["one.v1"] },
          { flow_id: "two", interfaces: ["two.v1"] },
        ],
      },
    ];
    expect(normalizeWorkflowCatalog(bundles)).toHaveLength(2);
    expect(
      normalizeWorkflowCatalog(bundles, undefined, {
        interfaceId: "two.v1",
      }).map((item) => item.flowId),
    ).toEqual(["two"]);
  });

  it("restores a catalog workflow only from the public gateway selection", () => {
    const workflows = normalizeWorkflowCatalog(undefined, {
      items: [
        {
          bundle_id: "reports",
          bundle_version: "1.0.0",
          entrypoints: [{ flow_id: "daily", name: "Daily" }],
        },
        {
          bundle_id: "reports",
          bundle_version: "2.0.0",
          entrypoints: [{ flow_id: "daily", name: "Daily v2" }],
        },
      ],
    });

    expect(
      resolveRestoredWorkflow(workflows, {
        inputData: {
          workflow_selection: {
            registry_scope: "tenant_catalog",
            bundle_id: "reports",
            bundle_version: "2.0.0",
            flow_id: "daily",
          },
        },
        runWorkflowId: "__catalog__v2__tenant@2.0.0:daily",
      })?.id,
    ).toBe("tenant_catalog:reports@2.0.0:daily");
    expect(
      resolveRestoredWorkflow(workflows, {
        runWorkflowId: "__catalog__v2__tenant@2.0.0:daily",
      }),
    ).toBeUndefined();
  });

  it("falls back to one exact private workflow id and never an ambiguous default", () => {
    const privateWorkflow = { ...agentWorkflow };
    const duplicate = { ...agentWorkflow, id: "private:duplicate" };
    expect(
      resolveRestoredWorkflow([privateWorkflow], {
        runWorkflowId: "code@1:agent",
      }),
    ).toBe(privateWorkflow);
    expect(
      resolveRestoredWorkflow([privateWorkflow, duplicate], {
        runWorkflowId: "code@1:agent",
      }),
    ).toBeUndefined();
    expect(resolveRestoredWorkflow([privateWorkflow], {})).toBeUndefined();
  });
});

describe("normalizers", () => {
  it("preserves raw workspace policy while exposing UI-safe fields (round 9: shared workspace, no modes)", () => {
    const policy = normalizeWorkspacePolicy({
      policy: {
        allowed_folders: ["/data/projects"],
        allow_any_folder: false,
        max_attachment_bytes: 1024,
        future_field: { enabled: true },
      },
    });
    expect(policy).toMatchObject({ maxAttachmentBytes: 1024 });
    expect(Object.keys(policy).sort()).toEqual(["maxAttachmentBytes", "raw"]);
    expect(policy.raw.future_field).toEqual({ enabled: true });
  });

  it("normalizes enabled and served-disabled tools without dropping metadata", () => {
    const tools = normalizeToolCatalog({
      items: [
        {
          name: "write_file",
          description: "Write",
          toolset: "fs",
          approval_default: "ask",
          risk_rank: 5,
          enabled: false,
          enable_gate: "workspace",
          why_disabled: "read only",
        },
        {
          name: "read_file",
          description: "Read",
          default_approval: "auto",
          parameters: { type: "object" },
        },
      ],
    });
    expect(tools.map((tool) => tool.name)).toEqual(["read_file", "write_file"]);
    expect(tools[0]).toMatchObject({
      enabled: true,
      servedDisabled: false,
      approval: "auto",
      inputSchema: { type: "object" },
    });
    expect(tools[1]).toMatchObject({
      enabled: false,
      servedDisabled: true,
      approval: "ask",
      riskRank: 5,
      enableGate: "workspace",
      whyDisabled: "read only",
    });
  });

  it("groups root runs, derives priority state, and retains latest run identity", () => {
    const sessions = normalizeSessionSummaries(
      {
        has_more: true,
        runs: [
          {
            run_id: "r2",
            session_id: "s1",
            status: "waiting",
            created_at: "2026-01-02T00:00:00Z",
            updated_at: "2026-01-03T00:00:00Z",
          },
          {
            run_id: "child",
            session_id: "s1",
            parent_run_id: "r2",
            status: "running",
          },
          {
            run_id: "r1",
            session_id: "s1",
            status: "completed",
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-01T01:00:00Z",
          },
          {
            run_id: "r3",
            session_id: "s2",
            status: "failed",
            created_at: "2026-01-04T00:00:00Z",
          },
        ],
      },
      { r1: { prompt: "first prompt" } },
    );

    expect(sessions.map((session) => session.sessionId)).toEqual(["s2", "s1"]);
    expect(
      sessions.find((session) => session.sessionId === "s1"),
    ).toMatchObject({
      sessionId: "s1",
      state: "waiting",
      firstRunId: "r1",
      latestRunId: "r2",
      runIds: ["r2", "r1"],
      turnCount: 2,
      prompt: "first prompt",
      truncated: true,
    });
    expect(sessions[0]).toMatchObject({
      sessionId: "s2",
      state: "failed",
      latestRunId: "r3",
    });
  });
});

describe("normalizeSessionSummaries session_totals", () => {
  const page = (extra: Record<string, unknown>) => ({
    has_more: true,
    runs: [
      { run_id: "a2", session_id: "sa", status: "completed", created_at: "2026-01-02T00:00:00Z", tool_calls: 3 },
      { run_id: "a1", session_id: "sa", status: "completed", created_at: "2026-01-01T00:00:00Z", tool_calls: 1 },
      { run_id: "b1", session_id: "sb", status: "completed", created_at: "2026-01-03T00:00:00Z", tool_calls: 2 },
    ],
    ...extra,
  });
  const byId = (body: unknown) =>
    Object.fromEntries(normalizeSessionSummaries(body, {}).map((s) => [s.sessionId, s]));

  it("shows the gateway's exact turn count and hides the tool sum of a cut session", () => {
    const s = byId(page({ session_totals: { sa: { turns: 9 }, sb: { turns: 1 } } }));
    expect(s.sa).toMatchObject({ turnCount: 9, truncated: false });
    expect(s.sa.toolCalls).toBeUndefined();
    // Every turn of sb is on the page: its tool count is exact.
    expect(s.sb).toMatchObject({ turnCount: 1, toolCalls: 2, truncated: false });
  });

  it("keeps lower bounds when the gateway sends no totals (older gateway)", () => {
    const s = byId(page({}));
    expect(s.sa).toMatchObject({ turnCount: 2, toolCalls: 4, truncated: true });
    expect(s.sb).toMatchObject({ turnCount: 1, toolCalls: 2, truncated: true });
  });

  it("keeps lower bounds for a session missing from the totals or with an unusable value", () => {
    const s = byId(page({ session_totals: { sa: { turns: 1 } } }));
    // 1 < the 2 turns listed: not a total, ignored.
    expect(s.sa).toMatchObject({ turnCount: 2, truncated: true });
    expect(s.sb).toMatchObject({ turnCount: 1, truncated: true });
  });
});

describe("buildWorkflowInput", () => {
  it("keeps generic public inputs exact while applying Gateway runtime controls", () => {
    const schemaInputs = {
      topic: "old",
      provider: "workflow-provider",
      _limits: { max_tokens: 7 },
      required_object: { a: 1 },
    };
    const output = buildWorkflowInput({
      workflow: {
        ...agentWorkflow,
        id: "tenant_catalog:reports@2:daily",
        workflowId: "reports@2:daily",
        flowId: "daily",
        interfaces: ["reports.v1"],
      },
      schemaInputs,
      prompt: "new topic",
      promptProperty: "topic",
      model: { provider: "app-provider", model: "model-a" },
      limits: { maxTokens: 99, maxIterations: 4 },
      tools: ["read_file"],
      workspace: { root: "/repo" },
    });

    expect(output).toEqual({
      topic: "new topic",
      workspace_root: "/repo",
      provider: "workflow-provider",
      _limits: { max_tokens: 99, max_iterations: 4 },
      _runtime: { provider: "app-provider", model: "model-a", allowed_tools: ["read_file"] },
      required_object: { a: 1 },
    });
    expect(schemaInputs.topic).toBe("old");
  });

  it("builds the canonical agent contract and preserves an explicit empty tool allowlist", () => {
    const output = buildWorkflowInput({
      workflow: agentWorkflow,
      prompt: "fix the tests",
      attachments: [{ $artifact: "a1" }],
      tools: [],
      model: { provider: "openai", model: "gpt" },
      workspace: { root: "/repo" },
      limits: { maxIterations: 10, maxTokens: 1000 },
      reasoning: "high",
      systemPromptExtra: "Be concise",
      review: { enabled: true, maxRounds: 2 },
      gatingMode: "auto",
      toolPolicy: {
        autoApproveTools: ["read_file"],
        requireApprovalTools: ["write_file"],
      },
      promptCache: { enabled: true, key: "session:key" },
    });

    expect(output).toEqual({
      prompt: "fix the tests",
      context: {
        task: "fix the tests",
        attachments: [{ $artifact: "a1" }],
      },
      use_context: false,
      use_session_history: true,
      provider: "openai",
      model: "gpt",
      tools: [],
      workspace_root: "/repo",
      _runtime: {
        provider: "openai",
        model: "gpt",
        allowed_tools: [],
        thinking: "high",
        system_prompt_extra: "Be concise",
        review_mode: true,
        review_max_rounds: 2,
        tool_policy: {
          auto_approve_tools: ["read_file"],
          require_approval_tools: ["write_file"],
        },
        prompt_cache: { enabled: true, key: "session:key" },
      },
      _limits: { max_iterations: 10, max_tokens: 1000 },
      max_iterations: 10,
      gating_mode: "auto",
    });
  });

  it("omits provider, model, limits, and tools when they are unspecified", () => {
    expect(
      buildWorkflowInput({ workflow: agentWorkflow, prompt: "hello" }),
    ).toEqual({
      prompt: "hello",
      context: { task: "hello" },
      use_context: false,
      use_session_history: true,
    });
  });
});

describe("a turn carries only its own attachments (2026-10-01: a new conversation answered about another conversation's screenshot)", () => {
  const restoredContext = {
    task: "can you see my screenshot?",
    attachments: [{ $artifact: "screenshot-from-conversation-A", name: "gateway.png" }],
    media: [{ $artifact: "screenshot-from-conversation-A", name: "gateway.png" }],
    messages: [{ role: "user", content: "earlier" }],
  };

  it("drops a restored context's attachments and media when the new turn attaches nothing", () => {
    const output = buildWorkflowInput({
      workflow: agentWorkflow,
      schemaInputs: { context: restoredContext, attachments: [{ $artifact: "stale" }], media: [{ $artifact: "stale" }] },
      prompt: "send an email with my host info",
    });
    expect(output.context).toEqual({ task: "send an email with my host info" });
    expect(output).not.toHaveProperty("attachments");
    expect(output).not.toHaveProperty("media");
    expect(JSON.stringify(output)).not.toContain("screenshot-from-conversation-A");
  });

  it("sends exactly the new turn's attachments, never the restored ones", () => {
    const output = buildWorkflowInput({
      workflow: agentWorkflow,
      schemaInputs: { context: restoredContext },
      prompt: "look at this",
      attachments: [{ $artifact: "new-one" }],
    });
    expect(output.context).toEqual({ task: "look at this", attachments: [{ $artifact: "new-one" }] });
  });
});

it("includes chat wake turns but excludes descendants and automation controllers", () => {
 const rows=normalizeSessionSummaries({items:[
  {run_id:"wake",session_id:"chat",role:"occurrence",session_kind:"chat",parent_run_id:"caller",status:"completed"},
  {run_id:"child",session_id:"chat",role:"descendant",session_kind:"chat",parent_run_id:"wake",status:"completed"},
  {run_id:"auto",session_id:"auto",role:"controller",session_kind:"automation",status:"running"},
 ]});
 expect(rows).toHaveLength(1);expect(rows[0].latestRunId).toBe("wake");
});
