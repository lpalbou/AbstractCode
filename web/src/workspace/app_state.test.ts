import { describe, expect, it } from "vitest";

import {
  advanceQueueIntents,
  queueIntentMatches,
  queueTerminalDisposition,
  staleSendAbort,
  workspacePrincipalIdentity,
  type QueueIntent,
} from "./app_state";

const first: QueueIntent = {
  id: "q1",
  text: "first",
  sessionId: "s1",
  sourceRunId: "r1",
  authEpoch: 4,
};

describe("workspace async identity", () => {
  it("separates the same user across tenant and runtime principals", () => {
    const left = workspacePrincipalIdentity({
      gateway_url: "https://gateway",
      gateway: {
        principal: { tenant_id: "a", runtime_id: "one", user_id: "sam" },
      },
    });
    const tenant = workspacePrincipalIdentity({
      gateway_url: "https://gateway",
      gateway: {
        principal: { tenant_id: "b", runtime_id: "one", user_id: "sam" },
      },
    });
    const runtime = workspacePrincipalIdentity({
      gateway_url: "https://gateway",
      gateway: {
        principal: { tenant_id: "a", runtime_id: "two", user_id: "sam" },
      },
    });
    expect(new Set([left, tenant, runtime]).size).toBe(3);
  });

  it("requires queue provenance to match session, route run, snapshot run, and auth epoch", () => {
    expect(
      queueIntentMatches(first, {
        sessionId: "s1",
        runId: "r1",
        snapshotRunId: "r1",
        snapshotSessionId: "s1",
        authEpoch: 4,
      }),
    ).toBe(true);
    expect(
      queueIntentMatches(first, {
        sessionId: "s2",
        runId: "r1",
        snapshotRunId: "r1",
        snapshotSessionId: "s1",
        authEpoch: 4,
      }),
    ).toBe(false);
    expect(
      queueIntentMatches(first, {
        sessionId: "s1",
        runId: "r2",
        snapshotRunId: "r1",
        snapshotSessionId: "s1",
        authEpoch: 4,
      }),
    ).toBe(false);
    expect(
      queueIntentMatches(first, {
        sessionId: "s1",
        runId: "r1",
        snapshotRunId: "r2",
        snapshotSessionId: "s1",
        authEpoch: 4,
      }),
    ).toBe(false);
    expect(
      queueIntentMatches(first, {
        sessionId: "s1",
        runId: "r1",
        snapshotRunId: "r1",
        snapshotSessionId: "s2",
        authEpoch: 4,
      }),
    ).toBe(false);
    expect(
      queueIntentMatches(first, {
        sessionId: "s1",
        runId: "r1",
        snapshotRunId: "r1",
        snapshotSessionId: "s1",
        authEpoch: 5,
      }),
    ).toBe(false);
  });

  it("advances only the completed queue chain to the newly created run", () => {
    const second = { ...first, id: "q2", text: "second" };
    const unrelated = { ...first, id: "other", sessionId: "s2" };
    expect(
      advanceQueueIntents([first, second, unrelated], first, "r2"),
    ).toEqual([{ ...second, sourceRunId: "r2" }, unrelated]);
  });

  it("auto-advances only after success and pauses after failure or cancellation", () => {
    expect(queueTerminalDisposition("completed")).toBe("advance");
    expect(queueTerminalDisposition("failed")).toBe("pause");
    expect(queueTerminalDisposition("cancelled")).toBe("pause");
    expect(queueTerminalDisposition("canceled")).toBe("pause");
    expect(queueTerminalDisposition("waiting")).toBe("wait");
  });

  it("uses AbortError for stale accepted runs", () => {
    expect(staleSendAbort()).toMatchObject({ name: "AbortError" });
  });
});
