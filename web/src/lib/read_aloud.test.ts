import { describe, expect, it } from "vitest";
import { READ_ALOUD_INITIAL, readAloudStep, runIsLive } from "./read_aloud";

/** The operator's case (2026-10-29): read aloud on, a session with replies is opened or
 * reloaded — nothing is spoken; only a reply generated while it is open is. */
function play(steps: Array<{ scope: string; status: string; reply: string | null }>): string[] {
  let state = READ_ALOUD_INITIAL;
  const spoken: string[] = [];
  for (const s of steps) {
    const out = readAloudStep(state, { scope: s.scope, live: runIsLive(s.status), reply: s.reply ? { id: s.reply, text: `text ${s.reply}` } : null });
    state = out.state;
    if (out.speak) spoken.push(out.speak.id);
  }
  return spoken;
}

describe("read aloud", () => {
  it("never speaks when a session with replies is opened or reloaded", () => {
    expect(play([
      { scope: "u:s1", status: "idle", reply: null },
      { scope: "u:s1", status: "loading", reply: null },
      { scope: "u:s1", status: "completed", reply: "a2" },
    ])).toEqual([]);
  });
  it("never speaks the existing reply of the session switched to", () => {
    expect(play([
      { scope: "u:s1", status: "running", reply: "a1" },
      { scope: "u:s1", status: "completed", reply: "a2" },
      { scope: "u:s2", status: "loading", reply: "a2" },
      { scope: "u:s2", status: "completed", reply: "b1" },
    ])).toEqual(["a2"]);
  });
  it("speaks a reply generated while the session is open, once", () => {
    expect(play([
      { scope: "u:s1", status: "completed", reply: "a1" },
      { scope: "u:s1", status: "running", reply: "a1" },
      { scope: "u:s1", status: "completed", reply: "a2" },
      { scope: "u:s1", status: "completed", reply: "a2" },
    ])).toEqual(["a2"]);
  });
});
