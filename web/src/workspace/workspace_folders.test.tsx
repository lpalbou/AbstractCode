// Round 9 FINAL wording (R9.3): Code's Workspace rail panel = the kit
// WorkspaceChooser over the gateway's workspace model. The account's
// narrowing through GET/PUT api/gateway/workspace/policy/me; an automation's
// chosen set in input_data.workspace_allowed_paths, only among the workspaces
// the gateway lists. Code holds no policy logic (no path checks, no clamp).
import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  WORKSPACE_CHOOSER_TEXT as T,
  WorkspaceChooser,
  workspaceAccountView,
  workspaceChooserClient,
  workspaceModeBody,
  workspaceRefusal,
  workspaceSelectionAfterToggle,
  workspaceSelectionView,
  type WorkspaceAccountState,
} from "@abstractframework/ui-kit";
import { automationRunPreferences, withAutomationRunPreferences } from "./automation_settings";
import { DEFAULT_PREFERENCES, SettingsContent } from "./settings_panel";
import { codeWorkspaceRequest } from "./workspace_folders";

const SHARED = "/srv/gw/workspaces";
const P = "/data/project";
const AR = "/archive";
const SEC = "/secrets";
const LINE = "Deny everything, allow listed workspaces · Shared workspace (rw) · /data/project (rw) · /archive (ro)";

const state = (): WorkspaceAccountState => ({
  policy: { account: "default:alice", default_mode: null, folders: [] },
  gateway: { shared_workspace: SHARED, posture: "allowed_only", default_mode: "rw", folders: [{ path: P, mode: "rw" }, { path: AR, mode: "ro" }] },
  effective: {
    account: "default:alice",
    posture: "allowed_only",
    default_mode: null,
    shared_workspace: SHARED,
    folders: [{ path: SHARED, mode: "rw", source: "shared" }, { path: P, mode: "rw", source: "gateway" }, { path: AR, mode: "ro", source: "gateway" }],
    summary: LINE,
  },
});

function jsonResponse(status: number, body: unknown) {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

afterEach(() => vi.unstubAllGlobals());

describe("account workspaces through the app proxy (GET/PUT workspace/policy/me)", () => {
  it("loads and writes the caller's own policy with ONE PUT per change", async () => {
    const calls: Array<[string, RequestInit]> = [];
    vi.stubGlobal("document", { cookie: "abstractcode_gateway_csrf=tok" });
    vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit) => {
      calls.push([url, init]);
      return jsonResponse(200, { ok: true, ...state() });
    }));
    const client = workspaceChooserClient(codeWorkspaceRequest);
    const loaded = await client.load();
    expect(calls[0][0]).toBe("api/gateway/workspace/policy/me");
    expect(calls[0][1].method).toBe("GET");
    const row = workspaceAccountView(loaded).rows.find((r) => r.path === P)!;
    await client.put(workspaceModeBody(loaded, row, "ro"));
    expect(calls[1][0]).toBe("api/gateway/workspace/policy/me");
    expect(calls[1][1].method).toBe("PUT");
    expect(JSON.parse(String(calls[1][1].body))).toEqual({ folders: [{ path: P, mode: "ro" }] });
    expect(new Headers(calls[1][1].headers).get("X-AbstractCode-CSRF")).toBe("tok");
  });

  it("shows the gateway's refusal sentence verbatim + 'Not saved.' (no local validation)", async () => {
    vi.stubGlobal("document", { cookie: "" });
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse(400, { detail: `${AR} cannot be raised above read-only.` })));
    let message = "";
    try {
      await workspaceChooserClient(codeWorkspaceRequest).put({ folders: [{ path: AR, mode: "deny" }] });
    } catch (e) {
      message = workspaceRefusal(e);
    }
    expect(message).toBe(`${AR} cannot be raised above read-only. Not saved.`);
  });

  it("a pre-round-9 gateway answer fails loudly (no empty chooser)", async () => {
    vi.stubGlobal("document", { cookie: "" });
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse(200, { ok: true, policy: { mode: "whitelist" } })));
    await expect(workspaceChooserClient(codeWorkspaceRequest).load()).rejects.toThrow(/round-9 workspace model/);
  });
});

