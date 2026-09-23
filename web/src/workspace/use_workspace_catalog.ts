import { useCallback, useEffect, useRef, useState } from "react";
import {
  normalizeWorkflowCatalog,
  publishedWorkflowChoices,
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

type CatalogState = {
  workflows: WorkflowDefinition[];
  choices: WorkflowDefinition[];
  policy: WorkspacePolicy | null;
  tools: ToolSpec[];
  sessions: SessionSummary[];
  capabilities: Record<string, any>;
  loading: boolean;
  errors: string[];
  hasMore: boolean;
  defaultModel?: { provider: string; model: string };
};
const empty: CatalogState = {
  workflows: [],
  choices: [],
  policy: null,
  tools: [],
  sessions: [],
  capabilities: {},
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
  const [limit, setLimit] = useState(100);
  const generation = useRef(0);
  const pending = useRef<AbortController>();
  const inputCache = useRef<{
    identity: string;
    values: Record<string, unknown>;
  }>({ identity: "", values: {} });
  const refresh = useCallback(async () => {
    const gen = ++generation.current;
    pending.current?.abort();
    const abort = new AbortController();
    pending.current = abort;
    if (inputCache.current.identity !== identity)
      inputCache.current = { identity, values: {} };
    if (!identity) {
      setState(empty);
      return;
    }
    setState((s) => ({ ...s, loading: true, errors: [] }));
    const paths = [
      "bundles?all_versions=true&include_drafts=false",
      "workspace/policy",
      "discovery/tools",
      `runs?root_only=true&include_ledger_len=false&limit=${limit}`,
      "discovery/capabilities",
      "workflow-catalog?scope=tenant",
      "config/capability-defaults",
    ];
    const result = await Promise.allSettled(
      paths.map((path) =>
        gatewayRequest(`/api/gateway/${path}`, { signal: abort.signal }),
      ),
    );
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
      errors.push(
        `${["Workflows", "Workspace policy", "Tools", "Conversations", "Capabilities", "Shared workflows", "Gateway defaults"][index]}: ${formatError(item.reason)}`,
      );
    });
    setState({
      workflows: normalizeWorkflowCatalog(value(0), value(5)),
      choices: publishedWorkflowChoices(normalizeWorkflowCatalog(value(0), value(5))),
      policy: value(1) ? normalizeWorkspacePolicy(value(1)) : null,
      tools: normalizeToolCatalog(value(2)),
      sessions: normalizeSessionSummaries(value(3), inputCache.current.values),
      capabilities: capabilityContracts(value(4)),
      defaultModel: defaultTextRoute(value(6)),
      loading: false,
      errors,
      hasMore: Boolean(value(3)?.has_more),
    });
    // Run summaries intentionally omit user input. Hydrate labels through the
    // authorized input endpoint, with bounded concurrency and identity-local cache.
    const missing = normalizeSessionSummaries(
      value(3),
      inputCache.current.values,
    ).filter(
      (item) => !item.prompt && !(item.firstRunId in inputCache.current.values),
    );
    for (let index = 0; index < missing.length; index += 4) {
      if (generation.current !== gen || abort.signal.aborted) return;
      await Promise.all(
        missing.slice(index, index + 4).map(async (item) => {
          try {
            const input = await gatewayRequest(
              `/api/gateway/runs/${encodeURIComponent(item.firstRunId)}/input_data`,
              { signal: abort.signal },
            );
            if (generation.current === gen && !abort.signal.aborted)
              inputCache.current.values[item.firstRunId] = input;
          } catch (reason: any) {
            if (!abort.signal.aborted && reason?.status === 401) onAuthError();
            // A missing/forbidden old run must not hide the conversation itself.
          }
        }),
      );
      if (generation.current !== gen || abort.signal.aborted) return;
      setState((previous) => ({
        ...previous,
        sessions: normalizeSessionSummaries(
          value(3),
          inputCache.current.values,
        ),
      }));
    }
  }, [identity, limit, onAuthError]);
  useEffect(() => {
    setLimit(100);
  }, [identity]);
  useEffect(() => {
    setState(empty);
    void refresh();
    return () => {
      generation.current += 1;
      pending.current?.abort();
    };
  }, [refresh]);
  return { ...state, refresh, loadMore: () => setLimit((n) => n + 100) };
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
      ? `/api/gateway/workflow-catalog/${bundle}/versions/${encodeURIComponent(workflow.bundleVersion)}/flows/${flow}/input_schema?scope=tenant`
      : `/api/gateway/bundles/${bundle}/flows/${flow}/input_schema${workflow.bundleVersion ? `?bundle_version=${encodeURIComponent(workflow.bundleVersion)}` : ""}`;
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
      ? `/api/gateway/workflow-catalog/${bundle}/versions/${encodeURIComponent(version)}/flows/${flow}?scope=tenant`
      : `/api/gateway/bundles/${bundle}/flows/${flow}?bundle_version=${encodeURIComponent(version)}`;
  const source = await gatewayRequest(sourcePath);
  assertSelection(source);
  return reconcileVisualFlowSchema(schema, source.flow);
}
