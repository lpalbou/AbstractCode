import type { SpeculationValue } from "@abstractframework/ui-kit";
export type JsonObject = Record<string, unknown>;
export type JsonSchema = JsonObject;

export type WorkflowRegistryScope = "private" | "tenant_catalog";

export interface WorkflowDefinition {
  /** UI identity. Includes registry scope so private and catalog copies do not collide. */
  id: string;
  /** Gateway workflow identity, when the discovery response provides one. */
  workflowId: string;
  bundleId?: string;
  bundleVersion?: string;
  flowId: string;
  name: string;
  description: string;
  interfaces: string[];
  inputSchema?: JsonSchema;
  registryScope?: WorkflowRegistryScope;
  isDefault?: boolean;
  isPreferredVersion?: boolean;
}

export interface RestoredWorkflowOptions {
  /** Response from GET /api/gateway/runs/{run_id}/input_data. */
  inputData?: unknown;
  /** Direct run workflow id used only as a private-registry fallback. */
  runWorkflowId?: unknown;
}

export interface NormalizeWorkflowCatalogOptions {
  /** Explicit opt-in filter. Omit it to retain every runnable interface. */
  interfaceId?: string;
  includeDeprecated?: boolean;
}

export interface WorkspaceMount {
  id: string;
  label: string;
  path?: string;
  readOnly: boolean;
  raw: JsonObject;
}

export interface WorkspacePolicy {
  raw: JsonObject;
  clientWorkspaceScopeOverrides: boolean;
  allowedAccessModes: string[];
  mounts: WorkspaceMount[];
  maxAttachmentBytes?: number;
}

export interface ToolSpec {
  name: string;
  description: string;
  toolset?: string;
  tier?: string;
  approval?: string;
  riskRank?: number;
  enabled: boolean;
  servedDisabled: boolean;
  enableGate?: string;
  whyDisabled?: string;
  inputSchema?: JsonSchema;
  raw: JsonObject;
}

export type SessionState =
  | "waiting"
  | "running"
  | "failed"
  | "done"
  | "unknown";

export interface SessionSummary {
  sessionId: string;
  state: SessionState;
  updatedAt?: string;
  firstRunId: string;
  latestRunId: string;
  /** Newest first, matching the gateway's recent-runs view. */
  runIds: string[];
  turnCount: number;
  prompt?: string;
  /** True when the source run page says there are more rows. */
  truncated: boolean;
}

export interface ModelSettings {
  provider?: string;
  model?: string;
}

export interface WorkspaceSettings {
  root?: string;
  accessMode?: string;
  allowedPaths?: string[];
}

export interface WorkflowLimits {
  maxIterations?: number;
  maxTokens?: number;
}

export interface ToolPolicySettings {
  autoApproveTools?: string[];
  requireApprovalTools?: string[];
}

export interface ReviewSettings {
  enabled: boolean;
  maxRounds?: number;
}

export interface WorkflowInputOptions {
  workflow?: WorkflowDefinition;
  /** Validated values from the selected workflow's JSON schema. */
  schemaInputs?: JsonObject;
  inputSchema?: JsonSchema;
  prompt?: string;
  /** The only generic-workflow field that buildWorkflowInput may overwrite. */
  promptProperty?: string;
  model?: ModelSettings;
  workspace?: WorkspaceSettings;
  /** Presence is significant: an explicit empty array means no tools. */
  tools?: string[];
  skills?: string[];
  messages?: Array<{ role: string; content: string }>;
  attachments?: JsonObject[];
  limits?: WorkflowLimits;
  reasoning?: string;
  speculation?: SpeculationValue;
  /** Replaces the agent workflow's system prompt when explicitly set. */
  system?: string;
  systemPromptExtra?: string;
  review?: ReviewSettings;
  /** Set false for agent variants (for example MemAct) that reject review args. */
  reviewCapable?: boolean;
  gatingMode?: "wait" | "auto";
  toolPolicy?: ToolPolicySettings;
  promptCache?: boolean | JsonObject;
  useSessionHistory?: boolean;
}

