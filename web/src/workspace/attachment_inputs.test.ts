import { describe, expect, it } from "vitest";
import { withWorkflowAttachments } from "./workflow_inputs";

describe("generic workflow attachment handoff", () => {
  it("never silently drops attachments or injects undeclared fields", () => {
    expect(() =>
      withWorkflowAttachments({ properties: {} }, {}, [{ $artifact: "new" }]),
    ).toThrow("does not declare an attachments input");
    expect(withWorkflowAttachments(undefined, { prompt: "hello" }, [])).toEqual(
      { prompt: "hello" },
    );
  });
  it("merges uploaded references only into a declared attachment array", () => {
    const original = { attachments: [{ $artifact: "existing" }] };
    expect(
      withWorkflowAttachments(
        { properties: { attachments: { type: "array" } } },
        original,
        [{ $artifact: "new" }],
      ),
    ).toEqual({
      attachments: [{ $artifact: "existing" }, { $artifact: "new" }],
    });
    expect(original.attachments).toHaveLength(1);
  });
});
