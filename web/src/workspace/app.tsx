import {WorkflowChat, useWorkflowSession, conversationSnapshot, playConversationCue, conversationReply, presentInteraction, mentionResend, mentionsStoppedError, type ChatAuthor, type ChatMentionCandidate, type ChatMentionSent, type MentionResend} from "../vendor/abstractchat/conversation.js";
import { useConversationView } from "./conversation_view";
import { MENTIONS_UNAVAILABLE, agentNotAddressed, conversationWorkflowTurn, entityCandidates, entityMentions, filterMentionCandidates, foldRepeatedTurns, isSideTurn, mentionMessageKey, mentionRepeatOf, mentionRequestBody, mentionRoster, mentionsSummary, sendMentions, startRunMentions, startedTurn, withLiveAuthors, type StartedTurn } from "./mentions";
import { guardedRunInput } from "./model_options_guard";
import type { ModelOptions } from "@abstractframework/ui-kit";
import type { IterationLimitApplied } from "@abstractframework/ui-kit";
import { AUTOMATION_ITERATION_DEFAULT, runLimitsReceipt } from "./iteration_limit";
import { ActivityAvatar, useVoiceLevel } from "@abstractframework/ui-kit";
import "../vendor/abstractchat/conversation.css";
import { sessionReadTarget, sessionOpeningReceipts } from "../vendor/abstractsessions/session_cards.js";
import { createSessionReadClient, useSessionReads, sessionReadScope, SessionReadMarker, sessionContentRevision } from "../vendor/abstractsessions/session_cards.js";
import { CodeDocsAssistant } from "./app_assistant";
import { LoadingStatus } from "./loading_status";
import {
  automationToolSelection,
  createToolLayerStore,
  gatewayApiPath,
  hasToolChoices,
  planRunTools,
  toWorkflowApprovalPolicy,
  withRunTools,
  type ToolModeLayer,
} from "@abstractframework/ui-kit";
import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  AfMenu,
  AfTopBarActions,
  AfSwitch,
  GatewayConnectModal,
  Icon,
  useAppearanceSettings,
  useVoiceDefaults,
  useVoicePreferences,
  useGatewayConnection,
  sttRouteText,
  appIdentity,
  AF_MEDIA,
  useAfMedia,
  WorkflowPicker,
  type VoiceClientPreferences,
  type AutomationTarget,
} from "@abstractframework/ui-kit";
import {
  escapeTarget,
  kitDialogOpen,
  paneModeFrom,
  sidebarOnModeChange,
  useDrawerFocus,
  type PaneMode,
} from "./layout";
import { SidebarDrawer } from "./sidebar_drawer";
import { CodeDefaultWorkspacesDialog } from "./workspace_folders";
import { ArchiveConfirm, ArchivedFooter, ConversationsPanel, SidebarLists, useArchivedOpen, useSidebarPanels } from "./sidebar_panels";
import { ArchivedConversations, archiveSession, nextConversationAfterArchive } from "./archived_conversations";
import { ConversationCard, UNTITLED_CONVERSATION, searchConversations } from "./sidebar_cards";
import {
  WorkspaceBrowser,
  uploadRefusal,
  useAttachmentUploads,
  type ComposerAttachment,
  workflowPendingInteraction,
  workflowProgress,
  type ChatMessage,
} from "@abstractframework/panel-chat";
import {
  buildWorkflowInput,
  isChatAgent,
  type WorkflowDefinition,
} from "./catalog";
import { conversationRestore } from "./conversation_restore";
import {
  fetchWorkflowSchema,
  useWorkspaceCatalog,
} from "./use_workspace_catalog";
import { gateway, gatewayRequest, formatError, newId, csrfHeaders } from "./transport";
import { createCodeSessionPolicyClient } from "./session_policy_client";
import { createWorkflowTransport } from "./session_transport";
import { cancelStartedRun, finishPendingStops, forgetStopWhileStarting, forgetStopsOfConversation, noteStartedRun, rememberStopWhileStarting, type PendingStop } from "./start_stop";
import { selectionFromAccountValue, useAccountWorkflow } from "./account_preferences";
import { AccountWorkflowDefault } from "./account_workflow_default";
import { AgentSettings } from "./agent_settings";
import { SettingsHelp } from "./settings_help";
import {
  SettingsContent,
  DEFAULT_PREFERENCES,
  type RunPreferences,
} from "./settings_panel";
import { WorkspaceFilesContent, WorkspaceActivityContent, AutomationActivity, activityAttention } from "./workspace_panels";
import { CodeRightRail, type CodePanel, type RailPanel, type SettingsRailPanel } from "./right_rail";
import { AutomationBinding, AutomationDefinitionForm, useAutomationSettings } from "./automation_settings_view";
import { proxyGatewayFetch, openRunFolder } from "./session_files";
import { copy_text } from "../lib/clipboard";
import { automationTarget, automationsAvailability, myEmailConsoleUrl } from "./automations";
import {
  AutomationMain,
  AutomationsSection,
  NewAutomationDialog,
  useAutomations,
  type AutomationHost,
} from "./automations_view";
import { needsYou, newResults, useMeEvents } from "./me_events";
import {
  readPreferences,
  writePreferences,
} from "./preferences";
// Kit gap K3 (ui-primitives 0030): the controller does not derive the "streaming unsupported" / malformed-delta notes.
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
  selectionSourceNote,
  gatewayDefaultDefinition,
  CODE_AGENT_INTERFACE,
  gatewayDefaultOptionLabel,
  reconcileSelection,
  resolvedWorkflowNote,
  startRunBody,
  executableChoices,
  pickerValue,
  selectionFromPicker,
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
import { RunStatusBar } from "./run_status_bar";
import { CODE_CAPTURE_DEFAULTS, CodeVoiceSettings, VoiceTools, fetchVoiceDefaults, spokenMessage, useWorkspaceVoice } from "./voice_tools";
import { workflowPromptProperty, restoreWorkflowFields } from "./input_schema";
import { READ_ALOUD_INITIAL, readAloudStep } from "@abstractframework/panel-chat";
import { codeReadAloudInput } from "./read_aloud_input";
import { TOOLS_MIGRATION_NOTICE, migrateLegacyTools, toolsMigrationKey } from "./tool_layers";
import {
  advanceQueueIntents,
  queueIntentMatches,
  queueTerminalDisposition,
  staleSendAbort,
  workspacePrincipalIdentity,
  type QueueIntent,
} from "./app_state";
import { liveConnectionNotice, unreachableBannerText } from "./live_connection";

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

function ConversationAvatar({ voice, active, pending, failed, paused }: { voice: ReturnType<typeof useWorkspaceVoice>; active: boolean; pending: boolean; failed: boolean; paused: boolean }) {
  const microphone = useVoiceLevel(voice.voice_ptt_recording ? voice : null);
  return <ActivityAvatar design="watch-hover" size={320} pose={voice.voice_ptt_recording ? "listening" : voice.tts_speaking ? "speaking" : paused || pending ? "waiting" : active ? "thinking" : failed ? "error" : "ready"} level={voice.voice_ptt_recording ? microphone : 0} label="Your assistant" />;
}

