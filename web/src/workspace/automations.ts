/**
 * Automations in AbstractCode (web) — the rules and the page controller.
 *
 * AbstractCode executes nothing: every action is a gateway route
 * (`api/gateway/automations…`), reached through the ui-kit client
 * (`createAutomationsClient`) over the app's authenticated proxy. The
 * presentation rules the other clients share (cadence labels, controls,
 * occurrence chat pairs, error text, the create body) come from the kit;
 * this module owns what is AbstractCode's own:
 *
 * - the client over the proxy (same-origin, CSRF header on mutations);
 * - the create target from the toolbar's workflow choice (the gateway
 *   default agent as `@default` + interface);
 * - the list rows (state as text + icon, what runs NOW only from
 *   `current_occurrence`, the NEXT run only from `next_fire_at`);
 * - the page controller (full-page polling, commands, discuss, seen, waits).
 *
 * Structure only: nothing here reads model prose.
 */
import { gatewayApiPath } from "@abstractframework/ui-kit";
import {
  attentionLabel,
  automationControls,
  createAutomationsClient,
  currentOccurrenceLabel,
  formatUtc,
  isApiError,
  relativeIn,
  triggerSummary,
  type ApiError,
  type AutomationChanges,
  type AutomationCommandType,
  type AutomationDefinition,
  type AutomationSummary,
  type AutomationTarget,
  type AutomationsClient,
  type CommandReceipt,
  type CreateAutomationRequest,
  type CreateAutomationResponse,
  type DiscussResponse,
  type OccurrenceRow,
  type TriggerSourceEntry,
} from "@abstractframework/ui-kit";

import { csrfHeaders, gatewayRequest } from "./transport";
import { CODE_AGENT_INTERFACE, GATEWAY_DEFAULT } from "./workflow_selection";
import type { WorkflowDefinition } from "./catalog";

/** Poll interval while the Automations view is visible. */
export const AUTOMATIONS_POLL_MS = 30_000;
/** Page size for list and occurrences (full pages are polled). */
export const AUTOMATIONS_PAGE_LIMIT = 50;

/** The kit client over the app's same-origin proxy (CSRF on mutations). */
export function codeAutomationsClient(): AutomationsClient {
  return createAutomationsClient({
    fetch: (url, init) => fetch(url, { ...init, credentials: "same-origin" }),
    baseUrl: "",
    headers: csrfHeaders,
  });
}

/** `capabilities.contracts.common.automations` (discovery contracts). */
export function automationsAvailability(contracts: Record<string, any> | undefined): { available: boolean; reason: string } {
  const auto = contracts?.common?.automations;
  if (auto && typeof auto === "object" && auto.available === true) return { available: true, reason: "" };
  if (auto && typeof auto === "object" && auto.available === false)
    return { available: false, reason: "This gateway has the Automations API turned off." };
  return {
    available: false,
    reason: "This gateway does not advertise the Automations API (capabilities.contracts.common.automations). Update AbstractGateway (0.6.0 or later) to use automations.",
  };
}

/**
 * The create target for the toolbar's choice: the gateway default agent as
 * `{flow_id: "@default", interface}` (resolved by the gateway), else the
 * published workflow as `{bundle_ref, flow_id}`.
 */
export function automationTarget(selection: string, workflow: WorkflowDefinition | null): AutomationTarget | null {
  if (selection === GATEWAY_DEFAULT) return { flow_id: "@default", interface: CODE_AGENT_INTERFACE };
  if (!workflow?.bundleId || !workflow.flowId) return null;
  const ref = workflow.bundleVersion ? `${workflow.bundleId}@${workflow.bundleVersion}` : workflow.bundleId;
  return { bundle_ref: ref, flow_id: workflow.flowId };
}

export type AutomationRowView = {
  id: string;
  title: string;
  cadence: string;
  /** "Run #7 running" — from `current_occurrence` only; null when nothing is in flight. */
  current: string | null;
  next: string;
  attention: string | null;
  legacy: boolean;
};

