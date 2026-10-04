// Round 9 (R9.3): Code's Workspace rail panel = the kit WorkspaceChooser over
// the gateway's folder model. The account's folders through
// GET/PUT api/gateway/workspace/policy/me; an automation's chosen set in
// input_data.workspace_allowed_paths, only among the folders the gateway
// lists. Code holds no policy logic (no path checks, no clamp).
import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  WORKSPACE_CHOOSER_TEXT as T,
  WorkspaceChooser,
  workspaceAccountView,
  workspaceChooserClient,
  workspaceExtraBody,
  workspaceRefusal,
  workspaceSelectionAfterToggle,
  workspaceSelectionView,
  type WorkspaceAccountState,
} from "@abstractframework/ui-kit";
import { automationRunPreferences, withAutomationRunPreferences } from "./automation_settings";
import { DEFAULT_PREFERENCES, SettingsContent } from "./settings_panel";
import { codeWorkspaceRequest } from "./workspace_folders";

const SHARED = "/srv/gw/workspaces";
const A = "/data/projects";
const B = "/data/notes";
const OWN = "/home/alice/thesis";
const NOT_ALLOWED = "/etc/secrets";

const state = (over: Partial<WorkspaceAccountState["effective"]> = {}, own: string[] = []): WorkspaceAccountState => ({
  policy: { account: "default:alice", enabled_folders: [A], own_folders: own },
  effective: {
    account: "default:alice",
    shared_workspace: SHARED,
    folders: [{ path: SHARED, source: "shared" }, { path: A, source: "allowed" }, ...own.map((path) => ({ path, source: "own" }))],
    available_folders: [{ path: A, enabled: true }, { path: B, enabled: false }],
    own_folders_allowed: false,
    own_folders_inactive: false,
    never_allowed: [NOT_ALLOWED],
    launch_folder_trust: true,
    summary: "Shared workspace + 1 folder. Never: 1 folder.",
    ...over,
  },
});

function jsonResponse(status: number, body: unknown) {
  return new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
}

afterEach(() => vi.unstubAllGlobals());

describe("account folders through the app proxy (GET/PUT workspace/policy/me)", () => {
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
    const body = workspaceExtraBody(workspaceAccountView(loaded), B, true);
    await client.put(body);
    expect(calls[1][0]).toBe("api/gateway/workspace/policy/me");
    expect(calls[1][1].method).toBe("PUT");
    expect(JSON.parse(String(calls[1][1].body))).toEqual({ enabled_folders: [A, B] });
    expect(new Headers(calls[1][1].headers).get("X-AbstractCode-CSRF")).toBe("tok");
  });

  it("shows the gateway's refusal sentence verbatim + 'Not saved.' (no local validation)", async () => {
    vi.stubGlobal("document", { cookie: "" });
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse(400, { detail: `Folder ${NOT_ALLOWED} is never allowed on this gateway.` })));
    let message = "";
    try {
      await workspaceChooserClient(codeWorkspaceRequest).put({ own_folders: [NOT_ALLOWED] });
    } catch (e) {
      message = workspaceRefusal(e);
    }
    expect(message).toBe(`Folder ${NOT_ALLOWED} is never allowed on this gateway. Not saved.`);
  });

  it("a pre-round-9 gateway answer fails loudly (no empty chooser)", async () => {
    vi.stubGlobal("document", { cookie: "" });
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse(200, { ok: true, policy: { mode: "whitelist" } })));
    await expect(workspaceChooserClient(codeWorkspaceRequest).load()).rejects.toThrow(/round-9 workspace model/);
  });
});

