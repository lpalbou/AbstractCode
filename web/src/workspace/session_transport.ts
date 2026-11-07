import { gatewayApiPath } from "@abstractframework/ui-kit";
import { createReplayViewTransport, type WorkflowTransport } from "@abstractframework/panel-chat";
import { gateway, gatewayRequest } from "./transport";
import { StreamGate } from "./stream_gate";

const runPath = (id: string) => gatewayApiPath(`runs/${encodeURIComponent(id)}`);

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
  /** Stream rules (hidden tab holds none; one per run and kind); tests inject theirs. */
  streams?: StreamGate;
}): WorkflowTransport {
  const streams = options.streams ?? new StreamGate();
  const replay = createReplayViewTransport({
    fetch: (path, init) =>
      fetch(gatewayApiPath(path), { ...init, credentials: "same-origin" }),
  });
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
    streamLedger: (id, after, onStep, signal, onOpen, onDelta) => {
      // A reopened stream (tab shown again) resumes after the last step it delivered.
      let cursor = after;
      return streams.run(`ledger:${id}`, signal, (inner) =>
        gateway.stream_ledger(id, {
          after: cursor,
          on_step: (event) => {
            if (typeof event.cursor === "number" && event.cursor > cursor) cursor = event.cursor;
            onStep(event);
          },
          signal: inner,
          on_open: onOpen,
          on_delta: onDelta,
          on_delta_error: (error, frame) =>
            options.onDeltaError({ runId: id, error, frame }),
        }),
      );
    },
    // The gateway's replay view (`GET runs/{id}/view` + `/view/stream`, AbstractGateway 0.17.0):
    // the controller renders the conversation from it and folds no ledger records. Through the
    // app's authenticated proxy (same-origin, HttpOnly session); a gateway without the route is
    // detected once and the ledger fold above is used, labeled (`snapshot.source`).
    replayView: {
      ...replay,
      // A reopened stream resumes with the last frame id it delivered (Last-Event-ID).
      streamView: (id, opts) => {
        let lastEventId = opts.lastEventId;
        let bytes = 0;
        return streams.run(`view:${id}`, opts.signal, async (inner) => {
          const result = await replay.streamView(id, {
            ...opts,
            lastEventId,
            signal: inner,
            onFrame: (frame) => {
              if (frame.id) lastEventId = frame.id;
              opts.onFrame(frame);
            },
          });
          bytes += result.bytes;
        }).then(() => ({ bytes }));
      },
    },
    submitCommand: (command, signal) =>
      gatewayRequest(gatewayApiPath("commands"), {
        method: "POST",
        body: JSON.stringify(command),
        signal,
      }),
  };
}