/** Two facts, two fields (never inferred from `last_occurrence`). */
export function automationRowView(s: AutomationSummary, nowMs: number = Date.now()): AutomationRowView {
  const needs = s.attention.unseen_count > 0 || s.attention.pending_waits > 0;
  return {
    id: s.automation_id,
    title: s.title,
    cadence: triggerSummary(s.trigger),
    current: currentOccurrenceLabel(s),
    next: s.next_fire_at ? `${formatUtc(s.next_fire_at)} (${relativeIn(s.next_fire_at, nowMs)})` : s.status === "paused" ? "none while paused" : "none scheduled",
    attention: needs ? attentionLabel(s) : null,
    legacy: s.legacy === true || s.capabilities.includes("legacy"),
  };
}

/** Archived automations are hidden unless asked for. */
export function visibleAutomations(items: AutomationSummary[], showArchived: boolean): AutomationSummary[] {
  return showArchived ? items : items.filter((s) => s.status !== "archived");
}

/**
 * Decision D1: the answer payload follows the wait's `kind`, never its text.
 * Throws for an untyped wait or a payload of the wrong shape (the gateway
 * would refuse it with 422 `invalid_request`).
 */
export function waitAnswerPayload(kind: unknown, payload: Record<string, any>): Record<string, any> {
  if (kind === "ask_user") {
    if (typeof payload.response !== "string") throw new Error("An ask_user wait is answered with {response: string}.");
    return { response: payload.response };
  }
  if (kind === "tool_approval") {
    if (typeof payload.approved !== "boolean") throw new Error("A tool_approval wait is answered with {approved: true|false}.");
    return { approved: payload.approved };
  }
  if (kind === "event") {
    if (!("payload" in payload)) throw new Error("An event wait is answered with {payload}.");
    return { payload: payload.payload };
  }
  throw new Error(`This wait has no known kind (${JSON.stringify(kind)}); open its run to answer it.`);
}

/** The resume command for a wait (`POST /api/gateway/commands`). */
export function waitResumeCommand(commandId: string, runId: string, waitKey: string, payload: Record<string, any>): Record<string, any> {
  return { command_id: commandId, run_id: runId, type: "resume", payload: { wait_key: waitKey, payload }, client_id: "abstractcode-web" };
}

/** What a started discussion is, in words, from the gateway's answer. */
export function discussionNotice(index: number, r: DiscussResponse): string {
  return `Discussion forked from run #${index}. It works in its own workspace ${r.workspace_root}; the automation's files are mounted read-only at ${r.mounted_workspace} for the file tools (shell commands are not sandboxed), and nothing is written back into the automation's session.`;
}

export function toApiError(e: unknown): ApiError {
  if (isApiError(e)) return e;
  const message = e && typeof e === "object" && "message" in e ? String((e as { message: unknown }).message) : String(e);
  return { status: 0, code: "client_error", message };
}

/** Newest first, merged by run id with the rows already loaded. */
export function mergeOccurrences(loaded: OccurrenceRow[], page: OccurrenceRow[]): OccurrenceRow[] {
  const byId = new Map<string, OccurrenceRow>();
  for (const r of loaded) byId.set(r.run_id, r);
  for (const r of page) byId.set(r.run_id, r);
  return Array.from(byId.values()).sort((a, b) => b.index - a.index);
}

export type AutomationDetailState = {
  automationId: string;
  summary: AutomationSummary;
  definition: AutomationDefinition | null;
  occurrences: OccurrenceRow[];
  nextCursor: string | null;
};

export type AutomationsState = {
  items: AutomationSummary[];
  loaded: boolean;
  loading: boolean;
  listError: ApiError | null;
  error: ApiError | null;
  showArchived: boolean;
  selectedId: string;
  detail: AutomationDetailState | null;
  triggerSources: TriggerSourceEntry[];
  busy: boolean;
  notice: string;
};

export const INITIAL_AUTOMATIONS_STATE: AutomationsState = {
  items: [],
  loaded: false,
  loading: false,
  listError: null,
  error: null,
  showArchived: false,
  selectedId: "",
  detail: null,
  triggerSources: [],
  busy: false,
  notice: "",
};

/** How a wait is answered: the gateway's resume command on the waiting run. */
export type AnswerWait = (runId: string, waitKey: string, payload: Record<string, any>) => Promise<void>;

export function proxyAnswerWait(mint: () => string): AnswerWait {
  return async (runId, waitKey, payload) => {
    await gatewayRequest(gatewayApiPath("commands"), {
      method: "POST",
      body: JSON.stringify(waitResumeCommand(mint(), runId, waitKey, payload)),
    });
  };
}

