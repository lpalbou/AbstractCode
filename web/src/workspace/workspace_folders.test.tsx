// Round 11 (DESIGN R11.1 FINAL / R11.6): Code's Workspace rail panel = the
// kit WorkspaceChooser at the SESSION level (this conversation's workspaces,
// stored by the gateway on the session: GET/PUT api/gateway/sessions/{id}/workspaces),
// "Use my default" = {configured:false}, a "My default workspaces" link to the
// ACCOUNT level (PUT api/gateway/workspace/policy/me), and an automation at the
// RUN level (input_data.workspace, the gateway's dry run for what it means).
// Code holds no policy logic (no path checks, no clamp, no caps).
import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  WORKSPACE_CHOOSER_TEXT as T,
  WorkspaceChooser,
  workspaceAsState,
  workspaceChooserClient,
  workspaceDryRun,
  workspaceModePayload,
  workspaceRefusal,
} from "@abstractframework/ui-kit";
import { automationRunPreferences, withAutomationRunPreferences } from "./automation_settings";
import { DEFAULT_PREFERENCES, SettingsContent } from "./settings_panel";
import { codeWorkspaceRequest, CodeSessionWorkspaces } from "./workspace_folders";

const PICS = "/Users/alice/Pictures";
const DOCS = "/Users/alice/Documents";
const GW_LINE = "Allow everything, refuse listed workspaces (rw) · /Users/alice/Documents (ro)";
const LINE = "Deny everything, allow listed workspaces · /Users/alice/Pictures (rw) · /Users/alice/Documents (ro)";
const SID = "session-abc 1";

const effective = (level: string) => ({
  ok: true,
  account: "default:alice",
  session_id: SID,
  level,
  posture: "allowed_only",
  default_mode: "rw",
  folders: [
    { path: PICS, mode: "rw", cap: "rw", source: level },
    { path: DOCS, mode: "ro", cap: "ro", source: "gateway" },
  ],
  summary: LINE,
  gateway_summary: GW_LINE,
});
const sessionAnswer = (configured = true) => ({
  ok: true,
  policy: configured
    ? { session_id: SID, account: "default:alice", configured: true, posture: "allowed_only", default_mode: "rw", folders: [{ path: PICS, mode: "rw" }, { path: DOCS, mode: "ro" }] }
    : { session_id: SID, account: "default:alice", configured: false, posture: "any_except_denied", default_mode: "rw", folders: [] },
  gateway: { posture: "any_except_denied", default_mode: "rw", folders: [{ path: DOCS, mode: "ro" }], summary: GW_LINE },
  account_default: effective("account"),
  effective: effective(configured ? "session" : "account"),
});

function jsonResponse(status: number, body: unknown) {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

afterEach(() => vi.unstubAllGlobals());

describe("the conversation's workspaces = the session level (GET/PUT sessions/{id}/workspaces)", () => {
  it("loads and writes THIS session's workspaces with ONE PUT per change (never the account route)", async () => {
    const calls: Array<[string, RequestInit]> = [];
    vi.stubGlobal("document", { cookie: "abstractcode_gateway_csrf=tok" });
    vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit) => {
      calls.push([url, init]);
      return jsonResponse(200, sessionAnswer());
    }));
    const client = workspaceChooserClient(codeWorkspaceRequest, { level: "session", session: SID });
    const loaded = await client.load();
    expect(calls[0][0]).toBe(`api/gateway/sessions/${encodeURIComponent(SID)}/workspaces`);
    expect(calls[0][1].method).toBe("GET");
    await client.save(workspaceModePayload("session", loaded.policy, PICS, "ro"));
    expect(calls).toHaveLength(2);
    expect(calls[1][0]).toBe(`api/gateway/sessions/${encodeURIComponent(SID)}/workspaces`);
    expect(calls[1][1].method).toBe("PUT");
    expect(JSON.parse(String(calls[1][1].body))).toEqual({
      configured: true,
      posture: "allowed_only",
      default_mode: "rw",
      folders: [{ path: PICS, mode: "ro" }, { path: DOCS, mode: "ro" }],
    });
    expect(new Headers(calls[1][1].headers).get("X-AbstractCode-CSRF")).toBe("tok");
    await client.save({ configured: false });
    expect(JSON.parse(String(calls[2][1].body))).toEqual({ configured: false });
    expect(calls.every(([url]) => !url.includes("workspace/policy"))).toBe(true);
  });

  it("a refused path shows the gateway's sentence verbatim + 'Not saved.' (FastAPI-wrapped refusal)", async () => {
    vi.stubGlobal("document", { cookie: "" });
    const sentence = "/etc is outside the workspaces the gateway allows.";
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse(400, { detail: { reason: "workspace_refused", message: sentence, path: "/etc" } })));
    let message = "";
    try {
      await workspaceChooserClient(codeWorkspaceRequest, { level: "session", session: SID }).save({ configured: true, posture: "allowed_only", default_mode: "rw", folders: [{ path: "/etc", mode: "ro" }] });
    } catch (e) {
      message = workspaceRefusal(e);
    }
    expect(message).toBe(`${sentence} Not saved.`);
  });

  it("a pre-round-11 gateway answer fails loudly (no empty chooser)", async () => {
    vi.stubGlobal("document", { cookie: "" });
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse(200, { ok: true, policy: { shared_workspace: "/srv", default_mode: null, folders: [] } })));
    await expect(workspaceChooserClient(codeWorkspaceRequest, { level: "session", session: SID }).load()).rejects.toThrow(/older workspace model/);
  });
});

