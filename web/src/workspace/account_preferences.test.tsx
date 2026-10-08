import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { WorkflowDefinition } from "./catalog";
import {
  ACCOUNT_PREFERENCES_PATH,
  accountTimeZone,
  accountValueFromSelection,
  accountWorkflowRow,
  loadAccountWorkflow,
  selectionFromAccountValue,
  type GatewayCall,
} from "./account_preferences";
import { AccountWorkflowDefault } from "./account_workflow_default";
import { AccountTimeZone } from "./account_time_zone";
import { parsePreferences, preferencesKey, writePreferences } from "./preferences";
import { DEFAULT_PREFERENCES } from "./settings_panel";

const IFACE = "abstractcode.agent.v1";

/** A fake gateway holding the account's value: GET/PUT /accounts/me/preferences (or 404). */
function fakeGateway(opts: { supported?: boolean; value?: string | null; runnable?: string[] } = {}) {
  const state = { value: opts.value ?? null, zone: null as string | null, calls: [] as Array<{ method: string; body: any }> };
  const runnable = opts.runnable ?? ["coding-agent:coder", "basic-agent:main"];
  const answer = () => ({
    ok: true,
    preferences: { default_workflow: { [IFACE]: state.value, "abstractassistant.agent.v1": null }, time_zone: state.zone },
    // R16.1 (W1 "API — FINAL" (4)): the time-zone block, served whole.
    time_zone: { value: state.zone, gateway_default: "Europe/Paris", effective: state.zone ?? "Europe/Paris", label: "Time zone", help: "Daily, weekly and monthly automations run on this clock. Gateway default follows this computer's time zone.", choices: ["America/Los_Angeles", "Asia/Tokyo", "Europe/Paris", "UTC"] },
    apps: [
      {
        interface: IFACE,
        value: state.value,
        state: state.value ? "set" : "default",
        reason: null,
        gateway_default_label: "Gateway default (Basic agent)",
        choices: runnable.map((v) => ({ value: v, label: v.split(":")[0], workflow_id: v })),
      },
    ],
  });
  const request: GatewayCall = async (path, init) => {
    expect(path).toBe(ACCOUNT_PREFERENCES_PATH);
    const method = String(init?.method || "GET");
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    state.calls.push({ method, body });
    if (opts.supported === false) throw Object.assign(new Error("Not Found"), { status: 404 });
    if (method === "PUT" && "time_zone" in body) {
      if (body.time_zone !== null && !["America/Los_Angeles", "Asia/Tokyo", "Europe/Paris", "UTC"].includes(body.time_zone))
        throw Object.assign(new Error(`'${body.time_zone}' is not a time zone this gateway knows (an IANA name such as Europe/Paris).`), { status: 400 });
      state.zone = body.time_zone;
    } else if (method === "PUT") {
      const v = body.default_workflow[IFACE];
      if (v !== null && !runnable.includes(v))
        throw Object.assign(new Error(`default_workflow.${IFACE} = '${v}' refused: workflow bundle 'gone' is not on this gateway.`), { status: 400 });
      state.value = v;
    }
    return answer();
  };
  return { state, request, puts: () => state.calls.filter((c) => c.method === "PUT").map((c) => c.body) };
}

const coder: WorkflowDefinition = {
  id: "private:coding-agent@0.2.0:coder",
  workflowId: "coding-agent@0.2.0:coder",
  bundleId: "coding-agent",
  bundleVersion: "0.2.0",
  flowId: "coder",
  name: "Coder",
  description: "",
  interfaces: [IFACE],
  registryScope: "private",
};

