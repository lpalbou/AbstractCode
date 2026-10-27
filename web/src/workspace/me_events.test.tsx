import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { MeEventsFollower, emptyFold, foldEvent, needsYou, newResults, type UserEvent } from "./me_events";
import { AutomationsSection, attentionBadgeTitle } from "./automations_view";

const ev = (sequence: number, type: string, data: Record<string, any>): UserEvent => ({ type, sequence, data });

describe("the person's event stream (backlog 1047)", () => {
  it("folds raised minus seen over waits and automation results", () => {
    let f = emptyFold();
    for (const e of [
      ev(1, "attention.raised", { automation_id: "a", seq: 1, kind: "notify" }),
      // The gateway's wait shapes (L3 item 4b): seen names the step, no record_key.
      ev(2, "attention.raised", { kind: "approval", wait_kind: "tool_approval", run_id: "r1", session_id: "s", wait_key: "k1", step_id: "a", record_key: "wait:r1:k1:a" }),
      ev(3, "attention.raised", { kind: "question", wait_kind: "ask_user", run_id: "r2", session_id: "s", wait_key: "k2", step_id: "b", record_key: "wait:r2:k2:b" }),
    ])
      f = foldEvent(f, e);
    expect([needsYou(f), newResults(f)]).toEqual([2, 1]);
    const epoch = f.automationEpoch;
    f = foldEvent(f, ev(4, "attention.seen", { kind: "approval", run_id: "r1", wait_key: "k1", step_id: "a", outcome: "approved" }));
    f = foldEvent(f, ev(5, "run.finished", { run_id: "r2", status: "completed", origin: { kind: "trigger" } }));
    f = foldEvent(f, ev(6, "attention.seen", { automation_id: "a", seq: 1 }));
    expect([needsYou(f), newResults(f)]).toEqual([0, 0]);
    expect(f.automationEpoch).toBeGreaterThan(epoch);
    // Replayed frames fold once; a seen item never returns.
    expect(foldEvent(f, ev(6, "attention.raised", { automation_id: "a", seq: 9 }))).toBe(f);
    expect(newResults(foldEvent(f, ev(7, "attention.raised", { automation_id: "a", seq: 1 })))).toBe(0);
    // The same node asking again is a new wait; an older step's seen leaves it open.
    f = foldEvent(f, ev(8, "attention.raised", { kind: "approval", run_id: "r3", wait_key: "k", step_id: "s2" }));
    f = foldEvent(f, ev(9, "attention.seen", { kind: "approval", run_id: "r3", wait_key: "k", step_id: "s1", outcome: "approved" }));
    expect(needsYou(f)).toBe(1);
    f = foldEvent(f, ev(10, "attention.seen", { kind: "approval", run_id: "r3", wait_key: "k", step_id: "s2", outcome: "denied" }));
    expect(needsYou(f)).toBe(0);
  });

  it("catches up, then tails with Last-Event-ID while the tab is visible; a hidden tab only catches up", async () => {
    const calls: { url: string; lastId: string | null }[] = [];
    const page = { events: [ev(1, "attention.raised", { automation_id: "a", seq: 1 }), ev(2, "attention.raised", { automation_id: "b", seq: 1 })], next_after: 2, head: 2, more: false };
    const sse = `: stream user:default:a\n\nid: 3\nevent: attention.raised\ndata: ${JSON.stringify(ev(3, "attention.raised", { kind: "approval", run_id: "r", wait_key: "k" }))}\n\n`;
    let visible = true;
    const fakeFetch = async (url: string, init?: RequestInit) => {
      const headers = new Headers(init?.headers);
      calls.push({ url, lastId: headers.get("Last-Event-ID") });
      if (url.includes("me/events/stream")) {
        visible = false; // the tab is hidden after this frame: the tail ends
        return new Response(new Blob([sse]).stream(), { status: 200, headers: { "Content-Type": "text/event-stream" } });
      }
      return new Response(JSON.stringify(url.includes("after=0") ? page : { events: [], next_after: 3, head: 3, more: false }), { status: 200 });
    };
    const f = new MeEventsFollower({ fetch: fakeFetch, live: () => visible, catchUpMs: 20 });
    f.start();
    await new Promise((r) => setTimeout(r, 120));
    f.stop();
    expect(calls[0].url).toBe("api/gateway/me/events?after=0&limit=1000");
    const tails = calls.filter((c) => c.url.includes("/stream"));
    expect(tails.length).toBe(1);
    expect(tails[0].lastId).toBe("2");
    expect([needsYou(f.fold), newResults(f.fold)]).toEqual([1, 2]);
    // Hidden: catch-up reads resume from the last sequence, no second tail.
    const after = calls.filter((c) => !c.url.includes("/stream")).slice(1);
    expect(after.length).toBeGreaterThan(0);
    expect(after.every((c) => c.url.startsWith("api/gateway/me/events?after=3"))).toBe(true);
  });

  it("a gateway without the stream is unsupported", async () => {
    const f = new MeEventsFollower({ fetch: async () => new Response("{}", { status: 404 }), live: () => true });
    f.start();
    await new Promise((r) => setTimeout(r, 20));
    expect(f.state).toBe("unsupported");
  });

  it("the Automations badge counts what the stream says", () => {
    const html = renderToStaticMarkup(
      <AutomationsSection
        state={{ items: [], loaded: true, loading: false, error: null, selectedId: "", archived: null, archivedCount: 0 } as any}
        available={{ available: true } as any}
        attention={{ needsYou: 1, newResults: 2 }}
        onSelect={() => undefined}
        onNew={() => undefined}
        onRefresh={() => undefined}
      />,
    );
    expect(html).toContain('data-field="attention-total"');
    expect(html).toContain(">3</span>");
    expect(html).toContain("1 waiting for you · 2 new");
    expect(attentionBadgeTitle(0, 1)).toBe("1 new");
  });
});
