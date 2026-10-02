import {
  executableWorkflowsPath,
  gatewayApiPath,
  parseExecutableWorkflows,
  type ExecutableWorkflows,
} from "@abstractframework/ui-kit";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  normalizeWorkflowCatalog,
  normalizeWorkspacePolicy,
  normalizeToolCatalog,
  normalizeSessionSummaries,
  type WorkflowDefinition,
  type WorkspacePolicy,
  type ToolSpec,
  type SessionSummary,
} from "./catalog";
import { gatewayRequest, formatError } from "./transport";
import {
  normalizeInputSchema,
  reconcileVisualFlowSchema,
} from "./input_schema";
import { defaultTextRoute } from "./model_discovery";
import {
  STREAMING_LOADING,
  streamingCapability,
  type StreamingCapability,
} from "./stream_replies";
import {
  CODE_AGENT_INTERFACE,
  gatewayDefaultFromEnvelope,
  type GatewayDefaultState,
} from "./workflow_selection";
import {
  CONVERSATIONS_PAGE,
  conversationPage,
  conversationRunsPath,
  fetchConversationRuns,
} from "./conversation_paging";

type CatalogState = {
  workflows: WorkflowDefinition[];
  policy: WorkspacePolicy | null;
  tools: ToolSpec[];
  sessions: SessionSummary[];
  capabilities: Record<string, any>;
  capabilitiesLoading: boolean;
  /** Live-reply support (`capabilities.streaming`) for "Stream replies". */
  streaming: StreamingCapability;
  loading: boolean;
  errors: string[];
  hasMore: boolean;
  defaultModel?: { provider: string; model: string };
  /** `default_agent_workflows["abstractcode.agent.v1"]` of the `/bundles` envelope. */
  gatewayDefault: GatewayDefaultState;
  /** The header picker's list: `GET /bundles?executable_for=abstractcode.agent.v1`
   * (the workflows this app can run that the signed-in person may use). */
  executable: { status: "idle" | "loading" | "ready" | "error"; data: ExecutableWorkflows | null; error: string };
};
const empty: CatalogState = {
  gatewayDefault: { status: "loading" },
  executable: { status: "idle", data: null, error: "" },
  workflows: [],
  policy: null,
  tools: [],
  sessions: [],
  capabilities: {},
  capabilitiesLoading: true,
  streaming: STREAMING_LOADING,
  loading: false,
  errors: [],
  hasMore: false,
};

export function capabilityContracts(value: any): Record<string, any> {
  const contracts = value?.capabilities?.contracts ?? value?.contracts ?? value;
  return contracts && typeof contracts === "object" && !Array.isArray(contracts)
    ? contracts
    : {};
}