describe("the panel as Code shows it", () => {
  it("Gateway line, posture, rows with Read-only | Read & write | Refused, Use my default, the effective line, the My default workspaces link", () => {
    const state = workspaceAsState(sessionAnswer(), "session");
    const html = renderToStaticMarkup(
      <WorkspaceChooser level="session" idPrefix="code-workspace-session" state={state} save={async () => {}} />,
    );
    expect(html).toContain(`${T.gatewayPrefix} ${GW_LINE}`);
    expect(html).toContain(`>${T.postureAllowedOnly}<`);
    expect(html).toContain(`>${T.useDefault}<`);
    expect(html).toContain(`data-path="${PICS}"`);
    for (const label of [T.accessRead, T.accessReadWrite, T.accessDenied]) expect(html).toContain(`>${label.replace("&", "&amp;")}<`);
    // /Users/alice/Documents is capped read-only by the gateway: Read & write is disabled with the tooltip.
    expect(html).toContain(T.capReadOnly);
    expect(html).toContain(`data-workspace="effective">${LINE}<`);
    expect(html).not.toMatch(/>[^<]*\b(folders?|shared workspace)\b[^<]*</i);
  });

  it("the conversation panel binds the open session and offers My default workspaces", () => {
    const offline = renderToStaticMarkup(<CodeSessionWorkspaces connected={false} sessionId={SID} onOpenDefaults={() => {}} />);
    expect(offline).toContain('id="code-workspace-session"');
    expect(offline).toContain("Connect to your gateway to change workspaces.");
    expect(offline).toContain('data-action="open-default-workspaces"');
    const html = renderToStaticMarkup(
      <SettingsContent tab="workspace" value={DEFAULT_PREFERENCES} onChange={() => {}} tools={[]} disabled={false} connected sessionId={SID} onOpenDefaultWorkspaces={() => {}} />,
    );
    expect(html).toContain('id="code-workspace-session"');
    expect(html).toContain("My default workspaces");
    expect(html).not.toMatch(/Access mode|Workspace root|Any folder|workspace_or_allowed/);
    const automation = renderToStaticMarkup(
      <SettingsContent tab="workspace" value={DEFAULT_PREFERENCES} onChange={() => {}} tools={[]} disabled={false} connected automationFolders workspaceRootFixed="/srv/gw/workspaces/automation-1" />,
    );
    expect(automation).toContain('id="code-workspace-automation"');
    expect(automation).toContain("automation-1");
    expect(automation).not.toContain('id="code-workspace-session"');
  });

  it("the app wires the session id, the account dialog and keeps the private workspace line", () => {
    const app = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
    expect(app).toMatch(/sessionId=\{automation \? undefined : session\.sessionId\}/);
    expect(app).toContain("<CodeDefaultWorkspacesDialog");
    expect(app).toContain("Current workspace <code");
    const panel = readFileSync(new URL("./workspace_folders.tsx", import.meta.url), "utf8");
    expect(panel).toContain('level: "session", session: sessionId');
    expect(panel).toMatch(/<WorkspaceChooser\s+level="session"/);
    expect(panel).toMatch(/title="My default workspaces"[\s\S]*level="account"/);
  });

  it("Code has no policy logic of its own: no path checks, clamps or caps in the panel", () => {
    const src = readFileSync(new URL("./workspace_folders.tsx", import.meta.url), "utf8");
    expect(src).not.toMatch(/\.cap\b|startsWith\(|\.filter\(|builtin_refused|["'`]api\/gateway\//);
  });
});

describe("an automation keeps its workspaces in its definition (run level)", () => {
  const value = { posture: "allowed_only" as const, default_mode: "rw" as const, folders: [{ path: PICS, mode: "ro" as const }] };

  it("absent = Use my default; a choice is stored as input_data.workspace; the old access mode is dropped", () => {
    const legacy = { prompt: "x", workspace_access_mode: "all_except_ignored" };
    const prefs = automationRunPreferences(legacy);
    expect(prefs.workspace).toBeNull();
    const next = withAutomationRunPreferences(legacy, { ...prefs, workspace: value });
    expect(next.workspace).toEqual(value);
    expect(next.workspace_access_mode).toBeUndefined();
    expect(automationRunPreferences(next).workspace).toEqual(value);
    expect(withAutomationRunPreferences(next, { ...prefs, workspace: null }).workspace).toBeUndefined();
  });

  it("a chosen payload replaces a stored R9 list; an untouched R9 list is left to the gateway", () => {
    const r9 = { prompt: "x", workspace_allowed_paths: [PICS] };
    const prefs = automationRunPreferences(r9);
    expect(withAutomationRunPreferences(r9, prefs).workspace_allowed_paths).toEqual([PICS]);
    const chosen = withAutomationRunPreferences(r9, { ...prefs, workspace: value });
    expect(chosen.workspace_allowed_paths).toBeUndefined();
    expect(chosen.workspace).toEqual(value);
  });

  it("what the payload means is the gateway's dry run (POST workspace/effective/me {workspace})", async () => {
    const calls: Array<[string, RequestInit]> = [];
    vi.stubGlobal("document", { cookie: "" });
    vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit) => {
      calls.push([url, init]);
      return jsonResponse(200, effective("run"));
    }));
    const answer = await workspaceDryRun(codeWorkspaceRequest)(value);
    expect(calls[0][0]).toBe("api/gateway/workspace/effective/me");
    expect(calls[0][1].method).toBe("POST");
    expect(JSON.parse(String(calls[0][1].body))).toEqual({ workspace: value });
    expect(answer.summary).toBe(LINE);
  });
});
