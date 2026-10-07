import { useState } from "react";
import type { AccountWorkflowRow } from "./account_preferences";

/** Settings → Workflow: "Default for new conversations", the ACCOUNT's choice kept by the
 * gateway (round 14). "Gateway default (<name>)" first — verbatim from the gateway, the words the
 * Assistant and the console show — then the workflows this account may run; a change is one PUT
 * at once (no Save); a refusal says "Not saved." + the gateway's sentence. */
export function AccountWorkflowDefault(props: {
  row: AccountWorkflowRow;
  save: (value: string | null) => Promise<unknown>;
  disabled?: boolean;
}) {
  const { row } = props;
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);
  const current = row.value ?? "";
  const listed = row.choices.some((c) => c.value === current);
  return (
    <div className="code-account-workflow" data-account-workflow>
      <label className="code-field">
        Default for new conversations
        <select
          aria-label="Default for new conversations"
          value={current}
          disabled={props.disabled || busy}
          aria-busy={busy || undefined}
          onChange={async (event) => {
            const next = event.target.value || null;
            setBusy(true);
            setNote(null);
            try {
              await props.save(next);
              setNote({ ok: true, text: "Saved." });
            } catch (reason) {
              setNote({ ok: false, text: `Not saved. ${reason instanceof Error ? reason.message : String(reason)}` });
            } finally {
              setBusy(false);
            }
          }}
        >
          <option value="">{row.gatewayDefaultLabel}</option>
          {current && !listed ? <option value={current}>{`${current} (no longer runs)`}</option> : null}
          {row.choices.map((c) => (
            <option key={c.value} value={c.value} title={c.workflowId || undefined}>
              {c.label}
            </option>
          ))}
        </select>
      </label>
      {row.state === "broken" && row.reason ? (
        <p className="code-field-help code-workflow-resolved is-missing" role="alert" data-account-workflow-reason>
          {row.reason}
        </p>
      ) : (
        <p className="code-field-help">
          Kept by the gateway for your account: the Assistant and every browser use the same choice. This conversation's workflow is picked above.
        </p>
      )}
      {note ? (
        <p className={`code-field-help${note.ok ? "" : " is-error"}`} role="status" data-account-workflow-note>
          {note.text}
        </p>
      ) : null}
    </div>
  );
}