export function useWorkspaceCatalog(identity: string, onAuthError: () => void) {
  const [state, setState] = useState<CatalogState>(empty);
  // Conversations shown (not runs): CONVERSATIONS_PAGE, "Load more" adds CONVERSATIONS_PAGE.
  const visible = useRef(CONVERSATIONS_PAGE);
  const generation = useRef(0);
  const pending = useRef<AbortController>();
  // The conversation list alone ("Load more") has its own generation: it never discards the
  // workflows/tools half of a refresh, and a newer list always wins over an older one.
  const listGeneration = useRef(0);
  const listPending = useRef<AbortController>();
  const inputCache = useRef<{
    identity: string;
    values: Record<string, unknown>;
  }>({ identity: "", values: {} });
  const fetchRuns = useCallback(
    (signal: AbortSignal) =>
      fetchConversationRuns(
        (runLimit) =>
          gatewayRequest(gatewayApiPath(conversationRunsPath(runLimit)), {
            signal,
          }),
        visible.current,
      ),
    [],
  );
  const pageOf = (runs: any) =>
    conversationPage(
      normalizeSessionSummaries(runs, inputCache.current.values),
      visible.current,
    );
  // Run summaries intentionally omit user input. Hydrate the SHOWN conversations' labels through
  // the authorized input endpoint, with bounded concurrency and identity-local cache.
  const hydrateLabels = useCallback(
    async (runs: any, listGen: number, signal: AbortSignal) => {
      const stale = () => listGeneration.current !== listGen || signal.aborted;
      const missing = pageOf(runs).sessions.filter(
        (item) => !item.prompt && !(item.firstRunId in inputCache.current.values),
      );
      for (let index = 0; index < missing.length; index += 4) {
        if (stale()) return;
        await Promise.all(
          missing.slice(index, index + 4).map(async (item) => {
            try {
              const input = await gatewayRequest(
                gatewayApiPath(`runs/${encodeURIComponent(item.firstRunId)}/input_data`),
                { signal },
              );
              if (!stale()) inputCache.current.values[item.firstRunId] = input;
            } catch (reason: any) {
              if (!signal.aborted && reason?.status === 401) onAuthError();
              // A missing/forbidden old run must not hide the conversation itself.
            }
          }),
        );
        if (stale()) return;
        setState((previous) => ({ ...previous, sessions: pageOf(runs).sessions }));
      }
    },
    [onAuthError],
  );
  /** "Load more": refetches the conversation list only (the rest of the catalog stays on screen). */
  const loadConversations = useCallback(async () => {
    if (!identity) return;
    const listGen = ++listGeneration.current;
    listPending.current?.abort();
    const abort = new AbortController();
    listPending.current = abort;
    setState((s) => ({ ...s, loading: true }));
    try {
      const runs = await fetchRuns(abort.signal);
      if (listGeneration.current !== listGen) return;
      const page = pageOf(runs);
      setState((s) => ({
        ...s,
        loading: false,
        sessions: page.sessions,
        hasMore: page.hasMore,
        errors: s.errors.filter((e) => !e.startsWith("Conversations: ")),
      }));
      await hydrateLabels(runs, listGen, abort.signal);
    } catch (reason: any) {
      if (abort.signal.aborted || listGeneration.current !== listGen) return;
      if (reason?.status === 401) onAuthError();
      setState((s) => ({
        ...s,
        loading: false,
        errors: [...s.errors.filter((e) => !e.startsWith("Conversations: ")), `Conversations: ${formatError(reason)}`],
      }));
    }
  }, [identity, fetchRuns, hydrateLabels, onAuthError]);
  const refresh = useCallback(async () => {
    const gen = ++generation.current;
    const listGen = ++listGeneration.current;
    listPending.current?.abort();
    pending.current?.abort();
    const abort = new AbortController();
    pending.current = abort;
    if (inputCache.current.identity !== identity)
      inputCache.current = { identity, values: {} };
    if (!identity) {
      setState(empty);
      return;
    }
    setState((s) => ({ ...s, loading: true, capabilitiesLoading: true, errors: [], executable: { ...s.executable, status: "loading", error: "" } }));
    const request = (path: string) =>
      gatewayRequest(gatewayApiPath(path), { signal: abort.signal });
    const result = await Promise.allSettled([
      request("bundles?all_versions=true&include_drafts=false"),
      request("workspace/policy"),
      request("discovery/tools"),
      fetchRuns(abort.signal),
      request("discovery/capabilities"),
      request("workflow-catalog?scope=tenant"),
      request("config/capability-defaults"),
      request(executableWorkflowsPath(CODE_AGENT_INTERFACE)),
    ]);
    if (generation.current !== gen) return;
    const value = (i: number): any =>
      result[i].status === "fulfilled"
        ? (result[i] as PromiseFulfilledResult<any>).value
        : undefined;
    const errors: string[] = [];
    result.forEach((item, index) => {
      if (item.status !== "rejected") return;
      if (item.reason?.status === 401) onAuthError();
      if (index === 5 && [403, 404, 501].includes(item.reason?.status)) return;
      // The picker says its own error (next to the control).
      if (index === 7) return;
      errors.push(
        `${["Workflows", "Workspace policy", "Tools", "Conversations", "Capabilities", "Shared workflows", "Gateway defaults"][index]}: ${formatError(item.reason)}`,
      );
    });
    // A "Load more" started while this refresh was in flight owns the list now.
    const listCurrent = listGeneration.current === listGen;
    const page = pageOf(value(3));
    let executable: CatalogState["executable"];
    if (result[7].status === "rejected")
      executable = { status: "error", data: null, error: `Workflows for this app: ${formatError((result[7] as PromiseRejectedResult).reason)}` };
    else {
      try {
        executable = { status: "ready", data: parseExecutableWorkflows(value(7), CODE_AGENT_INTERFACE), error: "" };
      } catch (reason: any) {
        // A gateway that does not filter per app is said, never papered over.
        executable = { status: "error", data: null, error: String(reason?.message || reason) };
      }
    }
    setState((previous) => ({
      executable,
      workflows: normalizeWorkflowCatalog(value(0), value(5)),
      policy: value(1) ? normalizeWorkspacePolicy(value(1)) : null,
      tools: normalizeToolCatalog(value(2)),
      sessions: listCurrent ? page.sessions : previous.sessions,
      capabilities: capabilityContracts(value(4)),
      capabilitiesLoading: false,
      streaming: streamingCapability(
        value(4),
        result[4].status === "rejected"
          ? formatError((result[4] as PromiseRejectedResult).reason)
          : undefined,
      ),
      defaultModel: defaultTextRoute(value(6)),
      gatewayDefault: gatewayDefaultFromEnvelope(value(0)),
      loading: false,
      errors,
      hasMore: listCurrent ? page.hasMore : previous.hasMore,
    }));
    if (listCurrent && value(3)) await hydrateLabels(value(3), listGen, abort.signal);
  }, [identity, onAuthError, fetchRuns, hydrateLabels]);
  useEffect(() => {
    // A new identity starts again at one page of conversations.
    visible.current = CONVERSATIONS_PAGE;
    setState(empty);
    void refresh();
    return () => {
      generation.current += 1;
      listGeneration.current += 1;
      pending.current?.abort();
      listPending.current?.abort();
    };
  }, [refresh]);
  const loadMore = useCallback(() => {
    visible.current += CONVERSATIONS_PAGE;
    void loadConversations();
  }, [loadConversations]);
  return { ...state, refresh, loadMore };
}

