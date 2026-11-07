// AbstractNote's background work (operator, 2026-11-07): Notes files and processes notes through
// its own workflows (capture, drain, reflect, consolidate, recall), and those sessions and
// automations are an internal process of Notes, not the person's conversations or automations.
//
// The rule is by PURPOSE, read from the workflow a session or automation runs: its bundle id is
// exactly `abstractnote`. Never by the client that created it (one session pool for every client,
// 2026-09-27), never by a session-id prefix or any text.
//
// Where the bundle id is read (the fields this app already reads):
// - a run row of `GET /runs`: `workflow_id` = `bundle@version:flow` (the gateway's workflow identity);
// - `GET /runs/{id}/input_data`: `workflow_selection.bundle_id`, else `bundle_id` (served as
//   `bundle@version`), else `workflow_id`;
// - an automation of `GET /automations`: `target.bundle_ref` (`bundle@version`), else
//   `target.workflow_id`.

/** The AbstractNote bundle id (exact match). */
export const NOTES_BUNDLE_ID = "abstractnote";

/** Shown in place of the composer when a Notes session is opened (by a link or a notification). */
export const NOTES_SESSION_NOTICE =
  "This is part of the background work of AbstractNote (it files and processes your notes). You can read it here, but it cannot be continued from AbstractCode.";

type Json = Record<string, unknown>;

const record = (value: unknown): Json | undefined =>
  value && typeof value === "object" && !Array.isArray(value) ? (value as Json) : undefined;

/** `bundle` of a bundle reference `bundle` or `bundle@version`. */
function bundleOfRef(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const bundle = value.split("@")[0].trim();
  return bundle || undefined;
}

/** `bundle` of a gateway workflow id `bundle[@version]:flow`; undefined when it names no bundle. */
export function workflowBundleId(workflowId: unknown): string | undefined {
  if (typeof workflowId !== "string") return undefined;
  const colon = workflowId.indexOf(":");
  return colon > 0 ? bundleOfRef(workflowId.slice(0, colon)) : undefined;
}

/** A `GET /runs` row (or a run record) runs an AbstractNote workflow. */
export function isNotesRun(run: unknown): boolean {
  return workflowBundleId(record(run)?.workflow_id ?? record(run)?.workflowId) === NOTES_BUNDLE_ID;
}

/** A `GET /runs/{id}/input_data` answer belongs to an AbstractNote workflow. */
export function isNotesRunInput(inputData: unknown): boolean {
  const body = record(inputData);
  if (!body) return false;
  const selection = record(record(body.input_data)?.workflow_selection ?? body.workflow_selection);
  const bundle =
    bundleOfRef(selection?.bundle_id) ?? bundleOfRef(body.bundle_id) ?? workflowBundleId(body.workflow_id);
  return bundle === NOTES_BUNDLE_ID;
}

/**
 * The header picker's list (`GET /bundles?executable_for=abstractcode.agent.v1`) without
 * AbstractNote's flows. Notes' "Ask your notes" declares the chat contract, but a session whose
 * root workflow is Notes' is Notes' work (left out of Conversations and read-only here), so Code
 * does not start one: a conversation started with it would leave the list after its first turn.
 */
export function withoutNotesEntries<T extends { entries: Array<{ bundleId?: string }> }>(workflows: T): T {
  return { ...workflows, entries: workflows.entries.filter((entry) => entry.bundleId !== NOTES_BUNDLE_ID) };
}

/** An automation of `GET /automations` runs an AbstractNote workflow. */
export function isNotesAutomation(summary: unknown): boolean {
  const target = record(record(summary)?.target);
  return (bundleOfRef(target?.bundle_ref) ?? workflowBundleId(target?.workflow_id)) === NOTES_BUNDLE_ID;
}
