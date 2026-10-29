import { readAloudPhase, type ReadAloudInput, type ReadAloudReply } from "../lib/read_aloud";

/**
 * What Code's conversation tells the read-aloud rule: its scope (principal + session), the phase
 * of its newest run (sending from here counts as live, so a run that goes loading → completed
 * without a live status still speaks its reply), and the newest reply on screen.
 */
export function codeReadAloudInput(args: {
  identity: string;
  sessionId: string;
  runId: string;
  status: string | null | undefined;
  sending: boolean;
  reply: ReadAloudReply | null;
}): ReadAloudInput {
  return {
    scope: `${args.identity}:${args.sessionId}`,
    phase: args.runId || args.sending ? readAloudPhase(args.status, args.sending) : "none",
    reply: args.reply,
  };
}
