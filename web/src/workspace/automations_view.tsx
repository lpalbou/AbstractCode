import { AutomationWorkflowPicker, type AutomationWorkflowPickerOptions, automationToolSelection, withAutomationTools } from "@abstractframework/ui-kit";
/**
 * Automations in the AbstractCode web app: a sidebar section (every
 * automation of the signed-in gateway user, state as text + icon, what runs
 * now, the next run) and, for the selected one, the kit's shared
 * `AutomationPanel` (with the chat renderer) plus its folder, browsed through
 * the gateway's run-workspace routes. "New automation" is the kit's
 * `AfScheduleDialog` with the conversation's workflow as target.
 *
 * The list/rows and the panel wiring are hook-free so tests render them in
 * any state; `useAutomations` only subscribes to the controller and polls.
 */
import React, { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { LoadingStatus } from "./loading_status";
import { ArchivedFooter, ArchivedRow, PanelHeader, panelIds } from "./sidebar_panels";
import { AutomationCard } from "./sidebar_cards";
import { AutomationHeaderBar } from "./automation_header";
import { copy_text } from "../lib/clipboard";
import { DetailDisclosure, detailPanelIds, useDetailPanels, useTimelineSlot, type DetailPanelsState } from "./detail_panels";
import {
  AfScheduleDialog,
  Icon,
  apiErrorText,
  type ApiError,
  type VoicePreferences,
  type AutomationTarget,
  type AutomationCommandType,
  type AutomationSummary,
  type DiscussResponse,
} from "@abstractframework/ui-kit";
import { AutomationPanelWithMarkdown, WorkspaceBrowser, type AutomationPanelWithMarkdownProps } from "@abstractframework/panel-chat";

import {
  AUTOMATIONS_POLL_MS,
  AutomationsController,
  codeAutomationsClient,
  discussionNotice,
  proxyAnswerWait,
  toApiError,
  visibleAutomations,
  type AutomationsState,
} from "./automations";
import { proxyGatewayFetch } from "./session_files";
import { useWorkspaceVoice } from "./voice_tools";
import { newId } from "./transport";

/** One controller per signed-in identity; polls while visible and available. */
export function useAutomations(identity: string, available: boolean): { ctl: AutomationsController; state: AutomationsState } {
  const ctl = useMemo(() => new AutomationsController(codeAutomationsClient(), proxyAnswerWait(newId)), [identity]);
  const state = useSyncExternalStore(
    (fn) => ctl.subscribe(fn),
    () => ctl.state,
  );
  const [visible, setVisible] = useState(() => typeof document === "undefined" || document.visibilityState !== "hidden");
  useEffect(() => {
    const on = () => setVisible(document.visibilityState !== "hidden");
    document.addEventListener("visibilitychange", on);
    return () => document.removeEventListener("visibilitychange", on);
  }, []);
  useEffect(() => {
    if (!identity || !available || !visible) return;
    void ctl.refresh();
    const t = window.setInterval(() => void ctl.refresh(), AUTOMATIONS_POLL_MS);
    return () => window.clearInterval(t);
  }, [ctl, identity, available, visible]);
  return { ctl, state };
}

function ErrorLine({ error }: { error: ApiError | null }): React.ReactElement | null {
  if (!error) return null;
  const t = apiErrorText(error);
  return (
    <p className="code-inline-error" role="alert" data-code={error.code}>
      <strong>{t.title}</strong> {t.detail}
    </p>
  );
}

/** The sidebar section: rows + New automation + the `Archived · N` footer (hook-free). */
export function AutomationsSection(props: {
  state: AutomationsState;
  available: { available: boolean; reason: string };
  discovering?: boolean;
  createDisabled?: boolean;
  selectedId: string;
  onSelect(id: string): void;
  onNew(): void;
  onRefresh(): void;
  /** The `Archived · N` footer: open state (remembered by the host) and its toggle. */
  archivedOpen?: boolean;
  onToggleArchived?(): void;
  /** Unarchive one automation (the gateway's `automation.unarchive`). */
  onUnarchive?(s: AutomationSummary): void;
  /** The card's Active switch (pause / resume through the gateway). */
  onToggleActive?(s: AutomationSummary): void;
  nowMs?: number;
  /** Collapsible panel (sidebar_panels.tsx): open by default. */
  open?: boolean;
  onToggle?(): void;
}): React.ReactElement {
  const open = props.open ?? true;
  const ids = panelIds("automations");
  const st = props.state;
  const rows = visibleAutomations(st.items);
  const archived = st.archived ?? [];
  const waiting = st.items.reduce((n, s) => n + s.attention.pending_waits + s.attention.unseen_count, 0);
  return (
    <section
      className="code-panel code-automations"
      aria-label="Automations"
      data-open={open ? "true" : "false"}
    >
      <PanelHeader
        panel="automations"
        open={open}
        onToggle={() => props.onToggle?.()}
        label={<>Automations{waiting ? <span className="code-auto-badge" data-field="attention-total">{waiting}</span> : null}</>}
        actions={<span className="code-auto-actions">
          <button className="code-icon-button" aria-label="New automation" title="New automation (choose a workflow and schedule)" disabled={!props.available.available || props.createDisabled} onClick={props.onNew}>
            <Icon name="plus" size={13} />
          </button>
          <button className="code-icon-button" aria-label="Refresh automations" disabled={!props.available.available || st.loading} onClick={props.onRefresh}>
            <Icon name="refresh" size={13} />
          </button>
        </span>}
      />
      <div className="code-auto-rows" id={ids.region} role="region" aria-labelledby={ids.toggle} hidden={!open}>
        {!props.discovering && !props.available.available ? (
          <p className="code-history-empty" role="note" data-unavailable="true">
            {props.available.reason}
          </p>
        ) : null}
        <ErrorLine error={st.listError} />
        {!st.loaded && (props.discovering || (props.available.available && st.loading)) ? <LoadingStatus>Loading automations…</LoadingStatus> : null}
        {props.available.available && st.loaded && !st.listError && !st.items.length ? (
          <p className="code-history-empty">No automations yet. + lets you choose a workflow and schedule.</p>
        ) : null}
        {rows.map((s) => (
          <React.Fragment key={s.automation_id}>
            <AutomationCard
              summary={s}
              selected={props.selectedId === s.automation_id}
              busy={st.busy}
              nowMs={props.nowMs ?? Date.now()}
              onSelect={() => props.onSelect(s.automation_id)}
              onToggleActive={() => props.onToggleActive?.(s)}
            />
            {st.rowError?.automationId === s.automation_id ? <ErrorLine error={st.rowError.error} /> : null}
          </React.Fragment>
        ))}
        <ArchivedFooter list="automations" count={st.archivedCount} open={props.archivedOpen === true} onToggle={() => props.onToggleArchived?.()}>
          {st.archived === null ? <li><LoadingStatus>Loading archived automations…</LoadingStatus></li> : null}
          {archived.map((s) => (
            <ArchivedRow
              key={s.automation_id}
              id={s.automation_id}
              title={s.title}
              busy={st.busy}
              error={st.rowError?.automationId === s.automation_id ? apiErrorText(st.rowError.error).title : undefined}
              onOpen={() => props.onSelect(s.automation_id)}
              onUnarchive={() => props.onUnarchive?.(s)}
            />
          ))}
        </ArchivedFooter>
      </div>
    </section>
  );
}

export type AutomationHost = {
  /** Show a run's folder in the automation's folder pane (the automation id = its own folder). */
  openWorkspace(runId: string): void;
  /** Open a gateway session as this app's conversation (a started discussion, a run). */
  openConversation(sessionId: string, runId: string, notice?: string): void;
  /** Open an occurrence run as a conversation (its session is read from the gateway). */
  openRun(runId: string): void;
  /** Open the gateway console's My email (absent when the gateway URL is unknown). */
  openMyEmail?(): void;
};

/** The kit panel's props for the selected automation (null when nothing is open). */
export function automationPanelProps(ctl: AutomationsController, host: AutomationHost): AutomationPanelWithMarkdownProps | null {
  const d = ctl.state.detail;
  if (!d) return null;
  const id = d.automationId;
  return {
    summary: d.summary,
    ...(d.definition ? { definition: d.definition } : {}),
    occurrences: d.occurrences,
    triggerSources: ctl.state.triggerSources,
    busy: ctl.state.busy,
    ...(ctl.state.error ? { error: ctl.state.error } : {}),
    // The panel mints one id per user action and reuses it on a transport
    // retry: forward it so the gateway answers the retry idempotently.
    onRevise: (changes, expected, meta) => ctl.revise(id, changes, expected, meta?.command_id),
    onCommand: (type, _payload, meta) => ctl.command(id, type as AutomationCommandType, meta?.command_id),
    onDiscuss: async (index, prompt, meta) => {
      const r: DiscussResponse = await ctl.discuss(id, index, prompt, meta?.request_id);
      // The fork is a real chat: this app switches to it, in place.
      host.openConversation(r.session_id, r.run_id, discussionNotice(index, r));
      return r;
    },
    onSeen: (cursor) => ctl.seen(id, cursor),
    onLoadMore: () => void ctl.loadMore(),
    onOpenRun: (runId) => host.openRun(runId),
    onOpenWorkspace: (runId) => host.openWorkspace(runId),
    onAnswerWait: (runId, waitKey, payload) => ctl.answerWait(runId, waitKey, payload as Record<string, any>),
    // The Edit form offers the email options only with a usable account.
    emailStatus: ctl.state.emailStatus,
    ...(host.openMyEmail ? { onOpenMyEmail: () => host.openMyEmail?.() } : {}),
  };
}

/** Re-list the folder whenever a run starts, moves or finishes (its files change then). */
export function folderRefreshKey(s: { occurrence_count: number; last_occurrence?: { index: number; status: string; finished_at?: string }; current_occurrence?: { index: number; status: string } | null }): string {
  const last = s.last_occurrence;
  const cur = s.current_occurrence;
  return [s.occurrence_count, last ? `${last.index}:${last.status}:${last.finished_at ?? ""}` : "", cur ? `${cur.index}:${cur.status}` : ""].join("|");
}

/** The folder pane's title: the automation's own folder, or one run's. */
export function folderTitle(automationId: string, runId: string, occurrences: Array<{ run_id: string; index: number }>): string {
  if (runId === automationId) return "Automation folder";
  const row = occurrences.find((o) => o.run_id === runId);
  return row ? `Run #${row.index} folder` : "Run folder";
}

/** The folder pane under a disclosure header; `own` = the automation's folder (its path is
 * already in the panel's Workspace fact), otherwise one run's. Collapsed = header only. */
export function AutomationFolderSection(props: {
  panels: DetailPanelsState;
  onToggle(): void;
  title: string;
  own: boolean;
  children: React.ReactNode;
}): React.ReactElement {
  const open = props.panels.folder;
  const ids = detailPanelIds("folder");
  return (
    <div
      className="code-auto-folder-section"
      data-open={open ? "true" : "false"}
      data-folder={props.own ? "automation" : "run"}
    >
      <div className="code-detail-head">
        <DetailDisclosure panel="folder" label={props.title} open={open} onToggle={props.onToggle} />
      </div>
      {/* The kit browser inside is the named region ("Automation folder"); this wrapper is not. */}
      <div className="code-detail-region" id={ids.region} hidden={!open}>
        {props.children}
      </div>
    </div>
  );
}

/** The main area for the selected automation: the kit panel + its folder. */
export function AutomationMain(props: {
  prepareTarget?(target: AutomationTarget): Promise<AutomationTarget>;
  workflowPickerOptions?: AutomationWorkflowPickerOptions;
  ctl: AutomationsController;
  host: Omit<AutomationHost, "openWorkspace">;
  voiceCapability?: Record<string, any>;
  voicePreferences?: VoicePreferences;
  availableTools?: string[];
  enabled: boolean;
  onClose(): void;
  /** The header's Edit: opens the Settings panel on this automation. */
  onEdit(automationId: string): void;
  nowMs?: number;
}): React.ReactElement {
  const st = props.ctl.state;
  const d = st.detail;
  // The folder pane shows the automation's folder, or the run the panel's
  // "Browse" asked for; it resets when another automation opens.
  const [folder, setFolder] = useState({ automationId: "", runId: "" });
  const automationId = d?.automationId ?? "";
  const [voiceError, setVoiceError] = useState("");
  const voice = useWorkspaceVoice({
    scope: automationId,
    runId: props.enabled ? automationId : "",
    sessionId: automationId,
    capability: props.voiceCapability || {},
    preferences: props.voicePreferences || {},
    onTranscript: () => {},
    onError: setVoiceError,
  });
  useEffect(() => setVoiceError(""), [automationId]);
  const folderRun = folder.automationId === automationId && folder.runId ? folder.runId : automationId;
  const p = automationPanelProps(props.ctl, {
    ...props.host,
    openWorkspace: (runId) => setFolder({ automationId, runId }),
  });
  // The detail's own lists (occurrences, folder) collapse like the sidebar panels.
  const [panels, togglePanel] = useDetailPanels();
  const bodyRef = useRef<HTMLDivElement>(null);
  const timelineSlot = useTimelineSlot(bodyRef, Boolean(d && d.occurrences.length));
  return (
    <main className="code-conversation code-automation-main" id="code-conversation" tabIndex={-1} aria-label="Automation">
      <div className="code-auto-main-head">
        <button className="code-subtle-button" onClick={props.onClose}>
          <Icon name="chat" size={14} /> <span>Back to the conversation</span>
        </button>
        {voice.tts_playback.status !== "idle" ? (
          <button type="button" className="code-subtle-button" onClick={voice.stop_tts}>Stop audio</button>
        ) : null}
        {st.notice ? (
          <p className="code-field-help" role="status">
            {st.notice}
          </p>
        ) : null}
      </div>
      {!d ? (
        st.error ? (
          <ErrorLine error={st.error} />
        ) : (
          <LoadingStatus>Loading automation…</LoadingStatus>
        )
      ) : (
        <div className="code-auto-main-body" ref={bodyRef} data-occurrences={panels.occurrences ? "open" : "closed"}>
          {/* Ledger and artifact links open through this app's proxy (safe tab-open rule). */}
          {voiceError ? <p className="code-inline-error" role="alert">{voiceError}</p> : null}
          <AutomationHeaderBar
            summary={d.summary}
            occurrences={d.occurrences}
            triggerSources={st.triggerSources}
            busy={st.busy}
            nowMs={props.nowMs}
            onCommand={(type) => props.ctl.command(d.automationId, type as AutomationCommandType)}
            onToggleActive={() => props.ctl.toggleActive(d.summary)}
            onEdit={() => props.onEdit(d.automationId)}
            onOpenFolder={() => {
              setFolder({ automationId, runId: "" });
              if (!panels.folder) togglePanel("folder");
              window.requestAnimationFrame(() => bodyRef.current?.querySelector(".code-auto-folder-section")?.scrollIntoView({ block: "nearest" }));
            }}
            onCopyPath={copy_text}
          />
          {p ? <AutomationPanelWithMarkdown {...p} hideHeader prepareTarget={props.prepareTarget} workflowPickerOptions={props.workflowPickerOptions} availableTools={props.availableTools} fetchGateway={proxyGatewayFetch} messageProps={{
            ...(voice.tts_supported ? { onSpeakToggle: (message) => {
              void voice.toggle_tts(String(message.id || message.content), message.content);
            } } : {}),
            getSpeakState: (message) => voice.tts_playback.key === String(message.id || message.content)
              ? voice.tts_playback.status : "idle",
          }} /> : null}
          {timelineSlot
            ? createPortal(
                <DetailDisclosure
                  panel="occurrences"
                  label="Occurrences"
                  count={d.summary.occurrence_count}
                  open={panels.occurrences}
                  onToggle={() => togglePanel("occurrences")}
                />,
                timelineSlot,
              )
            : null}
          {props.enabled ? (
            <AutomationFolderSection
              panels={panels}
              onToggle={() => togglePanel("folder")}
              title={folderTitle(d.automationId, folderRun, d.occurrences)}
              own={folderRun === d.automationId}
            >
              <WorkspaceBrowser
                className="code-auto-folder"
                fetchGateway={proxyGatewayFetch}
                runId={folderRun}
                title={folderTitle(d.automationId, folderRun, d.occurrences)}
                refreshKey={folderRefreshKey(d.summary)}
                onClose={folderRun !== d.automationId ? () => setFolder({ automationId, runId: "" }) : undefined}
              />
            </AutomationFolderSection>
          ) : null}
        </div>
      )}
    </main>
  );
}

/** "New automation": the kit dialog with the conversation's workflow as target. */
export function NewAutomationDialog(props: {
  open: boolean;
  onClose(): void;
  target: AutomationTarget | null;
  buildInput?(prompt: string, target?: AutomationTarget): Record<string, any> | Promise<Record<string, any>>;
  workflowPickerOptions?: AutomationWorkflowPickerOptions;
  availableTools?: string[];
  initialTools?: string[] | null;
  workflowLabel: string;
  initialPrompt: string;
  ctl: AutomationsController;
  onCreated(id: string): void;
  /** Opens the gateway console's My email ("Email isn't set up — open My email"). */
  onOpenMyEmail?: () => void;
}): React.ReactElement | null {
  const [error, setError] = useState<ApiError | undefined>();
  const [preparing, setPreparing] = useState(false);
  const submitting = useRef(false);
  const [chosenTarget, setChosenTarget] = useState<AutomationTarget>();
  useEffect(() => { if (!props.open) setChosenTarget(undefined); }, [props.open]);
  useEffect(() => {
    if (!props.open) return;
    setError(undefined);
    // Fresh email status each time the dialog opens (the user may have just connected their mailbox).
    void props.ctl.loadEmailStatus();
  }, [props.open]);
  return (
    <AfScheduleDialog
      open={props.open}
      onClose={props.onClose}
      target={chosenTarget || props.target}
      availableTools={props.availableTools}
      initialTools={chosenTarget ? null : props.initialTools}
      workflowPicker={
        props.workflowPickerOptions && (chosenTarget || props.target) ? <AutomationWorkflowPicker
          target={(chosenTarget || props.target)!} options={props.workflowPickerOptions} onChange={setChosenTarget} /> : <p className="code-field-help" data-field="target">
          {props.target ? `Initially uses ${props.workflowLabel}. Choose the workflow in this form.` : "Choose a workflow in Settings → Model & behavior first."}
        </p>
      }
      initialPrompt={props.initialPrompt}
      emailStatus={props.ctl.state.emailStatus}
      onOpenMyEmail={props.onOpenMyEmail}
      busy={props.ctl.state.busy || preparing}
      error={error}
      onSubmit={async (body) => {
        if (submitting.current) return;
        submitting.current = true;
        setPreparing(true);
        try {
          const built = await props.buildInput?.(String(body.target.input_data?.prompt || ""), chosenTarget);
          const input = built && props.availableTools !== undefined ? withAutomationTools(built, automationToolSelection(body.target.input_data)) : built;
          const created = await props.ctl.create(input ? { ...body, target: { ...body.target, input_data: input } } : body);
          props.onCreated(created.automation_id);
          props.onClose();
          return created;
        } catch (e) {
          setError(toApiError(e));
          throw e;
        } finally {
          submitting.current = false;
          setPreparing(false);
        }
      }}
    />
  );
}
