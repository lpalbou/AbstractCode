// How an opened conversation is restored (0.15.3): which workflow its next turn sends, what is
// said about it, and whether it can be continued at all. Pure, so each rule has a test; app.tsx
// only renders the result.
//
// - AbstractNote's background work (`notes_work.ts`) opens to read only, with a plain note, and its
//   workflow is never restored into the picker.
// - A conversation whose recorded workflow version is gone continues with the loaded version of
//   the same bundle and flow, and says so in plain words.
// - Only a conversation whose workflow (bundle or flow) is gone altogether refuses the next turn.
import { replacedVersionNote, resolveConversationWorkflow, type SessionSummary, type WorkflowDefinition } from "./catalog";
import { NOTES_SESSION_NOTICE, isNotesRun, isNotesRunInput } from "./notes_work";
import { runSelectionSource } from "./workflow_selection";

export const WORKFLOW_GONE =
  "The workflow this conversation used is no longer available, so it cannot be continued here. Start a new conversation to keep working.";

export interface ConversationRestore {
  /** AbstractNote's background work: shown to read, never continued from here. */
  notesSession: boolean;
  /** The words shown in place of the composer (read-only), else "". */
  readOnlyNotice: string;
  /** The workflow the next turn sends (undefined: none restored). */
  workflow?: WorkflowDefinition;
  /** Said when the conversation continues on another version of its workflow, else "". */
  versionNote: string;
  /** Why the next turn is refused (the workflow is gone), else "". */
  error: string;
}

export function conversationRestore(options: {
  sessionId: string;
  runId: string;
  /** `GET /runs/{runId}/input_data`, null until loaded. */
  restoredInputs: unknown;
  /** The open run's record (`snapshot.run`), when loaded. */
  run: { workflow_id?: unknown } | null | undefined;
  workflows: readonly WorkflowDefinition[];
  /** The conversations of the runs page (`catalog.sessions`): their ROOT workflow is not Notes'. */
  conversationSessions: readonly Pick<SessionSummary, "sessionId">[];
  /** Notes' sessions of the runs page (`catalog.notesSessions`): their ROOT workflow is Notes'. */
  notesSessions: readonly Pick<SessionSummary, "sessionId">[];
  catalogLoading: boolean;
}): ConversationRestore {
  // The rule is the session's ROOT workflow, which the runs page decides for a listed session: a
  // conversation whose later turn ran a Notes flow stays a conversation. Only a session the page
  // does not hold (an old link) is judged by the run it opens on.
  const listedConversation = options.conversationSessions.some((item) => item.sessionId === options.sessionId);
  const notesSession =
    !listedConversation &&
    (options.notesSessions.some((item) => item.sessionId === options.sessionId) ||
      (Boolean(options.runId) && (isNotesRunInput(options.restoredInputs) || isNotesRun(options.run))));
  if (notesSession)
    return { notesSession, readOnlyNotice: NOTES_SESSION_NOTICE, versionNote: "", error: "" };
  const resolution = resolveConversationWorkflow(options.workflows, {
    inputData: options.restoredInputs,
    runWorkflowId: options.run?.workflow_id,
  });
  const fromGatewayDefault = runSelectionSource(options.restoredInputs) === "gateway_default";
  const versionNote =
    options.runId && resolution.workflow && !fromGatewayDefault
      ? replacedVersionNote(resolution.workflow, resolution.replaced)
      : "";
  const error =
    options.runId && options.restoredInputs && !options.catalogLoading && options.run && !resolution.workflow && !fromGatewayDefault
      ? WORKFLOW_GONE
      : "";
  return { notesSession, readOnlyNotice: "", ...(resolution.workflow ? { workflow: resolution.workflow } : {}), versionNote, error };
}
