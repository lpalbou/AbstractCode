// Operator rule (2026-09-30, DESIGN §2/§8): every persistent on/off in the app is the kit's
// AfSwitch labelled by the FEATURE — highlighted when on, plain when off, unavailable with the
// reason — never a checkbox or a verb pair ("Pause queue" / "Resume queue"). These tests go red
// when a surface falls back to the old control.
import React from "react";
import { existsSync, readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { AfSwitch } from "@abstractframework/ui-kit";

import { SkillSwitch, type GatewaySkill } from "./skills_picker";

const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
const css = readFileSync(new URL("./workspace.css", import.meta.url), "utf8");
const catalogSource = readFileSync(new URL("./use_workspace_catalog.ts", import.meta.url), "utf8");

const skill = (over: Partial<GatewaySkill> = {}): GatewaySkill => ({
  name: "coredoc",
  description: "Documentation discipline",
  trustLevel: "adopted",
  blocked: false,
  requiresReview: false,
  reasons: [],
  ...over,
});

/** The AfSwitch element a component returns (components are not expanded). */
function switchOf(node: React.ReactElement): React.ReactElement<React.ComponentProps<typeof AfSwitch>> {
  const found: React.ReactElement[] = [];
  const walk = (n: unknown) => {
    if (Array.isArray(n)) n.forEach(walk);
    else if (React.isValidElement(n)) {
      if (n.type === AfSwitch) found.push(n);
      walk((n.props as { children?: unknown }).children);
    }
  };
  walk(node);
  expect(found).toHaveLength(1);
  return found[0] as React.ReactElement<React.ComponentProps<typeof AfSwitch>>;
}

describe("Run settings → Skills: one switch per skill", () => {
  it("an attached skill is ON (aria-checked, label = the skill), with its description and trust as helper text", () => {
    const html = renderToStaticMarkup(<SkillSwitch skill={skill()} checked disabled={false} onChange={() => {}} />);
    expect(html).toMatch(/role="switch" class="af-switch af-switch--row" data-action="skill" aria-checked="true"/);
    expect(html).toContain('<span class="af-switch__label">coredoc</span>');
    expect(html).toContain("Documentation discipline");
    expect(html).not.toContain('type="checkbox"');
  });

  it("switching calls back with the new state", () => {
    const onChange = vi.fn();
    const el = switchOf(SkillSwitch({ skill: skill(), checked: false, disabled: false, onChange }));
    expect(el.props.checked).toBe(false);
    el.props.onChange?.(true);
    expect(onChange).toHaveBeenCalledWith(true);
  });

  it("a blocked skill stays visible, unavailable, with the gateway's reason", () => {
    const html = renderToStaticMarkup(
      <SkillSwitch skill={skill({ name: "sketchy", blocked: true, reasons: ["Not trusted."] })} checked={false} disabled={false} onChange={() => {}} />,
    );
    expect(html).toMatch(/role="switch"[^>]*aria-checked="false" aria-disabled="true" aria-describedby="[^"]+"/);
    expect(html).toContain('class="af-switch__reason"');
    expect(html).toContain("Blocked by the gateway: Not trusted.");
  });

  it("while settings are locked the switch points at the panel's visible banner", () => {
    const html = renderToStaticMarkup(
      <SkillSwitch skill={skill()} checked disabled onChange={() => {}} lockedReasonId="code-settings-locked" />,
    );
    expect(html).toMatch(/role="switch"[^>]*aria-disabled="true" aria-describedby="code-settings-locked"/);
    // Without a banner to point at, the reason renders as visible text under the row.
    const alone = renderToStaticMarkup(<SkillSwitch skill={skill()} checked disabled onChange={() => {}} />);
    expect(alone).toMatch(/<span id="[^"]+" class="af-switch__reason">Available when a gateway is connected and no run is in progress\.<\/span>/);
  });

  it("the Run settings banner carries the id the skill switches point at", () => {
    const panel = readFileSync(new URL("./settings_panel.tsx", import.meta.url), "utf8");
    expect(panel).toMatch(/<p className="code-notice" id=\{SETTINGS_LOCKED_ID\}>/);
    expect(panel).toContain('export const SETTINGS_LOCKED_ID = "code-settings-locked"');
    expect(panel).toContain("lockedReasonId={SETTINGS_LOCKED_ID}");
  });
});

describe("workspace toolbar and queue", () => {
  // Operator 2026-10-01: no "Show all workflows" anywhere — the header is the
  // kit WorkflowPicker over GET /bundles?executable_for=abstractcode.agent.v1.
  it("the toolbar has no show-all switch: it renders the kit WorkflowPicker for abstractcode.agent.v1", () => {
    expect(appSource).not.toMatch(/Show all workflows|ShowAllWorkflowsSwitch|showAllWorkflows|code-workflow-all/);
    expect(css).not.toMatch(/code-workflow-all/);
    expect(existsSync(new URL("./workflow_all_switch.tsx", import.meta.url))).toBe(false);
    const block = /<WorkflowPicker\s[\s\S]*?\/>/.exec(appSource)?.[0] ?? "";
    expect(block).toContain("interfaceId={CODE_AGENT_INTERFACE}");
    expect(block).toContain("workflows={{ ...catalog.executable");
    expect(block).toContain('unavailableReason={!connection.connected ? "Connect to a gateway first." : locked ? "A run is in progress." : null}');
    expect(appSource).toContain("executableChoices(catalog.executable.data)");
    expect(appSource).toContain('<div className="code-workflow-select" aria-busy={schemaLoading || undefined}>');
    expect(appSource).not.toMatch(/<select\s+aria-label="Workflow"/);
    expect(catalogSource).toContain("request(executableWorkflowsPath(CODE_AGENT_INTERFACE))");
    expect(catalogSource).toContain("parseExecutableWorkflows(value(7), CODE_AGENT_INTERFACE)");
    expect(catalogSource).not.toMatch(/publishedWorkflowChoices/);
  });

  it('the queue is a "Run queued turns" switch, not a Pause/Resume queue verb pair', () => {
    const block = /<AfSwitch\s+className="code-queue__switch"[\s\S]*?\n\s*\/>/.exec(appSource)?.[0] ?? "";
    expect(block).toContain('label="Run queued turns"');
    expect(block).toContain("checked={queueRunning}");
    expect(appSource).not.toMatch(/"(Pause|Resume) queue"/);
  });

  it("the app's own button resets never restyle the kit switch", () => {
    expect(css).toContain(".code-queue button:not(.af-switch)");
    expect(css).not.toMatch(/\.code-queue button\s*\{/);
  });
});
