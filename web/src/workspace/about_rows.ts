/** Rows the About dialog adds after the shared AbstractFramework rows: the
 * versions the connected gateway reports. Nothing is hidden: a failed fetch
 * becomes one "Gateway: unavailable (HTTP <status>)" row. */

export type FetchOutcome =
  | { ok: true; value: unknown }
  | { ok: false; status?: number; message: string };

type Row = [string, string];

function record(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

/** Installed Abstract* package versions from `GET /discovery/capabilities`
 * (`{capabilities: {abstractgateway: {installed, version}, ...}}`). */
export function gatewayPackageVersions(capabilities: unknown): Row[] {
  const body = record(capabilities);
  const caps = record(body?.capabilities) ?? body ?? {};
  const rows: Row[] = [];
  for (const [name, raw] of Object.entries(caps)) {
    const status = record(raw);
    if (!name.startsWith("abstract") || status?.installed !== true) continue;
    if (typeof status.version === "string" && status.version.trim())
      rows.push([name, status.version.trim()]);
  }
  return rows;
}

function unavailable(outcome: Extract<FetchOutcome, { ok: false }>): Row {
  return [
    "Gateway",
    `unavailable (${outcome.status ? `HTTP ${outcome.status}` : outcome.message})`,
  ];
}

export function aboutExtraRows(options: {
  capabilities?: FetchOutcome;
  /** `GET /api/gateway/about`; undefined until the dialog has been opened. */
  about?: FetchOutcome;
}): Row[] {
  const failed = [options.about, options.capabilities].find(
    (outcome): outcome is Extract<FetchOutcome, { ok: false }> =>
      Boolean(outcome && !outcome.ok),
  );
  const rows: Row[] = [];
  if (options.about?.ok) {
    const framework = record(options.about.value)?.abstractframework;
    rows.push([
      "AbstractFramework on the gateway",
      typeof framework === "string" && framework ? framework : "not installed",
    ]);
  }
  if (options.capabilities?.ok)
    rows.push(...gatewayPackageVersions(options.capabilities.value));
  if (failed) rows.push(unavailable(failed));
  return rows;
}
