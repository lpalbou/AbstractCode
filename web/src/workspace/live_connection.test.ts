import { describe, expect, it } from "vitest";

import { liveConnectionNotice } from "./live_connection";

describe("liveConnectionNotice", () => {
  it("says why Send waits while a live run reconnects or is not connected", () => {
    expect(liveConnectionNotice({ runId: "r", status: "running", connection: "reconnecting" })).toMatch(/^Reconnecting to this conversation\./);
    expect(liveConnectionNotice({ runId: "r", status: "waiting", connection: "reconnecting" })).toMatch(/^Reconnecting/);
    expect(liveConnectionNotice({ runId: "r", status: "running", connection: "disconnected" })).toMatch(/not connected/);
  });

  it("says nothing when connected, connecting, ended, or with no run", () => {
    for (const connection of ["connected", "connecting", "idle", undefined]) expect(liveConnectionNotice({ runId: "r", status: "running", connection })).toBe("");
    // A finished run's streams close by design ("disconnected"): nothing to wait for.
    for (const status of ["completed", "failed", "cancelled"]) expect(liveConnectionNotice({ runId: "r", status, connection: "disconnected" })).toBe("");
    expect(liveConnectionNotice({ runId: "", status: "", connection: "reconnecting" })).toBe("");
  });
});
