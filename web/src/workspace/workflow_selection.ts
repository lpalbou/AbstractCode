import type { WorkflowDefinition, WorkflowRegistryScope } from "./catalog";

/** The persisted choice "whatever the gateway's default agent workflow is".
 * Stored as this sentinel, never as a copied id, so a change made on the
 * gateway applies to the next new turn (CONTRACTS §D). */
export const GATEWAY_DEFAULT = "@default";
export const CODE_AGENT_INTERFACE = "abstractcode.agent.v1";

export type DefaultAgentWorkflow = {
  workflowId: string;
  bundleId: string;
  bundleVersion?: string;
  flowId: string;
  registryScope: string;
  name: string;
  source?: string;
};

export type GatewayDefaultState =
  | { status: "loading" }
  | { status: "unavailable"; reason: string }
  | { status: "ok"; workflow: DefaultAgentWorkflow };

function record(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function text(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

export const NO_DEFAULT_REPORTED =
  "gateway does not report a default agent workflow";

/** Read `default_agent_workflows[<interface>]` from the `/bundles` envelope.
 * An envelope without the key is reported as such, loudly: it means the
 * gateway predates the default-agent-workflow contract. */
export function gatewayDefaultFromEnvelope(
  envelope: unknown,
  interfaceId: string = CODE_AGENT_INTERFACE,
): GatewayDefaultState {
  const body = record(envelope);
  if (!body)
    return {
      status: "unavailable",
      reason: "the gateway's workflow list could not be loaded",
    };
  const defaults = record(body.default_agent_workflows);
  if (!defaults) return { status: "unavailable", reason: NO_DEFAULT_REPORTED };
  const row = record(defaults[interfaceId]);
  if (!row) {
    const why = text(
      record(record(body.default_agent_workflows_unavailable)?.[interfaceId])?.reason,
    );
    return {
      status: "unavailable",
      reason: why
        ? `no default workflow for ${interfaceId}: ${why}`
        : `gateway reports no default workflow for ${interfaceId}`,
    };
  }
  const bundleId = text(row.bundle_id);
  const flowId = text(row.flow_id);
  if (!bundleId || !flowId)
    return {
      status: "unavailable",
      reason: `gateway's default workflow for ${interfaceId} has no bundle or flow id`,
    };
  const bundleVersion = text(row.bundle_version);
  const workflowId =
    text(row.workflow_id) ??
    `${bundleId}${bundleVersion ? `@${bundleVersion}` : ""}:${flowId}`;
  return {
    status: "ok",
    workflow: {
      workflowId,
      bundleId,
      ...(bundleVersion ? { bundleVersion } : {}),
      flowId,
      registryScope: text(row.registry_scope) ?? "private",
      name: text(row.name) ?? flowId,
      ...(text(row.source) ? { source: text(row.source) } : {}),
    },
  };
}

export function gatewayDefaultOptionLabel(state: GatewayDefaultState): string {
  if (state.status === "loading") return "Gateway default (loading…)";
  if (state.status === "unavailable") return `Gateway default — ${state.reason}`;
  const { name, bundleVersion } = state.workflow;
  return `Gateway default → ${name}${bundleVersion ? ` @${bundleVersion}` : ""}`;
}

/** The catalog row the gateway default points at (for its input schema and
 * agent contract). When the listing does not carry it, the gateway's own
 * description is used as-is; the server resolves the run either way. */
export function gatewayDefaultDefinition(
  state: GatewayDefaultState,
  workflows: readonly WorkflowDefinition[],
  interfaceId: string = CODE_AGENT_INTERFACE,
): WorkflowDefinition | null {
  if (state.status !== "ok") return null;
  const target = state.workflow;
  const match = workflows.find(
    (workflow) =>
      (workflow.registryScope ?? "private") === target.registryScope &&
      workflow.bundleId === target.bundleId &&
      workflow.flowId === target.flowId &&
      (!target.bundleVersion || workflow.bundleVersion === target.bundleVersion),
  );
  if (match)
    return match.interfaces.includes(interfaceId)
      ? match
      : { ...match, interfaces: [...match.interfaces, interfaceId] };
  return {
    id: `${target.registryScope}:${target.workflowId}`,
    workflowId: target.workflowId,
    bundleId: target.bundleId,
    ...(target.bundleVersion ? { bundleVersion: target.bundleVersion } : {}),
    flowId: target.flowId,
    name: target.name,
    description: "",
    interfaces: [interfaceId],
    registryScope: target.registryScope as WorkflowRegistryScope,
  };
}

/** Agent workflows only, unless the person asked to see every workflow. */
export function visibleWorkflowChoices(
  choices: readonly WorkflowDefinition[],
  showAll: boolean,
  interfaceId: string = CODE_AGENT_INTERFACE,
): WorkflowDefinition[] {
  return showAll
    ? [...choices]
    : choices.filter((workflow) => workflow.interfaces.includes(interfaceId));
}

/** Keep a valid selection; otherwise the saved preference; otherwise the
 * gateway default. A restored conversation may hold a workflow that is not
 * listed for new runs (older version, non-agent flow) and keeps it. */
export function reconcileSelection(options: {
  selection: string;
  preferred: string;
  visible: readonly WorkflowDefinition[];
  workflows: readonly WorkflowDefinition[];
  runId: string;
}): string {
  const valid = (id: string) =>
    id === GATEWAY_DEFAULT ||
    options.visible.some((workflow) => workflow.id === id) ||
    (Boolean(options.runId) &&
      options.workflows.some((workflow) => workflow.id === id));
  if (options.selection && valid(options.selection)) return options.selection;
  if (options.preferred && valid(options.preferred)) return options.preferred;
  return GATEWAY_DEFAULT;
}

/** Which workflow the next turn of a conversation sends. A conversation this
 * browser started with the gateway default keeps sending "@default", so a
 * change made on the gateway applies to its next turn (CONTRACTS A-4); any
 * other restored conversation keeps the exact workflow its run used. */
export function conversationSelection(options: {
  sessionId: string;
  defaultSessions: ReadonlySet<string>;
  restored?: WorkflowDefinition | null;
}): string | undefined {
  if (options.defaultSessions.has(options.sessionId)) return GATEWAY_DEFAULT;
  return options.restored?.id;
}

/** `POST /runs/start` body. The gateway default is sent as the sentinel and
 * resolved by the server (no bundle fields); any other choice is explicit. */
export function startRunBody(options: {
  selection: string;
  workflow: WorkflowDefinition;
  sessionId: string;
  input: Record<string, unknown>;
  interfaceId?: string;
}): Record<string, unknown> {
  if (options.selection === GATEWAY_DEFAULT)
    return {
      flow_id: GATEWAY_DEFAULT,
      interface: options.interfaceId ?? CODE_AGENT_INTERFACE,
      session_id: options.sessionId,
      input_data: options.input,
    };
  return {
    bundle_id: options.workflow.bundleId,
    bundle_version: options.workflow.bundleVersion,
    registry_scope: options.workflow.registryScope,
    flow_id: options.workflow.flowId,
    session_id: options.sessionId,
    input_data: options.input,
  };
}

export type ResolvedWorkflowNote = { text: string; missing: boolean };

/** What the gateway says it actually started (`resolved_workflow`). */
export function resolvedWorkflowNote(resolved: unknown): ResolvedWorkflowNote {
  const row = record(resolved);
  if (!row)
    return {
      text: "the gateway did not report which workflow it started",
      missing: true,
    };
  const name =
    text(row.name) ?? text(row.workflow_id) ?? text(row.flow_id) ?? "unnamed workflow";
  const version = text(row.bundle_version);
  return {
    text: `running ${name}${version ? ` @${version}` : ""}${row.source === "gateway_default" ? " (gateway default)" : ""}`,
    missing: false,
  };
}
