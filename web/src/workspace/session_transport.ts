import type { WorkflowTransport } from "@abstractframework/panel-chat";
import { gateway, gatewayRequest } from "./transport";

const runPath = (id: string) => `/api/gateway/runs/${encodeURIComponent(id)}`;

export const workflowTransport: WorkflowTransport = {
  getRun: (id, signal) => gatewayRequest(runPath(id), { signal }),
  getHistory: (id, signal) =>
    gatewayRequest(
      `${runPath(id)}/history_bundle?include_subruns=true&include_session=true&ledger_mode=full`,
      { signal },
    ),
  getLedger: (id, after, signal) =>
    gatewayRequest(`${runPath(id)}/ledger?after=${after}&limit=200`, {
      signal,
    }),
  // `onDelta` (live reply text) is handed straight to the controller, which
  // owns the live bubbles; delta frames never reach `onStep` or the cursor.
  streamLedger: (id, after, onStep, signal, onOpen, onDelta) =>
    gateway.stream_ledger(id, {
      after,
      on_step: onStep,
      signal,
      on_open: onOpen,
      on_delta: onDelta,
    }),
  submitCommand: (command, signal) =>
    gatewayRequest("/api/gateway/commands", {
      method: "POST",
      body: JSON.stringify(command),
      signal,
    }),
};
