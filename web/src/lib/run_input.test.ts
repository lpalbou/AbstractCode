import { describe, expect, it } from "vitest";
import { build_run_input_data } from "./run_input";

describe("run input history (ADR-0026)", () => {
  it("sends the whole transcript as context — no client-side message cap", () => {
    const repl_messages = Array.from({ length: 250 }, (_, i) => ({
      id: `m${i}`,
      role: i % 2 ? "assistant" : "user",
      content: `turn ${i}`,
      ts: "2026-09-28T00:00:00Z",
    })) as any;
    const out = build_run_input_data({
      prompt: "next",
      settings: { use_context: true } as any,
      repl_messages,
      session_id: "s",
      attached_files: [],
      template: null,
    });
    expect(out.context.messages).toHaveLength(250);
    expect(out.context.messages[0]).toEqual({ role: "user", content: "turn 0" });
  });
});
