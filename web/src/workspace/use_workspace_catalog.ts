import { sessionActivityFromRuns, type SessionActivity } from "../vendor/abstractsessions/session_cards.js";
import { iterationLimitDefaultFrom, type IterationLimitDefault, sessionToolPolicyCapability, normalizeToolCatalog as kitToolCatalog, toolCatalogWarnings, type SessionToolPolicyCapability, type ToolCatalogItem } from "@abstractframework/ui-kit";
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
  withoutNotesSessions,
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
import { archivedConversationCount } from "./archived_conversations";
import { withoutNotesEntries } from "./notes_work";
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
  /**
   * AbstractNote's sessions in the same runs page (`notes_work.ts`): never listed, kept only so a
   * link naming one opens it (read-only) on its latest turn.
   */
  notesSessions: SessionSummary[];
  sessionActivity: Record<string, SessionActivity>;
  capabilities: Record<string, any>;
  capabilitiesLoading: boolean;
  /**
   * The chat tool policy (`capabilities.session_tool_policy`, kit `sessionToolPolicyCapability`):
   * null when the gateway does not serve it; `capabilitiesError` says why the read failed.
   */
  sessionToolPolicy: SessionToolPolicyCapability | null;
  capabilitiesError: string;
  /** The same `GET discovery/tools` answer as the kit reads it (ToolModeEditor, planRunTools). */
  toolCatalog: ToolCatalogItem[];
  /** The catalog's warnings in the gateway's words (plugin load failures, MCP). */
  toolWarnings: string[];
  /** Live-reply support (`capabilities.streaming`) for "Stream replies". */
  streaming: StreamingCapability;
  /** The gateway iteration default (`capabilities.agent_limits.max_iterations`); null = not served. */
  iterationLimit: IterationLimitDefault | null;
  /** The workflows, and every answer a turn's start reads (policy, tools, model default, capabilities), are being read. */
  loading: boolean;
  /** The conversation list is being read (the first read, a refresh, "Load more"). */
  sessionsLoading: boolean;
  sessionsLoaded: boolean;
  errors: string[];
  hasMore: boolean;
  /** The gateway's count of archived conversations (`archived_sessions` of the runs list). */
  archivedSessions: number;
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
  notesSessions: [],
  sessionActivity: {},
  capabilities: {},
  capabilitiesLoading: true,
  sessionToolPolicy: null,
  capabilitiesError: "",
  toolCatalog: [],
  toolWarnings: [],
  streaming: STREAMING_LOADING,
  iterationLimit: null,
  loading: false,
  sessionsLoading: false,
  sessionsLoaded: false,
  errors: [],
  hasMore: false,
  archivedSessions: 0,
};

export function capabilityContracts(value: any): Record<string, any> {
  const contracts = value?.capabilities?.contracts ?? value?.contracts ?? value;
  return contracts && typeof contracts === "object" && !Array.isArray(contracts)
    ? contracts
    : {};
}

