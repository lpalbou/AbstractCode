/**
 * The served schedule block of an automation summary is OPTIONAL on read.
 *
 * Round 16 (R16.1) gateways serve `next_run_at`, `next_run_local`,
 * `time_zone`, `schedule_text` and `schedule_rule_text` on every row; a
 * gateway before round 16 (0.13.x) serves none of them — only the runtime's
 * `next_fire_at` (UTC). A missing or non-string field never fails the list
 * (2026-10-09 operator report): the rule reads "—" ({@link NOT_SERVED}) and
 * the next run falls back to the served `next_fire_at`, shown in UTC — the
 * zone of that one served string, CUT by the kit (no clock arithmetic).
 * A served value is never overridden.
 */
import type { AutomationsClient, AutomationSummary } from "@abstractframework/ui-kit";

/** What a row shows for a served field the gateway did not send. */
export const NOT_SERVED = "—";

function text(v: unknown): string | undefined {
  return typeof v === "string" && v ? v : undefined;
}

function isUtcIso(ts: string): boolean {
  return ts.endsWith("+00:00") || ts.endsWith("Z");
}

/** One summary with the served schedule block made total (see the module comment). */
export function servedSummary<T extends AutomationSummary>(s: T): T {
  if (!s || typeof s !== "object") return s;
  const raw = s as unknown as Record<string, unknown>;
  const out = { ...s } as T;
  const nextRunAt = text(raw.next_run_at) ?? text(raw.next_fire_at);
  let nextRunLocal = text(raw.next_run_local);
  let timeZone = text(raw.time_zone) ?? "";
  if (!nextRunLocal && !timeZone && nextRunAt && isUtcIso(nextRunAt)) {
    nextRunLocal = nextRunAt;
    timeZone = "UTC";
  }
  if (nextRunAt) out.next_run_at = nextRunAt;
  else delete out.next_run_at;
  if (nextRunLocal) out.next_run_local = nextRunLocal;
  else delete out.next_run_local;
  out.time_zone = timeZone;
  out.schedule_rule_text = text(raw.schedule_rule_text) ?? NOT_SERVED;
  out.schedule_text = text(raw.schedule_text) ?? NOT_SERVED;
  return out;
}

/** The kit client with every summary it returns (list, get, create) passed through {@link servedSummary}. */
export function withServedSummaries(client: AutomationsClient): AutomationsClient {
  return {
    ...client,
    listAutomations: async (query) => {
      const page = await client.listAutomations(query);
      return { ...page, items: (page.items ?? []).map(servedSummary) };
    },
    getAutomation: async (id) => {
      const detail = await client.getAutomation(id);
      return detail?.summary ? { ...detail, summary: servedSummary(detail.summary) } : detail;
    },
    createAutomation: async (body) => {
      const created = await client.createAutomation(body);
      return created?.summary ? { ...created, summary: servedSummary(created.summary) } : created;
    },
  };
}
