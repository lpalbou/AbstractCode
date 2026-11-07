// AbstractNote's background work and the restore rules of web 0.15.3 (operator, 2026-11-07),
// against real-shaped records read from a live gateway (fixtures/notes_background.json).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { AutomationsClient } from "@abstractframework/ui-kit";

import { sessionActivityFromRuns } from "../vendor/abstractsessions/session_cards.js";
import { normalizeSessionSummaries, normalizeWorkflowCatalog, resolveConversationWorkflow, withoutNotesSessions } from "./catalog";
import { fetchConversationRuns } from "./conversation_paging";
import { WORKFLOW_GONE, conversationRestore } from "./conversation_restore";
import { AutomationsController, automationSections } from "./automations";
import { NOTES_SESSION_NOTICE, isNotesAutomation, isNotesRun, isNotesRunInput, withoutNotesEntries, workflowBundleId } from "./notes_work";
import { archivedConversationCount } from "./archived_conversations";
import { ConversationCard, UNTITLED_CONVERSATION, conversationTitle, searchConversations } from "./sidebar_cards";

const F = JSON.parse(readFileSync(join(__dirname, "fixtures/notes_background.json"), "utf8"));
const CAPTURE_SESSION = "note-watch-e88aa3c2-88fc-4657-8bb1-b3c250a52f46";
const WEB_CAPTURE_SESSION = "note-web-0f3c2a9e-5b7d-4c1e-9a6f-2d8b4e1c7a90";
const DRAIN_SESSION = "a551dc9c-8614-5af5-8544-2e11ee0c1013";
const CODE_SESSION = "acode-8ec4cd0251ba";
const OLD_VERSION_SESSION = "acode-4b0e9d7c2f1a";
const GONE_SESSION = "acode-9f8e7d6c5b4a";
const run = (sessionId: string) => F.runs.items.find((r: any) => r.session_id === sessionId);
const workflows = () => normalizeWorkflowCatalog(F.bundles);

describe("the AbstractNote rule (bundle id, exact)", () => {
  it("reads the bundle from the workflow identity, never from a session id or text", () => {
    expect(workflowBundleId("abstractnote@0.3.0:note-capture")).toBe("abstractnote");
    expect(workflowBundleId("abstractnote:note-drain")).toBe("abstractnote");
    expect(workflowBundleId("abstractnote-labs@1.0.0:x")).toBe("abstractnote-labs");
    expect(workflowBundleId("note-capture")).toBeUndefined();
    expect(isNotesRun(run(CAPTURE_SESSION))).toBe(true);
    expect(isNotesRun(run(DRAIN_SESSION))).toBe(true);
    expect(isNotesRun(run(CODE_SESSION))).toBe(false);
    // Same id shapes, another bundle: a conversation (the session-id prefix means nothing).
    expect(isNotesRun({ ...run(CAPTURE_SESSION), workflow_id: "basic-agent@0.0.5:81795ea9" })).toBe(false);
    expect(isNotesRun({ ...run(CODE_SESSION), workflow_id: "abstractnote-labs@1.0.0:chat" })).toBe(false);
    expect(isNotesRunInput(F.input_data[run(CAPTURE_SESSION).run_id])).toBe(true);
    expect(isNotesRunInput(F.input_data[run(CODE_SESSION).run_id])).toBe(false);
  });
});

describe("Conversations list", () => {
  it("leaves out every Notes session (captures from the watch and the web, the drain) and keeps the others", () => {
    const ids = normalizeSessionSummaries(F.runs).map((s) => s.sessionId);
    expect(ids.sort()).toEqual([CODE_SESSION, OLD_VERSION_SESSION, GONE_SESSION].sort());
    expect(normalizeSessionSummaries(F.runs, {}, { purpose: "notes" }).map((s) => s.sessionId).sort()).toEqual(
      [CAPTURE_SESSION, WEB_CAPTURE_SESSION].sort(),
    );
  });

  it("decides by the session's ROOT workflow: a later turn on another workflow does not move it", () => {
    const later = { ...run(CODE_SESSION), run_id: "later-notes-turn", workflow_id: "abstractnote@0.3.1:note-recall", created_at: "2026-11-07T12:00:00+00:00", updated_at: "2026-11-07T12:00:00+00:00" };
    const ids = normalizeSessionSummaries({ ...F.runs, items: [later, ...F.runs.items] }).map((s) => s.sessionId);
    expect(ids).toContain(CODE_SESSION);
  });

  it("unread and attention marks never count a Notes session", () => {
    expect(Object.keys(sessionActivityFromRuns(F.runs))).toContain(CAPTURE_SESSION);
    const activity = sessionActivityFromRuns(withoutNotesSessions(F.runs));
    expect(Object.keys(activity)).not.toContain(CAPTURE_SESSION);
    expect(Object.keys(activity)).not.toContain(WEB_CAPTURE_SESSION);
    expect(Object.keys(activity)).toContain(CODE_SESSION);
  });

  it("paging counts conversations only: Notes sessions never fill a page", async () => {
    const fetchRuns = vi.fn(async (limit: number) => ({ ...F.runs, items: F.runs.items.slice(0, limit), has_more: limit < F.runs.items.length }));
    // Want 2: the first 4 rows hold only 1 conversation (and the Notes sessions), so it grows.
    await fetchConversationRuns(fetchRuns, 1);
    expect(fetchRuns.mock.calls.map((c) => c[0])).toEqual([4, 8]);
  });

  it("search follows the list: a Notes session is never found, even by its id or its text", () => {
    const sessions = normalizeSessionSummaries(F.runs, F.input_data);
    expect(searchConversations(sessions, "note-watch")).toEqual([]);
    expect(searchConversations(sessions, "tokenizer")).toEqual([]);
    expect(searchConversations(sessions, "PARSER").map((s) => s.sessionId)).toEqual([CODE_SESSION]);
  });
});

