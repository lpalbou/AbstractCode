// Operator rule (2026-09-30, DESIGN §2/§8): every persistent on/off in the app is the kit's
// AfSwitch labelled by the FEATURE — highlighted when on, plain when off, unavailable with the
// reason — never a checkbox or a verb pair ("Pause queue" / "Resume queue"). These tests go red
// when a surface falls back to the old control.
import React from "react";
import { readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { AfSwitch } from "@abstractframework/ui-kit";

import { SkillSwitch, type GatewaySkill } from "./skills_picker";
import { ShowAllWorkflowsSwitch } from "./workflow_all_switch";

const appSource = readFileSync(new URL("./app.tsx", import.meta.url), "utf8");
const css = readFileSync(new URL("./workspace.css", import.meta.url), "utf8");

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
  const allSwitch = (p: { checked?: boolean; connected?: boolean; locked?: boolean }) =>
    renderToStaticMarkup(
      <ShowAllWorkflowsSwitch checked={p.checked ?? false} connected={p.connected ?? true} locked={p.locked ?? false} onChange={() => {}} />,
    );

  it('"Show all workflows" is a kit switch named by the feature, ON and OFF', () => {
    const on = allSwitch({ checked: true });
    expect(on).toMatch(/role="switch" id="code-workflow-all" class="af-switch af-switch--sm code-workflow-all" data-action="show-all-workflows" aria-checked="true"/);
    expect(on).toContain('aria-label="Show all workflows"');
    expect(on).not.toContain("aria-disabled");
    expect(allSwitch({ checked: false })).toMatch(/aria-checked="false"/);
    expect(appSource).toContain("<ShowAllWorkflowsSwitch");
    expect(appSource).not.toMatch(/type="checkbox"/);
    expect(css).not.toMatch(/\.code-workflow-all::after/);
  });

  it("unavailable: aria-disabled, and the reason is VISIBLE text it points at (a tap must say why)", () => {
    for (const [p, reason] of [
      [{ connected: false }, "Connect to a gateway first."],
      [{ locked: true }, "A run is in progress."],
    ] as const) {
      const html = allSwitch(p);
      expect(html).toMatch(/role="switch"[^>]*aria-disabled="true" aria-describedby="code-workflow-all-reason"/);
      // Rendered as the kit's visible reason node (not the --hidden variant).
      expect(html).toContain(`<span id="code-workflow-all-reason" class="af-switch__reason">${reason}</span>`);
    }
    // Only a hover-capable fine pointer hides the text (the tooltip carries it there).
    const rule = /@media \(hover: hover\) and \(pointer: fine\) \{\s*#code-workflow-all-reason \{/;
    expect(css).toMatch(rule);
    expect(css.replace(rule, "")).not.toMatch(/#code-workflow-all-reason \{[^}]*clip-path/);
  });

  it("switching calls back with the new state", () => {
    const onChange = vi.fn();
    const el = switchOf(ShowAllWorkflowsSwitch({ checked: false, connected: true, locked: false, onChange }));
    el.props.onChange?.(true);
    expect(onChange).toHaveBeenCalledWith(true);
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
