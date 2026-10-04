// Round 13 (R13.4): the gateway's command sandbox on the Code surfaces.
// 1. Tools settings: a process-spawning tool's card shows the gateway's state ("Sandboxed to this
//    run's workspaces", or the refused / flag state) verbatim, with the gateway's sentence in the
//    kit tooltip; every other tool shows none.
// 2. Activity (the run view): each command call shows ONE line "Sandbox: <label> · N workspaces
//    enforced" or "Sandbox: none — refused" in its detail, from the ledger's evidence.
// Fixtures: a real GET /discovery/tools answer and a seeded ledger (a real execute_command record
// of a scratch gateway + the runtime's fail-closed refusal + a read_file).
import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import discovery from "./fixtures/discovery_tools_command_sandbox.json";
import ledger from "./fixtures/sandbox_ledger.json";
import { normalizeToolCatalog } from "./catalog";
import { DEFAULT_PREFERENCES, SettingsContent } from "./settings_panel";
import { ActivityGroups } from "./workspace_panels";
import { activity_rows } from "../lib/activity_rows";

const SANDBOXED = "Sandboxed to this run's workspaces";
const SPAWNING = ["execute_command", "local_helper_start", "shell_exec"];
const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/'/g, "&#x27;").replace(/"/g, "&quot;");

function payload(state: "sandboxed" | "refused" | "unsandboxed") {
  const p = JSON.parse(JSON.stringify(discovery));
  if (state === "refused") {
    Object.assign(p.command_sandbox, { state: "refused", kind: "none", line: "Commands refused: no sandbox on this host", sentence: "Commands are refused because this host has no command sandbox (macOS sandbox-exec, Linux bubblewrap or Landlock); restart the gateway with --unsandboxed-commands to allow them unsandboxed." });
    for (const item of p.items) if ("sandboxed" in item) Object.assign(item, { sandboxed: false, sandbox: "Refused: no command sandbox on this host" });
  } else if (state === "unsandboxed") {
    Object.assign(p.command_sandbox, { state: "unsandboxed", kind: "none", line: "Unsandboxed commands allowed (flag)", sentence: "This host has no command sandbox and the gateway was started with --unsandboxed-commands: commands run with the gateway's own file access (their environment is still scrubbed)." });
    for (const item of p.items) if ("sandboxed" in item) Object.assign(item, { sandboxed: false, sandbox: "Not sandboxed: unsandboxed commands allowed (flag)" });
  }
  return p;
}

function toolsTab(tools = normalizeToolCatalog(payload("sandboxed"))) {
  // Every tool enabled, so the kit renders every card.
  const enabled = tools.map((t) => ({ ...t, enabled: true, servedDisabled: false }));
  return renderToStaticMarkup(<SettingsContent tab="tools" value={DEFAULT_PREFERENCES} onChange={() => {}} tools={enabled} disabled={false} />);
}

function card(html: string, name: string): string {
  return html.split(/<div class="af-tool-row(?: is-enabled)?">/).find((r) => r.includes(`aria-label="${name}"`)) || "";
}

describe("tool cards: the gateway's command-sandbox state", () => {
  it("normalizes the gateway's row fields and sentence, nothing for other tools", () => {
    const tools = normalizeToolCatalog(discovery);
    const byName = Object.fromEntries(tools.map((t) => [t.name, t]));
    for (const name of SPAWNING) {
      expect(byName[name].sandboxState).toEqual({ label: SANDBOXED, tooltip: discovery.command_sandbox.sentence, tone: "ok" });
    }
    expect(byName.read_file.sandboxState).toBeUndefined();
    expect(byName.write_file.sandboxState).toBeUndefined();
  });

  it.each([
    ["sandboxed", SANDBOXED, "is-ok"],
    ["refused", "Refused: no command sandbox on this host", "is-warn"],
    ["unsandboxed", "Not sandboxed: unsandboxed commands allowed (flag)", "is-danger"],
  ] as const)("%s: the state on each spawning card, verbatim, with the sentence in the kit tooltip", (state, label, tone) => {
    const p = payload(state);
    const html = toolsTab(normalizeToolCatalog(p));
    for (const name of SPAWNING) {
      const c = card(html, name);
      expect(c).toContain(`data-tool-state="${name}"`);
      expect(c).toContain(`>${esc(label)}</span>`);
      expect(c).toContain(`data-af-tip="${esc(p.command_sandbox.sentence)}"`);
      expect(c).toContain(`af-tool-row__state ${tone}`);
    }
    expect(card(html, "read_file")).not.toContain("af-tool-row__state");
    expect((html.match(/data-tool-state=/g) || []).length).toBe(3);
  });

  it("a tool the gateway does not mark gets no state (no client-side guess)", () => {
    const p = payload("sandboxed");
    for (const item of p.items) if (item.name === "execute_command") { delete item.sandboxed; delete item.sandbox; }
    const html = toolsTab(normalizeToolCatalog(p));
    expect(card(html, "execute_command")).not.toContain("af-tool-row__state");
    expect(card(html, "shell_exec")).toContain("af-tool-row__state");
  });

  it("the app never spells the gateway's sandbox state", () => {
    for (const file of ["./catalog.ts", "./settings_panel.tsx", "./app.tsx"]) {
      const src = readFileSync(new URL(file, import.meta.url), "utf8");
      expect(src).not.toContain("Sandboxed to this run");
      expect(src).not.toContain("no command sandbox on this host");
    }
  });
});

describe("run view: one sandbox line per command, from the ledger", () => {
  const entries = ledger.records.map((record: unknown, i: number) => ({ runId: "run-1", cursor: i + 1, record }));

  it("each command call shows its line in the tool-call detail; read_file shows none", () => {
    const html = renderToStaticMarkup(<ActivityGroups rows={activity_rows(entries, "run-1")} />);
    const count = (sub: string) => html.split(sub).length - 1;
    expect(count("Sandbox: macOS sandbox-exec · 4 workspaces enforced")).toBe(1);
    expect(count("Sandbox: none — refused")).toBe(1);
    expect(count('class="pc-tool-sandbox ')).toBe(2);
    // In the call's detail, before its parameters, with the enforced paths.
    const exec = html.slice(html.indexOf("Sandbox: macOS sandbox-exec"));
    expect(html.lastIndexOf('class="pc-tool-activity__detail"', html.indexOf("Sandbox: macOS sandbox-exec"))).toBeGreaterThan(-1);
    expect(exec.indexOf("<code>/Users/ada/home/work/project</code>")).toBeGreaterThan(-1);
    expect(exec.indexOf("12 built-in protected folders refused")).toBeGreaterThan(-1);
    expect(exec.indexOf("Sandbox: macOS sandbox-exec")).toBeLessThan(exec.indexOf("Parameters"));
  });
});
