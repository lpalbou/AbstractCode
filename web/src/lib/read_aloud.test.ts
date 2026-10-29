import { describe, expect, it } from "vitest";
import { READ_ALOUD_INITIAL, readAloudStep } from "./read_aloud";
import { codeReadAloudInput } from "../workspace/read_aloud_input";

/** The operator's case (2026-10-29), through Code's own wiring: read aloud on, a session with
 * replies is opened or reloaded — nothing is spoken; only a reply generated while it is open is. */
type Step = { session: string; runId: string; status: string; sending?: boolean; reply: string | null };
function play(steps: Step[]): string[] {
  let state = READ_ALOUD_INITIAL;
  const spoken: string[] = [];
  for (const s of steps) {
    const input = codeReadAloudInput({
      identity: "u", sessionId: s.session, runId: s.runId, status: s.status, sending: Boolean(s.sending),
      reply: s.reply ? { id: s.reply, text: `text ${s.reply}` } : null,
    });
    const out = readAloudStep(state, input);
    state = out.state;
    if (out.speak) spoken.push(out.speak.id);
  }
  return spoken;
}

describe("read aloud (Code)", () => {
  it("never speaks when a session with replies is opened or reloaded", () => {
    expect(play([
      { session: "s1", runId: "r2", status: "idle", reply: null },
      { session: "s1", runId: "r2", status: "loading", reply: null },
      { session: "s1", runId: "r2", status: "completed", reply: "a2" },
    ])).toEqual([]);
  });
  it("never speaks the existing reply of the session switched to", () => {
    expect(play([
      { session: "s1", runId: "r1", status: "running", reply: "a1" },
      { session: "s1", runId: "r1", status: "completed", reply: "a2" },
      { session: "s2", runId: "r9", status: "loading", reply: "a2" },
      { session: "s2", runId: "r9", status: "completed", reply: "b1" },
    ])).toEqual(["a2"]);
  });
  it("speaks a reply generated while the session is open, once (a running run is live)", () => {
    expect(play([
      { session: "s1", runId: "r1", status: "completed", reply: "a1" },
      { session: "s1", runId: "r2", status: "running", reply: "a1" },
      { session: "s1", runId: "r2", status: "completed", reply: "a2" },
      { session: "s1", runId: "r2", status: "completed", reply: "a2" },
    ])).toEqual(["a2"]);
  });
  it("a run sent from here that went loading → completed still speaks", () => {
    expect(play([
      { session: "s1", runId: "r1", status: "completed", reply: "a1" },
      { session: "s1", runId: "", status: "idle", sending: true, reply: "a1" },
      { session: "s1", runId: "r2", status: "loading", reply: "a1" },
      { session: "s1", runId: "r2", status: "completed", reply: "a2" },
    ])).toEqual(["a2"]);
  });
});
