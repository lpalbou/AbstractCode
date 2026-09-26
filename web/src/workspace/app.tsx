import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  AfAppearanceDialog,
  AfDrawer,
  AfTopBarActions,
  GatewayConnectModal,
  Icon,
  useAppearanceSettings,
  useGatewayConnection,
  VoiceSettings,
  appIdentity,
  type VoicePreferences,
} from "@abstractframework/ui-kit";
import {
  WorkflowChat,
  chatToMarkdown,
  downloadTextFile,
  useWorkflowSession,
  workflowPendingInteraction,
  type ChatMessage,
} from "@abstractframework/panel-chat";
import {
  buildWorkflowInput,
  isChatAgent,
  workflowChoiceLabel,
  resolveRestoredWorkflow,
  type WorkflowDefinition,
  type SessionSummary,
} from "./catalog";
import {
  fetchWorkflowSchema,
  useWorkspaceCatalog,
} from "./use_workspace_catalog";
import { gateway, gatewayRequest, formatError, newId } from "./transport";
import { createWorkflowTransport } from "./session_transport";
import { presentInteraction } from "./interaction";
import {
  SettingsPanel,
  DEFAULT_PREFERENCES,
  type RunPreferences,
  type SettingsTab,
} from "./settings_panel";
import { WorkspaceInspector, type InspectorTab } from "./workspace_panels";
import { aboutExtraRows, type FetchOutcome } from "./about_rows";
import {
  readPreferences,
  writePreferences,
} from "./preferences";
import {
  addStreamNote,
  effectiveStreamReplies,
  malformedDeltaNote,
  mergeStreamNotes,
  streamingUnsupportedNote,
  type StreamNote,
} from "./stream_replies";
import {
  GATEWAY_DEFAULT,
  conversationSelection,
  defaultInterfaceMismatch,
  runSelectionSource,
  selectionSourceNote,
  gatewayDefaultDefinition,
  gatewayDefaultOptionLabel,
  reconcileSelection,
  resolvedWorkflowNote,
  startRunBody,
  visibleWorkflowChoices,
  type ResolvedWorkflowNote,
} from "./workflow_selection";
import {
  schemaDefaults,
  validateWorkflowInputs,
  withWorkflowAttachments,
  WorkflowInputs,
  AgentWorkflowInputs,
} from "./workflow_inputs";
import type { AttachmentRef } from "../lib/types";
import {
  UPLOAD_CONCURRENCY,
  queueUploads,
  runBounded,
  unattachedSendBlock,
  uploadAnnouncement,
  uploadFailure,
  type PendingUpload,
} from "./attachment_uploads";
import { ComposerAttachments } from "./composer_attachments";
import { RunStatusBar } from "./run_status_bar";
import { VoiceTools, useWorkspaceVoice } from "./voice_tools";
import { workflowPromptProperty, restoreWorkflowFields } from "./input_schema";
import { resolveToolPermissions, intersectToolPermissions } from "./tool_permissions";
import {
  advanceQueueIntents,
  queueIntentMatches,
  queueTerminalDisposition,
  staleSendAbort,
  workspacePrincipalIdentity,
  type QueueIntent,
} from "./app_state";

const APP_IDENTITY = appIdentity("abstractcode", __APP_VERSION__);

const terminal = (status: string) =>
  ["completed", "failed", "cancelled", "canceled"].includes(status);