describe("the chooser as Code shows it", () => {
  it("shared workspace always on, admin-allowed folders as switches, My folders only when allowed, the effective line", () => {
    const html = renderToStaticMarkup(<WorkspaceChooser idPrefix="code-workspace-account" state={state()} onPut={async () => {}} />);
    expect(html).toContain(`>${T.title}<`);
    expect(html).toContain('data-workspace="shared-always"');
    expect(html.indexOf('data-setting="workspace-shared"')).toBeLessThan(html.indexOf('role="switch"'));
    const switches = [...html.matchAll(/role="switch"[^>]*aria-label="([^"]+)"|aria-label="([^"]+)"[^>]*role="switch"/g)].map((m) => m[1] || m[2]);
    expect(switches).toEqual([A, B]);
    expect(html).not.toContain(NOT_ALLOWED);
    expect(html).toContain(T.ownHidden);
    expect(html).not.toContain('data-workspace="own-add"');
    expect(html).toContain(`<strong>${T.effectivePrefix}</strong> Shared workspace + 1 folder. Never: 1 folder.`);
    const allowed = renderToStaticMarkup(<WorkspaceChooser state={state({ own_folders_allowed: true }, [OWN])} onPut={async () => {}} />);
    expect(allowed).toContain(`data-path="${OWN}"`);
    expect(allowed).toContain('data-workspace="own-add"');
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
    expect(offline).toContain("Connect to your gateway to change workspace folders.");
    const automation = renderToStaticMarkup(
      <SettingsContent tab="workspace" value={DEFAULT_PREFERENCES} onChange={() => {}} tools={[]} disabled={false} connected automationFolders workspaceRootFixed="/srv/gw/workspaces/automation-1" />,
    );
    expect(automation).toContain('id="code-workspace-automation"');
    expect(automation).toContain("automation-1");
  });

  it("Code has no policy logic of its own (X4): no path checks, clamps or deny lists in the panel", () => {
    const src = readFileSync(new URL("./workspace_folders.tsx", import.meta.url), "utf8");
    expect(src).not.toMatch(/never_allowed|available_folders|allowed_folders|\.filter\(|startsWith\(/);
  });
});

describe("an automation stores its chosen set within the allowance", () => {
  const eff = state({
    folders: [{ path: SHARED, source: "shared" }, { path: A, source: "allowed" }, { path: OWN, source: "own" }],
  }).effective;

  it("absent = follows the account; a choice is stored as workspace_allowed_paths; the old access mode is dropped", () => {
    const legacy = { prompt: "x", workspace_access_mode: "all_except_ignored" };
    const prefs = automationRunPreferences(legacy);
    expect(prefs.workspaceFolders).toBeNull();
    const view = workspaceSelectionView(eff, prefs.workspaceFolders);
    expect(view.follows).toBe(true);
    const next = withAutomationRunPreferences(legacy, { ...prefs, workspaceFolders: workspaceSelectionAfterToggle(view, OWN, false) });
    expect(next.workspace_allowed_paths).toEqual([A]);
    expect(next.workspace_access_mode).toBeUndefined();
    expect(withAutomationRunPreferences(next, { ...prefs, workspaceFolders: null }).workspace_allowed_paths).toBeUndefined();
    expect(withAutomationRunPreferences(next, { ...prefs, workspaceFolders: [] }).workspace_allowed_paths).toEqual([]);
  });

  it("a folder the admin did not allow (or the account has not switched on) can never be stored", () => {
    // B is admin-allowed but OFF for this account; NOT_ALLOWED is denied. A stale definition holds both.
    const def = { prompt: "x", workspace_allowed_paths: [OWN, B, NOT_ALLOWED] };
    const prefs = automationRunPreferences(def);
    const view = workspaceSelectionView(eff, prefs.workspaceFolders);
    expect(view.extras.map((r) => r.path)).toEqual([A, OWN]);
    for (const path of [B, NOT_ALLOWED]) {
      const chosen = workspaceSelectionAfterToggle(view, path, true);
      const stored = withAutomationRunPreferences(def, { ...prefs, workspaceFolders: chosen }).workspace_allowed_paths;
      expect(stored).not.toContain(path);
    }
    const stored = withAutomationRunPreferences(def, { ...prefs, workspaceFolders: workspaceSelectionAfterToggle(view, A, true) }).workspace_allowed_paths;
    expect(stored).toEqual([A, OWN]);
  });
});
