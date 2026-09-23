import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { AgentWorkflowInputs } from "./workflow_inputs";

vi.mock("@abstractframework/ui-kit", () => ({
  ProviderModelPicker: () => <div data-testid="linked-model-picker" />,
}));

function render(properties: Record<string, unknown>, required: string[] = []) {
  return renderToStaticMarkup(
    <AgentWorkflowInputs
      schema={{ properties, required }}
      values={{}}
      onChange={() => {}}
    />,
  );
}

describe("agent workflow input disclosure", () => {
  const linked = {
    provider: { type: "string" },
    model: { type: "string" },
    reasoning: { type: "string" },
  };

  it.each(["model", "reasoning"])(
    "keeps the linked picker visible when only %s is required",
    (required) => {
      const html = render({ ...linked, memory: { type: "object" } }, [
        required,
      ]);
      expect(html).toContain("This workflow asks for the inputs below");
      expect(html.match(/data-testid="linked-model-picker"/g)).toHaveLength(1);
      expect(html.indexOf("linked-model-picker")).toBeLessThan(
        html.indexOf("<details"),
      );
    },
  );

  it("keeps optional linked controls together inside Advanced", () => {
    const html = render(linked);
    expect(html).toContain("Ready to chat");
    expect(html.match(/data-testid="linked-model-picker"/g)).toHaveLength(1);
    expect(html.indexOf("linked-model-picker")).toBeGreaterThan(
      html.indexOf("<details"),
    );
    expect(html).not.toMatch(/<details[^>]*\bopen/);
  });

  it("retains expert-only parameters without treating them as onboarding", () => {
    const html = render({
      temperature: { type: "number", default: 0.7 },
      seed: { type: "integer", default: -1 },
      system: { type: "string" },
      prompt: { type: "string" },
    });
    expect(html).toContain("No setup required");
    expect(html).not.toMatch(/<details[^>]*\bopen/);
    for (const name of ["temperature", "seed", "system"]) {
      expect(html).toContain(`id="workflow-input-${name}"`);
      expect(html.indexOf(`id="workflow-input-${name}"`)).toBeGreaterThan(
        html.indexOf("<details"),
      );
    }
    expect(html).not.toContain('id="workflow-input-prompt"');
  });
});