export async function fetchWorkflowSchema(
  workflow: WorkflowDefinition,
): Promise<Record<string, any> | undefined> {
  if (workflow.inputSchema) return normalizeInputSchema(workflow.inputSchema);
  if (!workflow.bundleId) return undefined;
  const bundle = encodeURIComponent(workflow.bundleId);
  const flow = encodeURIComponent(workflow.flowId);
  const path =
    workflow.registryScope === "tenant_catalog" && workflow.bundleVersion
      ? gatewayApiPath(`workflow-catalog/${bundle}/versions/${encodeURIComponent(workflow.bundleVersion)}/flows/${flow}/input_schema?scope=tenant`)
      : gatewayApiPath(`bundles/${bundle}/flows/${flow}/input_schema${workflow.bundleVersion ? `?bundle_version=${encodeURIComponent(workflow.bundleVersion)}` : ""}`);
  const data = await gatewayRequest(path);
  const schema = normalizeInputSchema(data);
  if (
    !schema ||
    data?.native_loop_factory ||
    data?.version !== 1 ||
    !Array.isArray(data?.inputs) ||
    !data?.input_data_schema ||
    !Array.isArray(schema.required) ||
    !schema.required.length
  )
    return schema;

  // Do not guess which fields are optional by their names, nor require a
  // Gateway upgrade for normal chat. Read author intent from the same selected
  // bundle/version/registry via the existing authenticated authoring endpoint.
  const version = workflow.bundleVersion || data.bundle_version;
  if (!version)
    throw new Error(
      "The Gateway did not identify the workflow version. Refresh workflows and retry.",
    );
  const selected = {
    bundle_id: workflow.bundleId,
    bundle_version: version,
    flow_id: workflow.flowId,
  };
  const assertSelection = (response: any) => {
    for (const [key, expected] of Object.entries(selected))
      if (response?.[key] !== expected)
        throw new Error(
          "The Gateway returned inputs for a different workflow version. Refresh workflows and retry.",
        );
    if (
      workflow.registryScope === "tenant_catalog" &&
      response?.registry_scope !== "tenant_catalog"
    )
      throw new Error(
        "The Gateway returned inputs from a different workflow registry. Refresh workflows and retry.",
      );
  };
  assertSelection(data);
  const sourcePath =
    workflow.registryScope === "tenant_catalog"
      ? gatewayApiPath(`workflow-catalog/${bundle}/versions/${encodeURIComponent(version)}/flows/${flow}?scope=tenant`)
      : gatewayApiPath(`bundles/${bundle}/flows/${flow}?bundle_version=${encodeURIComponent(version)}`);
  const source = await gatewayRequest(sourcePath);
  assertSelection(source);
  return reconcileVisualFlowSchema(schema, source.flow);
}
