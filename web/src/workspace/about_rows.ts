/** Rows the About dialog adds after the shared AbstractFramework rows: the
 * versions the connected gateway reports in `GET /api/gateway/about`,
 * formatted by the kit's `gatewayVersionRows` so every app shows the same
 * rows. Nothing is hidden: a failed fetch becomes one
 * "Gateway: unavailable (HTTP <status>)" row. */
import { gatewayVersionRows, type AboutRow, type GatewayAboutPayload } from "@abstractframework/ui-kit";

export type FetchOutcome =
  | { ok: true; value: unknown }
  | { ok: false; status?: number; message: string };

/** Shown while `GET /api/gateway/about` is in flight. */
export const GATEWAY_ABOUT_CHECKING: AboutRow[] = [["Gateway", "checking…"]];

/** `about` is the outcome of `GET /api/gateway/about`; undefined while the
 * request is in flight (or before the dialog was first opened). */
export function aboutExtraRows(about: FetchOutcome | undefined): AboutRow[] {
  if (!about) return GATEWAY_ABOUT_CHECKING;
  if (!about.ok)
    return gatewayVersionRows({
      error: about.status ? `HTTP ${about.status}` : about.message,
    });
  return gatewayVersionRows(about.value as GatewayAboutPayload);
}
