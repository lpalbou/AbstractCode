import { describe, expect, it } from "vitest";

import { liveConnectionNotice, staleSince, unreachableBannerText } from "./live_connection";

const gap = { reason: "Failed to fetch", retrying: true, since: "2026-11-08T12:21:34" };

describe("the page's own notice when it cannot reach the gateway", () => {
  it("banner: the reason, then Retrying… or Not retrying", () => {
    expect(unreachableBannerText(gap)).toBe("Can't reach the gateway: Failed to fetch. Retrying…");
    expect(unreachableBannerText({ ...gap, retrying: false })).toBe("Can't reach the gateway: Failed to fetch. Not retrying. Retry now to reconnect.");
    expect(unreachableBannerText({ ...gap, reason: "Backend not reachable at http://127.0.0.1:8080." })).toBe("Can't reach the gateway: Backend not reachable at http://127.0.0.1:8080. Retrying…");
  });

  it("composer: the same thing, only for a live run that cannot reach the gateway", () => {
    expect(liveConnectionNotice({ runId: "r", status: "waiting", unreachable: gap })).toBe("Can't reach the gateway; your message can be sent when it is back.");
    expect(liveConnectionNotice({ runId: "r", status: "running", unreachable: null })).toBe("");
    for (const status of ["completed", "failed", "cancelled"]) expect(liveConnectionNotice({ runId: "r", status, unreachable: gap })).toBe("");
    expect(liveConnectionNotice({ runId: "", status: "", unreachable: gap })).toBe("");
  });

  it("status bar cue: the local time the page last had the gateway's word", () => {
    expect(staleSince(gap)).toBe("as of 12:21:34");
    expect(staleSince(null)).toBe("");
    expect(staleSince({ ...gap, since: "nonsense" })).toBe("");
  });
});
