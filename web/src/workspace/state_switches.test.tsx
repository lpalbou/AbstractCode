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

  it("while settings are locked the switch says so instead of going silent", () => {
    const el = switchOf(SkillSwitch({ skill: skill(), checked: true, disabled: true, onChange: () => {} }));
    expect(el.props.unavailableReason).toMatch(/no run is in progress/);
  });
});

describe("workspace toolbar and queue", () => {
  it('"Show all workflows" is a kit switch named by the feature (no checkbox in the toolbar)', () => {
    const block = /<AfSwitch\s+className="code-workflow-all"[\s\S]*?\n\s*\/>/.exec(appSource)?.[0] ?? "";
    expect(block).toContain('ariaLabel="Show all workflows"');
    expect(block).toContain("checked={preferences.showAllWorkflows}");
    expect(block).toContain("unavailableReason=");
    expect(appSource).not.toMatch(/type="checkbox"/);
    // The phone's short label is the switch's own text, not a pseudo-element over a hidden checkbox.
    expect(css).not.toMatch(/\.code-workflow-all::after/);
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