/**
 * The Automations view's state machine, UI-free (React subscribes). Panel
 * callbacks REJECT with an `ApiError` so the kit panel shows it.
 */
export class AutomationsController {
  state: AutomationsState = { ...INITIAL_AUTOMATIONS_STATE };
  private listeners = new Set<() => void>();
  private listSeq = 0;
  private detailSeq = 0;
  private sourcesLoaded = false;

  /** `followupsMs`: re-reads after a command (the gateway applies it moments later). */
  constructor(
    private readonly client: AutomationsClient,
    private readonly answerWaitVia: AnswerWait,
    private readonly followupsMs: number[] = [1500, 4000],
  ) {}

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => {
      this.listeners.delete(fn);
    };
  }

  private set(patch: Partial<AutomationsState>): void {
    this.state = { ...this.state, ...patch };
    for (const fn of this.listeners) fn();
  }

  /** Every page of `GET /automations` (v1 has no change cursor). */
  async listAll(): Promise<AutomationSummary[]> {
    const out: AutomationSummary[] = [];
    let cursor: string | undefined;
    for (let guard = 0; guard < 200; guard += 1) {
      const page = await this.client.listAutomations({ ...(cursor ? { cursor } : {}), limit: AUTOMATIONS_PAGE_LIMIT });
      out.push(...page.items);
      if (!page.next_cursor) return out;
      cursor = page.next_cursor;
    }
    throw new Error("GET /api/gateway/automations kept returning next_cursor after 200 pages.");
  }

  async refresh(): Promise<void> {
    const seq = ++this.listSeq;
    this.set({ loading: true });
    try {
      const items = await this.listAll();
      if (seq !== this.listSeq) return;
      const detail = this.state.detail;
      const fresh = detail ? items.find((s) => s.automation_id === detail.automationId) : undefined;
      this.set({ items, loaded: true, loading: false, listError: null, ...(detail && fresh ? { detail: { ...detail, summary: fresh } } : {}) });
      if (detail) await this.reloadDetail(detail.automationId);
    } catch (e) {
      if (seq !== this.listSeq) return;
      this.set({ loading: false, loaded: true, listError: toApiError(e) });
    }
  }

  setShowArchived(show: boolean): void {
    this.set({ showArchived: show });
  }

  async select(automationId: string): Promise<void> {
    const id = automationId.trim();
    const seq = ++this.detailSeq;
    if (!id) {
      this.set({ selectedId: "", detail: null });
      return;
    }
    // Never show the previous automation's panel while this one loads.
    this.set({ selectedId: id, notice: "", error: null, ...(this.state.detail?.automationId !== id ? { detail: null } : {}) });
    try {
      const [detail, page] = await Promise.all([this.client.getAutomation(id), this.client.listOccurrences(id, { limit: AUTOMATIONS_PAGE_LIMIT })]);
      if (seq !== this.detailSeq) return;
      const listed = this.state.items.find((s) => s.automation_id === id);
      this.set({
        detail: { automationId: id, summary: listed ?? detail.summary, definition: detail.definition, occurrences: mergeOccurrences([], page.items), nextCursor: page.next_cursor },
      });
      if (!this.sourcesLoaded) {
        const res = await this.client.listTriggerSources();
        this.sourcesLoaded = true;
        this.set({ triggerSources: res.items });
      }
    } catch (e) {
      if (seq !== this.detailSeq) return;
      this.set({ error: toApiError(e) });
    }
  }

  private async reloadDetail(id: string): Promise<void> {
    const [detail, page] = await Promise.all([this.client.getAutomation(id), this.client.listOccurrences(id, { limit: AUTOMATIONS_PAGE_LIMIT })]);
    const cur = this.state.detail;
    if (!cur || cur.automationId !== id) return;
    const listed = this.state.items.find((s) => s.automation_id === id);
    this.set({
      detail: {
        ...cur,
        summary: listed ?? detail.summary,
        definition: detail.definition,
        occurrences: mergeOccurrences(cur.occurrences, page.items),
        // Keep the older-page cursor once pages beyond the first are loaded.
        nextCursor: cur.occurrences.length > page.items.length ? cur.nextCursor : page.next_cursor,
      },
    });
  }

  async loadMore(): Promise<void> {
    const d = this.state.detail;
    if (!d || !d.nextCursor) return;
    try {
      const page = await this.client.listOccurrences(d.automationId, { cursor: d.nextCursor, limit: AUTOMATIONS_PAGE_LIMIT });
      const cur = this.state.detail;
      if (!cur || cur.automationId !== d.automationId) return;
      this.set({ detail: { ...cur, occurrences: mergeOccurrences(cur.occurrences, page.items), nextCursor: page.next_cursor } });
    } catch (e) {
      this.set({ error: toApiError(e) });
    }
  }

  private async afterChange(): Promise<void> {
    await this.refresh();
    for (const ms of this.followupsMs) setTimeout(() => void this.refresh(), ms);
  }

  private async busyCall<T>(fn: () => Promise<T>): Promise<T> {
    this.set({ busy: true });
    try {
      const out = await fn();
      await this.afterChange();
      return out;
    } catch (e) {
      throw toApiError(e);
    } finally {
      this.set({ busy: false });
    }
  }

  /** `POST /automations/{id}/commands`. Rejects with ApiError. */
  command(id: string, type: AutomationCommandType, commandId?: string): Promise<CommandReceipt> {
    return this.busyCall(() => this.client.sendAutomationCommand(id, { type, ...(commandId ? { command_id: commandId } : {}) }));
  }

  /** `PATCH /automations/{id}` with `expected_revision`. Rejects with ApiError. */
  revise(id: string, changes: AutomationChanges, expectedRevision: number | null, commandId?: string): Promise<CommandReceipt> {
    return this.busyCall(() =>
      this.client.reviseAutomation(id, { changes, ...(expectedRevision !== null ? { expected_revision: expectedRevision } : {}), ...(commandId ? { command_id: commandId } : {}) }),
    );
  }

  /** `POST /automations` → the new automation is selected. Rejects with ApiError. */
  async create(body: CreateAutomationRequest): Promise<CreateAutomationResponse> {
    this.set({ busy: true, error: null });
    try {
      const created = await this.client.createAutomation(body);
      this.set({ notice: `Automation created: ${created.summary.title}.` });
      await this.refresh();
      await this.select(created.automation_id);
      for (const ms of this.followupsMs) setTimeout(() => void this.refresh(), ms);
      return created;
    } catch (e) {
      throw toApiError(e);
    } finally {
      this.set({ busy: false });
    }
  }

  /** `POST /automations/{id}/discuss` → the new session (the host opens it). */
  async discuss(id: string, occurrenceIndex: number, prompt: string, requestId?: string): Promise<DiscussResponse> {
    this.set({ busy: true });
    try {
      const r = await this.client.discuss(id, { occurrence_index: occurrenceIndex, prompt, ...(requestId ? { request_id: requestId } : {}) });
      this.set({ notice: discussionNotice(occurrenceIndex, r) });
      return r;
    } catch (e) {
      throw toApiError(e);
    } finally {
      this.set({ busy: false });
    }
  }

  /** `POST /automations/{id}/seen` with the LAST DISPLAYED item's cursor. */
  async seen(id: string, cursor: string): Promise<void> {
    try {
      await this.client.markSeen(id, cursor);
    } catch (e) {
      throw toApiError(e);
    }
  }

  /** Answer an occurrence's wait through the gateway's resume command. */
  answerWait(runId: string, waitKey: string, payload: Record<string, any>): Promise<void> {
    return this.busyCall(async () => {
      const wait = (this.state.detail?.occurrences || []).flatMap((o) => o.waits).find((w) => w.run_id === runId && w.wait_key === waitKey);
      if (!wait) throw new Error(`No loaded wait ${waitKey} on run ${runId}; reload the automation.`);
      await this.answerWaitVia(runId, waitKey, waitAnswerPayload(wait.kind, payload));
    });
  }

  /** Row controls from the kit rule (the panel shows the full set). */
  rowControls(s: AutomationSummary) {
    return automationControls(s, [], this.state.busy);
  }

  reportError(e: unknown): void {
    this.set({ error: toApiError(e) });
  }

  clearNotice(): void {
    this.set({ notice: "", error: null });
  }
}
