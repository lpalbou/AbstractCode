import { GATEWAY_API_PATH, randomId } from "@abstractframework/ui-kit";
import { GatewayClient, GatewayHttpError } from "../lib/gateway_client";

/** The app proxy's CSRF header (from the session cookie), for mutations. */
export function csrfHeaders(): Record<string, string> {
  const csrf = document.cookie
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith("abstractcode_gateway_csrf="));
  return csrf
    ? {
        "X-AbstractCode-CSRF": decodeURIComponent(
          csrf.slice("abstractcode_gateway_csrf=".length),
        ),
      }
    : {};
}

/** App-origin only. The server exchanges its HttpOnly session for gateway auth. */
export async function gatewayRequest<T = any>(
  path: string,
  init: RequestInit = {},
): Promise<T> {
  // Relative ("api/gateway/…", ui-kit gatewayApiPath): it resolves under the
  // page's base, where this app's server proxies it to the gateway.
  if (!path.startsWith(`${GATEWAY_API_PATH}/`))
    throw new Error("Gateway requests must use the app's authenticated proxy (a relative api/gateway/… path).");
  const headers = new Headers(init.headers);
  headers.set("Accept", "application/json");
  if (init.body && !(init.body instanceof FormData))
    headers.set("Content-Type", "application/json");
  for (const [name, value] of Object.entries(csrfHeaders()))
    headers.set(name, value);
  const response = await fetch(path, {
    ...init,
    headers,
    credentials: "same-origin",
  });
  if (!response.ok) {
    const raw = await response.text();
    let detail = raw;
    try {
      const body = JSON.parse(raw);
      detail =
        typeof body.detail === "string"
          ? body.detail
          : JSON.stringify(body.detail || body);
    } catch {
      /* Keep server text. */
    }
    throw new GatewayHttpError(
      detail || `Gateway request failed (${response.status})`,
      { status: response.status, body_text: raw },
    );
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}

export const gateway = new GatewayClient({ base_url: "" });

export function formatError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** A v4 UUID; the kit's randomId() also works over plain http (no crypto.randomUUID there). */
export function newId(): string {
  return randomId();
}

export async function downloadArtifact(
  runId: string,
  artifactId: string,
  filename: string,
): Promise<void> {
  const { blob } = await gateway.get_run_artifact_blob(runId, artifactId);
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename.replace(/[\\/]/g, "_") || "artifact";
  link.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}
