/** The versions the shared About (ui-kit `AfAbout`, round 5) states besides the
 * app's own: the AbstractFramework and AbstractGateway versions the connected
 * gateway reports in `GET /api/gateway/about`, read by the kit's
 * `aboutVersionsFromGateway` (no package list). Nothing is hidden: a failed
 * fetch says "unavailable (HTTP <status>)" in place of the gateway version. */
import { aboutVersionsFromGateway, type AfAboutVersions, type GatewayAboutPayload } from "@abstractframework/ui-kit";

export type FetchOutcome =
  | { ok: true; value: unknown }
  | { ok: false; status?: number; message: string };

/** Shown while `GET /api/gateway/about` is in flight. */
export const GATEWAY_ABOUT_CHECKING: AfAboutVersions = { framework: null, frameworkNote: "checking…", gateway: null, gatewayNote: "checking…" };

/** `about` is the outcome of `GET /api/gateway/about`; undefined while the
 * request is in flight (or before About was first opened). */
export function aboutVersions(about: FetchOutcome | undefined): AfAboutVersions {
  if (!about) return GATEWAY_ABOUT_CHECKING;
  if (!about.ok) return aboutVersionsFromGateway(null, about.status ? `HTTP ${about.status}` : about.message);
  return aboutVersionsFromGateway(about.value as GatewayAboutPayload);
}