describe("the conversation's name (ADR 0043)", () => {
  it("is its first message, else 'Untitled conversation', never built from the session id", () => {
    const code = normalizeSessionSummaries(F.runs, F.input_data).find((s) => s.sessionId === CODE_SESSION)!;
    expect(conversationTitle(code)).toBe("Fix the failing parser test in src/parse.ts");
    const [unlabeled] = normalizeSessionSummaries(F.runs);
    expect(conversationTitle(unlabeled)).toBe(UNTITLED_CONVERSATION);
    expect(conversationTitle({ prompt: "  " })).toBe(UNTITLED_CONVERSATION);
    const html = renderToStaticMarkup(<ConversationCard item={{ ...unlabeled, sessionId: "note-watch-e88aa3c2" }} selected={false} onClick={() => {}} />);
    expect(html).toContain(UNTITLED_CONVERSATION);
    expect(html).not.toMatch(/Conversation note-wat|>note-wat/);
  });
});

describe("restoring an opened conversation", () => {
  const restore = (sessionId: string, extra: Partial<Parameters<typeof conversationRestore>[0]> = {}) => {
    const r = run(sessionId);
    return conversationRestore({
      sessionId,
      runId: r.run_id,
      restoredInputs: F.input_data[r.run_id],
      run: r,
      workflows: workflows(),
      conversationSessions: [],
      notesSessions: [],
      catalogLoading: false,
      ...extra,
    });
  };

  it("the loaded version of the same workflow continues a conversation whose version is gone, and says so", () => {
    const r = restore(OLD_VERSION_SESSION);
    expect(r.error).toBe("");
    expect(r.workflow?.workflowId).toBe("basic-agent@0.0.5:81795ea9");
    expect(r.versionNote).toBe(
      "This conversation started with a version of Basic agent that is no longer available. It continues with the version available now, so its next answers may differ.",
    );
    const old = run(OLD_VERSION_SESSION);
    const resolution = resolveConversationWorkflow(workflows(), { inputData: F.input_data[old.run_id], runWorkflowId: old.workflow_id });
    expect(resolution.replaced).toEqual({ recordedVersion: "0.0.1", loadedVersion: "0.0.5" });
  });

  it("says nothing when the recorded version is the loaded one", () => {
    const r = restore(CODE_SESSION);
    expect(r).toMatchObject({ error: "", versionNote: "", notesSession: false, readOnlyNotice: "" });
    expect(r.workflow?.workflowId).toBe("basic-agent@0.0.5:81795ea9");
  });

  it("refuses only when the workflow's bundle is gone altogether", () => {
    const r = restore(GONE_SESSION);
    expect(r.workflow).toBeUndefined();
    expect(r.error).toBe(WORKFLOW_GONE);
    expect(r.error).not.toMatch(/exact|gateway/i);
    // Not decided while the catalog loads.
    expect(restore(GONE_SESSION, { catalogLoading: true }).error).toBe("");
  });

  it("a Notes session opened by link reads only, with a plain note, and restores no workflow (no version banner)", () => {
    const viaRun = restore(CAPTURE_SESSION);
    expect(viaRun).toMatchObject({ notesSession: true, readOnlyNotice: NOTES_SESSION_NOTICE, error: "", versionNote: "" });
    expect(viaRun.workflow).toBeUndefined();
    // A link naming only the session: known from the runs page before its run loads.
    const viaList = conversationRestore({ sessionId: WEB_CAPTURE_SESSION, runId: "", restoredInputs: null, run: null, workflows: workflows(), conversationSessions: [], notesSessions: [{ sessionId: WEB_CAPTURE_SESSION }], catalogLoading: false });
    expect(viaList.readOnlyNotice).toBe(NOTES_SESSION_NOTICE);
    expect(NOTES_SESSION_NOTICE).not.toMatch(/session|workflow|bundle|gateway/i);
  });

  it("a listed conversation whose LATEST turn ran a Notes flow stays continuable (the root decides)", () => {
    const code = run(CODE_SESSION);
    const recallRun = { ...code, run_id: "recall-turn", workflow_id: "abstractnote@0.3.1:note-recall" };
    const recallInput = { ...F.input_data[code.run_id], run_id: "recall-turn", workflow_id: "abstractnote@0.3.1:note-recall", bundle_id: "abstractnote@0.3.1" };
    const listed = conversationRestore({
      sessionId: CODE_SESSION, runId: "recall-turn", restoredInputs: recallInput, run: recallRun, workflows: workflows(),
      conversationSessions: [{ sessionId: CODE_SESSION }], notesSessions: [], catalogLoading: false,
    });
    expect(listed.notesSession).toBe(false);
    expect(listed.readOnlyNotice).toBe("");
  });

  it("falls back across registries only for a recorded selection, never for a bare run id", () => {
    const shared = workflows().map((w) => ({ ...w, id: w.id.replace("private:", "tenant_catalog:"), registryScope: "tenant_catalog" as const }));
    const old = run(OLD_VERSION_SESSION);
    const input = F.input_data[old.run_id];
    const selection = { ...input, workflow_selection: input.input_data.workflow_selection };
    expect(resolveConversationWorkflow(shared, { inputData: selection }).workflow?.registryScope).toBe("tenant_catalog");
    expect(resolveConversationWorkflow(shared, { runWorkflowId: old.workflow_id }).workflow).toBeUndefined();
  });
});

