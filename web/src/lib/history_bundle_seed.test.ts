import { describe, expect, it } from "vitest";

import { seed_repl_messages_from_history_bundle } from "./history_bundle_seed";

describe("history bundle seeding", () => {
  it("deduplicates Telegram out messages (started + completed tool_calls)", () => {
    const bundle = {
      ledgers: {
        "run-1": {
          items: [
            {
              record: {
                status: "completed",
                started_at: "2026-02-15T00:00:00Z",
                effect: {
                  type: "resume",
                  payload: { payload: { payload: { telegram: { text: "hi" }, attachments: [] } } },
                },
              },
            },
            {
              record: {
                status: "started",
                started_at: "2026-02-15T00:00:01Z",
                effect: {
                  type: "tool_calls",
                  payload: { tool_calls: [{ name: "send_telegram_message", arguments: { chat_id: 99, text: "hello" } }] },
                },
              },
            },
            {
              record: {
                status: "completed",
                started_at: "2026-02-15T00:00:01Z",
                ended_at: "2026-02-15T00:00:02Z",
                effect: {
                  type: "tool_calls",
                  payload: { tool_calls: [{ name: "send_telegram_message", arguments: { chat_id: 99, text: "hello" } }] },
                },
                result: {
                  mode: "executed",
                  results: [{ call_id: "c1", name: "send_telegram_message", success: true, output: { ok: true } }],
                },
              },
            },
          ],
        },
      },
    };

    const msgs = seed_repl_messages_from_history_bundle(bundle);
    expect(msgs.map((m) => [m.role, m.content])).toEqual([
      ["user", "hi"],
      ["assistant", "hello"],
    ]);
  });

  it("does not treat waiting Telegram tool_calls as sent", () => {
    const bundle = {
      ledgers: {
        "run-1": {
          items: [
            {
              record: {
                status: "completed",
                started_at: "2026-02-15T00:00:00Z",
                effect: {
                  type: "resume",
                  payload: { payload: { payload: { telegram: { text: "hi" }, attachments: [] } } },
                },
              },
            },
            {
              record: {
                status: "waiting",
                started_at: "2026-02-15T00:00:01Z",
                effect: {
                  type: "tool_calls",
                  payload: { tool_calls: [{ name: "send_telegram_message", arguments: { chat_id: 99, text: "hello" } }] },
                },
                result: { wait: { reason: "event", wait_key: "wk", details: { mode: "approval_required" } } },
              },
            },
          ],
        },
      },
    };

    const msgs = seed_repl_messages_from_history_bundle(bundle);
    expect(msgs.map((m) => [m.role, m.content])).toEqual([["user", "hi"]]);
  });
});