function route(): { sessionId: string; runId: string } {
  const params = new URLSearchParams(window.location.hash.replace(/^#/, ""));
  return {
    sessionId: params.get("session") || newId(),
    runId: params.get("run") || "",
  };
}
function writeRoute(sessionId: string, runId = "") {
  const params = new URLSearchParams({ session: sessionId });
  if (runId) params.set("run", runId);
  window.history.replaceState(null, "", `#${params}`);
}

export function CodeWorkspace() {
  const connection = useGatewayConnection({
    appName: "AbstractCode",
    variant: "blocking",
  });
  const identity = connection.connected
    ? workspacePrincipalIdentity(connection.status)
    : "";
  const onAuthError = useCallback(() => {
    void connection.refresh();
  }, [connection.refresh]);
  const catalog = useWorkspaceCatalog(identity, onAuthError);
  const [appearance, setAppearance] = useAppearanceSettings("abstractcode", {
    defaults: { theme: "observer-night" },
  });
  const [appearanceOpen, setAppearanceOpen] = useState(false);
  // `GET /api/gateway/about`, fetched each time the About dialog opens.
  const [gatewayAbout, setGatewayAbout] = useState<FetchOutcome>();
  const refreshGatewayAbout = useCallback(() => {
    setGatewayAbout(undefined);
    void gatewayRequest("/api/gateway/about")
      .then((value) => setGatewayAbout({ ok: true, value }))
      .catch((reason) =>
        setGatewayAbout({
          ok: false,
          status: reason?.status,
          message: formatError(reason),
        }),
      );
  }, []);
  const [voiceOpen, setVoiceOpen] = useState(false);
  const [voicePreferences, setVoicePreferences] = useState<VoicePreferences>(
    {},
  );
  useEffect(() => {
    try {
      setVoicePreferences(
        JSON.parse(
          localStorage.getItem(`abstractcode.voice:${identity}`) || "{}",
        ),
      );
    } catch {
      setVoicePreferences({});
    }
    setVoiceOpen(false);
  }, [identity]);
  const changeVoicePreferences = (next: VoicePreferences) => {
    setVoicePreferences(next);
    if (identity)
      try {
        localStorage.setItem(
          `abstractcode.voice:${identity}`,
          JSON.stringify(next),
        );
      } catch {
        /* In-memory preferences still work. */
      }
  };
  const [preferences, setPreferences] =
    useState<RunPreferences>(DEFAULT_PREFERENCES);
  const preferencesRef = useRef(preferences);
  preferencesRef.current = preferences;
  // "Stream replies" as sent with the next run (nothing when the gateway does
  // not advertise live replies; the setting then says why).
  const streamReplies = effectiveStreamReplies(
    preferences.streamReplies,
    catalog.streaming,
  );
  // "@default" (the gateway default agent workflow) or a catalog workflow id.
  const [selection, setSelection] = useState(DEFAULT_PREFERENCES.workflow);
  const visibleChoices = useMemo(
    () => visibleWorkflowChoices(catalog.choices, preferences.showAllWorkflows),
    [catalog.choices, preferences.showAllWorkflows],
  );
  const defaultWorkflow = useMemo(
    () => gatewayDefaultDefinition(catalog.gatewayDefault, catalog.workflows),
    [catalog.gatewayDefault, catalog.workflows],
  );
  const workflow =
    selection === GATEWAY_DEFAULT
      ? defaultWorkflow
      : catalog.workflows.find((item) => item.id === selection) || null;
  // What the gateway said it started, for the run this page started.
  const [resolvedNote, setResolvedNote] = useState<
    (ResolvedWorkflowNote & { runId: string }) | null
  >(null);
  const isAgent = isChatAgent(workflow);
  const [session, setSession] = useState(route);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [sending, setSending] = useState(false);
  const sendLock = useRef(false);
  const [attachments, setAttachments] = useState<AttachmentRef[]>([]);
  // Files dropped/pasted/picked that are not an AttachmentRef yet (queued,
  // uploading, refused or failed). Completed uploads move to `attachments`.
  const [uploads, setUploads] = useState<PendingUpload[]>([]);
  const removedUploads = useRef(new Set<string>());
  // Display-only sizes of files uploaded from this browser (the gateway's ref
  // carries no size); never added to the AttachmentRef that rides the run.
  const uploadedSizes = useRef(new Map<string, number>());
  const [uploadNotice, setUploadNotice] = useState("");
  const fileInput = useRef<HTMLInputElement>(null);
  const [schema, setSchema] = useState<Record<string, any>>();
  const [schemaLoading, setSchemaLoading] = useState(false);
  const [schemaError, setSchemaError] = useState("");
  const [schemaRevision, setSchemaRevision] = useState(0);
  const [inputs, setInputs] = useState<Record<string, unknown>>({});
  const [inputEditorError, setInputEditorError] = useState("");
  const [restoredInputs, setRestoredInputs] = useState<Record<
    string,
    any
  > | null>(null);
  const [restoreError, setRestoreError] = useState("");
  const [inputsOpen, setInputsOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("model");
  const [inspectorOpen, setInspectorOpen] = useState(
    () => window.innerWidth > 1120,
  );
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("files");
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [search, setSearch] = useState("");
  const searchInput = useRef<HTMLInputElement>(null);
  const [composeMode, setComposeMode] = useState<"steer" | "queue">("steer");
  const [queue, setQueue] = useState<QueueIntent[]>([]);
  const [queueRunning, setQueueRunning] = useState(false);
  const [optimistic, setOptimistic] = useState<ChatMessage[]>([]);
  const toolPermissions = useMemo(() => resolveToolPermissions(catalog.tools, preferences.tools, preferences.permissions), [catalog.tools, preferences.tools, preferences.permissions]);
  // The input this page itself posted for the run it just started. Until the
  // Gateway's filtered copy (`/input_data`) arrives, it scopes the approver,
  // so a first batch that lands inside that fetch is not shown as a question.
  // The Gateway copy replaces it as soon as it loads (it can only narrow).
  const startedRunInput = useRef<{ runId: string; input: Record<string, unknown> } | null>(null);
  const currentToolPermissions = useMemo(() => {
    const ownStart = startedRunInput.current?.runId === session.runId ? startedRunInput.current.input : null;
    if (session.runId && !restoredInputs && !ownStart) return { enabledTools: [], autoApproveTools: [] };
    const runInput = restoredInputs ? restoredInputs.input_data : ownStart;
    const policy = intersectToolPermissions(toolPermissions, (runInput as any)?._runtime?.allowed_tools);
    // Untouched defaults remain Gateway-owned; explicit per-tool/tier choices
    // use the same policy in the browser and the next run's Runtime namespace.
    return { ...policy, autoApproveTools: preferences.toolsCustomized || preferences.permissions !== "default" ? policy.autoApproveTools : [] };
  }, [toolPermissions, restoredInputs, session.runId, preferences.toolsCustomized, preferences.permissions]);
  // Notes about the live-reply lane (a malformed frame from the gateway),
  // scoped to the conversation they were raised in.
  const streamScope = `${identity}:${session.sessionId}:${session.runId}`;
  const streamScopeRef = useRef(streamScope);
  streamScopeRef.current = streamScope;
  const lastMessageIdRef = useRef<string | null>(null);
  const [streamNotes, setStreamNotes] = useState<{ scope: string; notes: StreamNote[] }>({ scope: "", notes: [] });
  const workflowTransport = useMemo(
    () =>
      createWorkflowTransport({
        onDeltaError: (report) => {
          const scope = streamScopeRef.current;
          const note = malformedDeltaNote(report, lastMessageIdRef.current);
          setStreamNotes((previous) => ({
            scope,
            notes: addStreamNote(previous.scope === scope ? previous.notes : [], note),
          }));
        },
      }),
    [],
  );
  const { controller, snapshot } = useWorkflowSession({
    transport: workflowTransport,
    runId: session.runId,
    enabled: connection.connected,
    onAuthError,
    clientId: "abstractcode-web",
    authScopeKey: identity,
    toolApprovalPolicy: currentToolPermissions,
  });
  const active =
    Boolean(session.runId) &&
    !terminal(snapshot.status) &&
    snapshot.status !== "idle";
  const paused = snapshot.run?.paused === true || snapshot.status === "paused";
  const locked = active || sending;
  const baseMessages =
    snapshot.loading && optimistic.length ? optimistic : snapshot.messages;
  const lastBase = baseMessages[baseMessages.length - 1];
  lastMessageIdRef.current = lastBase?.id ? String(lastBase.id) : null;
  const messages = mergeStreamNotes(baseMessages, [
    streamingUnsupportedNote(preferences.streamReplies, catalog.streaming, baseMessages),
    ...(streamNotes.scope === streamScope ? streamNotes.notes : []),
  ]);
  const voiceCapability = catalog.capabilities?.assistant?.voice || {};
  const voice = useWorkspaceVoice({
    scope: `${identity}:${session.sessionId}:${session.runId}`,
    runId: identity ? session.runId : "",
    sessionId: session.sessionId,
    capability: voiceCapability,
    preferences: voicePreferences,
    onTranscript: (text) =>
      setDraft((previous) =>
        previous ? `${previous.trimEnd()}\n${text}` : text,
      ),
    onError: setError,
  });
  // A tool batch this client already granted (the standing permission, or an
  // accepted Allow) is running work, not a question: no card, no "Approval
  // needed". The server still asks, so the raw wait stays in the snapshot.
  const pendingInteraction = workflowPendingInteraction(snapshot);
  const interaction = useMemo(
    () =>
      presentInteraction(
        pendingInteraction,
        controller,
        snapshot.records,
        snapshot.run,
        async () => { setPreferences(previous => ({ ...previous, permissions: "all" })); },
      ),
    [pendingInteraction, snapshot.records, snapshot.run, controller],
  );
  const interactionBlocksSteer =
    Boolean(interaction) && composeMode === "steer";
  const promptProperty = isAgent
    ? "prompt"
    : workflowPromptProperty(schema);
  const chatReady = isAgent || Boolean(promptProperty && !schema?.required?.some((name: string) => name !== promptProperty));
  const effectiveWorkspace = String(
    restoredInputs?.workspace?.workspace_root ||
      (snapshot.run?.vars as any)?.workspace_root ||
      "",
  );
  const restoredWorkflow = useMemo(
    () =>
      resolveRestoredWorkflow(catalog.workflows, {
        inputData: restoredInputs,
        runWorkflowId: snapshot.run?.workflow_id,
      }),
    [catalog.workflows, restoredInputs, snapshot.run?.workflow_id],
  );
  const restoreSelectionError =
    session.runId &&
    restoredInputs &&
    !catalog.loading &&
    snapshot.run &&
    !restoredWorkflow &&
    runSelectionSource(restoredInputs) !== "gateway_default"
      ? "This conversation's exact workflow version is unavailable. Restore it in the gateway, or start a new conversation with an available workflow."
      : "";
  const currentSession = catalog.sessions.find(
    (item) => item.sessionId === session.sessionId,
  );
  const title =
    currentSession?.prompt ||
    messages.find((m) => m.role === "user")?.content ||
    "New conversation";
  const principalRef = useRef("");
  const identityRef = useRef(identity);
  const authEpoch = useRef(0);
  if (identityRef.current !== identity) {
    authEpoch.current += 1;
    identityRef.current = identity;
  }
  const skipPreferenceWrite = useRef(false);
  const sessionRef = useRef(session);
  sessionRef.current = session;

  useEffect(() => {
    sendLock.current = false;
    setSending(false);
    setUploads([]);
    setError("");
    setNotice("");
    if (!identity) {
      setAttachments([]);
      setDraft("");
      setQueue([]);
      setQueueRunning(false);
      setOptimistic([]);
      setPreferences(DEFAULT_PREFERENCES);
      return;
    }
    if (principalRef.current && principalRef.current !== identity) {
      const next = { sessionId: newId(), runId: "" };
      setSession(next);
      writeRoute(next.sessionId);
    }
    principalRef.current = identity;
    skipPreferenceWrite.current = true;
    const saved = readPreferences(identity);
    setPreferences(saved);
    setSelection(saved.workflow);
  }, [identity]);
  useEffect(() => {
    if (!identity) return;
    if (skipPreferenceWrite.current) {
      skipPreferenceWrite.current = false;
      return;
    }
    writePreferences(identity, preferences);
  }, [preferences, identity]);
  useEffect(() => {
    // Until the catalog has loaded, a saved workflow id cannot be checked.
    if (catalog.loading || !catalog.workflows.length) return;
    const next = reconcileSelection({
      selection,
      preferred: preferences.workflow,
      visible: visibleChoices,
      workflows: catalog.workflows,
      runId: session.runId,
    });
    if (next !== selection) setSelection(next);
  }, [catalog.loading, catalog.workflows, visibleChoices, selection, preferences.workflow, session.runId]);
  useEffect(() => {
    let alive = true;
    setInputs({});
    setInputEditorError("");
    setSchema(undefined);
    setSchemaError("");
    if (!workflow) {
      setSchemaLoading(false);
      return;
    }
    setSchemaLoading(true);
    void fetchWorkflowSchema(workflow)
      .then((next) => {
        if (alive) {
          setSchema(next);
          setInputs(schemaDefaults(next));
        }
      })
      .catch((e) => {
        if (alive) setSchemaError(formatError(e));
      })
      .finally(() => {
        if (alive) setSchemaLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [workflow?.id, schemaRevision]);
  useEffect(() => {
    setRestoredInputs(null);
    setRestoreError("");
    if (!identity || !session.runId) return;
    const abort = new AbortController();
    void gatewayRequest(
      `/api/gateway/runs/${encodeURIComponent(session.runId)}/input_data`,
      { signal: abort.signal },
    )
      .then((data) => {
        setRestoredInputs(data);
      })
      .catch((reason) => {
        if (!abort.signal.aborted) setRestoreError(formatError(reason));
      });
    return () => abort.abort();
  }, [session.runId, identity]);
  useEffect(() => {
    if (restoredInputs && restoredInputs.flow_id === workflow?.flowId && schema)
      setInputs({ ...schemaDefaults(schema), ...restoreWorkflowFields(schema, restoredInputs.input_data) });
  }, [restoredInputs, schema, workflow?.flowId]);
  useEffect(() => {
    if (!session.runId && connection.connected && currentSession?.latestRunId)
      setSession((s) => ({ ...s, runId: currentSession.latestRunId }));
  }, [session.runId, connection.connected, currentSession?.latestRunId]);
  useEffect(() => {
    writeRoute(session.sessionId, session.runId);
    sendLock.current = false;
    setSending(false);
    setUploads([]);
    setAttachments([]);
    setOptimistic([]);
    setNotice("");
    setError("");
    setQueue([]);
    setQueueRunning(false);
  }, [session.sessionId]);
  useEffect(() => {
    const update = () => {
      const next = route();
      sendLock.current = false;
      setSending(false);
      setUploads([]);
      setQueue([]);
      setQueueRunning(false);
      setDraft("");
      setSession(next);
    };
    window.addEventListener("hashchange", update);
    return () => window.removeEventListener("hashchange", update);
  }, []);
  useEffect(() => {
    if (!session.runId || !terminal(snapshot.status)) return;
    void catalog.refresh();
  }, [session.runId, snapshot.status]);
  useEffect(() => {
    if (!session.runId || !restoredInputs) return;
    const next = conversationSelection({ restoredInputs, restored: restoredWorkflow });
    if (next) setSelection(next);
  }, [session.runId, restoredInputs, restoredWorkflow?.id]);
  const sourceNote = session.runId ? selectionSourceNote(restoredInputs) : "";

  const newConversation = useCallback(() => {
    const next = { sessionId: newId(), runId: "" };
    setSession(next);
    setSelection(preferencesRef.current.workflow);
    writeRoute(next.sessionId);
    setDraft("");
    setQueue([]);
    setQueueRunning(false);
    setSidebarOpen(false);
  }, []);
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && sidebarOpen) {
        setSidebarOpen(false);
        document.querySelector<HTMLButtonElement>(".code-mobile-nav")?.focus();
        return;
      }
      if (!(event.metaKey || event.ctrlKey)) return;
      if (event.key.toLowerCase() === "k") {
        event.preventDefault();
        setSidebarOpen(true);
        window.setTimeout(() => searchInput.current?.focus(), 0);
      }
      if (event.key.toLowerCase() === "n" && event.shiftKey) {
        event.preventDefault();
        newConversation();
      }
    };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  }, [newConversation, sidebarOpen]);

  async function startTurn(text: string): Promise<string> {
    if (sendLock.current) throw new Error("A turn is already being submitted.");
    if (!connection.connected)
      throw new Error("Connect to your gateway first.");
    if (selection === GATEWAY_DEFAULT && catalog.gatewayDefault.status === "unavailable")
      throw new Error(
        `Gateway default: ${catalog.gatewayDefault.reason}. Choose a workflow from the list.`,
      );
    if (!workflow) throw new Error("Choose a workflow first.");
    if (!catalog.policy)
      throw new Error(
        "Workspace policy is unavailable. Reconnect or refresh before starting.",
      );
    if (session.runId && (!restoredInputs || restoreError))
      throw new Error(
        restoreError ||
          "Wait for the gateway to restore this conversation's workspace.",
      );
    if (restoreSelectionError) throw new Error(restoreSelectionError);
    if (schemaLoading || schemaError)
      throw new Error(schemaError || "Wait for workflow inputs to load.");
    if (inputEditorError) {
      setInputsOpen(true);
      throw new Error(inputEditorError);
    }
    const uploadBlock = unattachedSendBlock(uploads);
    if (uploadBlock) throw new Error(uploadBlock);
    if (active) throw new Error("The current run is still active.");
    if (!isAgent && text.trim() && !promptProperty) {
      setInputsOpen(true);
      throw new Error(
        "This workflow uses structured inputs. Configure its inputs, then select Run workflow.",
      );
    }
    const promptValues = {
      ...schemaDefaults(schema),
      ...inputs,
      ...(promptProperty && text.trim()
        ? { [promptProperty]: text.trim() }
        : {}),
    };
    const values = isAgent
      ? promptValues
      : withWorkflowAttachments(schema, promptValues, attachments);
    for (const [label, value] of [
      ["Iteration limit", preferences.maxIterations],
      ["Context token limit", preferences.maxTokens],
    ]) {
      if (value && (!Number.isInteger(Number(value)) || Number(value) <= 0))
        throw new Error(`${label} must be a positive whole number.`);
    }
    sendLock.current = true;
    setSending(true);
    setError("");
    setNotice("");
    const startedSession = session.sessionId;
    const startedRun = session.runId;
    const startedIdentity = identity;
    const startedEpoch = authEpoch.current;
    let attachedRun = "";
    try {
      const input = buildWorkflowInput({
        workflow,
        inputSchema: schema,
        schemaInputs: values,
        prompt: text,
        promptProperty,
        model: preferences,
        reasoning: preferences.reasoning || undefined,
        speculation: preferences.speculation,
        streamReplies: streamReplies,
        systemPromptExtra: preferences.system || undefined,
        attachments,
        limits: {
          maxIterations: preferences.maxIterations
            ? Number(preferences.maxIterations)
            : undefined,
          maxTokens: preferences.maxTokens
            ? Number(preferences.maxTokens)
            : undefined,
        },
        workspace: catalog.policy.clientWorkspaceScopeOverrides
          ? {
              root:
                preferences.workspaceRoot || effectiveWorkspace || undefined,
              accessMode: preferences.workspaceMode || undefined,
              allowedPaths: preferences.allowedPaths
                .split("\n")
                .filter(Boolean),
            }
          : effectiveWorkspace
            ? { root: effectiveWorkspace }
            : undefined,
        tools: preferences.toolsCustomized ? toolPermissions.enabledTools : undefined,
        toolPolicy: preferences.toolsCustomized || preferences.permissions !== "default"
          // Keep consent revocable by this host. The durable server policy
          // asks; the controller approves only currently enabled, permitted
          // tools. Closing the client parks an asking call safely.
          ? { autoApproveTools: [], requireApprovalTools: toolPermissions.enabledTools }
          : undefined,
        skills: preferences.skills.length ? preferences.skills : undefined,
      });
      // Validate the actual outgoing payload, after composer context and
      // explicit Settings overrides. Empty optional pins stay absent so the
      // workflow/Gateway resolves them just as it does for the TUI/Assistant.
      const problems = validateWorkflowInputs(schema, input);
      if (problems.length) {
        setInputsOpen(true);
        throw new Error(problems.join(" "));
      }
      const body = startRunBody({
        selection,
        workflow,
        sessionId: startedSession,
        input,
      });
      const result = await gatewayRequest("/api/gateway/runs/start", {
        method: "POST",
        body: JSON.stringify(body),
      });
      if (!result.run_id)
        throw new Error("The gateway did not return a run identifier.");
      if (
        sessionRef.current.sessionId !== startedSession ||
        sessionRef.current.runId !== startedRun ||
        identityRef.current !== startedIdentity ||
        authEpoch.current !== startedEpoch
      )
        throw staleSendAbort();
      attachedRun = String(result.run_id);
      startedRunInput.current = { runId: attachedRun, input };
      // "@default" resolves at every start: refresh the label from each one.
      setResolvedNote({
        runId: attachedRun,
        ...resolvedWorkflowNote(result.resolved_workflow),
      });
      setOptimistic([
        ...messages,
        ...(text.trim()
          ? [
              {
                id: `pending:${result.run_id}`,
                role: "user",
                content: text.trim(),
              },
            ]
          : []),
      ]);
      setSession({ sessionId: startedSession, runId: result.run_id });
      writeRoute(startedSession, result.run_id);
      setDraft("");
      setAttachments([]);
      setInputsOpen(false);
      if (result.runner_warning) setNotice(String(result.runner_warning));
      void catalog.refresh();
      return attachedRun;
    } finally {
      if (
        sessionRef.current.sessionId === startedSession &&
        (sessionRef.current.runId === startedRun ||
          sessionRef.current.runId === attachedRun) &&
        identityRef.current === startedIdentity &&
        authEpoch.current === startedEpoch
      ) {
        sendLock.current = false;
        setSending(false);
      }
    }
  }

  async function send(text: string) {
    if (!active) return startTurn(text);
    if (composeMode === "queue") {
      setQueue((items) => [
        ...items,
        {
          id: newId(),
          text,
          sessionId: session.sessionId,
          sourceRunId: session.runId,
          authEpoch: authEpoch.current,
        },
      ]);
      setQueueRunning(true);
      setDraft("");
      setNotice("Queued for the next turn.");
    } else {
      if (interaction)
        throw new Error(
          "Answer the pending workflow request above, or choose Queue next turn.",
        );
      const sid = session.sessionId;
      const rid = session.runId;
      const epoch = authEpoch.current;
      await controller.sendCommand("inject_guidance", { guidance: text });
      if (
        sessionRef.current.sessionId !== sid ||
        sessionRef.current.runId !== rid ||
        authEpoch.current !== epoch
      )
        return;
      setDraft("");
      setNotice(
        "Guidance queued. The workflow will read it at its next supported boundary.",
      );
    }
  }
  async function executeQueuedIntent(intent: QueueIntent): Promise<void> {
    const snapshotRunId = String(snapshot.run?.run_id || "");
    if (
      !queueIntentMatches(intent, {
        sessionId: session.sessionId,
        runId: session.runId,
        snapshotRunId,
        snapshotSessionId: String(snapshot.run?.session_id || ""),
        authEpoch: authEpoch.current,
      })
    )
      throw staleSendAbort();
    const nextRunId = await startTurn(intent.text);
    setQueue((items) => advanceQueueIntents(items, intent, nextRunId));
  }
  useEffect(() => {
    if (
      !queueRunning ||
      !queue.length ||
      !connection.connected ||
      !restoredInputs ||
      restoreError ||
      sendLock.current
    )
      return;
    const disposition = queueTerminalDisposition(snapshot.status);
    if (disposition === "wait") return;
    if (disposition === "pause") {
      setQueueRunning(false);
      const failed = ["failed", "error"].includes(
        snapshot.status.toLowerCase(),
      );
      setNotice(
        `Queue paused because the run ${failed ? "failed" : "was cancelled"}. Review the result, then use Run next to continue explicitly.`,
      );
      return;
    }
    const next = queue[0];
    if (
      !queueIntentMatches(next, {
        sessionId: session.sessionId,
        runId: session.runId,
        snapshotRunId: String(snapshot.run?.run_id || ""),
        snapshotSessionId: String(snapshot.run?.session_id || ""),
        authEpoch: authEpoch.current,
      })
    )
      return;
    void executeQueuedIntent(next).catch((e) => {
      if (e instanceof DOMException && e.name === "AbortError") return;
      setQueueRunning(false);
      setError(formatError(e));
    });
  }, [
    queueRunning,
    queue,
    snapshot.status,
    snapshot.run?.run_id,
    snapshot.run?.session_id,
    connection.connected,
    restoredInputs,
    restoreError,
    session.sessionId,
    session.runId,
  ]);

  // Every file (picked, dropped or pasted) becomes a chip at once. Uploads
  // run UPLOAD_CONCURRENCY at a time into the session's attachment store;
  // the gateway's maxAttachmentBytes is the only size rule (ADR-0026) and a
  // refusal stays on its chip with both numbers — nothing is dropped silently.
  async function uploadBatch(items: PendingUpload[]): Promise<void> {
    const sid = session.sessionId;
    const owner = identity;
    const epoch = authEpoch.current;
    // Attachments belong to the session, not to a run: a run starting or
    // finishing mid-upload must not strand a spinning chip.
    const stale = () =>
      sessionRef.current.sessionId !== sid ||
      identityRef.current !== owner ||
      authEpoch.current !== epoch;
    const patch = (id: string, change: Partial<PendingUpload>) =>
      setUploads((list) =>
        list.map((item) => (item.id === id ? { ...item, ...change } : item)),
      );
    let attached = 0;
    let failed = 0;
    await runBounded(items, UPLOAD_CONCURRENCY, async (item) => {
      if (stale() || removedUploads.current.has(item.id)) return;
      patch(item.id, { status: "uploading", message: undefined });
      try {
        const ref = await gateway.attachments_upload(sid, item.file);
        if (stale() || removedUploads.current.has(item.id)) return;
        uploadedSizes.current.set(ref.$artifact, item.size);
        setUploads((list) => list.filter((entry) => entry.id !== item.id));
        setAttachments((list) => [...list, ref]);
        attached += 1;
      } catch (e) {
        if (stale() || removedUploads.current.has(item.id)) return;
        patch(item.id, {
          status: "failed",
          message: uploadFailure(formatError(e)),
        });
        failed += 1;
      }
    });
    if (!stale() && (attached || failed))
      setUploadNotice(uploadAnnouncement(attached, failed));
  }
  function attachUploads(files: File[]): PendingUpload[] {
    if (!files.length) return [];
    const items = queueUploads(
      files,
      catalog.policy?.maxAttachmentBytes,
      newId,
    );
    setUploads((list) => [...list, ...items]);
    const refused = items.filter((item) => item.status === "refused").length;
    if (refused)
      setUploadNotice(uploadAnnouncement(0, refused));
    void uploadBatch(items.filter((item) => item.status === "queued"));
    return items;
  }
  function retryUpload(id: string) {
    const item = uploads.find((entry) => entry.id === id);
    if (!item || item.status !== "failed") return;
    const again: PendingUpload = { ...item, status: "queued", message: undefined };
    setUploads((list) => list.map((entry) => (entry.id === id ? again : entry)));
    void uploadBatch([again]);
  }
  function removeUpload(id: string) {
    removedUploads.current.add(id);
    setUploads((list) => list.filter((entry) => entry.id !== id));
  }
  async function cancelCurrentRun(): Promise<void> {
    const sid = session.sessionId;
    const rid = session.runId;
    const epoch = authEpoch.current;
    // The Stop outcome is no longer a client notice: the controller derives
    // Stopping… / Stopped / "Stop forced …" from ledger records
    // (snapshot.stop) and WorkflowChat shows it where the Stop button was.
    await controller.cancel();
    if (
      sessionRef.current.sessionId === sid &&
      sessionRef.current.runId === rid &&
      authEpoch.current === epoch
    ) {
      setNotice("");
    }
  }
  const openSettings = (tab: SettingsTab) => {
    setSettingsTab(tab);
    setSettingsOpen(true);
  };
  const act = (callback: () => Promise<unknown>) => {
    const epoch = authEpoch.current;
    const sid = session.sessionId;
    const rid = session.runId;
    setError("");
    void callback().catch((e) => {
      if (e instanceof DOMException && e.name === "AbortError") return;
      if (
        authEpoch.current === epoch &&
        sessionRef.current.sessionId === sid &&
        sessionRef.current.runId === rid
      )
        setError(formatError(e));
    });
  };
  const filteredSessions = catalog.sessions.filter((item) =>
    `${item.prompt || ""} ${item.sessionId}`
      .toLowerCase()
      .includes(search.toLowerCase()),
  );
  const gatewayName = (() => {
    try {
      return new URL(connection.status?.gateway_url || "").host;
    } catch {
      return "Your gateway";
    }
  })();

  return (
    <div
      className={`code-app${sidebarOpen ? " code-app--nav-open" : ""}${inspectorOpen ? " code-app--inspector" : ""}`}
    >
      <a className="code-skip-link" href="#code-conversation">
        Skip to conversation
      </a>
      <aside className="code-sidebar" aria-label="Conversations">
        <div className="code-brand">
          <span className="code-brand-mark" aria-hidden="true">
            a<span>c</span>
          </span>
          <div>
            <strong>AbstractCode</strong>
            <span>YOUR WORK, IN CONTEXT</span>
          </div>
          <button
            className="code-mobile-close code-icon-button"
            aria-label="Close navigation"
            onClick={() => setSidebarOpen(false)}
          >
            <Icon name="x" size={18} />
          </button>
        </div>
        <button className="code-new-chat" onClick={newConversation}>
          <Icon name="plus" size={17} />
          <span>New conversation</span>
          <kbd>⇧⌘N</kbd>
        </button>
        <div className="code-session-search">
          <Icon name="history" size={14} />
          <input
            ref={searchInput}
            aria-label="Search conversations"
            placeholder="Search conversations"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <kbd>⌘K</kbd>
        </div>
        <div className="code-section-label">
          <span>CONVERSATIONS</span>
          <button
            className="code-icon-button"
            aria-label="Refresh conversations"
            disabled={!connection.connected || catalog.loading}
            onClick={() => void catalog.refresh()}
          >
            <Icon name="refresh" size={13} />
          </button>
        </div>
        <nav className="code-sessions" aria-label="Conversation history">
          {!currentSession ? (
            <button
              className="code-session is-selected"
              onClick={() => setSidebarOpen(false)}
            >
              <Icon name="chat" size={15} />
              <span>
                <strong>New conversation</strong>
                <small>Ready when you are</small>
              </span>
            </button>
          ) : null}
          {filteredSessions.map((item) => (
            <SessionButton
              key={item.sessionId}
              item={item}
              selected={item.sessionId === session.sessionId}
              onClick={() => {
                sendLock.current = false;
                setSending(false);
                setUploads([]);
                setSession({
                  sessionId: item.sessionId,
                  runId: item.latestRunId,
                });
                writeRoute(item.sessionId, item.latestRunId);
                setSidebarOpen(false);
                setQueue([]);
                setQueueRunning(false);
                setDraft("");
              }}
            />
          ))}
          {!catalog.loading && !filteredSessions.length ? (
            <p className="code-history-empty">
              {search
                ? "No conversations match your search."
                : "Your conversations will live here. Pick up where you left off, on any device."}
            </p>
          ) : null}
          {catalog.loading ? (
            <p className="code-history-empty" role="status">
              Loading conversations…
            </p>
          ) : null}
          {catalog.hasMore ? (
            <button className="code-load-more" onClick={catalog.loadMore}>
              Load more conversations
            </button>
          ) : null}
        </nav>
        <div className="code-sidebar-bottom">
          <button onClick={() => openSettings("workspace")}>
            <Icon name="terminal" size={17} />
            <span>
              <strong>Workspace</strong>
              <small>
                {effectiveWorkspace.split("/").filter(Boolean).pop() ||
                  "Gateway managed"}
              </small>
            </span>
            <Icon name="chevronRight" size={14} />
          </button>
          <button onClick={() => openSettings("model")}>
            <Icon name="settings" size={17} />
            <span>Settings</span>
          </button>
          <div className="code-gateway-foot">
            <span
              className={`code-status-dot ${connection.connected ? "is-online" : ""}`}
            />
            <span>{gatewayName}</span>
            <small>
              {connection.status?.gateway?.principal?.user_id || "Signed out"}
            </small>
          </div>
        </div>
      </aside>
      {sidebarOpen ? (
        <button
          className="code-nav-scrim"
          aria-label="Close conversation navigation"
          onClick={() => setSidebarOpen(false)}
        />
      ) : null}
      <div className="code-main-shell">
        <header className="code-topbar">
          <button
            className="code-mobile-nav code-icon-button"
            aria-label="Open conversation navigation"
            onClick={() => setSidebarOpen(true)}
          >
            <Icon name="list" size={19} />
          </button>
          <div className="code-breadcrumb">
            <span>Conversations</span>
            <span aria-hidden="true">/</span>
            <strong title={title}>{title}</strong>
          </div>
          <AfTopBarActions
            appearance={{ onOpen: () => setAppearanceOpen(true) }}
            about={{
              identity: APP_IDENTITY,
              extraRows: aboutExtraRows(gatewayAbout),
              onOpen: refreshGatewayAbout,
            }}
            extraActions={
              <button
                className={`af-topbar__btn${inspectorOpen ? " is-active" : ""}`}
                aria-label="Toggle workspace inspector"
                aria-pressed={inspectorOpen}
                onClick={() => setInspectorOpen((v) => !v)}
              >
                <Icon name="board" size={17} />
              </button>
            }
            connection={{
              phase: connection.phase,
              signingOut: connection.signingOut,
              onConnect: connection.openModal,
              onDisconnect: () => {
                void connection.signOut();
              },
            }}
          />
        </header>
        <div className="code-toolbar">
          <div className="code-workflow-select">
            <Icon name="agent" size={17} />
            <select
              aria-label="Workflow"
              value={selection}
              disabled={locked || catalog.loading || !connection.connected}
              onChange={(e) => {
                const next = e.target.value;
                setSelection(next);
                setPreferences((previous) => ({ ...previous, workflow: next }));
              }}
            >
              <option value={GATEWAY_DEFAULT}>
                {gatewayDefaultOptionLabel(catalog.gatewayDefault)}
              </option>
              {workflow && selection !== GATEWAY_DEFAULT && !visibleChoices.some(item => item.id === workflow.id) ? <option value={workflow.id}>{workflow.name}{session.runId ? ` · conversation version ${workflow.bundleVersion || ""}` : ""}</option> : null}
              {visibleChoices.map((item) => (
                <option key={item.id} value={item.id}>
                  {workflowChoiceLabel(item, visibleChoices)}
                </option>
              ))}
            </select>
            <span className="code-workflow-kind">
              {isAgent ? "AGENT" : "WORKFLOW"}
            </span>
            <label className="code-workflow-all" title="List workflows that are not coding agents too">
              <input
                type="checkbox"
                checked={preferences.showAllWorkflows}
                disabled={locked || !connection.connected}
                onChange={(e) =>
                  setPreferences((previous) => ({
                    ...previous,
                    showAllWorkflows: e.target.checked,
                  }))
                }
              />
              <span>Show all workflows</span>
            </label>
            {selection === GATEWAY_DEFAULT && defaultInterfaceMismatch(defaultWorkflow) ? (
              <span className="code-workflow-resolved is-missing" role="alert" title={defaultInterfaceMismatch(defaultWorkflow)}>
                {defaultInterfaceMismatch(defaultWorkflow)}
              </span>
            ) : null}
            {sourceNote ? (
              <span className="code-workflow-resolved is-missing" role="status" title={sourceNote}>
                {sourceNote}
              </span>
            ) : null}
            {resolvedNote && resolvedNote.runId === session.runId ? (
              <span
                className={`code-workflow-resolved${resolvedNote.missing ? " is-missing" : ""}`}
                role="status"
              >
                {resolvedNote.text}
              </span>
            ) : null}
          </div>
          <span className="code-toolbar-divider" />
          <button
            className="code-model-button"
            onClick={() => openSettings("model")}
          >
            <Icon name="sparkle" size={14} />
            <span>
              {preferences.model ||
                  (inputs.provider && inputs.model
                    ? `Workflow · ${inputs.model}`
                    : "Gateway default")}
            </span>
            <Icon name="chevronDown" size={12} />
          </button>
          <div className="code-toolbar-spacer" />
          <button
            className="code-subtle-button"
            onClick={() => setInputsOpen(true)}
            disabled={!workflow}
          >
            <Icon name="settings" size={14} />
            <span>Inputs</span>
          </button>
          <button
            className="code-subtle-button"
            onClick={() => openSettings("tools")}
          >
            <Icon name="terminal" size={14} />
            <span>Tools</span>
          </button>
          {messages.length ? (
            <button
              className="code-icon-button"
              aria-label="Export conversation"
              title="Export conversation"
              onClick={() =>
                downloadTextFile({
                  filename: "conversation.md",
                  text: chatToMarkdown(messages),
                })
              }
            >
              <Icon name="download" size={15} />
            </button>
          ) : null}
        </div>
        <div className="code-content">
          <main
            className="code-conversation"
            id="code-conversation"
            tabIndex={-1}
          >
            {error ||
            snapshot.error ||
            connection.signOutError ||
            restoreError ? (
              <div className="code-alert" role="alert">
                <Icon name="warning" size={16} />
                <span>
                  {error ||
                    snapshot.error ||
                    connection.signOutError ||
                    restoreError}
                </span>
                {snapshot.error || restoreError ? (
                  <button
                    onClick={() => {
                      if (restoreError) window.location.reload();
                      else act(() => controller.load(session.runId));
                    }}
                  >
                    Reconnect
                  </button>
                ) : (
                  <button
                    aria-label="Dismiss error"
                    onClick={() => setError("")}
                  >
                    <Icon name="x" size={14} />
                  </button>
                )}
              </div>
            ) : null}
            {catalog.errors.length ? (
              <div className="code-discovery-errors" role="status">
                <details>
                  <summary>
                    Some gateway information could not be loaded
                  </summary>
                  {catalog.errors.map((text) => (
                    <p key={text}>{text}</p>
                  ))}
                  <button onClick={() => void catalog.refresh()}>
                    Retry discovery
                  </button>
                </details>
              </div>
            ) : null}
            {restoreSelectionError ? (
              <div className="code-alert" role="alert">
                <span>{restoreSelectionError}</span>
                <button onClick={newConversation}>New conversation</button>
              </div>
            ) : null}
            {schemaError ? (
              <div className="code-alert" role="alert">
                <span>Workflow inputs: {schemaError}</span>
                <button onClick={() => setSchemaRevision((n) => n + 1)}>
                  Retry
                </button>
              </div>
            ) : null}
            {notice ? (
              <div className="code-notice" role="status">
                {notice}
                <button
                  aria-label="Dismiss notice"
                  onClick={() => setNotice("")}
                >
                  <Icon name="x" size={13} />
                </button>
              </div>
            ) : null}
            {active || terminal(snapshot.status) ? (
              <RunStatusBar
                active={active}
                paused={paused}
                snapshot={snapshot}
                permissionsAll={preferences.permissions === "all"}
                onInteraction={
                  interaction && !paused
                    ? () => {
                        const thread = document.querySelector<HTMLElement>(
                          ".code-conversation .pc-chat-thread",
                        );
                        const request = thread?.querySelector<HTMLElement>(
                          ".pc-workflow-interaction",
                        );
                        if (!thread || !request) return;
                        // Move only the transcript: scrollIntoView can shift the entire app shell.
                        thread.scrollTop +=
                          request.getBoundingClientRect().top -
                          thread.getBoundingClientRect().top;
                        request
                          .querySelector<HTMLElement>("h3")
                          ?.focus({ preventScroll: true });
                      }
                    : undefined
                }
                onRevokeApproval={() => {
                  controller.revokeToolApproval();
                  setPreferences(previous => ({ ...previous, permissions: "default" }));
                }}
                onCommand={(command) =>
                  act(() => controller.sendCommand(command, {}))
                }
                onActivity={() => {
                  setInspectorTab("activity");
                  setInspectorOpen(true);
                }}
              />
            ) : null}
            <WorkflowChat
              streamReplies={streamReplies}
              messages={
                interaction?.kind === "tool-approval"
                  ? messages.filter(
                      (message) =>
                        !(
                          message.toolActivity?.status === "waiting" &&
                          message.toolActivity.runId ===
                            snapshot.interaction?.runId
                        ),
                    )
                  : messages
              }
              messageProps={{
                ...(voice.tts_supported
                  ? {
                      onSpeakToggle: (message) => {
                        void voice.toggle_tts(
                          message.id || message.content,
                          message.content,
                        );
                      },
                    }
                  : {}),
                getSpeakState: (message) =>
                  voice.tts_playback.key === (message.id || message.content)
                    ? voice.tts_playback.status
                    : "idle",
              }}
              draft={draft}
              onDraftChange={setDraft}
              onSend={send}
              busy={locked}
              busyLabel={
                interactionBlocksSteer
                  ? "Waiting for you"
                  : sending
                    ? "Starting…"
                    : paused
                      ? "Paused"
                      : "Working…"
              }
              sendWhileBusy={active && !sending && !interactionBlocksSteer}
              sendLabel={
                active ? (composeMode === "queue" ? "Queue" : "Steer") : "Send"
              }
              onCancel={cancelCurrentRun}
              stopState={snapshot.stop ?? null}
              disabled={!connection.connected}
              interaction={interaction}
              onAttach={() => fileInput.current?.click()}
              onFiles={(files) => { attachUploads(files); }}
              attachments={
                attachments.length || uploads.length ? (
                  <ComposerAttachments
                    attachments={attachments}
                    uploads={uploads}
                    sizes={uploadedSizes.current}
                    onRemoveAttachment={(index) =>
                      setAttachments((items) =>
                        items.filter((_, n) => n !== index),
                      )
                    }
                    onRemoveUpload={removeUpload}
                    onRetryUpload={retryUpload}
                  />
                ) : null
              }
              placeholder={
                active
                  ? composeMode === "queue"
                    ? "Queue the next task after this run…"
                    : interaction
                      ? "Answer the request above, or choose Queue next turn…"
                      : "Guide the current workflow…"
                  : "Describe what you want to build, fix, or explore…"
              }
              emptyState={
                active ? null : (
                  <EmptyConversation
                    workflow={workflow}
                    loading={catalog.loading || schemaLoading}
                    onSuggestion={setDraft}
                    onInputs={() => setInputsOpen(true)}
                    isAgent={chatReady}
                    onRun={() => act(() => startTurn(draft))}
                    connected={connection.connected}
                  />
                )
              }
              composerExtras={
                <>
                  <button
                    className="code-composer-workspace"
                    onClick={() => openSettings("workspace")}
                    title={
                      effectiveWorkspace ||
                      "Workspace is managed by the gateway"
                    }
                  >
                    <Icon name="terminal" size={14} />
                    <span>
                      {effectiveWorkspace.split("/").filter(Boolean).pop() ||
                        "Gateway workspace"}
                    </span>
                  </button>
                  {connection.connected ? (
                    <VoiceTools
                      key={`${identity}:${session.sessionId}:${session.runId}`}
                      runId={session.runId}
                      voice={voice}
                      capability={voiceCapability}
                      onSettings={() => setVoiceOpen(true)}
                      answer={[...messages]
                        .reverse()
                        .find((message) => message.role === "assistant")}
                    />
                  ) : null}
                  {active ? (
                    <select
                      aria-label="Message destination"
                      value={composeMode}
                      onChange={(e) =>
                        setComposeMode(e.target.value as "steer" | "queue")
                      }
                    >
                      <option value="steer" disabled={Boolean(interaction)}>
                        Guide this run{interaction ? " (answer above)" : ""}
                      </option>
                      <option value="queue">Queue next turn</option>
                    </select>
                  ) : null}
                </>
              }
              footer={
                <>
                  <span>
                    Enter to send <span aria-hidden="true">·</span> Shift +
                    Enter for a new line
                  </span>
                  <span>
                    {catalog.policy?.clientWorkspaceScopeOverrides
                      ? "Gateway policy enforced"
                      : "Managed workspace"}
                    <span className="code-small-dot" />
                  </span>
                </>
              }
            />
            {queue.length ? (
              <div className="code-queue">
                <div>
                  <strong>
                    {queue.length} queued{" "}
                    {queue.length === 1 ? "turn" : "turns"}
                  </strong>
                  <button onClick={() => setQueueRunning((v) => !v)}>
                    {queueRunning ? "Pause queue" : "Resume queue"}
                  </button>
                </div>
                {queue.map((item) => (
                  <div key={item.id}>
                    <span>{item.text}</span>
                    {!active ? (
                      <button
                        onClick={() => act(() => executeQueuedIntent(item))}
                      >
                        Run next
                      </button>
                    ) : null}
                    <button
                      aria-label="Remove queued turn"
                      onClick={() =>
                        setQueue((items) =>
                          items.filter((it) => it.id !== item.id),
                        )
                      }
                    >
                      <Icon name="x" size={13} />
                    </button>
                  </div>
                ))}
              </div>
            ) : null}
          </main>
          {inspectorOpen ? (
            <WorkspaceInspector
              tab={inspectorTab}
              onTab={setInspectorTab}
              policy={catalog.policy}
              runId={session.runId}
              records={snapshot.records}
              enabled={connection.connected}
              isAdmin={connection.status?.gateway?.principal?.admin === true}
              refreshKey={snapshot.status}
              onAttachFiles={attachUploads}
              onClose={() => setInspectorOpen(false)}
              onAttach={async (path) => {
                const sid = session.sessionId;
                const rid = session.runId;
                const epoch = authEpoch.current;
                const ref = await gateway.attachments_ingest(sid, path);
                if (
                  sessionRef.current.sessionId === sid &&
                  sessionRef.current.runId === rid &&
                  authEpoch.current === epoch
                )
                  setAttachments((items) => [...items, ref]);
              }}
            />
          ) : null}
        </div>
        <footer className="code-statusbar">
          <span>
            <span
              className={`code-status-dot ${connection.connected ? "is-online" : ""}`}
            />
            {connection.connected
              ? "Connected"
              : connection.phase === "loading"
                ? "Connecting"
                : "Disconnected"}
          </span>
          <span>{workflow?.name || "No workflow selected"}</span>
          <div />
          <span>
            {session.runId
              ? `Run ${session.runId.slice(0, 8)}`
              : "Ready to start"}
          </span>
          <button onClick={() => setAppearanceOpen(true)}>
            {appearance.theme.replace(/-/g, " ")}
          </button>
        </footer>
      </div>
      <div className="pc-sr-only" role="status" aria-live="polite">
        {uploadNotice}
      </div>
      <input
        ref={fileInput}
        hidden
        type="file"
        multiple
        onChange={(e) => {
          void attachUploads(Array.from(e.target.files || []));
          e.target.value = "";
        }}
      />
      <GatewayConnectModal {...connection.modalProps} />
      <AfDrawer
        open={voiceOpen}
        onClose={() => setVoiceOpen(false)}
        label="Voice settings"
        title="AI voice"
        width={480}
        topOffset={60}
        className="code-settings-drawer"
      >
        <div className="code-settings">
          <VoiceSettings
            value={voicePreferences}
            onChange={(next) => {
              voice.stop_tts();
              changeVoicePreferences(next);
            }}
            fetchCatalog={(provider, model) =>
              gatewayRequest(
                `/api/gateway/voice/voices?compact=true${provider ? `&provider=${encodeURIComponent(provider)}` : ""}${model ? `&model=${encodeURIComponent(model)}` : ""}`,
              )
            }
          />
        </div>
      </AfDrawer>
      <AfAppearanceDialog
        open={appearanceOpen}
        onClose={() => setAppearanceOpen(false)}
        value={appearance}
        onChange={setAppearance}
      />
      <SettingsPanel
          key={identity}
          open={settingsOpen}
          onClose={() => setSettingsOpen(false)}
          tab={settingsTab}
          onTab={setSettingsTab}
          value={preferences}
          onChange={(next) => {
            // Reset only on an explicit Settings transition. Runtime values
            // matching flat pins are not proof they were host-authored.
            const reset: string[] = [];
            if ((preferences.provider || preferences.model) && !next.provider && !next.model) reset.push("provider", "model");
            if (preferences.toolsCustomized && !next.toolsCustomized) reset.push("tools");
            if (preferences.maxIterations && !next.maxIterations) reset.push("max_iterations");
            if (reset.length) setInputs(previous => {
              const values = { ...previous }, defaults = schemaDefaults(schema);
              for (const key of reset) {
                delete values[key];
                if (Object.prototype.hasOwnProperty.call(defaults, key)) values[key] = defaults[key];
              }
              return values;
            });
            setPreferences(next);
          }}
          policy={catalog.policy}
          tools={catalog.tools}
          defaultModel={
            inputs.provider && inputs.model
              ? {
                  provider: String(inputs.provider),
                  model: String(inputs.model),
                }
              : catalog.defaultModel
          }
          workflowDefault={Boolean(inputs.provider && inputs.model)}
          streaming={catalog.streaming}
          disabled={locked || !connection.connected}
        />
      <AfDrawer
        open={inputsOpen}
        onClose={() => setInputsOpen(false)}
        label="Workflow inputs"
        title={workflow?.name || "Workflow inputs"}
        width={480}
        topOffset={60}
        className="code-settings-drawer"
      >
        <div className="code-settings">
          <p className="code-muted">
            {workflow?.description ||
              "Configure the values this workflow needs to run."}
          </p>
          {schemaLoading ? (
            <p role="status">Loading input schema…</p>
          ) : schemaError ? null : isAgent || promptProperty ? (
            <AgentWorkflowInputs
              key={workflow?.id}
              schema={schema}
              promptProperty={promptProperty}
              defaultModel={catalog.defaultModel}
              values={inputs}
              onChange={setInputs}
              disabled={locked}
              onEditorError={setInputEditorError}
            />
          ) : (
            <WorkflowInputs
              key={workflow?.id}
              schema={schema}
              defaultModel={catalog.defaultModel}
              values={inputs}
              onChange={setInputs}
              disabled={locked}
              rawObject
              onEditorError={setInputEditorError}
            />
          )}
          {schemaError ? (
            <p role="alert" className="code-error-text">
              {schemaError}
            </p>
          ) : null}
          {chatReady ? (
            <button
              className="code-primary-button"
              onClick={() => setInputsOpen(false)}
            >
              Back to chat <Icon name="chat" size={16} />
            </button>
          ) : (
            <button
              className="code-primary-button"
              disabled={
                locked ||
                schemaLoading ||
                Boolean(schemaError) ||
                Boolean(inputEditorError) ||
                !connection.connected
              }
              onClick={() =>
                act(() =>
                  startTurn(
                    draft || String(inputs[promptProperty || "prompt"] || ""),
                  ),
                )
              }
            >
              Run workflow <Icon name="playCircle" size={16} />
            </button>
          )}
        </div>
      </AfDrawer>
    </div>
  );
}

function SessionButton({
  item,
  selected,
  onClick,
}: {
  item: SessionSummary;
  selected: boolean;
  onClick: () => void;
}) {
  const label = item.prompt || `Conversation ${item.sessionId.slice(0, 8)}`;
  const date = item.updatedAt ? new Date(item.updatedAt) : null;
  return (
    <button
      className={`code-session${selected ? " is-selected" : ""}`}
      aria-current={selected ? "page" : undefined}
      onClick={onClick}
      title={label}
    >
      <Icon name="chat" size={15} />
      <span>
        <strong>{label}</strong>
        <small>
          {date && Number.isFinite(date.getTime())
            ? date.toLocaleDateString(undefined, {
                month: "short",
                day: "numeric",
              })
            : "Saved conversation"}{" "}
          <span aria-hidden="true">·</span> {item.turnCount}{" "}
          {item.turnCount === 1 ? "turn" : "turns"}
        </small>
      </span>
      {item.state === "running" || item.state === "waiting" ? (
        <span
          className={`code-status-dot ${item.state === "running" ? "is-working" : "is-waiting"}`}
          title={item.state}
        />
      ) : null}
    </button>
  );
}

function EmptyConversation({
  workflow,
  loading,
  onSuggestion,
  onInputs,
  isAgent,
  onRun,
  connected,
}: {
  workflow: WorkflowDefinition | null;
  loading: boolean;
  onSuggestion: (text: string) => void;
  onInputs: () => void;
  isAgent: boolean;
  onRun: () => void;
  connected: boolean;
}) {
  return (
    <div className="code-welcome">
      <div className="code-welcome-symbol" aria-hidden="true">
        <span>›</span>
        <span>_</span>
      </div>
      <p className="code-eyebrow">A LITTLE CONTEXT. A LOT OF POSSIBILITY.</p>
      <h1>What are we building?</h1>
      <p className="code-welcome-description">
        A focused space to think, code, and get things done.
        <br />
        Your workflows. Your tools. One conversation.
      </p>
      {workflow && !isAgent ? (
        <div className="code-workflow-welcome">
          <Icon name="playCircle" size={21} />
          <div>
            <strong>{workflow.name}</strong>
            <p>
              {workflow.description ||
                "Run this workflow with its registered inputs."}
            </p>
          </div>
          <button onClick={onInputs}>Configure inputs</button>
          <button onClick={onRun} disabled={loading || !connected}>
            Run workflow
          </button>
        </div>
      ) : (
        <div className="code-suggestions">
          {[
            {
              icon: "terminal" as const,
              title: "Explore this project",
              detail: "Find your bearings in the codebase",
              prompt:
                "Explore this project and explain its architecture, key entry points, and how to run its tests.",
            },
            {
              icon: "edit" as const,
              title: "Build something new",
              detail: "Turn an idea into working code",
              prompt:
                "I'd like to build a new feature. Help me clarify the requirements and plan the implementation.",
            },
            {
              icon: "warning" as const,
              title: "Track down a bug",
              detail: "Understand what went wrong",
              prompt:
                "Help me investigate a bug. Start by asking what happened and what I expected.",
            },
            {
              icon: "check" as const,
              title: "Review my changes",
              detail: "Get a second pair of eyes",
              prompt:
                "Review the current changes for correctness, edge cases, and maintainability. Explain any findings before making edits.",
            },
          ].map((item) => (
            <button
              key={item.title}
              onClick={() => onSuggestion(item.prompt)}
              disabled={loading}
            >
              <Icon name={item.icon} size={18} />
              <span>
                <strong>{item.title}</strong>
                <small>{item.detail}</small>
              </span>
              <Icon name="chevronRight" size={13} />
            </button>
          ))}
        </div>
      )}
      <p className="code-welcome-hint">
        {loading ? (
          "Discovering your workflows…"
        ) : workflow ? (
          <>
            Using <strong>{workflow.name}</strong> · Change workflows above
          </>
        ) : connected ? (
          "Register a workflow with AbstractGateway to get started."
        ) : (
          "Connect your gateway to get started."
        )}
      </p>
    </div>
  );
}

export class WorkspaceErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: string }
> {
  state = { error: "" };
  static getDerivedStateFromError(error: Error) {
    return { error: error.message };
  }
  render() {
    return this.state.error ? (
      <div className="code-crash" role="alert">
        <h1>Something interrupted this view.</h1>
        <p>{this.state.error}</p>
        <p>Your runs are stored by the gateway.</p>
        <button onClick={() => window.location.reload()}>
          Reload AbstractCode
        </button>
      </div>
    ) : (
      this.props.children
    );
  }
}
