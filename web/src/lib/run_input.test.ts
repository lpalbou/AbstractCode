import { describe, expect, it } from "vitest";
import { build_run_input_data } from "./run_input";
import { load_settings } from "./storage";

// ADR-0026 and the gateway's seeded sessions: the conversation reaches the
// model only through the gateway's session replay (one window, server-side).
// A client copy in context.messages would bypass that window, and a Discuss
// chat refuses it (HTTP 400 "seeded by the gateway").
describe("legacy REPL run input: history comes from the gateway only", () => {
  it("never sends context.messages; asks the gateway to replay the session", () => {
    const out = build_run_input_data({
      prompt: "next",
      settings: { ...load_settings(), use_context: true } as any,
      session_id: "s",
      attached_files: [],
      template: null,
    });
    expect(out.context).toEqual({ task: "next" });
    expect(out.context).not.toHaveProperty("messages");
    expect(out).not.toHaveProperty("use_context");
    expect(out.use_session_history).toBe(true);
  });

  it("drops the retired use_context setting from older saved settings", () => {
    const store = new Map<string, string>();
    const ls = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    };
    const prev = (globalThis as any).localStorage;
    (globalThis as any).localStorage = ls;
    try {
      store.set("abstractcode.settings.v1", JSON.stringify({ use_context: true, seed: 7 }));
      expect(load_settings().seed).toBe(7);
      expect(load_settings()).not.toHaveProperty("use_context");
    } finally {
      (globalThis as any).localStorage = prev;
    }
  });
});