describe("account default workflow (R14.2)", () => {
  it("maps picker selections to version-less account values and back", () => {
    expect(accountValueFromSelection("@default")).toBeNull();
    expect(accountValueFromSelection("private:coding-agent@0.1.0:coder")).toBe("coding-agent:coder");
    expect(accountValueFromSelection("tenant_catalog:x@1:y")).toBe("catalog:x:y");
    expect(selectionFromAccountValue("coding-agent:coder", [coder])).toBe(coder.id);
    expect(selectionFromAccountValue(null, [coder])).toBe("@default");
    expect(selectionFromAccountValue("catalog:coding-agent:coder", [coder])).toBe("@default");
    expect(selectionFromAccountValue("gone:agent", [coder])).toBe("@default");
  });

  it("round-trips: reads the account value and saves one PUT", async () => {
    const gw = fakeGateway();
    let cleared = 0;
    const loaded = await loadAccountWorkflow(gw.request, "@default", () => (cleared += 1));
    expect(loaded.status).toBe("ok");
    if (loaded.status !== "ok") return;
    expect(loaded.row.value).toBeNull();
    expect(loaded.row.gatewayDefaultLabel).toBe("Gateway default (Basic agent)");
    expect(gw.puts()).toEqual([]);
    expect(cleared).toBe(0);
    await gw.request(ACCOUNT_PREFERENCES_PATH, { method: "PUT", body: JSON.stringify({ default_workflow: { [IFACE]: "coding-agent:coder" } }) });
    const again = await loadAccountWorkflow(gw.request, "@default", () => (cleared += 1));
    expect(again.status === "ok" && again.row.value).toBe("coding-agent:coder");
  });

  it("uploads this browser's old choice once, then clears it", async () => {
    const gw = fakeGateway();
    let cleared = 0;
    const loaded = await loadAccountWorkflow(gw.request, "private:coding-agent@0.1.0:coder", () => (cleared += 1));
    expect(gw.puts()).toEqual([{ default_workflow: { [IFACE]: "coding-agent:coder" } }]);
    expect(loaded.status === "ok" && loaded.row.value).toBe("coding-agent:coder");
    expect(cleared).toBe(1);
    // After the clear the browser holds "@default": nothing is uploaded again.
    await loadAccountWorkflow(gw.request, "@default", () => (cleared += 1));
    expect(gw.puts().length).toBe(1);
    expect(cleared).toBe(1);
  });

  it("an account choice made elsewhere wins; a refused upload clears; an older gateway keeps the browser's", async () => {
    const elsewhere = fakeGateway({ value: "basic-agent:main" });
    let cleared = 0;
    const kept = await loadAccountWorkflow(elsewhere.request, "private:coding-agent@0.1.0:coder", () => (cleared += 1));
    expect(elsewhere.puts()).toEqual([]);
    expect(kept.status === "ok" && kept.row.value).toBe("basic-agent:main");
    expect(cleared).toBe(1);

    const refused = fakeGateway();
    cleared = 0;
    const r = await loadAccountWorkflow(refused.request, "private:gone@1.0.0:agent", () => (cleared += 1));
    expect(refused.puts().length).toBe(1);
    expect(r.status === "ok" && r.row.value).toBeNull();
    expect(cleared).toBe(1);

    const old = fakeGateway({ supported: false });
    cleared = 0;
    expect(await loadAccountWorkflow(old.request, "private:coding-agent@0.1.0:coder", () => (cleared += 1))).toEqual({ status: "unsupported" });
    expect(cleared).toBe(0);
  });

  it("a network failure keeps the browser's choice for the next load", async () => {
    const gw = fakeGateway();
    let cleared = 0;
    const flaky: GatewayCall = async (path, init) => {
      if (init?.method === "PUT") throw Object.assign(new Error("Failed to fetch"), { status: 0 });
      return gw.request(path, init);
    };
    await loadAccountWorkflow(flaky, "private:coding-agent@0.1.0:coder", () => (cleared += 1));
    expect(cleared).toBe(0);
  });

  it("R16.1: reads the served time-zone block; a missing block fails loudly (the IANA list is never the browser's)", async () => {
    const gw = fakeGateway();
    const loaded = await loadAccountWorkflow(gw.request, "@default", () => undefined);
    expect(loaded.status === "ok" && loaded.timeZone).toEqual({ value: null, gateway_default: "Europe/Paris", effective: "Europe/Paris", label: "Time zone", help: "Daily, weekly and monthly automations run on this clock. Gateway default follows this computer's time zone.", choices: ["America/Los_Angeles", "Asia/Tokyo", "Europe/Paris", "UTC"] });
    const answer = await gw.request(ACCOUNT_PREFERENCES_PATH, { method: "PUT", body: JSON.stringify({ time_zone: "Asia/Tokyo" }) });
    expect(gw.puts()).toEqual([{ time_zone: "Asia/Tokyo" }]);
    expect(accountTimeZone(answer).value).toBe("Asia/Tokyo");
    expect(accountTimeZone(answer).effective).toBe("Asia/Tokyo");
    expect(() => accountTimeZone({ apps: [], preferences: { default_workflow: {} } })).toThrow(/no time_zone block/);
    const noTz = fakeGateway();
    const bare: GatewayCall = async (path, init) => {
      const a = (await noTz.request(path, init)) as Record<string, unknown>;
      delete a.time_zone;
      return a;
    };
    expect(await loadAccountWorkflow(bare, "@default", () => undefined)).toEqual({ status: "error", message: expect.stringMatching(/no time_zone block/) });
  });

  it("R16.1: Settings → Workflow shows the kit time-zone picker, Gateway default (<zone>) first, no Save", () => {
    const block = accountTimeZone({ time_zone: { value: null, gateway_default: "Europe/Paris", effective: "Europe/Paris", label: "Time zone", help: "h", choices: ["UTC"] } });
    const html = renderToStaticMarkup(<AccountTimeZone block={block} save={async () => undefined} />);
    expect(html).toContain('for="code-account-time-zone">Time zone</label>');
    expect(html).toContain("Gateway default (Europe/Paris)");
    expect(html).toContain('data-af-tip="h"');
    expect(html).not.toMatch(/>Save</);
    const set = renderToStaticMarkup(<AccountTimeZone block={{ ...block, value: "UTC" }} save={async () => undefined} />);
    expect(set).toContain(">UTC<");
  });

  it("rejects an answer without the app row (fails loudly, never guesses)", () => {
    expect(() => accountWorkflowRow({ apps: [], preferences: { default_workflow: {} } })).toThrow(/no row for abstractcode.agent.v1/);
    expect(() => accountWorkflowRow({ apps: [{ interface: IFACE }] })).toThrow(/no apps or default_workflow/);
  });

  it("the setting shows Gateway default (<name>) first, verbatim, and the broken reason", () => {
    const row = accountWorkflowRow({
      preferences: { default_workflow: { [IFACE]: "gone:agent" } },
      apps: [{ interface: IFACE, value: "gone:agent", state: "broken", reason: "gone:agent no longer runs for you: x. Pick another workflow or Gateway default.", gateway_default_label: "Gateway default (Basic agent)", choices: [{ value: "basic-agent:main", label: "Basic agent" }] }],
    });
    const html = renderToStaticMarkup(<AccountWorkflowDefault row={row} save={async () => undefined} />);
    expect(html).toContain("Default for new conversations");
    expect(html.indexOf('<option value="">Gateway default (Basic agent)</option>')).toBeGreaterThan(-1);
    expect(html.indexOf("Gateway default (Basic agent)")).toBeLessThan(html.indexOf("Basic agent</option>", html.indexOf("Gateway default (Basic agent)") + 30));
    expect(html).toContain("gone:agent (no longer runs)");
    expect(html).toContain("Pick another workflow or Gateway default.");
    expect(html).not.toMatch(/>Save</);
  });

  it("this browser stops storing the workflow key once it is the gateway default", () => {
    const store = new Map<string, string>();
    (globalThis as any).localStorage = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) };
    writePreferences("alice", { ...DEFAULT_PREFERENCES, workflow: "private:coding-agent@0.1.0:coder" });
    expect(JSON.parse(store.get(preferencesKey("alice"))!).workflow).toBe("private:coding-agent@0.1.0:coder");
    writePreferences("alice", { ...DEFAULT_PREFERENCES, workflow: "@default" });
    const saved = JSON.parse(store.get(preferencesKey("alice"))!);
    expect("workflow" in saved).toBe(false);
    expect(parsePreferences(store.get(preferencesKey("alice"))!).workflow).toBe("@default");
    delete (globalThis as any).localStorage;
  });
});