export function CodeWorkspace() {
  const connection = useGatewayConnection({
    appName: "AbstractCode",
    variant: "blocking",
  });
  const identity = connection.connected
    ? workspacePrincipalIdentity(connection.status)
    : "";
  const avatarPreferences = useConversationView(`abstractcode:${identity}`);
  const avatarMode = avatarPreferences.mode === "avatar";
  const [avatarReplyContext, setAvatarReplyContext] = useState<ChatMessage | null>(null);
  const [avatarAttended, setAvatarAttended] = useState(false);
  const onAuthError = useCallback(() => {
    void connection.refresh();
  }, [connection.refresh]);
  const catalog = useWorkspaceCatalog(identity, onAuthError);
  const readScope = sessionReadScope(connection.status);
  const observedReceipts = useMemo(() => Object.values(catalog.sessionActivity || {}).flatMap((a) => a.content), [catalog.sessionActivity]);
  const readClient = useMemo(() => createSessionReadClient((path, init) => fetch(path, { ...init, credentials: "same-origin", headers: { ...init?.headers, ...csrfHeaders() } })), [identity]);
  const sessionReads = useSessionReads(readScope, observedReceipts, readClient);
  const [appearance, setAppearance] = useAppearanceSettings("abstractcode", {
    defaults: { theme: "observer-night" },
  });
  // "My default workspaces" (the account level); a saved change reloads the session panel's default.
  const [defaultWorkspacesOpen, setDefaultWorkspacesOpen] = useState(false);
  const [defaultWorkspacesRevision, setDefaultWorkspacesRevision] = useState(0);
  const bumpDefaultWorkspaces = useCallback(() => setDefaultWorkspacesRevision((n) => n + 1), []);
  // Voice preferences in this browser, per account, shared by the Voice panel and the composer
  // microphone (kit useVoicePreferences: same tab and other tabs).
  const [voicePreferences, changeVoicePreferences] = useVoicePreferences(identity ? `abstractcode.voice:${identity}` : null);
  useEffect(() => {
    setPanelOpen(false);
    setDocsOpen(false);
  }, [identity]);
  const [preferences, setPreferences] =
    useState<RunPreferences>(DEFAULT_PREFERENCES);
  const preferencesRef = useRef(preferences);
  preferencesRef.current = preferences;
  // This app's tool choices (backlog 0033): the kit's tool layer store, per account, in this
  // browser. Auto tools are sent to the gateway with each run (they run without a client open).
  const toolStore = useMemo(() => (identity ? createToolLayerStore(identity) : null), [identity]);
  const [appToolLayer, setAppToolLayerState] = useState<ToolModeLayer>({});
  const [toolsNoticePending, setToolsNoticePending] = useState(false);
  useEffect(() => {
    setAppToolLayerState(toolStore ? toolStore.readApp(CODE_AGENT_INTERFACE) : {});
    let pending = false;
    try {
      pending = Boolean(identity) && localStorage.getItem(toolsMigrationKey(identity)) === "pending";
    } catch {
      /* no storage: no notice */
    }
    setToolsNoticePending(pending);
  }, [toolStore, identity]);
  const setAppToolLayer = useCallback(
    (next: ToolModeLayer) => {
      setAppToolLayerState(next);
      toolStore?.writeApp(CODE_AGENT_INTERFACE, Object.keys(next).length ? next : null);
    },
    [toolStore],
  );
  const dismissToolsNotice = () => {
    setToolsNoticePending(false);
    try {
      localStorage.setItem(toolsMigrationKey(identity), "dismissed");
    } catch {
      /* the notice stays dismissed for this page */
    }
  };
  // "Stream replies" as sent with the next run (nothing when the gateway does
  // not advertise live replies; the setting then says why).
  const streamReplies = effectiveStreamReplies(
    preferences.streamReplies,
    catalog.streaming,
  );
  // "@default" (the gateway default agent workflow) or a catalog workflow id.
  const [selection, setSelection] = useState(DEFAULT_PREFERENCES.workflow);
  // The header picker's choices: exactly what the gateway lists for this app
  // (GET /bundles?executable_for=abstractcode.agent.v1), never widened here.
  const visibleChoices = useMemo(
    () => executableChoices(catalog.executable.data),
    [catalog.executable.data],
  );
  // Round 14: the workflow for NEW conversations is the account's choice kept by the gateway
  // (GET/PUT /accounts/me/preferences, shared with the Assistant and the console); a gateway
  // without that route (older than 0.13.1) keeps it in this browser, as before.
  const clearDeviceWorkflow = useCallback(
    () => setPreferences((previous) => ({ ...previous, workflow: GATEWAY_DEFAULT })),
    [],
  );
  const accountWorkflow = useAccountWorkflow(identity, clearDeviceWorkflow);
  const accountManaged = accountWorkflow.state.status === "ok";
  const defaultSelection =
    accountWorkflow.state.status === "ok"
      ? selectionFromAccountValue(accountWorkflow.state.row.value, visibleChoices)
      : preferences.workflow;
  const defaultSelectionRef = useRef(defaultSelection);
  defaultSelectionRef.current = defaultSelection;
  const defaultWorkflow = useMemo(
    () => gatewayDefaultDefinition(catalog.gatewayDefault, catalog.workflows),
    [catalog.gatewayDefault, catalog.workflows],
  );
  const workflow =
    selection === GATEWAY_DEFAULT
      ? defaultWorkflow
      : catalog.workflows.find((item) => item.id === selection) ||
        visibleChoices.find((item) => item.id === selection) ||
        null;
  // What the gateway said it started, for the run this page started.
  // The last start's iteration receipt (`limits` on the POST /runs/start answer) for "Last run: …".
  const [runLimits, setRunLimits] = useState<IterationLimitApplied | null>(null);
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
  // A Stop pressed while the run was starting: answered at once ("Stopping…" at the top) until the
  // run the gateway returns is stopped (start_stop.ts).
  // `runId` is the run THIS start returned ("" until it answers): "Stopping…" ends only when that
  // run is shown stopped, never on the previous run's state (review 2026-11-08 D5). `text` keeps
  // the message on screen meanwhile (the box was cleared at the press).
  const [stopping, setStopping] = useState<{ pressId: string; runId: string; text: string } | null>(null);
  const stoppingStart = Boolean(stopping);
  // Stops an earlier page of this tab left behind that match more than one run: the person chooses.
  const [stopChoices, setStopChoices] = useState<PendingStop[]>([]);
  // Stops dropped without a match: the conversation says so until dismissed.
  const [stopMisses, setStopMisses] = useState<PendingStop[]>([]);
  const [stopsVersion, setStopsVersion] = useState(0);
  // @ mentions (`mentions.ts`, contract 2026-11-08). `mentionsOff`: this gateway has no mentions
  // route (for this sign-in). `mentionStatus`: the sentence above the composer about the last
  // mentions of this conversation. `followTurn`: the newest mention reply, shown once the current
  // reply has ended (the conversation then opens on it; its earlier turns come before it).
  const [mentionsOff, setMentionsOff] = useState<string | null>(null);
  const [mentionStatus, setMentionStatus] = useState<{ sessionId: string; text: string; answered?: boolean } | null>(null);
  const [mentioning, setMentioning] = useState(false);
  const [followTurn, setFollowTurn] = useState<{ sessionId: string; runId: string; identity: string } | null>(null);
  const refusedEntities = useRef(new Set<string>());
  const entityRoster = useRef<{ identity: string; load: Promise<ChatMentionCandidate[]> } | null>(null);
  // Who is writing each live reply, from its stream frames (by run): the bubble is labelled before
  // the saved message, which carries its own author, replaces it.
  const liveAuthors = useRef(new Map<string, ChatAuthor>());
  // Turns whose prompt repeats another turn's words (`repeat_of`), noted by the transport from the
  // views; the conversation folds such a prompt only while the turn it repeats is shown.
  const repeatTurns = useRef(new Map<string, string>());
  // Turns the agent was not addressed in (mentions routing, operator 2026-11-10): the person's turn
  // the gateway recorded for a message whose chips named entities and not the agent. Learned from
  // the start's answer (`agent: "not_addressed"`) and from the gateway's marker on the turn
  // (`agent_addressed: false` on its run and its view message), so it reads the same live and after
  // a reload: no reply under way, no working dots.
  const unaddressedTurns = useRef(new Set<string>());
  // A send whose mentions stopped partway (a 503, a failure): its words, its `repeat_of` and the
  // mentions not answered. The next send of the same words with chips asks only those (no new turn).
  const pendingResend = useRef<{ sessionId: string; identity: string; epoch: number; message: string; resend: MentionResend } | null>(null);
  const unloadingRef = useRef(false);
  // Workspace files attached by path (Files panel → `attachments_ingest`): already references.
  const [attachments, setAttachments] = useState<AttachmentRef[]>([]);
  const fileInput = useRef<HTMLInputElement>(null);
  const [schema, setSchema] = useState<Record<string, any>>();
  const schemaRef = useRef(schema);
  schemaRef.current = schema;
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
  // The shared rail restores its docked panel. Docs uses the standalone shared
  // drawer, independently of the current workspace panel.
  const [railPanel, setRailPanel] = useState<CodePanel | null>(null);
  const [docsOpen, setDocsOpen] = useState(false);
  const railOverlay = useAfMedia(AF_MEDIA.md);
  // A phone always starts with the content visible: becoming an overlay folds the panel away.
  useEffect(() => {
    if (railOverlay) setRailPanel(null);
  }, [railOverlay]);
  const panelOpen = railPanel !== null;
  /** Legacy "close the panel" call sites: only a FLOATING panel gets out of the way; a docked one stays. */
  const setPanelOpen = (open: boolean) => {
    if (!open && railOverlay) setRailPanel(null);
  };
  const openPanel = (section: RailPanel) => {
    setSidebarOpen(false);
    setRailPanel(section === "workflow" || section === "tools" || section === "skills" ? "agent" : section);
  };
  /** The workflow's inputs live in the Agent panel; "Back to chat" collapses it to the rail. */
  const setInputsOpen = (open: boolean) => {
    if (open) openPanel("agent");
    else if (railPanel === "agent") setRailPanel(null);
  };
  // Conversation navigation becomes a drawer below the tablet breakpoint.
  const panesAreDrawers = useAfMedia(AF_MEDIA.md);
  const belowLg = useAfMedia(AF_MEDIA.lg);
  const paneMode: PaneMode = paneModeFrom(belowLg, panesAreDrawers);
  const paneModeRef = useRef<PaneMode>(paneMode);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  // Collapsible Automations / Conversations panels, remembered per viewer.
  const [panels, togglePanelOpen] = useSidebarPanels();
  // The `Archived · N` footers of both lists: open or closed, remembered per viewer.
  const [archivedOpen, toggleArchivedOpen] = useArchivedOpen();
  // Automations: the sidebar section and, for the selected one, the main view.
  const automationsAvailable = automationsAvailability(catalog.capabilities);
  // The person's event stream (backlog 1047): the badge and the list refresh follow it.
  const meEvents = useMeEvents(identity, connection.connected);
  const eventsFollowing = meEvents.state === "following";
  useEffect(() => {
    if (!meEvents.fold.cursor) return;
    const timer = setTimeout(() => { void catalog.refreshSessions(); void sessionReads.refresh(); }, 500);
    return () => clearTimeout(timer);
  }, [meEvents.fold.cursor, catalog.refreshSessions, sessionReads.refresh]);
  const { ctl: automations, state: automationsState, nowMs: automationNowMs } = useAutomations(
    identity,
    connection.connected && automationsAvailable.available,
    { following: eventsFollowing, epoch: meEvents.fold.automationEpoch },
  );
  // The automations' `Archived · N` footer lists the archived ones once opened (the gateway lists them only when asked).
  useEffect(() => {
    if (archivedOpen.automations && automationsState.loaded && automationsState.archivedCount > 0 && automationsState.archived === null)
      void automations.loadArchived();
  }, [archivedOpen.automations, automationsState.loaded, automationsState.archivedCount, automationsState.archived, automations]);
  const [automationView, setAutomationView] = useState(false);
  // ADR 0048: the Paused section of the automations list, closed at first.
  const [pausedOpen, setPausedOpen] = useState(false);
  const [newAutomationOpen, setNewAutomationOpen] = useState(false);
  const switchNotice = useRef("");
  const [search, setSearch] = useState("");
  const searchInput = useRef<HTMLInputElement>(null);
  const [queue, setQueue] = useState<QueueIntent[]>([]);
  const [queueRunning, setQueueRunning] = useState(false);
  const [optimistic, setOptimistic] = useState<ChatMessage[]>([]);
  // The one-time migration of the Permissions tier / tool picker into this app's tool layer
  // (decision 2026-10-27): labeled, never silent — the Agent panel says so until dismissed.
  useEffect(() => {
    const legacy = preferences.legacyTools;
    if (!legacy || !toolStore || catalog.loading || !catalog.toolCatalog.length) return;
    if (!Object.keys(toolStore.readApp(CODE_AGENT_INTERFACE)).length) {
      const migrated = migrateLegacyTools(legacy, catalog.toolCatalog);
      if (Object.keys(migrated).length) {
        setAppToolLayer(migrated);
        setToolsNoticePending(true);
        try {
          localStorage.setItem(toolsMigrationKey(identity), "pending");
        } catch {
          /* shown for this page only */
        }
        setNotice(`${TOOLS_MIGRATION_NOTICE} in Tools.`);
      }
    }
    setPreferences(({ legacyTools: _done, ...rest }) => rest);
  }, [preferences.legacyTools, toolStore, catalog.loading, catalog.toolCatalog, identity, setAppToolLayer]);
  // The input this page itself posted for the run it just started. Until the
  // Gateway's filtered copy (`/input_data`) arrives, it scopes the approver,
  // so a first batch that lands inside that fetch is not shown as a question.
  // The Gateway copy replaces it as soon as it loads (it can only narrow).
  const startedRunInput = useRef<{ runId: string; input: Record<string, unknown> } | null>(null);
  const currentToolPermissions = useMemo(() => {
    const ownStart = startedRunInput.current?.runId === session.runId ? startedRunInput.current.input : null;
    if (session.runId && !restoredInputs && !ownStart) return { enabledTools: [], autoApproveTools: [] };
    const runInput = restoredInputs ? restoredInputs.input_data : ownStart;
    // The run's own ceiling (its `_runtime.allowed_tools`) narrows what this client approves for it.
    const ceiling = (runInput as any)?._runtime?.allowed_tools;
    return toWorkflowApprovalPolicy(planRunTools(catalog.toolCatalog, { app: appToolLayer }, { ceiling: Array.isArray(ceiling) ? ceiling : null }));
  }, [catalog.toolCatalog, appToolLayer, restoredInputs, session.runId]);
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
        onDeltaAuthor: (runId, author) => {
          liveAuthors.current.set(runId, author);
        },
        repeats: repeatTurns.current,
        unaddressed: unaddressedTurns.current,
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
  // The approval card's Allow (this tool) and Allow All (every tool) last for this session only,
  // stored on the gateway when it serves the session tool policy
  // (capabilities.session_tool_policy.supported; contract v1): the kit client is bound to this
  // conversation's session. Otherwise Allow covers the shown request and the card says so.
  const sessionPolicyCapability = catalog.sessionToolPolicy;
  useEffect(() => {
    if (!connection.connected || catalog.capabilitiesLoading) return;
    if (catalog.capabilitiesError) {
      controller.setSessionToolPolicyClient(null, { reason: catalog.capabilitiesError });
      return;
    }
    controller.setSessionToolPolicyClient(
      sessionPolicyCapability?.supported === true && session.sessionId
        ? createCodeSessionPolicyClient(session.sessionId, sessionPolicyCapability)
        : null,
    );
  }, [controller, connection.connected, catalog.capabilitiesLoading, catalog.capabilitiesError, sessionPolicyCapability, session.sessionId]);
  useEffect(() => setAvatarReplyContext(null), [identity, session.sessionId]);
  // The open turn is one the agent was not addressed in: nothing is under way there, whatever
  // status its record carries (no spinner, no working dots, the conversation can move on).
  const unaddressedTurn =
    Boolean(session.runId) &&
    (unaddressedTurns.current.has(session.runId) ||
      (String(snapshot.run?.run_id || "") === session.runId && agentNotAddressed(snapshot.run)));
  const active =
    Boolean(session.runId) &&
    !unaddressedTurn &&
    !terminal(snapshot.status) &&
    snapshot.status !== "idle";
  const paused = snapshot.run?.paused === true || snapshot.status === "paused";
  const locked = active || sending;
  const recordedMessages =
    snapshot.loading && optimistic.length ? optimistic : snapshot.messages;
  // A message stopped while its run was starting stays on screen until that run is shown.
  const baseMessages =
    stopping?.text && (!stopping.runId || session.runId !== stopping.runId)
      ? [...recordedMessages, { id: `stopping:${stopping.pressId}`, role: "user", content: stopping.text }]
      : recordedMessages;
  const lastBase = baseMessages[baseMessages.length - 1];
  lastMessageIdRef.current = lastBase?.id ? String(lastBase.id) : null;
  const messages = mergeStreamNotes(withLiveAuthors(foldRepeatedTurns(baseMessages, repeatTurns.current), liveAuthors.current), [
    streamingUnsupportedNote(preferences.streamReplies, catalog.streaming, baseMessages),
    ...(streamNotes.scope === streamScope ? streamNotes.notes : []),
  ]);
  const voiceCapability = catalog.capabilities?.assistant?.voice || {};
  const voiceDefaults = useVoiceDefaults(connection.connected ? fetchVoiceDefaults : null, identity);
  const voice = useWorkspaceVoice({
    scope: `${identity}:${session.sessionId}:${session.runId}:${automationView}`,
    runId: identity ? session.runId : "",
    sessionId: session.sessionId,
    capability: voiceCapability,
    preferences: voicePreferences,
    // "Send what you say automatically" (Code's default: on): send it, else add it to the draft.
    onTranscript: (text) => {
      if ((voicePreferences.auto_send ?? CODE_CAPTURE_DEFAULTS.auto_send) === false) setDraft((d) => spokenMessage(d, text));
      else void sendSpoken(text);
    },
    onError: setError,
  });

  // Talking in the Avatar view stops a reply that is being read aloud first, so the assistant
  // never talks over you into the microphone (0.15.0's Talk button did this; the composer
  // microphone replaced it in 0.15.1).
  useEffect(() => {
    if (avatarMode && voice.voice_ptt_recording) voice.stop_tts();
  }, [avatarMode, voice.voice_ptt_recording, voice.stop_tts]);
  // Read aloud (the Voice panel): a reply is spoken by itself only when it was
  // generated while this conversation is open — its run was seen in progress
  // here (operator 2026-10-29: opening, reloading or switching to a session
  // spoke its existing messages). The kit's rule (`read_aloud.ts`).
  const latestReply = [...messages].reverse().find((message) => message.role === "assistant");
  const latestReplyId = latestReply ? String(latestReply.id || latestReply.content) : "";
  const readAloudState = useRef(READ_ALOUD_INITIAL);
  const readAloud = codeReadAloudInput({
    identity,
    sessionId: session.sessionId,
    runId: session.runId,
    status: snapshot.status,
    sending,
    reply: latestReply?.content ? { id: latestReplyId, text: String(latestReply.content) } : null,
  });
  useEffect(() => {
    const { state, speak } = readAloudStep(readAloudState.current, readAloud);
    readAloudState.current = state;
    if (speak && voicePreferences.read_aloud === true && voice.tts_supported && (!avatarMode || (!avatarAttended && !draft.trim() && !voice.voice_ptt_recording))) void voice.toggle_tts(speak.id, speak.text);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [readAloud.scope, readAloud.phase, readAloud.reply?.id, voicePreferences.read_aloud, voice.tts_supported]);
  // A tool batch this client already granted (the standing permission, or an
  // accepted Allow) is running work, not a question: no card, no "Approval
  // needed". The server still asks, so the raw wait stays in the snapshot.
  const pendingInteraction = workflowPendingInteraction(snapshot);
  // The assistant is working on a reply: the run's own state (the run strip's "working" tone —
  // a model call, a tool, a step), or a send still starting. Never while it waits on the person,
  // a timer or an event, nor while paused. WorkflowChat shows the working dots for it.
  const progressTone = useMemo(() => workflowProgress(snapshot).tone, [snapshot]);
  const working = !paused && (sending || (active && progressTone === "working"));
  const interaction = useMemo(
    () =>
      // Deny / Allow (this tool) / Allow All (every tool), the two allows for the rest of this
      // session only (operator ruling 2026-11-07): the card never writes the app's tool defaults.
      presentInteraction(pendingInteraction, controller, {
        records: snapshot.records,
        currentRun: snapshot.run,
        sessionPolicy: true,
      }),
    // The session-policy mode is the controller's: re-present when it (or what it stored) changes.
    [pendingInteraction, snapshot.records, snapshot.run, snapshot.sessionPolicyMode, snapshot.sessionPolicy, snapshot.sessionPolicyClamped, controller],
  );
  const promptProperty = isAgent
    ? "prompt"
    : workflowPromptProperty(schema);
  const chatReady = isAgent || Boolean(promptProperty && !schema?.required?.some((name: string) => name !== promptProperty));
  const effectiveWorkspace = String(
    restoredInputs?.workspace?.workspace_root ||
      (snapshot.run?.vars as any)?.workspace_root ||
      "",
  );
  // The opened conversation's workflow, what is said about it, and whether it can continue
  // (`conversation_restore.ts`: Notes' background work reads only; a superseded version continues).
  const restore = useMemo(
    () =>
      conversationRestore({
        sessionId: session.sessionId,
        runId: session.runId,
        restoredInputs,
        run: snapshot.run,
        workflows: catalog.workflows,
        conversationSessions: catalog.sessions,
        notesSessions: catalog.notesSessions,
        catalogLoading: catalog.loading,
      }),
    [session.sessionId, session.runId, restoredInputs, snapshot.run, catalog.workflows, catalog.sessions, catalog.notesSessions, catalog.loading],
  );
  const notesSession = restore.notesSession;
  const restoredWorkflow = restore.workflow;
  const restoredVersionNote = restore.versionNote;
  const restoreSelectionError = restore.error;
  const currentSession =
    catalog.sessions.find((item) => item.sessionId === session.sessionId) ??
    catalog.notesSessions.find((item) => item.sessionId === session.sessionId);
  const title =
    currentSession?.prompt ||
    messages.find((m) => m.role === "user")?.content ||
    (session.runId ? UNTITLED_CONVERSATION : "New conversation");
  // "Email isn't set up — open My email": the gateway console's Users tab,
  // in a new tab (the console has its own sign-in).
  const myEmailUrl = myEmailConsoleUrl(connection.status?.gateway_url);
  const openMyEmail = myEmailUrl
    ? () => {
        window.open(myEmailUrl, "_blank", "noopener,noreferrer");
      }
    : undefined;
  // Automations open gateway sessions here: a Discuss fork becomes THIS
  // app's conversation (one session pool for every client).
  const automationHost: Omit<AutomationHost, "openWorkspace"> = {
    ...(openMyEmail ? { openMyEmail } : {}),
    openConversation: (sessionId, runId, text) => {
      switchNotice.current = text || "";
      openConversation(sessionId, runId);
    },
    openRun: (runId) => {
      void gatewayRequest<{ session_id?: string | null }>(
        gatewayApiPath(`runs/${encodeURIComponent(runId)}`),
      )
        .then((run) => {
          if (!run?.session_id)
            throw new Error(`The gateway reported no session for run ${runId}.`);
          openConversation(run.session_id, runId);
        })
        .catch((e) => automations.reportError(e));
    },
  };
  /**
   * The automation header's Edit (W3) and any other "edit this automation":
   * show that automation and open the rail's Workflow panel bound to its
   * definition (task, schedule, workflow; a change saves a new revision).
   */
  const openAutomationSettings = (automationId: string) => {
    if (!automationView || automationsState.selectedId !== automationId) {
      setAutomationView(true);
      void automations.select(automationId);
    }
    setSidebarOpen(false);
    setRailPanel("agent");
  };
  /** The automation header's "Change workspaces": the same, on the rail's Workspace panel (its chooser). */
  const openAutomationWorkspaces = (automationId: string) => {
    openAutomationSettings(automationId);
    setRailPanel("workspace");
  };
  /** A new automation target is checked against that workflow's input schema before a revision is sent. */
  const prepareAutomationTargetForCode = async (target: AutomationTarget): Promise<AutomationTarget> => {
    const next = target.flow_id === "@default" ? defaultWorkflow : catalog.workflows.find(w => `${w.bundleId}@${w.bundleVersion}` === ("bundle_ref" in target ? target.bundle_ref : "") && w.flowId === target.flow_id);
    if (!next) throw new Error("The selected workflow is no longer available. Refresh the workflow list.");
    const nextSchema = await fetchWorkflowSchema(next);
    if (!nextSchema) throw new Error("Workflow inputs could not be checked. Refresh the workflow list and try again.");
    const input = { ...schemaDefaults(nextSchema), ...target.input_data };
    const problems = validateWorkflowInputs(nextSchema, input);
    if (problems.length) throw new Error(`${problems.join(" ")} Configure a new automation from that workflow's input form, or choose a compatible workflow.`);
    return { ...target, input_data: input as typeof target.input_data };
  };
  const automationTitle =
    automationsState.detail?.summary.title || "Automation";
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
  // Files picked, dropped or pasted: the kit's upload queue (panel-chat useAttachmentUploads,
  // backlog 0030). Each becomes a chip at once; uploads run into the conversation's session
  // attachment store; the gateway's maxAttachmentBytes is the only size rule (ADR-0026). The
  // queue is cleared on every session/account change, so a late upload never lands elsewhere.
  const uploads = useAttachmentUploads<AttachmentRef>({
    upload: (file) => gateway.attachments_upload(sessionRef.current.sessionId, file),
    maxBytes: catalog.policy?.maxAttachmentBytes,
  });
  /** What the next run carries: workspace files attached by path, then uploaded files. */
  const runAttachments = [...attachments, ...uploads.attached];
  /** The composer's chips: attached workspace files (references), then the upload queue. */
  const attachmentItems: ComposerAttachment[] = [
    ...attachments.map((ref, index) => ({
      id: `ref:${index}:${ref.$artifact}`,
      name: ref.filename || ref.path || "Attachment",
      ...(typeof ref.size_bytes === "number" ? { size: ref.size_bytes } : {}),
      state: "attached" as const,
    })),
    ...uploads.items,
  ];
  const removeAttachmentItem = (id: string) => {
    if (!id.startsWith("ref:")) return uploads.remove(id);
    const index = Number(id.split(":")[1]);
    setAttachments((items) => items.filter((_, n) => n !== index));
  };
  /** Queue files (picked, dropped, pasted, or read from the workspace); returns each file's chip state. */
  function attachUploads(files: File[]): Pick<ComposerAttachment, "name" | "state" | "error">[] {
    uploads.add(files);
    return files.map((file) => {
      const refusal = uploadRefusal(file, catalog.policy?.maxAttachmentBytes);
      return { name: file.name, state: refusal ? "refused" : "queued", ...(refusal ? { error: refusal } : {}) };
    });
  }

  useEffect(() => {
    sendLock.current = false;
    setSending(false);
    uploads.clear();
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
      preferred: defaultSelection,
      visible: visibleChoices,
      workflows: catalog.workflows,
      runId: session.runId,
    });
    if (next !== selection) setSelection(next);
  }, [catalog.loading, catalog.workflows, visibleChoices, selection, defaultSelection, session.runId]);
  // A fresh conversation (no run yet) starts on the account's default once the gateway's answer
  // and the workflow list are in, and follows a change of that default.
  const accountDefaultKey =
    accountWorkflow.state.status === "ok"
      ? `${identity}|${accountWorkflow.state.row.value ?? ""}|${visibleChoices.length}`
      : "";
  useEffect(() => {
    if (!accountDefaultKey || session.runId || !visibleChoices.length) return;
    setSelection(defaultSelectionRef.current);
  }, [accountDefaultKey]);
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
    const loaded = session.runId;
    const inputsOf = (runId: string) =>
      gatewayRequest(gatewayApiPath(`runs/${encodeURIComponent(runId)}/input_data`), { signal: abort.signal });
    void inputsOf(loaded)
      .then(async (data) => {
        // An entity's reply to an @mention, or the person's turn the agent was not addressed in, is
        // a turn of the conversation, not its workflow: the conversation continues with its newest
        // agent turn's (`mentions.ts` isSideTurn). Only such turns: none is restored
        // (conversation_restore.ts says nothing is gone).
        if (!isSideTurn(data)) return data;
        const bundle = await gatewayRequest<any>(
          gatewayApiPath(`runs/${encodeURIComponent(loaded)}/history_bundle?include_session=true&include_subruns=false&ledger_max_items=1&detail=replay&session_turn_limit=201`),
          { signal: abort.signal },
        );
        const turn = conversationWorkflowTurn(bundle?.session?.turns, loaded);
        return turn ? inputsOf(turn) : data;
      })
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
    uploads.clear();
    setAttachments([]);
    // A conversation's form values belong to it: a new or other conversation
    // starts from the schema defaults, never from the fields restored out of
    // the previous conversation's run (its context carried its attachments).
    setInputs(schemaDefaults(schemaRef.current));
    setOptimistic([]);
    // A notice raised by the switch itself (a Discuss fork) survives it.
    setNotice(switchNotice.current);
    switchNotice.current = "";
    setError("");
    setQueue([]);
    setQueueRunning(false);
  }, [session.sessionId]);
  useEffect(() => {
    const update = () => {
      const next = route();
      sendLock.current = false;
      setSending(false);
      uploads.clear();
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
    if (!session.runId || !restoredInputs || notesSession) return;
    const next = conversationSelection({ restoredInputs, restored: restoredWorkflow });
    if (next) setSelection(next);
  }, [session.runId, restoredInputs, restoredWorkflow?.id, notesSession]);
  const sourceNote = session.runId && !isSideTurn(restoredInputs) ? selectionSourceNote(restoredInputs) : "";

  const newConversation = useCallback(() => {
    const next = { sessionId: newId(), runId: "" };
    setSession(next);
    setSelection(defaultSelectionRef.current);
    writeRoute(next.sessionId);
    setDraft("");
    setQueue([]);
    setQueueRunning(false);
    setSidebarOpen(false);
    setAutomationView(false);
  }, []);
  /** Open a gateway session as the conversation (sidebar, a discussion fork, a run). */
  const openConversation = useCallback((sessionId: string, runId: string) => {
    sendLock.current = false;
    setSending(false);
    uploads.clear();
    setSession({ sessionId, runId });
    writeRoute(sessionId, runId);
    setSidebarOpen(false);
    setQueue([]);
    setQueueRunning(false);
    setDraft("");
    setAutomationView(false);
  }, []);
  // Close the mobile conversation navigation when returning to a docked layout.
  useEffect(() => {
    const from = paneModeRef.current;
    paneModeRef.current = paneMode;
    if (from === paneMode) return;
    setSidebarOpen((open) => sidebarOnModeChange(open, paneMode));
  }, [paneMode]);
  useDrawerFocus(
    sidebarOpen,
    panesAreDrawers,
    () => document.querySelector<HTMLElement>(".code-sidebar"),
    () => document.querySelector<HTMLElement>(".code-mobile-close"),
  );
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      // One Escape closes one layer: a dialog or kit drawer above the panes owns it.
      const target = escapeTarget({
        key: event.key,
        defaultPrevented: event.defaultPrevented,
        overlayOpen:
          panelOpen || newAutomationOpen || kitDialogOpen(),
        sidebarOpen,
        inspectorOpen: false,
        inspectorIsOverlay: false,
      });
      if (target === "sidebar") {
        setSidebarOpen(false);
        document.querySelector<HTMLButtonElement>(".code-mobile-nav")?.focus();
        return;
      }
      if (!(event.metaKey || event.ctrlKey)) return;
      if (event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPanelOpen(false);
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
  }, [
    newConversation,
    sidebarOpen,
    newAutomationOpen,
    panelOpen,
  ]);

  // Reasoning and MTP reach a run only when the capability card of the model THAT run uses allows
  // them: the input's own provider/model (Settings choice, else the target workflow's pins), else
  // the gateway default's model.
  const guardedWorkflowInput = (build: (modelOptions: ModelOptions) => ReturnType<typeof currentWorkflowInput>) =>
    guardedRunInput(build, preferences, catalog.defaultModel || undefined);
  function currentWorkflowInput(text: string, values: Record<string, unknown>, attachments: AttachmentRef[] = [], forAutomation = false, selectedWorkflow = workflow, selectedSchema = schema, modelOptions: ModelOptions = {}) {
    let input = buildWorkflowInput({
      workflow: selectedWorkflow || undefined,
      inputSchema: selectedSchema,
      schemaInputs: values,
      prompt: text,
      promptProperty,
      model: preferences,
      reasoning: modelOptions.reasoning || undefined,
      speculation: modelOptions.speculation,
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
      // Round 9: a turn sends only its conversation folder (continuity); the
      // gateway scopes the run to the account's effective folders.
      workspace: effectiveWorkspace ? { root: effectiveWorkspace } : undefined,
      skills: preferences.skills.length ? preferences.skills : undefined,
    });
    // This app's tool choices (kit planRunTools/withRunTools, backlog 0033): Auto tools in
    // `_runtime.tool_policy.auto_approve_tools` (they run without a client open), Ask tools in
    // `require_approval_tools`, Off tools outside `allowed_tools`. No choice: nothing is sent and
    // the gateway's grant applies.
    if (hasToolChoices(catalog.toolCatalog, { app: appToolLayer }))
      input = withRunTools(input, planRunTools(catalog.toolCatalog, { app: appToolLayer })) as typeof input;
    if (forAutomation) {
      // Automations own their workspace and consent policy; never inherit a
      // conversation's directory or its client-dependent approval policy.
      delete input.workspace_root;
      delete input.workspace_access_mode;
      delete input.workspace_allowed_paths;
      delete input.workspace;
      if (input._runtime && typeof input._runtime === "object") delete (input._runtime as Record<string, unknown>).tool_policy;
    }
    return input;
  }

  // `mentions`: the names picked with the words. They ride the start (the agent's chip as "agent"):
  // entities without the agent start no agent run (`agent: "not_addressed"`: the gateway recorded the
  // person's turn, `turn` below, which the mentions repeat); with the agent, the gateway tells it the
  // mentioned entities answer for themselves (it may say nothing: no bubble shows).
  async function startTurn(text: string, signal?: AbortSignal, mentions: readonly ChatMentionSent[] = []): Promise<StartedTurn> {
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
    if (restore.readOnlyNotice) throw new Error(restore.readOnlyNotice);
    if (restoreSelectionError) throw new Error(restoreSelectionError);
    if (schemaLoading || schemaError)
      throw new Error(schemaError || "Wait for workflow inputs to load.");
    if (inputEditorError) {
      setInputsOpen(true);
      throw new Error(inputEditorError);
    }
    if (uploads.sendBlock) throw new Error(uploads.sendBlock);
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
      : withWorkflowAttachments(schema, promptValues, runAttachments);
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
      const input = await guardedWorkflowInput(options => currentWorkflowInput(text, values, runAttachments, false, workflow, schema, options));
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
        mentions: startRunMentions(mentions),
      });
      if (avatarMode) Object.assign(body, { conversation: true });
      // Stop pressed before the start left: nothing is started, nothing to cancel.
      if (signal?.aborted) throw new DOMException("Stopped before the run started.", "AbortError");
      // Sending again here: a stop an earlier page left for this conversation no longer applies,
      // and is dropped BEFORE this start leaves, so its run can never be taken for that stop's.
      forgetStopsOfConversation(startedSession);
      const sentAt = Date.now();
      // Stop pressed while the gateway is starting the run (review 2026-11-08 D1, D3, R12): the
      // press is answered now ("Stopping…" at the top, the message box cleared and free) and kept
      // in this tab until the run the gateway returns is cancelled, even if the person reloads or
      // opens another conversation meanwhile.
      let pendingStop = "";
      const sameConversation = () =>
        sessionRef.current.sessionId === startedSession &&
        identityRef.current === startedIdentity &&
        authEpoch.current === startedEpoch;
      const onStop = () => {
        pendingStop = rememberStopWhileStarting(startedSession, sentAt);
        if (!sameConversation()) return;
        setStopping({ pressId: pendingStop, runId: "", text: text.trim() });
        setDraft("");
      };
      const endStopping = () => setStopping((now) => (now?.pressId === pendingStop ? null : now));
      signal?.addEventListener("abort", onStop, { once: true });
      const starting = gatewayRequest<any>(gatewayApiPath("runs/start"), {
        method: "POST",
        body: JSON.stringify(body),
      });
      void starting.then(
        (answer) => {
          if (!signal?.aborted) return;
          // A message the agent was not addressed in started no run: nothing to stop.
          const runId = answer?.agent === "not_addressed" ? "" : String(answer?.run_id || "");
          if (!runId) {
            forgetStopWhileStarting(pendingStop);
            if (sameConversation()) endStopping();
            return;
          }
          noteStartedRun(pendingStop, runId);
          void cancelStartedRun(runId).then(
            () => forgetStopWhileStarting(pendingStop),
            (e) => {
              setStopsVersion((v) => v + 1); // kept: the tab's retry loop tries again
              if (sameConversation()) { endStopping(); setError(`The run could not be stopped: ${formatError(e)}`); }
            },
          );
          // Still on this conversation: show the run, which the gateway now stops ("Stopped").
          if (sameConversation()) {
            setStopping((now) => (now?.pressId === pendingStop ? { ...now, runId } : now));
            setSession({ sessionId: startedSession, runId });
            writeRoute(startedSession, runId);
          }
        },
        (e) => {
          if (!signal?.aborted) return;
          // A reload cut the start: the stop stays remembered for the next page (finishPendingStops).
          if (unloadingRef.current) return;
          forgetStopWhileStarting(pendingStop);
          if (!sameConversation()) return;
          endStopping();
          // The gateway refused the start: nothing was started. The network cut it: Code cannot
          // tell whether a run began, and says so instead of guessing later.
          if (typeof (e as { status?: unknown })?.status !== "number")
            setError("The gateway did not answer the start, so Code cannot tell whether a reply began. If one appears in this conversation, stop it.");
        },
      );
      const result = signal
        ? await Promise.race([
            starting,
            new Promise<never>((_, reject) =>
              signal.addEventListener("abort", () => reject(new DOMException("Stopped while the run was starting.", "AbortError")), { once: true }),
            ),
          ])
        : await starting;
      signal?.removeEventListener("abort", onStop);
      // A started run needs its id; a message the agent was not addressed in needs its recorded
      // turn (`turn_id`): either missing throws.
      const started = startedTurn(result);
      // The conversation may already show this new run (it follows its session's newest run on a
      // slow start): that is this start, not a change of conversation.
      if (
        sessionRef.current.sessionId !== startedSession ||
        (sessionRef.current.runId !== startedRun && sessionRef.current.runId !== started.runId) ||
        identityRef.current !== startedIdentity ||
        authEpoch.current !== startedEpoch
      )
        throw staleSendAbort();
      if (!started.agentAddressed) {
        // No agent run: the conversation opens on the person's recorded turn when the gateway names
        // its run (finished: no spinner, no answer to wait for), else stays where it is; the
        // entities' answers follow it. The last run's workflow note and limits stay as they were.
        if (started.runId) unaddressedTurns.current.add(started.runId);
        attachedRun = started.runId || startedRun;
        if (started.runId) {
          setOptimistic([...messages, ...(text.trim() ? [{ id: `pending:${started.runId}`, role: "user", content: text.trim() }] : [])]);
          setSession({ sessionId: startedSession, runId: started.runId });
          writeRoute(startedSession, started.runId);
        }
        setDraft("");
        setAttachments([]);
        uploads.clear();
        setInputsOpen(false);
        return started;
      }
      attachedRun = started.runId;
      startedRunInput.current = { runId: attachedRun, input };
      // "@default" resolves at every start: refresh the label from each one.
      setResolvedNote({
        runId: attachedRun,
        ...resolvedWorkflowNote(result.resolved_workflow),
      });
      setRunLimits(runLimitsReceipt(result.limits));
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
      uploads.clear();
      setInputsOpen(false);
      if (result.runner_warning) setNotice(String(result.runner_warning));
      void catalog.refresh();
      return started;
    } finally {
      if (
        sessionRef.current.sessionId === startedSession &&
        (sessionRef.current.runId === startedRun ||
          sessionRef.current.runId === attachedRun ||
          Boolean(signal?.aborted)) &&
        identityRef.current === startedIdentity &&
        authEpoch.current === startedEpoch
      ) {
        sendLock.current = false;
        setSending(false);
      }
    }
  }

  // @ mentions: the names the `@` list offers, the conversation's agent and then `GET /entities`
  // (read again at each new `@`, the result kept for the letters typed after it); entities the
  // gateway refused are left out.
  const agentLabel = workflow?.name || "";
  const mentionCandidates = useCallback(
    async (query: string): Promise<ChatMentionCandidate[]> => {
      if (!entityRoster.current || entityRoster.current.identity !== identity || !query) {
        const load = gatewayRequest(gatewayApiPath("entities")).then(entityCandidates);
        entityRoster.current = { identity, load };
        load.catch(() => {
          if (entityRoster.current?.load === load) entityRoster.current = null;
        });
      }
      // The conversation's agent first, named as its workflow is (`agent` on the start).
      return filterMentionCandidates(mentionRoster(agentLabel, await entityRoster.current.load), query, refusedEntities.current);
    },
    [identity, agentLabel],
  );
  useEffect(() => {
    entityRoster.current = null;
    refusedEntities.current = new Set();
    liveAuthors.current = new Map();
    setMentionsOff(null);
    setFollowTurn(null);
  }, [identity]);

  /**
   * Ask the picked entities, in order, once the person's message is in the conversation (its turn
   * started, its guidance accepted, or its queued turn started). 409/404: said, the next one is
   * asked. 503 or any other failure: said, and the rest is not asked; `keepDraft` puts the message
   * back in the composer and the send rejects naming the chips NOT answered (the kit keeps only
   * those). `resendable`: that message is then remembered, so sending it again asks only those
   * mentions with the same `repeat_of` (`resendMentions`). `message`: a resend's key, so its mentions
   * name the `mention_id`s they were first sent with (the gateway answers each once); a new message
   * takes `mentionMessageKey(repeatOf)`. Each reply is a session turn the gateway recorded; the
   * conversation opens on the newest once the current reply has ended.
   */
  async function askMentions(sessionId: string, text: string, picked: readonly ChatMentionSent[], options: { repeatOf: string; message?: string; signal?: AbortSignal; keepDraft: boolean; resendable?: boolean; agentAddressed?: boolean }) {
    // The agent's chip rode the start; only the entities are asked here.
    const mentions = entityMentions(picked);
    if (!mentions.length) return;
    const message = options.message || mentionMessageKey(options.repeatOf);
    const askedBy = identityRef.current;
    const epoch = authEpoch.current;
    const here = () => sessionRef.current.sessionId === sessionId && identityRef.current === askedBy && authEpoch.current === epoch;
    setMentioning(true);
    const outcome = await sendMentions(
      mentions,
      (mention) =>
        gatewayRequest(gatewayApiPath(`sessions/${encodeURIComponent(sessionId)}/mentions`), {
          method: "POST",
          body: JSON.stringify(mentionRequestBody(text, mention, options.repeatOf, message)),
        }),
      {
        signal: options.signal,
        onAsking: (mention) => { if (here()) setMentionStatus({ sessionId, text: `${mention.name} is answering…` }); },
      },
    ).finally(() => setMentioning(false));
    if (identityRef.current !== askedBy || authEpoch.current !== epoch) return;
    for (const skipped of outcome.skipped) if (skipped.refusal.kind === "not_offerable") refusedEntities.current.add(skipped.mention.id);
    if (outcome.stopped?.refusal.kind === "unavailable") setMentionsOff(askedBy);
    const last = outcome.answered[outcome.answered.length - 1];
    if (last?.runId) setFollowTurn({ sessionId, runId: last.runId, identity: askedBy });
    // No agent run (it was not addressed): no reply to wait for before the answers show.
    const summary = mentionsSummary(outcome, { replyStillRunning: Boolean(last?.runId) && options.agentAddressed !== false });
    setMentionStatus(summary ? { sessionId, text: summary, answered: Boolean(last?.runId) } : null);
    const stop = outcome.stopped?.refusal;
    if (stop && stop.kind !== "unavailable") {
      if (options.keepDraft && here()) setDraft(text);
      if (options.resendable && outcome.unsent.length)
        pendingResend.current = { sessionId, identity: askedBy, epoch, message, resend: { text, repeatOf: options.repeatOf || null, unsent: outcome.unsent } };
      throw mentionsStoppedError(stop.sentence, outcome.unsent);
    }
  }
  /**
   * A resend of the message whose mentions stopped partway (`pendingResend`: the same words, with
   * chips): ONLY those mentions are asked, with the same `repeat_of` — never a new turn of the
   * person, never an entity that already answered. False (and the remembered message is dropped)
   * for any other send: that one is a new message.
   */
  async function resendMentions(text: string, mentions: readonly ChatMentionSent[], signal?: AbortSignal): Promise<boolean> {
    const held = pendingResend.current;
    pendingResend.current = null;
    if (!held || held.sessionId !== session.sessionId || held.identity !== identityRef.current || held.epoch !== authEpoch.current) return false;
    const asked = mentionResend(held.resend, text, mentions);
    if (!asked) return false;
    await askMentions(held.sessionId, held.resend.text, asked, { repeatOf: held.resend.repeatOf ?? "", message: held.message, signal, keepDraft: true, resendable: true });
    if (sessionRef.current.sessionId === held.sessionId) setDraft("");
    return true;
  }
  // The newest mention reply becomes the conversation's open turn once nothing else is under way
  // there: the current reply has ended, no send or mention is in flight and no turn is queued
  // (a queued turn opens on its own, with the replies before it).
  useEffect(() => {
    if (!followTurn) return;
    const done = () => {
      setFollowTurn(null);
      setMentionStatus((now) => (now?.answered && now.sessionId === followTurn.sessionId ? null : now));
    };
    if (followTurn.sessionId !== session.sessionId || followTurn.identity !== identity || queue.length || session.runId === followTurn.runId) {
      done();
      return;
    }
    if (sending || mentioning || !(terminal(snapshot.status) || unaddressedTurn) || String(snapshot.run?.run_id || "") !== session.runId) return;
    done();
    setSession({ sessionId: followTurn.sessionId, runId: followTurn.runId });
    writeRoute(followTurn.sessionId, followTurn.runId);
  }, [followTurn, session.sessionId, session.runId, identity, queue.length, sending, mentioning, snapshot.status, snapshot.run?.run_id, unaddressedTurn]);

  // Press to talk (like the terminal's Space): releasing the microphone SENDS
  // what was said, after anything already typed. While a start is in flight,
  // or when the send fails, the words stay in the draft — never lost.
  async function sendSpoken(transcript: string) {
    const message = spokenMessage(draft, transcript);
    if (sending) {
      setDraft(message);
      return;
    }
    try {
      await send(message);
    } catch (e) {
      setDraft(message);
      setError(formatError(e));
    }
  }

  // While a run is busy the composer steers it or queues the next turn (kit WorkflowChat `steer`,
  // backlog 0030): "Guide this run" posts inject_guidance; "Queue next turn" holds the message
  // here and starts it when the run ends. A pending question/approval leaves only Queue.
  async function guideRun(text: string): Promise<{ accepted: boolean; seq?: number; duplicate?: boolean }> {
    if (avatarMode && interaction?.kind === "ask-user") {
      await interaction.onSubmit(text);
      setAvatarReplyContext(null);
      return { accepted: true };
    }
    if (interaction) throw new Error(avatarMode ? "Please use the request controls. A message does not grant permission." : "Answer the pending workflow request above, or queue the next turn.");
    const sid = session.sessionId;
    const rid = session.runId;
    const epoch = authEpoch.current;
    const answer = (await controller.sendCommand("inject_guidance", { guidance: avatarMode ? conversationReply(text, avatarReplyContext, session.runId, snapshot.view?.root_run_id === session.runId ? snapshot.view.runs.map((run) => run.run_id) : []) : text, ...(avatarMode ? { display_text: text } : {}) })) as { accepted?: boolean; seq?: number; duplicate?: boolean } | null;
    if (sessionRef.current.sessionId !== sid || sessionRef.current.runId !== rid || authEpoch.current !== epoch) throw staleSendAbort();
    if (avatarMode && answer?.accepted !== false) setAvatarReplyContext(null);
    return {
      accepted: answer?.accepted !== false,
      ...(typeof answer?.seq === "number" ? { seq: answer.seq } : {}),
      ...(answer?.duplicate === true ? { duplicate: true } : {}),
    };
  }
  function queueNextTurn(text: string, mentions: readonly ChatMentionSent[] = []): { accepted: boolean } {
    setQueue((items) => [
      ...items,
      {
        id: newId(),
        text,
        sessionId: session.sessionId,
        sourceRunId: session.runId,
        authEpoch: authEpoch.current,
        ...(mentions.length ? { mentions: [...mentions] } : {}),
      },
    ]);
    setQueueRunning(true);
    return { accepted: true };
  }
  const steer =
    active && !sending
      ? {
          modes: (avatarMode ? ["guide"] : interaction ? ["queue"] : ["guide", "queue"]) as ("guide" | "queue")[],
          parked: snapshot.status === "waiting" && !interaction,
          onSteer: async (text: string, mode: "guide" | "queue", ctx?: { mentions: ChatMentionSent[] }) => {
            const mentions = ctx?.mentions ?? [];
            setMentionStatus(null);
            if (await resendMentions(text, mentions)) return { accepted: true };
            if (mode === "queue") return queueNextTurn(text, mentions);
            const sessionId = session.sessionId;
            const guided = session.runId;
            const result = await guideRun(text);
            // The guidance is in the run: then the picked entities are asked (WorkflowChat keeps
            // the draft and the chips when this rejects). Guidance starts no turn, and the guided
            // turn's prompt is an earlier message: the mentions name no `repeat_of`.
            if (result.accepted) await askMentions(sessionId, text, mentions, { repeatOf: mentionRepeatOf({ steered: guided }), keepDraft: false, resendable: true });
            return result;
          },
        }
      : undefined;

  /** A message from the composer or the microphone: starts a turn, or steers / queues a busy run.
   * The `@` names picked with it (`mentions`) are asked once the message is in: right after its
   * turn started or its guidance was accepted; a queued message carries them to its turn. */
  async function send(text: string, opts?: { signal?: AbortSignal; mentions?: readonly ChatMentionSent[] }) {
    const mentions = opts?.mentions ?? [];
    const sessionId = session.sessionId;
    setMentionStatus(null);
    if (await resendMentions(text, mentions, opts?.signal)) return;
    if (!active) {
      const started = await startTurn(text, opts?.signal, mentions);
      await askMentions(sessionId, text, mentions, { repeatOf: mentionRepeatOf({ started: started.turn }), agentAddressed: started.agentAddressed, signal: opts?.signal, keepDraft: true, resendable: true });
      return;
    }
    const guided = session.runId;
    const queued = !avatarMode && Boolean(interaction);
    const result = queued ? queueNextTurn(text, mentions) : await guideRun(text);
    if (!result.accepted) throw new Error("The gateway did not accept this message.");
    setDraft("");
    setNotice(avatarMode ? "Sent. Your message is available to the assistant." : queued ? "Queued for the next turn." : "Guidance queued. The workflow will read it at its next supported boundary.");
    // Guidance (or an answer) starts no turn: its mentions name no `repeat_of` (`mentionRepeatOf`).
    if (!queued) await askMentions(sessionId, text, mentions, { repeatOf: mentionRepeatOf({ steered: guided }), signal: opts?.signal, keepDraft: true, resendable: true });
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
    const next = await startTurn(intent.text, undefined, intent.mentions ?? []);
    // The queue follows the turn the conversation opens on (a message the agent was not addressed
    // in without a run of its own leaves it where it was).
    setQueue((items) => advanceQueueIntents(items, intent, next.runId || intent.sourceRunId));
    // Its names are asked now; a refusal is said and never pauses the queue (the turn started).
    await askMentions(intent.sessionId, intent.text, intent.mentions ?? [], { repeatOf: mentionRepeatOf({ started: next.turn }), agentAddressed: next.agentAddressed, keepDraft: false }).catch((e) => setError(formatError(e)));
  }
  useEffect(() => {
    if (
      !queueRunning ||
      !queue.length ||
      !connection.connected ||
      !restoredInputs ||
      restoreError ||
      sendLock.current ||
      mentioning
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
    mentioning,
  ]);

  async function cancelCurrentRun(): Promise<void> {
    // A start in flight has no run to cancel yet: WorkflowChat aborted the send's signal, and
    // startTurn stops the run the gateway returns (never a cancel of the previous run).
    if (sendLock.current) return;
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
  // "Stopping…" for a Stop pressed while starting ends when THE RUN THIS START RETURNED is shown
  // stopped (never on the previous run's state), when the start failed, or when the person opens
  // another conversation (the stop itself still completes).
  useEffect(() => {
    if (stopping?.runId && session.runId === stopping.runId && String(snapshot.run?.run_id || "") === stopping.runId && terminal(snapshot.status)) setStopping(null);
  }, [stopping, session.runId, snapshot.run?.run_id, snapshot.status]);
  useEffect(() => setStopping(null), [session.sessionId, identity]);
  useEffect(() => {
    const leaving = () => { unloadingRef.current = true; };
    window.addEventListener("pagehide", leaving);
    window.addEventListener("beforeunload", leaving);
    return () => {
      window.removeEventListener("pagehide", leaving);
      window.removeEventListener("beforeunload", leaving);
    };
  }, []);
  // An earlier page of this tab stopped a run while it was starting and never saw its answer
  // (a reload): finish that stop now, retrying every 3 s while its run is not listed yet. The loop
  // pauses while the tab is hidden (a slow gateway pays for every list).
  useEffect(() => {
    if (!connection.connected) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const run = () => {
      timer = undefined;
      if (typeof document !== "undefined" && document.hidden) return;
      void finishPendingStops().then(({ waiting, choices, dropped }) => {
        if (!alive) return;
        setStopChoices(choices);
        if (dropped.length) setStopMisses((list) => [...list, ...dropped]);
        if (waiting && !timer) timer = setTimeout(run, 3_000);
      });
    };
    const onVisible = () => {
      if (!document.hidden && !timer) run();
    };
    document.addEventListener("visibilitychange", onVisible);
    run();
    return () => {
      alive = false;
      document.removeEventListener("visibilitychange", onVisible);
      if (timer) clearTimeout(timer);
    };
  }, [connection.connected, identity, stopsVersion]);
  const chooseStop = (choice: PendingStop, runId: string | null) => {
    forgetStopWhileStarting(choice.id);
    setStopChoices((list) => list.filter((c) => c.id !== choice.id));
    if (runId) void cancelStartedRun(runId).catch((e) => setError(`The run could not be stopped: ${formatError(e)}`));
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
  const filteredSessions = searchConversations(catalog.sessions, search);
  // Round 6 (DESIGN R6.2): archive a conversation from its card's "⋯" or the header's "⋯", always
  // through the one inline confirm; the gateway hides it (nothing is deleted), the list moves it
  // under `Archived · N`, and an archived ACTIVE conversation hands over to the next one.
  const [archiveAsk, setArchiveAsk] = useState<{ sessionId: string; where: "card" | "header" } | null>(null);
  const [archiveBusy, setArchiveBusy] = useState(false);
  const [archiveError, setArchiveError] = useState("");
  const askArchive = (sessionId: string, where: "card" | "header") => {
    setArchiveError("");
    setArchiveAsk({ sessionId, where });
  };
  const cancelArchive = () => {
    setArchiveAsk(null);
    setArchiveError("");
  };
  const confirmArchive = async () => {
    if (!archiveAsk || archiveBusy) return;
    const { sessionId } = archiveAsk;
    setArchiveBusy(true);
    setArchiveError("");
    try {
      await archiveSession(sessionId);
    } catch (e) {
      setArchiveError(`Not archived: ${formatError(e)}`);
      setArchiveBusy(false);
      return;
    }
    setArchiveBusy(false);
    setArchiveAsk(null);
    if (sessionId === session.sessionId) {
      const next = nextConversationAfterArchive(filteredSessions, sessionId);
      if (next) {
        openConversation(next.sessionId, next.latestRunId);
      } else {
        newConversation();
      }
    }
    catalog.forgetSession(sessionId);
  };
  const archiveConfirm = (
    <ArchiveConfirm busy={archiveBusy} error={archiveError} onConfirm={() => void confirmArchive()} onCancel={cancelArchive} />
  );

  // Settings: one renderer for a conversation (its run preferences) and an
  // automation (its definition, through AutomationSettingsPanel).
  const conversationSettingsChange = (next: RunPreferences) => {
            // Reset only on an explicit Settings transition. Runtime values
            // matching flat pins are not proof they were host-authored.
            const reset: string[] = [];
            if ((preferences.provider || preferences.model) && !next.provider && !next.model) reset.push("provider", "model");
            // The tool choices live in this app's tool layer (kit store), not in the preferences.
            const layerCleared = Object.keys(appToolLayer).length > 0 && !Object.keys(next.toolLayer).length;
            if (layerCleared) reset.push("tools");
            if (next.toolLayer !== appToolLayer) setAppToolLayer(next.toolLayer);
            if (preferences.maxIterations && !next.maxIterations) reset.push("max_iterations");
            if (reset.length) setInputs(previous => {
              const values = { ...previous }, defaults = schemaDefaults(schema);
              for (const key of reset) {
                delete values[key];
                if (Object.prototype.hasOwnProperty.call(defaults, key)) values[key] = defaults[key];
              }
              return values;
            });
            setPreferences({ ...next, toolLayer: {} });
  };
  const renderSettings = (
    section: "model" | "workspace" | "tools" | "skills" | "behavior",
    value: RunPreferences = { ...preferences, toolLayer: appToolLayer },
    onChange: (next: RunPreferences) => void = conversationSettingsChange,
    automation?: { root: string },
  ) => (
    <SettingsContent
      tab={section}
      lockedReasonId={`code-${section}-locked`}
      value={value}
      onChange={onChange}
      toolCatalog={catalog.toolCatalog}
      toolWarnings={catalog.toolWarnings}
      automationTools={Boolean(automation)}
      toolsNotice={!automation && toolsNoticePending ? { text: TOOLS_MIGRATION_NOTICE, onDismiss: dismissToolsNotice } : null}
      connected={connection.connected}
      automationFolders={Boolean(automation)}
      defaultModel={
        !automation && inputs.provider && inputs.model
          ? { provider: String(inputs.provider), model: String(inputs.model) }
          : catalog.defaultModel
      }
      workflowDefault={!automation && Boolean(inputs.provider && inputs.model)}
      streaming={catalog.streaming}
      // Automations run on a schedule, where the gateway iteration default does not apply.
      iterationDefault={automation ? AUTOMATION_ITERATION_DEFAULT : catalog.iterationLimit}
      lastRunLimits={automation ? null : runLimits}
      disabled={automation ? !connection.connected : locked || !connection.connected}
      workspaceRootFixed={automation?.root}
      sessionId={automation ? undefined : session.sessionId}
      onOpenDefaultWorkspaces={() => setDefaultWorkspacesOpen(true)}
      workspaceRefreshKey={defaultWorkspacesRevision}
      hideStreamReplies={Boolean(automation)}
      lockedText={automation ? "Connect to a gateway to edit this automation." : undefined}
    />
  );
  const voiceSection = (
      <CodeVoiceSettings
        value={voicePreferences}
        onChange={(next) => { voice.stop_tts(); changeVoicePreferences(next); }}
        defaults={voiceDefaults}
        connected={connection.connected}
        isAdmin={connection.status?.gateway?.principal?.admin === true}
        levelSource={voice}
        sampleSessionId={session.sessionId}
        ttsAvailable={voiceCapability?.tts?.available === true}
      />
  );
  const automationDetail = automationsState.detail;
  // One draft per selected automation, shared by every settings panel (one debounce, one revision line).
  const automationSettings = useAutomationSettings(
    automationView && automationDetail?.definition ? { summary: automationDetail.summary, definition: automationDetail.definition } : null,
    (automationId, changes, expected) => automations.revise(automationId, changes, expected),
    catalog.toolCatalog,
  );
  /**
   * One settings panel of the rail, bound to the selection: an automation's
   * definition (binding line + revision + save outcome on top) or the
   * selected conversation's run settings. `id` keeps the `#code-settings-<id>`
   * anchor each panel had as a group in round 4.
   */
  const settingsPanel = (
    id: SettingsRailPanel,
    conversation: () => React.ReactNode,
    automation: (value: RunPreferences, onChange: (next: RunPreferences) => void, root: { root: string }) => React.ReactNode,
    binding = true,
  ) => {
    if (automationView) {
      if (!automationDetail?.definition || !automationSettings)
        return automationsState.selectedId ? <LoadingStatus>Loading automation…</LoadingStatus> : <p className="code-muted">Select an automation in the sidebar.</p>;
      const root = { root: automationDetail.definition.workspace_root || "" };
      return (
        <div className="code-automation-settings" id={`code-settings-${id}`} data-revision={automationSettings.revision}>
          {binding ? <AutomationBinding summary={automationDetail.summary} settings={automationSettings} /> : null}
          {automation(automationSettings.draft, automationSettings.onChange, root)}
        </div>
      );
    }
    return (
      <div className="code-conversation-settings" id={`code-settings-${id}`}>
        {binding ? <p className="code-settings-binding"><span>Conversation</span> <strong title={title}>{title}</strong></p> : null}
        {conversation()}
      </div>
    );
  };
  const modelPanel = settingsPanel(
    "model",
    () => renderSettings("model"),
    (value, onChange, root) => renderSettings("model", value, onChange, root),
  );
  const workflowPanel = settingsPanel(
    "workflow",
    () => (
      <>
          <div className="code-workflow-select" aria-busy={schemaLoading || undefined}>
            <Icon name="agent" size={17} />
            <WorkflowPicker
              id="code-workflow-picker"
              className="code-workflow-picker"
              interfaceId={CODE_AGENT_INTERFACE}
              ariaLabel="Workflow"
              workflows={{ ...catalog.executable, reload: () => void catalog.refresh() }}
              value={pickerValue(selection, workflow)}
              currentLabel={
                workflow && selection !== GATEWAY_DEFAULT && !visibleChoices.some((item) => item.id === workflow.id)
                  ? { name: workflow.name, detail: session.runId && workflow.bundleVersion ? `conversation version ${workflow.bundleVersion}` : workflow.bundleVersion ? `@${workflow.bundleVersion}` : "" }
                  : null
              }
              unavailableReason={!connection.connected ? "Connect to a gateway first." : locked ? "A run is in progress." : null}
              onChange={(value, entry) => {
                const next = selectionFromPicker(value, entry);
                setSelection(next);
                // The picker is THIS conversation's workflow; with the gateway keeping the account
                // default (round 14) that default is its own row below, never a side effect.
                if (!accountManaged) setPreferences((previous) => ({ ...previous, workflow: next }));
              }}
            />
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
            {/* The picker already names what runs; only a resolution that went wrong is said here. */}
            {resolvedNote && resolvedNote.runId === session.runId && resolvedNote.missing ? (
              <span className="code-workflow-resolved is-missing" role="status">
                {resolvedNote.text}
              </span>
            ) : null}
          </div>
        {accountWorkflow.state.status === "ok" ? (
          <>
            <details className="code-settings-advanced">
              <summary>Default for new chats <SettingsHelp label="About the default workflow">This account default is used by new conversations in every app. The workflow above belongs to this conversation.</SettingsHelp></summary>
              <AccountWorkflowDefault
              row={accountWorkflow.state.row}
              save={accountWorkflow.save}
              disabled={!connection.connected}
              />
            </details>
          </>
        ) : null}
        <div className="code-settings code-workflow-inputs">
          {schemaLoading ? (
            <p role="status">Loading input schema…</p>
          ) : schemaError ? null : isAgent || promptProperty ? (
            <AgentWorkflowInputs
              concise
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
          {chatReady ? null : (
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
      </>
    ),
    () =>
      automationDetail?.definition && automationSettings ? (
        <>
        <AutomationDefinitionForm
          summary={automationDetail.summary}
          definition={automationDetail.definition}
          settings={automationSettings}
          busy={automationsState.busy}
          disabled={!connection.connected}
          emailStatus={automationsState.emailStatus}
          onOpenMyEmail={automationHost.openMyEmail}
          onRevise={(changes, expected) => automations.revise(automationDetail.automationId, changes, expected)}
          previewSchedule={automations.previewSchedule}
          prepareTarget={prepareAutomationTargetForCode}
          workflowPickerOptions={{ interfaceId: CODE_AGENT_INTERFACE, workflows: { ...catalog.executable, reload: () => void catalog.refresh() } }}
        />

        </>
      ) : null,
    false,
  );
  const workspacePanel = settingsPanel(
    "workspace",
    () => (
      <>
        <p className="code-current-workspace">
          Run folder <code title={effectiveWorkspace || "Created by the Gateway for each run"}>{effectiveWorkspace.split("/").filter(Boolean).pop() || "Gateway managed"}</code>
        </p>
        {renderSettings("workspace")}
      </>
    ),
    (value, onChange, root) => renderSettings("workspace", value, onChange, root),
  );
  const toolsPanel = settingsPanel(
    "tools",
    () => renderSettings("tools"),
    (value, onChange, root) => renderSettings("tools", value, onChange, root),
    false,
  );
  const skillsPanel = settingsPanel(
    "skills",
    () => renderSettings("skills"),
    (value, onChange, root) => renderSettings("skills", value, onChange, root),
    false,
  );
  const agentContents = (behavior: React.ReactNode, skillCount = preferences.skills.length) => (
    <AgentSettings workflow={workflowPanel} tools={toolsPanel} skills={skillsPanel}
      behavior={behavior} skillCount={skillCount} workflowDescription={automationView ? undefined : workflow?.description} />
  );
  const agentPanel = settingsPanel("agent", () => agentContents(renderSettings("behavior")), (value, onChange, root) => agentContents(renderSettings("behavior", value, onChange, root), value.skills.length));
  const voicePanel = voiceSection;
  // Settings follow the selection: a newly selected conversation or automation starts at the top of the open panel.
  const settingsBinding = automationView ? `automation:${automationsState.selectedId}` : `conversation:${session.sessionId}`;
  const settingsScrolledFor = useRef<Record<string, string>>({});
  useEffect(() => {
    // Only a VISIBLE panel can scroll: reset when it shows a binding it has not shown yet.
    if (!railPanel || railPanel === "activity" || railPanel === "files" || settingsScrolledFor.current[railPanel] === settingsBinding) return;
    settingsScrolledFor.current[railPanel] = settingsBinding;
    window.requestAnimationFrame(() => {
      const body = document.querySelector<HTMLElement>(`#code-rail-panel-${railPanel} .af-rail__body`);
      if (body) body.scrollTop = 0;
    });
  }, [settingsBinding, railPanel]);
  const activityAttn = automationView ? { count: 0, hint: "" } : activityAttention(snapshot.records, session.runId, snapshot.view);
  const railElement = (
    <CodeRightRail
      key={`rail:${identity}`}
      panel={railPanel}
      onPanel={setRailPanel}
      activityBadge={activityAttn.count}
      activityHint={activityAttn.hint}
      content={{
      activity: automationView ? (
        <AutomationActivity
          occurrences={automationDetail?.occurrences || []}
          nowMs={Date.now()}
          onOpenRun={automationHost.openRun}
        />
      ) : (
        <WorkspaceActivityContent records={snapshot.records} runId={session.runId} view={snapshot.view} />
      ),
      files: automationView ? (
        connection.connected && automationsState.selectedId ? (
          <WorkspaceBrowser
            fetchGateway={proxyGatewayFetch}
            runId={automationsState.selectedId}
            title="Automation files"
            refreshKey={automationDetail?.summary.updated_at}
            copyText={copy_text}
            onOpenFolder={() => openRunFolder(automationsState.selectedId)}
          />
        ) : (
          <p className="code-muted">Connect to a gateway and select an automation to browse its files.</p>
        )
      ) : (
        <>
            <WorkspaceFilesContent
              policy={catalog.policy}
              runId={session.runId}
              enabled={connection.connected}
              isAdmin={connection.status?.gateway?.principal?.admin === true}
              refreshKey={snapshot.status}
              onAttachFiles={attachUploads}
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
        </>
      ),
      model: modelPanel,
      agent: agentPanel,
      workspace: workspacePanel,
      voice: voicePanel,
      }}
    />
  );

  return (
    <div
      className={`code-app${sidebarOpen ? " code-app--nav-open" : ""}`}
    >
      <a className="code-skip-link" href="#code-conversation">
        Skip to conversation
      </a>
      {/* Kit gap K1 (ui-primitives 0036): AfAppShell cannot host this full-height sidebar beside the header, nor the one-row landscape chrome. */}
      <SidebarDrawer isDrawer={panesAreDrawers} open={sidebarOpen}>
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
        <SidebarLists panels={panels}>
        <ConversationsPanel
          open={panels.conversations}
          onToggle={() => togglePanelOpen("conversations")}
          actions={
            <span className="code-auto-actions">
              <button
                className="code-icon-button code-new-chat"
                aria-label="New conversation"
                title="New conversation (⇧⌘N)"
                aria-keyshortcuts="Shift+Meta+N"
                onClick={newConversation}
              >
                <Icon name="plus" size={13} />
              </button>
              <button
                className="code-icon-button"
                aria-label="Refresh conversations"
                disabled={!connection.connected || catalog.loading}
                onClick={() => void catalog.refresh()}
              >
                <Icon name="refresh" size={13} />
              </button>
            </span>
          }
        >
          {!currentSession ? (
            <div className="af-session-card code-session-placeholder" data-selected="true">
              <button type="button" className="af-session-card__open" aria-current="page" onClick={() => setSidebarOpen(false)}>
                <span className="af-session-card__title">New conversation</span>
                <span className="af-session-card__meta">Ready when you are</span>
              </button>
            </div>
          ) : null}
          {filteredSessions.map((item) => (
            <ConversationCard
              key={item.sessionId}
              item={item}
              activity={catalog.sessionActivity[item.sessionId]}
              unread={Boolean(catalog.sessionActivity[item.sessionId] && catalog.sessionActivity[item.sessionId].content.some(sessionReads.isUnread))}
              needsAttention={Boolean(catalog.sessionActivity[item.sessionId]?.needsAttention || Object.values(meEvents.fold.waits).includes(item.sessionId))}
              selected={item.sessionId === session.sessionId}
              onClick={() =>
                openConversation(item.sessionId, item.latestRunId)
              }
              onAskArchive={connection.connected ? () => askArchive(item.sessionId, "card") : undefined}
              confirm={archiveAsk?.where === "card" && archiveAsk.sessionId === item.sessionId ? archiveConfirm : undefined}
            />
          ))}
          {catalog.sessionsLoaded && !filteredSessions.length ? (
            <p className="code-history-empty">
              {search
                ? "No conversations match your search."
                : "Your conversations will live here. Pick up where you left off, on any device."}
            </p>
          ) : null}
          {/* One notice per page: while the banner says the gateway cannot be reached, a read marker
              that could not be saved is part of it; otherwise it reads like the banner. */}
          {sessionReads.error && !snapshot.unreachable ? <p className="code-alert code-alert--inline" role="status"><Icon name="warning" size={14} /><span>{sessionReads.error}</span></p> : null}
          {catalog.loading && !catalog.sessionsLoaded ? (
            <LoadingStatus>Loading conversations…</LoadingStatus>
          ) : null}
          {catalog.hasMore ? (
            <button className="code-load-more" onClick={catalog.loadMore}>
              Load more conversations
            </button>
          ) : null}
          <ArchivedFooter list="conversations" count={catalog.archivedSessions} open={archivedOpen.conversations} onToggle={() => toggleArchivedOpen("conversations")}>
            <ArchivedConversations
              enabled={connection.connected && archivedOpen.conversations}
              revision={catalog.archivedSessions}
              onOpen={(item) => { setSidebarOpen(false); openConversation(item.sessionId, item.latestRunId); }}
              onUnarchived={() => void catalog.refresh()}
            />
          </ArchivedFooter>
        </ConversationsPanel>
        <AutomationsSection
          nowMs={automationNowMs}
          state={automationsState}
          attention={eventsFollowing ? { needsYou: needsYou(meEvents.fold), newResults: newResults(meEvents.fold) } : null}
          available={automationsAvailable}
          discovering={catalog.capabilitiesLoading}
          createDisabled={schemaLoading || !!schemaError || !workflow}
          selectedId={automationView ? automationsState.selectedId : ""}
          onSelect={(id) => {
            setAutomationView(true);
            setSidebarOpen(false);
            void automations.select(id);
          }}
          onNew={() => setNewAutomationOpen(true)}
          onRefresh={() => void automations.refresh()}
          archivedOpen={archivedOpen.automations}
          onToggleArchived={() => toggleArchivedOpen("automations")}
          pausedOpen={pausedOpen}
          onTogglePaused={() => setPausedOpen((v) => !v)}
          onUnarchive={(s) => void automations.unarchive(s.automation_id).catch(() => undefined)}
          onArchive={(s) => automations.command(s.automation_id, "automation.archive")}
          open={panels.automations}
          onToggle={() => togglePanelOpen("automations")}
        />
        </SidebarLists>
      </SidebarDrawer>
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
            onClick={() => { setPanelOpen(false); setSidebarOpen(true); }}
          >
            <Icon name="list" size={19} />
          </button>
          {!automationView && <div className="code-view-choice" role="group" aria-label="Conversation view">
            <button type="button" aria-pressed={!avatarMode} onClick={() => { voice.stop_tts(); avatarPreferences.setMode("chat"); }}>Conversation</button>
            <button type="button" aria-pressed={avatarMode} onClick={() => { voice.stop_tts(); avatarPreferences.setMode("avatar"); }}>Avatar</button>
          </div>}
          <div className="code-breadcrumb">
            <span>{automationView ? "Automations" : "Conversations"}</span>
            <span aria-hidden="true">/</span>
            <strong title={automationView ? automationTitle : title}>
              {automationView ? automationTitle : title}
            </strong>
          </div>
          {!automationView && currentSession && connection.connected ? (
            <AfMenu
              className="code-conversation-menu"
              label="Conversation actions"
              title="More actions"
              items={[{ id: "archive", label: "Archive", danger: true, onSelect: () => askArchive(currentSession.sessionId, "header") }]}
            />
          ) : null}
          <AfTopBarActions
            docs={{ open: docsOpen, onToggle: () => { setDocsOpen((open) => !open); setSidebarOpen(false); }, label: "Docs assistant" }}
            appearance={{ value: appearance, onChange: setAppearance }}
            about={{ identity: APP_IDENTITY, fetch: proxyGatewayFetch }}
            identity="menu"
            connection={connection}
          />
        </header>
        <div className="code-content">
          {automationView ? (
            <AutomationMain
              prepareTarget={prepareAutomationTargetForCode}
              workflowPickerOptions={{ interfaceId: CODE_AGENT_INTERFACE, workflows: { ...catalog.executable, reload: () => void catalog.refresh() } }}
              ctl={automations}
              availableTools={catalog.tools.filter(t => t.enabled).map(t => t.name)}
              host={automationHost}
              voiceCapability={voiceCapability}
              voicePreferences={voicePreferences}
              enabled={connection.connected}
              onClose={() => setAutomationView(false)}
              onEdit={openAutomationSettings}
              onEditWorkspaces={openAutomationWorkspaces}
            />
          ) : (
          <main
            className="code-conversation"
            id="code-conversation"
            tabIndex={-1}
          >
            {/* This client's trouble reaching the gateway: the page's one notice (operator 2026-11-08),
                with Retry now / Stop retrying. The run's own state stays the view's (status bar). */}
            {snapshot.unreachable ? (
              <div className="code-alert code-alert--unreachable" role="alert">
                <Icon name="warning" size={16} />
                <span>{unreachableBannerText(snapshot.unreachable)}</span>
                <button onClick={() => controller.retryNow()}>Retry now</button>
                {snapshot.unreachable.retrying ? (
                  <button onClick={() => controller.stopRetrying()}>Stop retrying</button>
                ) : null}
              </div>
            ) : error ||
            snapshot.error ||
            restoreError ? (
              <div className="code-alert" role="alert">
                <Icon name="warning" size={16} />
                <span>
                  {error ||
                    snapshot.error ||
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
            {archiveAsk?.where === "header" && archiveAsk.sessionId === session.sessionId ? (
              <div className="code-archive-confirm-bar">{archiveConfirm}</div>
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
            {restoredVersionNote && selection === restoredWorkflow?.id ? (
              <div className="code-notice" role="status" data-field="version-note">
                {restoredVersionNote}
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
            {stopChoices.filter((c) => c.sessionId === session.sessionId).map((choice) => {
              const count = choice.candidates?.length || 0;
              return (
                <div key={choice.id} className="code-stop-choice" role="status">
                  <span>
                    You pressed Stop while a message was starting, then the page reloaded. {count} replies of this
                    conversation started at that moment; Code will not guess which one you meant.
                  </span>
                  {(choice.candidates || []).map((c, index) => (
                    <button key={c.runId} type="button" className="af-setting-btn" onClick={() => chooseStop(choice, c.runId)}>
                      Stop the reply started at {new Date(c.createdAt).toLocaleTimeString()} ({index + 1} of {count})
                    </button>
                  ))}
                  <button type="button" className="af-setting-btn af-setting-btn--quiet" onClick={() => chooseStop(choice, null)}>Keep them</button>
                </div>
              );
            })}
            {stopMisses.filter((m) => m.sessionId === session.sessionId).map((miss) => (
              <div key={miss.id} className="code-stop-choice" role="status">
                <span>The stop you pressed before the reload could not be matched to a reply; if a reply appears here, stop it.</span>
                <button type="button" className="af-setting-btn af-setting-btn--quiet" onClick={() => setStopMisses((list) => list.filter((m) => m.id !== miss.id))}>Dismiss</button>
              </div>
            ))}
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
            {/* Kit gap K2 (ui-primitives 0030): WorkflowChat has no runStatus slot (metrics, pause/resume, activity). */}
            {active || terminal(snapshot.status) || stoppingStart ? (
              <RunStatusBar
                publicView={avatarMode}
                stopping={stoppingStart}
                active={active && !stoppingStart}
                paused={paused}
                snapshot={snapshot}
                onInteraction={
                  interaction && !paused
                    ? () => {
                        const thread = document.querySelector<HTMLElement>(
                          avatarMode ? ".code-conversation .pc-avatar-conversation" : ".code-conversation .pc-chat-thread",
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
                onCommand={(command) =>
                  act(() => controller.sendCommand(command, {}))
                }
                onActivity={() => {
                  openPanel("activity");
                }}
              />
            ) : null}
            <WorkflowChat
              avatarConversation={avatarMode ? {
                onTechnicalView: () => { voice.stop_tts(); avatarPreferences.setMode("chat"); },
                sessionKey: `${identity}:${session.sessionId}`,
                avatar: <ConversationAvatar voice={voice} active={active} pending={Boolean(pendingInteraction)} failed={snapshot.status === "failed"} paused={paused && !terminal(snapshot.status)} />,
                ...conversationSnapshot(snapshot),
                listening: voice.voice_ptt_recording,
                speaking: voice.tts_speaking,
                preferences: avatarPreferences.preferences,
                onPreferencesChange: avatarPreferences.setPreferences,
                onCue: (cue) => { void playConversationCue(cue).catch((reason) => setNotice(String(reason instanceof Error ? reason.message : reason))); },
                onInterruptSpeech: () => voice.stop_tts(),
                onReplyContext: setAvatarReplyContext,
                onAttendedChange: setAvatarAttended,
                narrate: voicePreferences.read_aloud === true && !voicePreferences.reply_muted,
                onNarrate: (message) => { if (voice.tts_supported) void voice.toggle_tts(message.id || message.content, message.content); },
              } : undefined}
                artifactFetch={proxyGatewayFetch}
                attachmentScope={JSON.stringify([identity,session.sessionId])}
                attachmentRun={{runId:session.runId,output:snapshot.run?.output}}
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
                renderFooter: (message) => {
                  const status = message.runId === session.runId ? snapshot.status : catalog.sessionActivity[session.sessionId]?.content.find((c) => c.runId === message.runId)?.status;
                  if (!status) return null;
                  const target = sessionReadTarget(message.kind === "attachments" ? [...messages,message] : messages, message.runId || "", status);
                  return target.message === message && readScope && !snapshot.loading && !snapshot.historyLoading && !snapshot.error ? <><span>{target.outcome}</span><SessionReadMarker ownerKey={readScope} receipt={{ sessionId: session.sessionId, revision: sessionContentRevision(message.runId || "", status), updatedAt: "" }} relatedReceipts={sessionOpeningReceipts(messages, catalog.sessionActivity[session.sessionId]?.content, message.runId || "")} enabled={!automationView && !sidebarOpen && identity === principalRef.current} onRead={sessionReads.markRead} /></> : null;
                },
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
              mentionCandidates={mentionCandidates}
              mentionSessionKey={`${identity}:${session.sessionId}`}
              mentionsUnavailable={mentionsOff === identity ? MENTIONS_UNAVAILABLE : null}
              mentionStatus={mentionStatus?.sessionId === session.sessionId ? mentionStatus.text : null}
              busy={locked && !stoppingStart}
              busyLabel={sending ? "Starting…" : paused ? "Paused" : "Working…"}
              working={working}
              workingLabel={sending ? "Starting" : "Working"}
              steer={steer}
              toolbar={messages.length ? { exportTranscript: { filename: "conversation.md" } } : undefined}
              onCancel={cancelCurrentRun}
              stopState={snapshot.stop ?? null}
              disabled={!connection.connected}
              blockedNotice={restore.readOnlyNotice || liveConnectionNotice({ runId: session.runId, status: snapshot.status, unreachable: snapshot.unreachable }) || undefined}
              interaction={interaction}
              onAttach={() => fileInput.current?.click()}
              onFiles={(files) => { attachUploads(files); }}
              attachmentItems={attachmentItems}
              onRemoveAttachment={removeAttachmentItem}
              onRetryAttachment={uploads.retry}
              placeholder={
                avatarMode ? "Tell your assistant…" : active
                  ? interaction
                    ? "Answer the request above, or queue the next task…"
                    : "Guide the current workflow, or queue the next task…"
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
                  {!avatarMode && <span
                    className="code-composer-workspace"
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
                  </span>}
                </>
              }
              composerTrailingActions={connection.connected ? (
                    <VoiceTools
                      key={`${identity}:${session.sessionId}:${session.runId}`}
                      voice={voice}
                      capability={voiceCapability}
                      preferences={voicePreferences}
                      route={sttRouteText(voicePreferences, voiceDefaults.value)}
                    />
                  ) : null}
              footer={
                <>
                  <span>
                    Enter to send <span aria-hidden="true">·</span> Shift +
                    Enter for a new line
                  </span>
                  <span>
                    Managed workspace
                    <span className="code-small-dot" />
                  </span>
                </>
              }
            />
            {/* Kit gap K5 (ui-primitives 0030): WorkflowChat's steer has no queued-turns list. */}
            {queue.length ? (
              <div className="code-queue">
                <div>
                  <strong>
                    {queue.length} queued{" "}
                    {queue.length === 1 ? "turn" : "turns"}
                  </strong>
                  <AfSwitch
                    className="code-queue__switch"
                    variant="sm"
                    action="queue-running"
                    label="Run queued turns"
                    hint="On: each queued turn starts when the current run ends. Off: they wait here."
                    checked={queueRunning}
                    onChange={setQueueRunning}
                  />
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
          )}
          {railElement}
          <CodeDocsAssistant key={identity || "signed-out"} connected={connection.connected} open={docsOpen} onClose={() => setDocsOpen(false)} voicePreferences={voicePreferences} />
        </div>
      </div>
      <div className="pc-sr-only" role="status" aria-live="polite">
        {uploads.announcement}
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
      <NewAutomationDialog
        connected={connection.connected}
        workflowPickerOptions={{ interfaceId: CODE_AGENT_INTERFACE, workflows: { ...catalog.executable, reload: () => void catalog.refresh() } }}
        open={newAutomationOpen}
        onClose={() => setNewAutomationOpen(false)}
        target={automationTarget(selection, workflow)}
        availableTools={catalog.tools.filter(t => t.enabled).map(t => t.name)}
        initialTools={automationToolSelection(hasToolChoices(catalog.toolCatalog, { app: appToolLayer }) ? { ...inputs, tools: planRunTools(catalog.toolCatalog, { app: appToolLayer }).tools } : inputs)}
        buildInput={async (prompt, target) => {
          if (!target) return guardedWorkflowInput(options => currentWorkflowInput(prompt, { ...schemaDefaults(schema), ...inputs }, [], true, workflow, schema, options));
          const next = target.flow_id === "@default" ? defaultWorkflow : catalog.workflows.find(w => `${w.bundleId}@${w.bundleVersion}` === ("bundle_ref" in target ? target.bundle_ref : "") && w.flowId === target.flow_id);
          if (!next) throw new Error("The selected workflow is no longer available. Refresh the workflow list.");
          const nextSchema = await fetchWorkflowSchema(next);
                if (!nextSchema) throw new Error("Workflow inputs could not be checked. Refresh the workflow list and try again.");
          const input = await guardedWorkflowInput(options => currentWorkflowInput(prompt, schemaDefaults(nextSchema), [], true, next, nextSchema, options));
          const problems = validateWorkflowInputs(nextSchema, input);
          if (problems.length) throw new Error(problems.join(" "));
          return input;
        }}
        workflowLabel={
          selection === GATEWAY_DEFAULT
            ? gatewayDefaultOptionLabel(catalog.gatewayDefault)
            : workflow?.name || "the selected workflow"
        }
        initialPrompt={
          draft ||
          [...messages].reverse().find((m) => m.role === "user")?.content ||
          ""
        }
        ctl={automations}
        onOpenMyEmail={openMyEmail}
        onCreated={() => {
          setAutomationView(true);
          setSidebarOpen(false);
        }}
      />
      <GatewayConnectModal {...connection.modalProps} />
      <CodeDefaultWorkspacesDialog
        open={defaultWorkspacesOpen}
        onClose={() => setDefaultWorkspacesOpen(false)}
        connected={connection.connected}
        onChanged={bumpDefaultWorkspaces}
      />
    </div>
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
            Using <strong>{workflow.name}</strong> · Choose another in Agent
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