describe("the chooser as Code shows it", () => {
  it("posture, shared workspace (rw), each workspace with Read & write / Read-only / Refused, the gateway line", () => {
    const html = renderToStaticMarkup(<WorkspaceChooser idPrefix="code-workspace-account" state={state()} onPut={async () => {}} />);
    expect(html).toContain(`>${T.title}<`);
    expect(html).toContain(`>${T.postureAllowedOnly}<`);
    expect(html).toContain('data-workspace="shared-always"');
    expect(html).toContain(`data-path="${P}"`);
    expect(html).toContain(`data-path="${AR}"`);
    // /archive is read-only for the admin: Read & write is unavailable (cannot raise).
    const ar = (new RegExp(`<li[^>]*data-path="${AR}"[\\s\\S]*?</li>`).exec(html) || [""])[0];
    expect(ar).toMatch(/aria-disabled="true"[^>]*data-af-tip="The gateway admin allows read only\."[^>]*data-action="workspace-mode-rw"/);
    expect(html).toContain(T.adminOnlyAdds);
    expect(html).not.toContain('data-workspace="add"');
    expect(html).toContain(`data-workspace="effective">${LINE}<`);
    expect(html).not.toMatch(/>[^<]*\bfolders?\b[^<]*</i);
  });

  it("the Workspace tab mounts the chooser (no access modes, no root field, no 'Any folder')", () => {
    const html = renderToStaticMarkup(
      <SettingsContent tab="workspace" value={DEFAULT_PREFERENCES} onChange={() => {}} tools={[]} disabled={false} connected />,
    );
    expect(html).toContain('id="code-workspace-account"');
    expect(html).toContain(`>${T.title}<`);
    expect(html).not.toMatch(/Access mode|Workspace root|Any folder|workspace_or_allowed/);
    const offline = renderToStaticMarkup(
      <SettingsContent tab="workspace" value={DEFAULT_PREFERENCES} onChange={() => {}} tools={[]} disabled connected={false} />,
    );
    expect(offline).toContain("Connect to your gateway to change workspaces.");
    const automation = renderToStaticMarkup(
      <SettingsContent tab="workspace" value={DEFAULT_PREFERENCES} onChange={() => {}} tools={[]} disabled={false} connected automationFolders workspaceRootFixed="/srv/gw/workspaces/automation-1" />,
    );
    expect(automation).toContain('id="code-workspace-automation"');
    expect(automation).toContain("automation-1");
  });

  it("Code has no policy logic of its own (X4): no path checks, clamps or deny lists in the panel", () => {
    const src = readFileSync(new URL("./workspace_folders.tsx", import.meta.url), "utf8");
    expect(src).not.toMatch(/never_allowed|available_folders|default_mode|\.filter\(|startsWith\(/);
  });
});

describe("an automation stores its chosen set within the allowance", () => {
  const eff = state().effective;

  it("absent = follows the account; a choice is stored as workspace_allowed_paths; the old access mode is dropped", () => {
    const legacy = { prompt: "x", workspace_access_mode: "all_except_ignored" };
    const prefs = automationRunPreferences(legacy);
    expect(prefs.workspaceFolders).toBeNull();
    const view = workspaceSelectionView(eff, prefs.workspaceFolders);
    expect(view.follows).toBe(true);
    const next = withAutomationRunPreferences(legacy, { ...prefs, workspaceFolders: workspaceSelectionAfterToggle(view, AR, false) });
    expect(next.workspace_allowed_paths).toEqual([P]);
    expect(next.workspace_access_mode).toBeUndefined();
    expect(withAutomationRunPreferences(next, { ...prefs, workspaceFolders: null }).workspace_allowed_paths).toBeUndefined();
    expect(withAutomationRunPreferences(next, { ...prefs, workspaceFolders: [] }).workspace_allowed_paths).toEqual([]);
  });

  it("a workspace the admin did not list (or refused) can never be stored", () => {
    const withRefused = { ...eff, folders: [...eff.folders, { path: SEC, mode: "deny" as const, source: "gateway" }] };
    const def = { prompt: "x", workspace_allowed_paths: [AR, SEC, "/not/listed"] };
    const prefs = automationRunPreferences(def);
    const view = workspaceSelectionView(withRefused, prefs.workspaceFolders);
    expect(view.rows.map((r) => r.path)).toEqual([P, AR]);
    for (const path of [SEC, "/not/listed"]) {
      const stored = withAutomationRunPreferences(def, { ...prefs, workspaceFolders: workspaceSelectionAfterToggle(view, path, true) }).workspace_allowed_paths;
      expect(stored).not.toContain(path);
    }
    expect(withAutomationRunPreferences(def, { ...prefs, workspaceFolders: workspaceSelectionAfterToggle(view, P, true) }).workspace_allowed_paths).toEqual([P, AR]);
  });
});
