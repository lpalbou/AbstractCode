import { describe, expect, it } from "vitest";

import {
  parseWorkflowInputObject,
  validateWorkflowInputs,
} from "./workflow_inputs";

describe("generic workflow object payloads", () => {
  it("accepts nested and additional properties without rewriting them", () => {
    const source =
      '{"topic":"release","nested":{"flags":[true,false]},"extension":{"mode":"custom"}}';
    const parsed = parseWorkflowInputObject(source);
    expect(parsed).toEqual({
      ok: true,
      value: {
        topic: "release",
        nested: { flags: [true, false] },
        extension: { mode: "custom" },
      },
    });
    if (parsed.ok)
      expect(
        validateWorkflowInputs(
          { type: "object", properties: { topic: { type: "string" } } },
          parsed.value,
        ),
      ).toEqual([]);
  });

  it("requires valid JSON with an object at the top level", () => {
    expect(parseWorkflowInputObject("{")).toEqual({
      ok: false,
      error: "Enter valid JSON.",
    });
    expect(parseWorkflowInputObject("[1,2]")).toEqual({
      ok: false,
      error: "The workflow input payload must be a JSON object.",
    });
    expect(parseWorkflowInputObject("")).toEqual({ ok: true, value: {} });
  });
});
