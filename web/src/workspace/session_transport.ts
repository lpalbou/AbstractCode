import type { WorkflowTransport } from "@abstractframework/panel-chat";
import { gateway, gatewayRequest } from "./transport";

const runPath = (id: string) => `/api/gateway/runs/${encodeURIComponent(id)}`;

/** A live-reply frame the gateway sent malformed: reported, then skipped. */
export type DeltaFrameError = {
  runId: string;
  error: Error;
  frame: { event: string; data: string };
};

/**
 * The workflow transport for panel-chat's controller. `onDeltaError` is
 * required: a malformed `llm.delta` / `llm.delta_end` frame is reported to it
 * and skipped, and the ledger stream (steps, final answer) keeps flowing.
 */
export function createWorkflowTransport(options: {
  onDeltaError: (report: DeltaFrameError) => void;
}): WorkflowTransport {
  return {
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
        on_delta_error: (error, frame) =>
          options.onDeltaError({ runId: id, error, frame }),
      }),
    submitCommand: (command, signal) =>
      gatewayRequest("/api/gateway/commands", {
        method: "POST",
        body: JSON.stringify(command),
        signal,
      }),
  };
}
