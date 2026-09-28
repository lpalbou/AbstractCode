/**
 * Automations in the AbstractCode web app: a sidebar section (every
 * automation of the signed-in gateway user, state as text + icon, what runs
 * now, the next run) and, for the selected one, the kit's shared
 * `AutomationPanel` (with the chat renderer) plus its folder, browsed through
 * the gateway's run-workspace routes. "New automation" is the kit's
 * `AfScheduleDialog` with the toolbar's workflow as target.
 *
 * The list/rows and the panel wiring are hook-free so tests render them in
 * any state; `useAutomations` only subscribes to the controller and polls.
 */
import React, { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import {
  AfScheduleDialog,
  AutomationStateLabel,
  Icon,
  apiErrorText,
  type ApiError,
  type AutomationTarget,
  type AutomationCommandType,
  type DiscussResponse,
} from "@abstractframework/ui-kit";
import { AutomationPanelWithMarkdown, WorkspaceBrowser, type AutomationPanelWithMarkdownProps } from "@abstractframework/panel-chat";

import {
  AUTOMATIONS_POLL_MS,
  AutomationsController,
  automationRowView,
  codeAutomationsClient,
  discussionNotice,
  proxyAnswerWait,
  toApiError,
  visibleAutomations,
  type AutomationsState,
} from "./automations";
import { proxyGatewayFetch } from "./session_files";
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

/** The sidebar section: rows + New automation + Show archived (hook-free). */
export function AutomationsSection(props: {
  state: AutomationsState;
  available: { available: boolean; reason: string };
  selectedId: string;
  onSelect(id: string): void;
  onNew(): void;
  onRefresh(): void;
  onShowArchived(show: boolean): void;
  nowMs?: number;
}): React.ReactElement {
  const st = props.state;
  const rows = visibleAutomations(st.items, st.showArchived);
  const archived = st.items.filter((s) => s.status === "archived").length;
  const waiting = st.items.reduce((n, s) => n + s.attention.pending_waits + s.attention.unseen_count, 0);
  return (
    <section className="code-automations" aria-label="Automations">
      <div className="code-section-label">
        <span>
          AUTOMATIONS{waiting ? <span className="code-auto-badge" data-field="attention-total">{waiting}</span> : null}
        </span>
        <span className="code-auto-actions">
          <button className="code-icon-button" aria-label="New automation" title="New automation (runs the toolbar's workflow on a schedule)" disabled={!props.available.available} onClick={props.onNew}>
            <Icon name="plus" size={13} />
          </button>
          <button className="code-icon-button" aria-label="Refresh automations" disabled={!props.available.available || st.loading} onClick={props.onRefresh}>
            <Icon name="refresh" size={13} />
          </button>
        </span>
      </div>
      <div className="code-auto-rows">
        {!props.available.available ? (
          <p className="code-history-empty" role="note" data-unavailable="true">
            {props.available.reason}
          </p>
        ) : null}
        <ErrorLine error={st.listError} />
        {props.available.available && st.loaded && !st.items.length ? (
          <p className="code-history-empty">No automations yet. + runs the toolbar's workflow on a schedule.</p>
        ) : null}
        {rows.map((s) => {
          const v = automationRowView(s, props.nowMs);
          const selected = props.selectedId === s.automation_id;
          return (
            <button
              key={s.automation_id}
              className={`code-session code-auto-row${selected ? " is-selected" : ""}`}
              aria-current={selected ? "page" : undefined}
              data-automation-id={s.automation_id}
              data-status={s.status}
              title={`${v.title} — ${v.cadence}`}
              onClick={() => props.onSelect(s.automation_id)}
            >
              <Icon name="history" size={15} />
              <span>
                <strong>{v.title}</strong>
                <small>
                  <span data-field="state">
                    <AutomationStateLabel status={s.status} />
                  </span>{" "}
                  {v.cadence}
                  {v.legacy ? " · legacy schedule" : ""}
                </small>
                {v.current ? (
                  <small data-field="current">now: {v.current}</small>
                ) : null}
                <small data-field="next">next: {v.next}</small>
                {v.attention ? (
                  <small className="code-auto-attention" data-field="attention">
                    {v.attention}
                  </small>
                ) : null}
              </span>
            </button>
          );
        })}
        {archived > 0 ? (
          <label className="code-auto-archived">
            <input type="checkbox" data-action="show-archived" checked={st.showArchived} onChange={(e) => props.onShowArchived(e.target.checked)} /> Show archived ({archived})
          </label>
        ) : null}
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
  };
}

/** The folder pane's title: the automation's own folder, or one run's. */
export function folderTitle(automationId: string, runId: string, occurrences: Array<{ run_id: string; index: number }>): string {
  if (runId === automationId) return "Automation folder";
  const row = occurrences.find((o) => o.run_id === runId);
  return row ? `Run #${row.index} folder` : "Run folder";
}

/** The main area for the selected automation: the kit panel + its folder. */
export function AutomationMain(props: {
  ctl: AutomationsController;
  host: Omit<AutomationHost, "openWorkspace">;
  enabled: boolean;
  onClose(): void;
}): React.ReactElement {
  const st = props.ctl.state;
  const d = st.detail;
  // The folder pane shows the automation's folder, or the run the panel's
  // "Browse" asked for; it resets when another automation opens.
  const [folder, setFolder] = useState({ automationId: "", runId: "" });
  const automationId = d?.automationId ?? "";
  const folderRun = folder.automationId === automationId && folder.runId ? folder.runId : automationId;
  const p = automationPanelProps(props.ctl, {
    ...props.host,
    openWorkspace: (runId) => setFolder({ automationId, runId }),
  });
  return (
    <main className="code-conversation code-automation-main" id="code-conversation" tabIndex={-1} aria-label="Automation">
      <div className="code-auto-main-head">
        <button className="code-subtle-button" onClick={props.onClose}>
          <Icon name="chat" size={14} /> <span>Back to the conversation</span>
        </button>
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
          <p className="code-muted" role="status">
            Loading the automation…
          </p>
        )
      ) : (
        <div className="code-auto-main-body">
          {p ? <AutomationPanelWithMarkdown {...p} /> : null}
          {props.enabled ? (
            <WorkspaceBrowser
              className="code-auto-folder"
              fetchGateway={proxyGatewayFetch}
              runId={folderRun}
              title={folderTitle(d.automationId, folderRun, d.occurrences)}
              refreshKey={String(d.summary.occurrence_count)}
              onClose={folderRun !== d.automationId ? () => setFolder({ automationId, runId: "" }) : undefined}
            />
          ) : null}
        </div>
      )}
    </main>
  );
}

/** "New automation": the kit dialog with the toolbar's workflow as target. */
export function NewAutomationDialog(props: {
  open: boolean;
  onClose(): void;
  target: AutomationTarget | null;
  workflowLabel: string;
  initialPrompt: string;
  ctl: AutomationsController;
  onCreated(id: string): void;
}): React.ReactElement | null {
  const [error, setError] = useState<ApiError | undefined>();
  useEffect(() => {
    if (props.open) setError(undefined);
  }, [props.open]);
  return (
    <AfScheduleDialog
      open={props.open}
      onClose={props.onClose}
      target={props.target}
      workflowPicker={
        <p className="code-field-help" data-field="target">
          {props.target ? `Runs ${props.workflowLabel} (the toolbar's workflow).` : "Choose a published workflow in the toolbar first."}
        </p>
      }
      initialPrompt={props.initialPrompt}
      busy={props.ctl.state.busy}
      error={error}
      onSubmit={(body) =>
        props.ctl.create(body).then(
          (created) => {
            props.onCreated(created.automation_id);
            props.onClose();
            return created;
          },
          (e) => {
            setError(toApiError(e));
            throw e;
          },
        )
      }
    />
  );
}