/** The catalog's fields from `GET discovery/capabilities`, or from why that read failed. */
function capabilityFields(value: any, failure?: string) {
  return {
    capabilities: capabilityContracts(value),
    capabilitiesLoading: false,
    sessionToolPolicy: sessionToolPolicyCapability(value),
    capabilitiesError: failure ? `Could not read the gateway's capabilities: ${failure}` : "",
    streaming: streamingCapability(value, failure),
    iterationLimit: iterationLimitDefaultFrom(value),
  };
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
  /**
   * The conversation list, shown as soon as it lands; then its `Archived · N` count (one more
   * listing, read once the list is on screen) and the shown conversations' labels.
   */
  const listConversations = useCallback(
    async (listGen: number, signal: AbortSignal) => {
      let runs: any;
      try {
        runs = await fetchRuns(signal);
      } catch (reason: any) {
        if (signal.aborted || listGeneration.current !== listGen) return;
        if (reason?.status === 401) onAuthError();
        setState((s) => ({
          ...s,
          sessionsLoading: false,
          errors: [...s.errors.filter((e) => !e.startsWith("Conversations: ")), `Conversations: ${formatError(reason)}`],
        }));
        return;
      }
      if (listGeneration.current !== listGen) return;
      const page = pageOf(runs);
      setState((s) => ({
        ...s,
        sessionsLoading: false,
        sessions: page.sessions,
        notesSessions: normalizeSessionSummaries(runs, {}, { purpose: "notes" }),
        sessionActivity: sessionActivityFromRuns(withoutNotesSessions(runs)),
        sessionsLoaded: true,
        hasMore: page.hasMore,
        errors: s.errors.filter((e) => !e.startsWith("Conversations: ")),
      }));
      await Promise.all([
        archivedConversationCount(runs, gatewayRequest, signal).then(
          (archivedSessions) => {
            if (listGeneration.current === listGen) setState((s) => ({ ...s, archivedSessions }));
          },
          () => {
            /* aborted: the newer list reads its own count */
          },
        ),
        hydrateLabels(runs, listGen, signal),
      ]);
    },
    [fetchRuns, hydrateLabels, onAuthError],
  );
  /** "Load more": refetches the conversation list only (the rest of the catalog stays on screen). */
  const loadConversations = useCallback(async () => {
    if (!identity) return;
    const listGen = ++listGeneration.current;
    listPending.current?.abort();
    const abort = new AbortController();
    listPending.current = abort;
    setState((s) => ({ ...s, sessionsLoading: true }));
    await listConversations(listGen, abort.signal);
  }, [identity, listConversations]);
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
    setState((s) => ({ ...s, loading: true, sessionsLoading: true, capabilitiesLoading: true, errors: [], executable: { ...s.executable, status: "loading", error: "" } }));
    const request = (path: string) =>
      gatewayRequest(gatewayApiPath(path), { signal: abort.signal });
    // Each part is shown once its own answers are in (CODE-WEB-LOAD-PROFILE 2026-11-10, §4): the
    // conversations and the capabilities (the automations) wait for nothing else. The workflows,
    // the picker and the gateway default wait for every answer a turn's start reads (policy, tools,
    // model default, capabilities): a turn sent before those would go without the person's tool
    // choices.
    const listed = listConversations(listGen, abort.signal);
    const show = (fields: ReturnType<typeof capabilityFields>) => {
      if (generation.current === gen) setState((s) => ({ ...s, ...fields }));
    };
    const capabilities = request("discovery/capabilities").then(
      (value) => {
        show(capabilityFields(value));
        return value;
      },
      (reason) => {
        show(capabilityFields(undefined, formatError(reason)));
        throw reason;
      },
    );
    const result = await Promise.allSettled([
      request("bundles?all_versions=true&include_drafts=false"),
      request("workspace/policy"),
      request("discovery/tools"),
      capabilities,
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
      if (index === 4 && [403, 404, 501].includes(item.reason?.status)) return;
      // The picker says its own error (next to the control).
      if (index === 6) return;
      errors.push(
        `${["Workflows", "Workspace policy", "Tools", "Capabilities", "Shared workflows", "Gateway defaults"][index]}: ${formatError(item.reason)}`,
      );
    });
    let toolCatalog: ToolCatalogItem[] = [];
    if (result[2].status === "fulfilled") {
      try {
        toolCatalog = kitToolCatalog(value(2));
      } catch (reason) {
        errors.push(`Tools: ${formatError(reason)}`);
      }
    }
    let executable: CatalogState["executable"];
    if (result[6].status === "rejected")
      executable = { status: "error", data: null, error: `Workflows for this app: ${formatError((result[6] as PromiseRejectedResult).reason)}` };
    else {
      try {
        executable = { status: "ready", data: withoutNotesEntries(parseExecutableWorkflows(value(6), CODE_AGENT_INTERFACE)), error: "" };
      } catch (reason: any) {
        // A gateway that does not filter per app is said, never papered over.
        executable = { status: "error", data: null, error: String(reason?.message || reason) };
      }
    }
    setState((previous) => ({
      ...previous,
      executable,
      workflows: normalizeWorkflowCatalog(value(0), value(4)),
      policy: value(1) ? normalizeWorkspacePolicy(value(1)) : null,
      tools: normalizeToolCatalog(value(2)),
      toolCatalog,
      toolWarnings: toolCatalogWarnings(value(2)),
      defaultModel: defaultTextRoute(value(5)),
      gatewayDefault: gatewayDefaultFromEnvelope(value(0)),
      loading: false,
      // The conversation list says its own error whenever it lands (listConversations).
      errors: [...errors, ...previous.errors.filter((e) => e.startsWith("Conversations: "))],
    }));
    await listed;
  }, [identity, onAuthError, listConversations]);
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
  /** Archived (round 6): drop it from the shown list and count it now; the list reload confirms. */
  const forgetSession = useCallback((sessionId: string) => {
    setState((s) => ({
      ...s,
      sessions: s.sessions.filter((item) => item.sessionId !== sessionId),
      archivedSessions: s.archivedSessions + 1,
    }));
    void loadConversations();
  }, [loadConversations]);
  return { ...state, refresh, refreshSessions: loadConversations, loadMore, forgetSession };
}

/**
 * Calls `refresh` (the conversation list and the read marks) 500 ms after the person's event
 * stream moves (backlog 1047). The stream's first catch-up after sign-in replays the history up to now,
 * which the catalog's startup list, asked at the same moment, already shows: the cursor that
 * catch-up ends on is the baseline, and only a move past it reads the list again
 * (CODE-WEB-LOAD-PROFILE 2026-11-10, §3.3: reading it again for the catch-up cost one more runs
 * listing, archived listing and session-reads read on every load, and threw the startup list
 * away while it was in flight). An event the gateway records between its read of the startup
 * list and the end of that catch-up shows at the next move.
 */
export function useRefreshOnEvents(identity: string, cursor: number, following: boolean, refresh: () => void): void {
  const listed = useRef<{ identity: string; cursor: number | null }>({ identity, cursor: null });
  useEffect(() => {
    if (listed.current.identity !== identity) listed.current = { identity, cursor: null };
    const seen = listed.current;
    if (seen.cursor === null) {
      if (following) seen.cursor = cursor;
      return;
    }
    if (cursor <= seen.cursor) return;
    const timer = setTimeout(() => {
      seen.cursor = cursor;
      refresh();
    }, 500);
    return () => clearTimeout(timer);
  }, [identity, cursor, following, refresh]);
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
