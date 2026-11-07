// The session tool policy client the approval card writes through (Allow / Allow All, for this
// session only): ui-kit's client over this app's gateway proxy, with every request bounded. The
// card writes the policy BEFORE it answers the request, so a gateway that never answers would
// leave "Allowing…" on screen forever; after SESSION_POLICY_TIMEOUT_MS the request is aborted and
// the card shows its failure line with the reason, and the person can try again or Deny.
import { createSessionToolPolicyClient, type SessionToolPolicyCapability } from "@abstractframework/ui-kit";
import { gatewayRequest } from "./transport";

export const SESSION_POLICY_TIMEOUT_MS = 15_000;

type Request = (path: string, init: RequestInit) => Promise<unknown>;

/** `request` with an abort after `timeoutMs`; rejects with a plain reason even if `request` ignores the signal. */
export function boundedPolicyRequest(request: Request = gatewayRequest, timeoutMs = SESSION_POLICY_TIMEOUT_MS) {
  return async (path: string, init: { method: string; body?: unknown }): Promise<any> => {
    const abort = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const expired = new Promise<never>((_, reject) => {
      timer = setTimeout(() => {
        abort.abort();
        reject(new Error(`the gateway did not answer within ${Math.round(timeoutMs / 1000)} seconds. Try again, or Deny.`));
      }, timeoutMs);
    });
    try {
      return await Promise.race([
        request(path, { method: init.method, signal: abort.signal, ...(init.body === undefined ? {} : { body: JSON.stringify(init.body) }) }),
        expired,
      ]);
    } finally {
      clearTimeout(timer);
    }
  };
}

/** The client app.tsx injects into the controller for this conversation's session. */
export function createCodeSessionPolicyClient(
  sessionId: string,
  capability: SessionToolPolicyCapability | null | undefined,
  options: { request?: Request; timeoutMs?: number } = {},
) {
  return createSessionToolPolicyClient(boundedPolicyRequest(options.request, options.timeoutMs), sessionId, capability, { client: "abstractcode-web" });
}
