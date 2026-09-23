import type { WorkflowTransport } from "@abstractframework/panel-chat";
import { gateway, gatewayRequest } from "./transport";

const runPath = (id: string) => `/api/gateway/runs/${encodeURIComponent(id)}`;
// Keep source compatibility with the currently vendored panel-chat type while
// forwarding the optional SSE-open signal introduced by the next package cut.
type OpeningWorkflowTransport = Omit<WorkflowTransport, "streamLedger"> & {
  streamLedger(
    runId: string,
    after: number,
    onStep: (item: unknown) => void,
    signal: AbortSignal,
    onOpen?: () => void,
  ): Promise<void>;
};

export const workflowTransport: OpeningWorkflowTransport = {
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
  streamLedger: (id, after, onStep, signal, onOpen) =>
    gateway.stream_ledger(id, {
      after,
      on_step: onStep,
      signal,
      on_open: onOpen,
    }),
  submitCommand: (command, signal) =>
    gatewayRequest("/api/gateway/commands", {
      method: "POST",
      body: JSON.stringify(command),
      signal,
    }),
};
