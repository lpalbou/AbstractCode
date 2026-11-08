// The iteration limit in Code (operator 2026-11-08): the kit's IterationLimitField is the third
// row of Model → Limits, reads the gateway default from the capabilities descriptor, sends
// `_limits.max_iterations` only when set, and shows the start's `limits` receipt.
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { iterationLimitDefaultFrom } from "@abstractframework/ui-kit";
import { AUTOMATION_ITERATION_DEFAULT, runLimitsReceipt } from "./iteration_limit";
import { DEFAULT_PREFERENCES, SettingsContent, type SettingsContentProps } from "./settings_panel";
import { buildWorkflowInput } from "./catalog";

const caps = { capabilities: { agent_limits: { max_iterations: { default: 12, source: "stored", run_field: "_limits.max_iterations", min: 1, warn_above: 200 } } } };

function render(tab: SettingsContentProps["tab"], extra: Partial<SettingsContentProps> = {}, maxIterations = "") {
  return renderToStaticMarkup(
    <SettingsContent
      tab={tab}
      value={{ ...DEFAULT_PREFERENCES, maxIterations }}
      onChange={() => {}}
      toolCatalog={[]}
      disabled={false}
      {...extra}
    />,
  );
}

describe("iteration limit", () => {
  it("is the kit field in Model → Limits, after the context limit, naming the gateway default", () => {
    const html = render("model", { iterationDefault: iterationLimitDefaultFrom(caps) });
    expect(html).toContain('data-iteration-limit="true"');
    expect(html).toContain('placeholder="Gateway default (12)"');
    expect(html.indexOf("Context token limit")).toBeLessThan(html.indexOf("Iteration limit"));
    expect(html.indexOf("Stream replies")).toBeLessThan(html.indexOf("Iteration limit"));
    expect(html).toContain('class="af-limit-field code-field"');
  });

  it("is gone from the Behavior tab (one control, not two)", () => {
    expect(render("behavior")).not.toContain("Iteration limit");
  });

  it("says Workflow default for an automation and shows the last run's receipt", () => {
    expect(render("model", { iterationDefault: AUTOMATION_ITERATION_DEFAULT, hideStreamReplies: true })).toContain('placeholder="Workflow default"');
    const html = render("model", { iterationDefault: iterationLimitDefaultFrom(caps), lastRunLimits: { max_iterations: 12, source: "gateway_default" } });
    expect(html).toContain("Last run: 12 iterations (gateway default).");
  });

  it("reads the start's limits receipt and refuses junk", () => {
    expect(runLimitsReceipt({ max_iterations: 12, source: "gateway_default" })).toEqual({ max_iterations: 12, source: "gateway_default" });
    expect(runLimitsReceipt({ max_iterations: null, source: "builtin" })).toEqual({ max_iterations: null, source: "builtin" });
    expect(runLimitsReceipt(undefined)).toBeNull();
    expect(runLimitsReceipt({ max_iterations: null, source: "request" })).toBeNull();
    expect(runLimitsReceipt({ max_iterations: 3, source: "magic" })).toBeNull();
  });

  it("sends _limits.max_iterations only when the person set one", () => {
    const set = buildWorkflowInput({ prompt: "hi", limits: { maxIterations: 30 } } as any) as any;
    expect(set._limits.max_iterations).toBe(30);
    const unset = buildWorkflowInput({ prompt: "hi", limits: { maxIterations: undefined } } as any) as any;
    expect(unset._limits?.max_iterations).toBeUndefined();
  });
});