describe("Code does not start Notes' work", () => {
  it("the workflow picker leaves Notes' flows out (\"Ask your notes\" serves the chat contract)", () => {
    const listed = { interfaceId: "abstractcode.agent.v1", gatewayDefault: { status: "unavailable" as const, reason: "x" }, entries: [{ bundleId: "abstractnote", flowId: "note-recall" }, { bundleId: "basic-agent", flowId: "81795ea9" }] };
    expect(withoutNotesEntries(listed).entries.map((e) => e.bundleId)).toEqual(["basic-agent"]);
  });
});

describe("Conversations Archived · N", () => {
  it("does not count Notes' archived sessions; nothing archived means no extra read", async () => {
    const archived = { ...F.runs, items: F.runs.items.filter((r: any) => r.session_kind === "chat") };
    const request = vi.fn(async () => archived);
    // Served 5: the archived listing holds 2 Notes sessions (watch and web captures).
    expect(await archivedConversationCount({ ...F.runs, archived_sessions: 5 }, request)).toBe(3);
    expect(request).toHaveBeenCalledTimes(1);
    expect(await archivedConversationCount({ ...F.runs, archived_sessions: 0 }, request)).toBe(0);
    expect(request).toHaveBeenCalledTimes(1);
    expect(await archivedConversationCount({ ...F.runs, archived_sessions: 4 }, vi.fn(async () => Promise.reject(new Error("down"))))).toBe(4);
  });
});

describe("Automations list", () => {
  const client = (): AutomationsClient =>
    ({
      listAutomations: vi.fn(async (q: any = {}) => (q.status === "archived" ? F.automations_archived : F.automations)),
      getMyEmail: vi.fn(async () => null),
    }) as unknown as AutomationsClient;

  it("leaves out Notes' automations (Process, Reflect, Consolidate) and counts what it shows", async () => {
    expect(F.automations.items.filter(isNotesAutomation).map((s: any) => s.title).sort()).toEqual(["Consolidate my notes", "Process my notes", "Reflect on my notes"]);
    const ctl = new AutomationsController(client(), vi.fn(), []);
    await ctl.refresh();
    expect(ctl.state.items.map((s) => s.title)).toEqual(["Morning news brief", "Mailbox watch"]);
    const { main, paused } = automationSections(ctl.state.items);
    expect(main.map((s) => s.title)).toEqual(["Morning news brief"]);
    expect(paused.map((s) => s.title)).toEqual(["Mailbox watch"]);
    // Served: 3 paused (Reflect and Consolidate are Notes'), 3 archived (two are Notes').
    expect(ctl.state.pausedCount).toBe(1);
    expect(ctl.state.archivedCount).toBe(1);
    await ctl.loadArchived();
    expect(ctl.state.archived?.map((s) => s.title)).toEqual(["Memory monitor"]);
  });
});