const AGENT_INTERFACE = "abstractcode.agent.v1";
export function isChatAgent(workflow?: WorkflowDefinition | null): boolean {
  return Boolean(workflow?.interfaces.some((name) => [AGENT_INTERFACE, "abstractassistant.agent.v1"].includes(name)));
}

function record(value: unknown): JsonObject | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as JsonObject)
    : undefined;
}

function array(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function text(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const normalized = value.trim();
  return normalized || undefined;
}

function strings(value: unknown): string[] {
  return array(value).flatMap((item) => {
    const normalized = text(item);
    return normalized ? [normalized] : [];
  });
}

function finiteInteger(value: unknown, minimum = 0): number | undefined {
  if (typeof value !== "number" || !Number.isFinite(value)) return undefined;
  const normalized = Math.trunc(value);
  return normalized >= minimum ? normalized : undefined;
}

function responseItems(value: unknown, keys: string[]): unknown[] {
  if (Array.isArray(value)) return value;
  const body = record(value);
  if (!body) return [];
  for (const key of keys) {
    if (Array.isArray(body[key])) return body[key] as unknown[];
  }
  return [];
}

function schema(value: unknown): JsonSchema | undefined {
  const candidate = record(value);
  return candidate ? { ...candidate } : undefined;
}

function entrypointSchema(entrypoint: JsonObject): JsonSchema | undefined {
  const direct = schema(
    entrypoint.input_schema ??
      entrypoint.inputSchema ??
      entrypoint.inputs_schema,
  );
  if (direct) return direct;
  const inputs = record(entrypoint.inputs);
  // Gateway bundle listings historically exposed input pins as arrays. Only an
  // object that looks like JSON Schema is promoted to inputSchema.
  if (
    inputs &&
    ("properties" in inputs || "type" in inputs || "$schema" in inputs)
  )
    return { ...inputs };
  return undefined;
}

function inferBundleVersion(bundle: JsonObject): string | undefined {
  const direct = text(
    bundle.bundle_version ?? bundle.bundleVersion ?? bundle.version,
  );
  if (direct) return direct;
  const bundleRef = text(bundle.bundle_ref ?? bundle.bundleRef);
  if (!bundleRef) return undefined;
  const at = bundleRef.lastIndexOf("@");
  return at > 0 && at < bundleRef.length - 1
    ? bundleRef.slice(at + 1)
    : undefined;
}

function normalizedWorkflowId(
  bundleId: string | undefined,
  bundleVersion: string | undefined,
  flowId: string,
): string {
  if (!bundleId) return flowId;
  return `${bundleId}${bundleVersion ? `@${bundleVersion}` : ""}:${flowId}`;
}

function workflowsFrom(
  source: unknown,
  registryScope: WorkflowRegistryScope,
  options: NormalizeWorkflowCatalogOptions,
): WorkflowDefinition[] {
  const envelope = record(source);
  const envelopeDefaultBundleId = text(
    envelope?.default_bundle_id ?? envelope?.defaultBundleId,
  );
  const bundles = responseItems(source, ["items", "bundles"]);
  const out: WorkflowDefinition[] = [];
  for (const rawBundle of bundles) {
    const bundle = record(rawBundle);
    if (!bundle) continue;
    const actions = record(bundle.actions);
    if (actions?.can_run === false) continue;
    const bundleStatus = text(bundle.status)?.toLowerCase();
    if (bundle.is_draft === true || bundle.is_published === false || bundle.version_channel === "draft" ||
      (bundleStatus && !["published", "deprecated"].includes(bundleStatus))) continue;
    if (
      !options.includeDeprecated &&
      (bundleStatus === "deprecated" ||
        bundleStatus === "deleted" ||
        bundleStatus === "disabled")
    )
      continue;

    const bundleId = text(bundle.bundle_id ?? bundle.bundleId ?? bundle.id);
    const bundleVersion = inferBundleVersion(bundle);
    if (bundleVersion && /(?:^|[.\-])draft(?:[.\-]|$)/i.test(bundleVersion)) continue;
    const defaultEntrypoint = text(
      bundle.default_entrypoint ?? bundle.defaultEntrypoint,
    );
    const bundleDefault =
      registryScope === "private" && (bundle.is_default === true ||
      bundle.isDefault === true ||
      (
        Boolean(bundleId) &&
        bundleId === envelopeDefaultBundleId));

    for (const rawEntrypoint of array(bundle.entrypoints)) {
      const entrypoint = record(rawEntrypoint);
      if (!entrypoint) continue;
      if (
        !options.includeDeprecated &&
        (entrypoint.deprecated === true || entrypoint.enabled === false)
      )
        continue;
      const flowId = text(
        entrypoint.flow_id ?? entrypoint.flowId ?? entrypoint.id,
      );
      if (!flowId) continue;
      const interfaces = strings(
        entrypoint.interfaces ??
          entrypoint.interface_ids ??
          entrypoint.interfaceIds,
      );
      if (options.interfaceId && !interfaces.includes(options.interfaceId))
        continue;
      const workflowId =
        text(entrypoint.workflow_id ?? entrypoint.workflowId) ??
        normalizedWorkflowId(bundleId, bundleVersion, flowId);
      const isDefaultEntrypoint = defaultEntrypoint
        ? defaultEntrypoint === flowId
        : array(bundle.entrypoints).length === 1;
      out.push({
        id: `${registryScope}:${workflowId}`,
        workflowId,
        ...(bundleId ? { bundleId } : {}),
        ...(bundleVersion ? { bundleVersion } : {}),
        flowId,
        name: text(entrypoint.name ?? entrypoint.title) ?? flowId,
        description: text(entrypoint.description) ?? "",
        interfaces,
        ...(entrypointSchema(entrypoint)
          ? { inputSchema: entrypointSchema(entrypoint) }
          : {}),
        registryScope,
        isPreferredVersion: registryScope === "tenant_catalog"
          ? bundle.is_default === true || bundle.default_version === bundleVersion
          : bundle.latest_published_version === bundleVersion,
        ...(bundleDefault && isDefaultEntrypoint ? { isDefault: true } : {}),
      });
    }
  }
  return out;
}

/** New-run projection, separate from exact identities retained for history.
 * Catalog publication shadows the private installed copy of the SAME bundle.
 * Select the version before entrypoints, so removed entrypoints stay removed. */
export function publishedWorkflowChoices(workflows: readonly WorkflowDefinition[]): WorkflowDefinition[] {
  const bundles = new Map<string, WorkflowDefinition[]>();
  for (const workflow of workflows) {
    const key = workflow.bundleId || workflow.id;
    bundles.set(key, [...(bundles.get(key) || []), workflow]);
  }
  const choices: WorkflowDefinition[] = [];
  for (const rows of bundles.values()) {
    const shared = rows.filter(row => row.registryScope === "tenant_catalog");
    const candidates = shared.length ? shared : rows;
    const preferred = [...candidates].sort((a, b) =>
      Number(Boolean(b.isPreferredVersion)) - Number(Boolean(a.isPreferredVersion)) ||
      (b.bundleVersion || "").localeCompare(a.bundleVersion || "", undefined, { numeric: true })) [0];
    const defaultFlow = rows.find(row => row.isDefault)?.flowId;
    for (const row of candidates.filter(row => row.bundleVersion === preferred.bundleVersion))
      choices.push({ ...row, isDefault: row.flowId === defaultFlow });
  }
  return choices.sort((a, b) => Number(Boolean(b.isDefault)) - Number(Boolean(a.isDefault)) || a.name.localeCompare(b.name) || a.id.localeCompare(b.id));
}

export function workflowChoiceLabel(workflow: WorkflowDefinition, choices: readonly WorkflowDefinition[]): string {
  return choices.some(other => other.id !== workflow.id && other.name === workflow.name)
    ? `${workflow.name} · ${workflow.bundleId || workflow.flowId}` : workflow.name;
}

/** Normalize legacy private bundles and the tenant workflow catalog together. */
export function normalizeWorkflowCatalog(
  bundles: unknown,
  catalog?: unknown,
  options: NormalizeWorkflowCatalogOptions = {},
): WorkflowDefinition[] {
  const merged = [
    ...workflowsFrom(bundles, "private", options),
    ...workflowsFrom(catalog, "tenant_catalog", options),
  ];
  const unique = new Map<string, WorkflowDefinition>();
  for (const workflow of merged) unique.set(workflow.id, workflow);
  return [...unique.values()].sort(
    (left, right) =>
      Number(Boolean(right.isDefault)) - Number(Boolean(left.isDefault)) ||
      left.name.localeCompare(right.name) ||
      left.id.localeCompare(right.id),
  );
}

function restoredWorkflowPolicy(value: unknown): JsonObject | undefined {
  const response = record(value);
  if (!response) return undefined;
  const direct = record(
    response.workflow_selection ?? response.workflowSelection,
  );
  if (direct) return direct;
  const runtime = record(response._runtime);
  const runtimePolicy = record(
    runtime?.workflow_policy ?? runtime?.workflowPolicy,
  );
  if (runtimePolicy) return runtimePolicy;
  const input = record(response.input_data ?? response.inputData);
  const inputRuntime = record(input?._runtime);
  return record(inputRuntime?.workflow_policy ?? inputRuntime?.workflowPolicy);
}

/**
 * Resolve a restored run without guessing. Catalog runs must carry the
 * gateway-owned public workflow selection; an internal host workflow id is
 * never allowed to select a shared workflow. Private runs may fall back to an
 * exact workflow id match. Ambiguous identities resolve to undefined.
 */
export function resolveRestoredWorkflow(
  workflows: readonly WorkflowDefinition[],
  options: RestoredWorkflowOptions,
): WorkflowDefinition | undefined {
  const policy = restoredWorkflowPolicy(options.inputData);
  if (policy) {
    const scope = text(policy.registry_scope ?? policy.registryScope);
    const bundleId = text(policy.bundle_id ?? policy.bundleId);
    const bundleVersion = text(policy.bundle_version ?? policy.bundleVersion);
    const flowId = text(policy.flow_id ?? policy.flowId);
    if (
      (scope === "private" || scope === "tenant_catalog") &&
      bundleId &&
      flowId
    ) {
      const matches = workflows.filter(
        (workflow) =>
          (workflow.registryScope ?? "private") === scope &&
          workflow.bundleId === bundleId &&
          workflow.flowId === flowId &&
          (!bundleVersion || workflow.bundleVersion === bundleVersion),
      );
      if (matches.length === 1) return matches[0];
    }
    return undefined;
  }

  const runWorkflowId = text(options.runWorkflowId);
  if (!runWorkflowId) return undefined;
  const privateMatches = workflows.filter(
    (workflow) =>
      (workflow.registryScope ?? "private") === "private" &&
      workflow.workflowId === runWorkflowId,
  );
  return privateMatches.length === 1 ? privateMatches[0] : undefined;
}

export function normalizeWorkspacePolicy(value: unknown): WorkspacePolicy {
  const raw = record(value) ?? {};
  const policy = record(raw.policy) ?? raw;
  const rawMounts = array(policy.mounts ?? policy.workspace_mounts);
  const mounts = rawMounts.flatMap((value, index): WorkspaceMount[] => {
    const item = record(value);
    if (!item) return [];
    const path = text(item.path ?? item.root);
    const id = text(item.id ?? item.name) ?? path ?? `mount-${index + 1}`;
    return [
      {
        id,
        label: text(item.label ?? item.name) ?? id,
        ...(path ? { path } : {}),
        readOnly:
          item.read_only === true ||
          item.readOnly === true ||
          item.writable === false,
        raw: { ...item },
      },
    ];
  });
  const maxAttachmentBytes = finiteInteger(
    policy.max_attachment_bytes ?? policy.maxAttachmentBytes,
    1,
  );
  return {
    raw: { ...policy },
    clientWorkspaceScopeOverrides:
      policy.client_workspace_scope_overrides === true ||
      policy.clientWorkspaceScopeOverrides === true,
    allowedAccessModes: strings(
      policy.allowed_access_modes ?? policy.allowedAccessModes,
    ),
    mounts,
    ...(maxAttachmentBytes !== undefined ? { maxAttachmentBytes } : {}),
  };
}

export function normalizeToolCatalog(value: unknown): ToolSpec[] {
  const items = responseItems(value, ["items", "tools"]);
  const seen = new Set<string>();
  const tools: ToolSpec[] = [];
  for (const rawItem of items) {
    const item = record(rawItem);
    if (!item) continue;
    const name = text(item.name ?? item.tool_name ?? item.id);
    if (!name || seen.has(name)) continue;
    seen.add(name);
    const enabled =
      item.enabled !== false &&
      item.served_disabled !== true &&
      item.servedDisabled !== true;
    const riskRank = finiteInteger(item.risk_rank ?? item.riskRank);
    const inputSchema = schema(
      item.input_schema ?? item.inputSchema ?? item.parameters,
    );
    tools.push({
      name,
      description: text(item.description) ?? "",
      ...(text(item.toolset) ? { toolset: text(item.toolset) } : {}),
      ...(text(item.tier) ? { tier: text(item.tier) } : {}),
      ...(text(
        item.approval_default ??
          item.default_approval ??
          item.approvalDefault ??
          item.defaultApproval ??
          item.approval,
      )
        ? {
            approval: text(
              item.approval_default ??
                item.default_approval ??
                item.approvalDefault ??
                item.defaultApproval ??
                item.approval,
            ),
          }
        : {}),
      ...(riskRank !== undefined ? { riskRank } : {}),
      enabled,
      servedDisabled: !enabled,
      ...(text(item.enable_gate ?? item.enableGate)
        ? { enableGate: text(item.enable_gate ?? item.enableGate) }
        : {}),
      ...(text(item.why_disabled ?? item.whyDisabled)
        ? { whyDisabled: text(item.why_disabled ?? item.whyDisabled) }
        : {}),
      ...(inputSchema ? { inputSchema } : {}),
      raw: { ...item },
    });
  }
  return tools.sort((left, right) => left.name.localeCompare(right.name));
}

function stateForRun(run: JsonObject): SessionState {
  const status = text(run.status ?? run.state)?.toLowerCase();
  const waitReason = text(run.wait_reason ?? run.waitReason);
  if (
    waitReason ||
    run.waiting === true ||
    status === "waiting" ||
    status === "paused"
  )
    return "waiting";
  if (
    status === "running" ||
    status === "queued" ||
    status === "pending" ||
    status === "created"
  )
    return "running";
  if (
    status === "failed" ||
    status === "error" ||
    status === "cancelled" ||
    status === "canceled"
  )
    return "failed";
  if (status === "completed" || status === "complete" || status === "done")
    return "done";
  return "unknown";
}

const STATE_PRIORITY: Record<SessionState, number> = {
  waiting: 4,
  running: 3,
  failed: 2,
  done: 1,
  unknown: 0,
};

function timestamp(run: JsonObject): string | undefined {
  return text(
    run.updated_at ??
      run.updatedAt ??
      run.finished_at ??
      run.finishedAt ??
      run.created_at ??
      run.createdAt,
  );
}

function timestampValue(value: string | undefined): number {
  if (!value) return Number.NEGATIVE_INFINITY;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : Number.NEGATIVE_INFINITY;
}

function promptFrom(value: unknown): string | undefined {
  const direct = text(value);
  if (direct) return direct;
  const item = record(value);
  if (!item) return undefined;
  const prompt = text(item.prompt);
  if (prompt) return prompt;
  const input = record(item.input_data ?? item.inputData);
  const inputPrompt = ["prompt", "request", "task", "message", "query", "input"].map(key => text(input?.[key])).find(Boolean);
  if (inputPrompt) return inputPrompt;
  return text(record(input?.context)?.task);
}

export function normalizeSessionSummaries(
  value: unknown,
  inputByRunId: Readonly<Record<string, unknown>> = {},
): SessionSummary[] {
  const body = record(value);
  const truncated = body?.has_more === true || body?.hasMore === true;
  const runs = responseItems(value, ["items", "runs"]);
  const groups = new Map<string, Array<{ run: JsonObject; index: number }>>();
  runs.forEach((rawRun, index) => {
    const run = record(rawRun);
    if (!run || text(run.parent_run_id ?? run.parentRunId)) return;
    const sessionId = text(run.session_id ?? run.sessionId);
    const runId = text(run.run_id ?? run.runId ?? run.id);
    if (!sessionId || !runId) return;
    const group = groups.get(sessionId) ?? [];
    group.push({ run, index });
    groups.set(sessionId, group);
  });

  const summaries: SessionSummary[] = [];
  for (const [sessionId, group] of groups) {
    const newest = [...group].sort(
      (left, right) =>
        timestampValue(timestamp(right.run)) -
          timestampValue(timestamp(left.run)) || left.index - right.index,
    );
    const oldest = [...group].sort((left, right) => {
      const leftCreated =
        text(left.run.created_at ?? left.run.createdAt) ?? timestamp(left.run);
      const rightCreated =
        text(right.run.created_at ?? right.run.createdAt) ??
        timestamp(right.run);
      return (
        timestampValue(leftCreated) - timestampValue(rightCreated) ||
        left.index - right.index
      );
    });
    const runIds = newest.flatMap(({ run }) => {
      const runId = text(run.run_id ?? run.runId ?? run.id);
      return runId ? [runId] : [];
    });
    if (!runIds.length) continue;
    let state: SessionState = "unknown";
    for (const { run } of group) {
      const candidate = stateForRun(run);
      if (STATE_PRIORITY[candidate] > STATE_PRIORITY[state]) state = candidate;
    }
    let prompt: string | undefined;
    for (const { run } of oldest) {
      const runId = text(run.run_id ?? run.runId ?? run.id);
      prompt =
        (runId ? promptFrom(inputByRunId[runId]) : undefined) ??
        promptFrom(run);
      if (prompt) break;
    }
    summaries.push({
      sessionId,
      state,
      ...(timestamp(newest[0].run)
        ? { updatedAt: timestamp(newest[0].run) }
        : {}),
      firstRunId: text(
        oldest[0].run.run_id ?? oldest[0].run.runId ?? oldest[0].run.id,
      ) as string,
      latestRunId: runIds[0],
      runIds,
      turnCount: group.length,
      ...(prompt ? { prompt } : {}),
      truncated,
    });
  }
  // Match the gateway page and Rust session picker: recency orders rows;
  // state priority is only used while folding runs within one session.
  return summaries.sort(
    (left, right) =>
      timestampValue(right.updatedAt) - timestampValue(left.updatedAt) ||
      left.sessionId.localeCompare(right.sessionId),
  );
}

function cloneObject(value: JsonObject | undefined): JsonObject {
  return value ? { ...value } : {};
}

function cleanStrings(value: string[] | undefined): string[] | undefined {
  if (value === undefined) return undefined;
  return value.flatMap((item) => {
    const normalized = text(item);
    return normalized ? [normalized] : [];
  });
}

function assignCommonRuntimeInputs(
  output: JsonObject,
  options: WorkflowInputOptions,
  mayOverwrite: boolean,
): void {
  const assign = (key: string, value: unknown) => {
    // Public pins belong to the authored workflow; runtime controls do not.
    if (!isChatAgent(options.workflow) && !record(options.inputSchema?.properties)?.[key]) return;
    if (value !== undefined && (mayOverwrite || output[key] === undefined))
      output[key] = value;
  };
  const provider = text(options.model?.provider);
  const model = text(options.model?.model);
  if (Boolean(provider) !== Boolean(model)) throw new Error("Choose both a provider and a model, or use the workflow / Gateway defaults.");
  assign("provider", provider);
  assign("model", model);
  if (options.tools !== undefined)
    assign("tools", cleanStrings(options.tools) ?? []);
  const skills = cleanStrings(options.skills);
  if (skills?.length) output.skills = skills; // Gateway-resolved run envelope.

  const workspaceRoot = text(options.workspace?.root);
  const workspaceMode = text(options.workspace?.accessMode);
  const workspacePaths = cleanStrings(options.workspace?.allowedPaths);
  // These are Gateway-owned envelope fields, not arbitrary authored pins.
  // The server validates them; the same conversation must retain its workspace
  // even when a workflow exposes no workspace input of its own.
  if (workspaceRoot !== undefined) output.workspace_root = workspaceRoot;
  if (workspaceMode !== undefined) output.workspace_access_mode = workspaceMode;
  if (workspacePaths?.length) output.workspace_allowed_paths = workspacePaths;
  assign("system", text(options.system));

  const runtime = cloneObject(record(output._runtime));
  const runtimeAssign = (key: string, value: unknown) => {
    if (value !== undefined && (mayOverwrite || runtime[key] === undefined))
      runtime[key] = value;
  };
  runtimeAssign("provider", provider);
  runtimeAssign("model", model);
  runtimeAssign("thinking", text(options.reasoning));
  runtimeAssign("speculation", options.speculation);
  runtimeAssign("system_prompt_extra", text(options.systemPromptExtra));
  if (options.tools !== undefined) {
    const requested = cleanStrings(options.tools) ?? [];
    const authored = Array.isArray(runtime.allowed_tools) ? runtime.allowed_tools : null;
    runtime.allowed_tools = authored ? requested.filter(name => authored.includes(name)) : requested;
  }
  if (options.review !== undefined && options.reviewCapable !== false) {
    runtimeAssign("review_mode", options.review.enabled);
    const rounds = options.review.enabled
      ? finiteInteger(options.review.maxRounds, 1)
      : undefined;
    runtimeAssign("review_max_rounds", rounds);
  }
  const autoApproveTools = cleanStrings(options.toolPolicy?.autoApproveTools);
  const requireApprovalTools = cleanStrings(
    options.toolPolicy?.requireApprovalTools,
  );
  if (
    (autoApproveTools?.length ?? 0) > 0 ||
    (requireApprovalTools?.length ?? 0) > 0
  ) {
    runtimeAssign("tool_policy", {
      ...(autoApproveTools?.length
        ? { auto_approve_tools: autoApproveTools }
        : {}),
      ...(requireApprovalTools?.length
        ? { require_approval_tools: requireApprovalTools }
        : {}),
    });
  }
  if (options.promptCache !== undefined)
    runtimeAssign("prompt_cache", options.promptCache);
  if (Object.keys(runtime).length) output._runtime = runtime;

  const limits = cloneObject(record(output._limits));
  const maxIterations = finiteInteger(options.limits?.maxIterations, 1);
  const maxTokens = finiteInteger(options.limits?.maxTokens, 1);
  if (
    maxIterations !== undefined &&
    (mayOverwrite || limits.max_iterations === undefined)
  )
    limits.max_iterations = maxIterations;
  if (
    maxTokens !== undefined &&
    (mayOverwrite || limits.max_tokens === undefined)
  )
    limits.max_tokens = maxTokens;
  if (Object.keys(limits).length) output._limits = limits;
  // Older engines still consume the flat pin. Keep it only for an explicit
  // request; the runtime remains responsible for all defaults and clamps.
  assign("max_iterations", maxIterations);

  if (options.gatingMode === "auto") assign("gating_mode", "auto");
}

/**
 * Build the gateway input payload without inventing defaults owned by the
 * gateway. Generic workflows keep their schema values authoritative. The
 * AbstractCode agent interface additionally receives its prompt/context and
 * durable-session conventions.
 */
export function buildWorkflowInput(options: WorkflowInputOptions): JsonObject {
  const output = cloneObject(options.schemaInputs);
  const isAgent = isChatAgent(options.workflow);

  if (options.promptProperty) {
    const property = text(options.promptProperty);
    if (property && options.prompt?.trim())
      output[property] = options.prompt;
  }

  if (isAgent) {
    const prompt = options.prompt ?? "";
    output.prompt = prompt;
    const context = cloneObject(record(output.context));
    context.task = prompt;
    // Prior input_data may contain gateway-seeded context. Never replay it as
    // client authority on a fresh turn; the gateway reconstructs history.
    delete context.messages;
    output.use_context = false;
    if (options.messages?.length) {
      context.messages = options.messages.map((message) => ({
        role: message.role,
        content: message.content,
      }));
      output.use_context = true;
    }
    if (options.attachments?.length)
      context.attachments = options.attachments.map((attachment) => ({
        ...attachment,
      }));
    output.context = context;
    output.use_session_history = options.useSessionHistory ?? true;
  }

  // Gateway runtime controls apply to every workflow. Public pins are only
  // written when declared, except for the explicit agent-interface contract.
  assignCommonRuntimeInputs(output, options, true);
  return output;
}
