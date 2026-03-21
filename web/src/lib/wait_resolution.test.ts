// AbstractCode Web: wait resolution tests (subworkflow approvals).
import { describe, expect, it } from "vitest";

import { resolve_blocking_wait } from "./wait_resolution";

describe("resolve_blocking_wait", () => {
  it("clears subworkflow wait after resume/completion", () => {
    const root_wait = { reason: "subworkflow", wait_key: "subworkflow:child-1", details: { sub_run_id: "child-1" } };
    const records = [
      {
        cursor: 1,
        record: {
          run_id: "child-1",
          status: "waiting",
          result: { wait: { reason: "user", wait_key: "tool_approval:1", details: { mode: "approval_required" } } },
        },
      },
      {
        cursor: 2,
        record: {
          run_id: "child-1",
          status: "completed",
          effect: { type: "resume" },
          result: { resumed: true },
        },
      },
    ];

    const out = resolve_blocking_wait({ root_run_id: "root-1", root_wait, records });
    expect(out.wait).toBe(null);
  });

  it("returns the newest waiting record for subworkflow", () => {
    const root_wait = { reason: "subworkflow", wait_key: "subworkflow:child-2", details: { sub_run_id: "child-2" } };
    const records = [
      {
        cursor: 1,
        record: {
          run_id: "child-2",
          status: "waiting",
          result: { wait: { reason: "user", wait_key: "tool_approval:2", details: { mode: "approval_required" } } },
        },
      },
    ];

    const out = resolve_blocking_wait({ root_run_id: "root-1", root_wait, records });
    expect(out.wait?.wait_key).toBe("tool_approval:2");
  });
});
