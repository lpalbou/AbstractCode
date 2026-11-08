import { describe, expect, it } from "vitest";
import { compactSchedule } from "./sidebar_cards";

// Sidebar rows (2026-11-08): the schedule in a few words from the rule's STRUCTURE (interval or
// calendar fields), never by reading the gateway's sentence; zone and bounds stay in the details.
const row = (config: Record<string, unknown>, source_id = "schedule", status = "active") =>
  ({ status, trigger: { binding_id: "b", source_id, source_version: 2, config } }) as never;

describe("compactSchedule", () => {
  it("intervals", () => {
    expect(compactSchedule(row({ every: "2m" }))).toBe("Every 2 min");
    expect(compactSchedule(row({ every: "8h" }))).toBe("Every 8 h");
    expect(compactSchedule(row({ every: "1h" }))).toBe("Every hour");
    expect(compactSchedule(row({ every: "1d" }))).toBe("Every day");
    expect(compactSchedule(row({ every: "7d" }))).toBe("Every 7 d");
    expect(compactSchedule(row({ kind: "every", every: "30m", time_zone: "UTC" }))).toBe("Every 30 min");
  });
  it("calendar rules: the rule's own wall time", () => {
    expect(compactSchedule(row({ kind: "daily", at: "08:00", time_zone: "Europe/Paris" }))).toBe("Daily 08:00");
    expect(compactSchedule(row({ kind: "weekly", days: ["mon"], at: "09:00" }))).toBe("Weekly Mon 09:00");
    expect(compactSchedule(row({ kind: "weekly", days: ["mon", "thu"], at: "09:30" }))).toBe("Weekly Mon, Thu 09:30");
    expect(compactSchedule(row({ kind: "weekly", days: ["thu", "mon"], at: "09:30" }))).toBe("Weekly Mon, Thu 09:30");
    expect(compactSchedule(row({ kind: "weekly", days: ["mon", "tue", "wed", "thu", "fri"], at: "09:00" }))).toBe("Weekdays 09:00");
    expect(compactSchedule(row({ kind: "weekly", days: ["mon", "tue", "wed", "thu", "fri", "sat", "sun"], at: "07:00" }))).toBe("Daily 07:00");
    expect(compactSchedule(row({ kind: "monthly", day: 1, at: "07:00" }))).toBe("Monthly 1st 07:00");
    expect(compactSchedule(row({ kind: "monthly", day: 22, at: "07:00" }))).toBe("Monthly 22nd 07:00");
    expect(compactSchedule(row({ kind: "monthly", day: "last", at: "18:00" }))).toBe("Monthly last day 18:00");
    expect(compactSchedule(row({ kind: "once", at: "2026-11-09T08:00" }))).toBe("Once 2026-11-09 08:00");
  });
  it("other triggers and states", () => {
    expect(compactSchedule(row({}, "manual"))).toBe("Manual");
    expect(compactSchedule(row({}, "email.received"))).toBe("On new email");
    expect(compactSchedule(row({ every: "8h" }, "schedule", "paused"))).toBe("Paused");
    // A trigger the row cannot word briefly: no schedule on the row (it stays in the details).
    expect(compactSchedule(row({}, "feed.rss"))).toBe("");
    expect(compactSchedule(row({ kind: "cron", expr: "0 9 * * 1" }))).toBe("");
  });
});
