// The session tool policy client the approval card writes through (Allow / Allow All, for this
// session only): ui-kit's client over this app's gateway proxy. The card writes the policy BEFORE it
// answers the request. A pending permission is a durable wait, so no timer ever fails that write: while
// it is in flight the card shows "Allowing…", and only a real answer (an HTTP error, a refused
// connection) shows the failure line with the reason and Try again.
import { createSessionToolPolicyClient, type SessionToolPolicyCapability } from "@abstractframework/ui-kit";
import { gatewayRequest } from "./transport";

type Request = (path: string, init: RequestInit) => Promise<unknown>;

/** `request` with the JSON body the kit client hands over; no signal, no timeout. */
export function policyRequest(request: Request = gatewayRequest) {
  return async (path: string, init: { method: string; body?: unknown }): Promise<any> =>
    request(path, { method: init.method, ...(init.body === undefined ? {} : { body: JSON.stringify(init.body) }) });
}

/** The client app.tsx injects into the controller for this conversation's session. */
export function createCodeSessionPolicyClient(
  sessionId: string,
  capability: SessionToolPolicyCapability | null | undefined,
  options: { request?: Request } = {},
) {
  return createSessionToolPolicyClient(policyRequest(options.request), sessionId, capability, { client: "abstractcode-web" });
}
