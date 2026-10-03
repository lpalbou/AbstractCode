// Settings bound to an AUTOMATION (round 4): the same sections as a
// conversation's settings, editing the automation's definition in place.
// Every change is saved as a new revision through the gateway
// (`PATCH /automations/{id}` with `expected_revision`); the revision shown
// is the definition's, re-read after each save. Pickers save at once; typed
// fields save when typing pauses (no Save button per section). The
// "Automation" card keeps the kit's Edit form for title / task / schedule /
// context / tool approval / email result (one Save for that card).
import React, { useEffect, useRef, useState } from "react";
import {
  AfSettingsGroup,
  AutomationReviseForm,
  apiErrorText,
  isApiError,
  reviseChanges,
  type AutomationChanges,
  type AutomationDefinition,
  type AutomationSummary,
  type AutomationTarget,
  type AutomationWorkflowPickerOptions,
  type CommandReceipt,
  type MyEmailStatus,
  type ReviseForm,
} from "@abstractframework/ui-kit";
import type { RunPreferences } from "./settings_panel";
import { automationRunPreferences, automationSettingsChanges } from "./automation_settings";

/** Typed fields save this long after the last keystroke. */
export const AUTOMATION_SAVE_DEBOUNCE_MS = 700;

export type SaveState =
  | { status: "idle" }
  | { status: "saving" }
  | { status: "saved"; revision: number }
  | { status: "error"; message: string };

export function saveStateText(s: SaveState): string {
  if (s.status === "saving") return "Saving…";
  if (s.status === "saved") return `Saved as revision ${s.revision}; applies from the next run.`;
  if (s.status === "error") return s.message;
  return "";
}

/** The gateway's typed refusal (Contract G `reason_code`), never a guess from the text. */
export function saveErrorText(e: unknown): string {
  if (isApiError(e)) {
    if (e.code === "revision_conflict") return "Not saved: the automation changed elsewhere. The latest revision is shown; make the change again.";
    const t = apiErrorText(e);
    return `Not saved: ${t.title}${t.detail ? ` ${t.detail}` : ""}`;
  }
  return `Not saved: ${String((e as { message?: string } | null)?.message || e)}`;
}

export type AutomationSettingsPanelProps = {
  summary: AutomationSummary;
  definition: AutomationDefinition;
  busy: boolean;
  disabled: boolean;
  emailStatus?: MyEmailStatus | null;
  onOpenMyEmail?: () => void;
  onRevise: (changes: AutomationChanges, expectedRevision: number) => Promise<CommandReceipt>;
  /** Checks a NEW workflow target against its input schema (the Automation card's workflow picker). */
  prepareTarget?: (target: AutomationTarget) => Promise<AutomationTarget>;
  workflowPickerOptions?: AutomationWorkflowPickerOptions;
  /** The conversation sections, rendered for these preferences. */
  sections: (value: RunPreferences, onChange: (next: RunPreferences) => void) => React.ReactNode;
};

export function AutomationSettingsPanel(p: AutomationSettingsPanelProps): React.ReactElement {
  const revision = p.definition.revision;
  const [draft, setDraft] = useState<RunPreferences>(() => automationRunPreferences(p.definition.target.input_data));
  const [save, setSave] = useState<SaveState>({ status: "idle" });
  const [formErrors, setFormErrors] = useState<string[]>([]);
  const pending = useRef<RunPreferences | null>(null);
  const timer = useRef<number | null>(null);
  const definitionRef = useRef(p.definition);
  definitionRef.current = p.definition;
  const onReviseRef = useRef(p.onRevise);
  onReviseRef.current = p.onRevise;

  // A new revision (ours or someone else's) is the truth, unless the user is mid-edit.
  useEffect(() => {
    if (!pending.current) setDraft(automationRunPreferences(p.definition.target.input_data));
  }, [p.summary.automation_id, revision]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => {
    if (timer.current) window.clearTimeout(timer.current);
  }, []);

  const flush = async () => {
    timer.current = null;
    const prefs = pending.current;
    pending.current = null;
    if (!prefs) return;
    const def = definitionRef.current;
    const changes = automationSettingsChanges(def, prefs);
    if (!changes) return;
    setSave({ status: "saving" });
    try {
      await onReviseRef.current(changes, def.revision);
      setSave({ status: "saved", revision: def.revision + 1 });
    } catch (e) {
      setSave({ status: "error", message: saveErrorText(e) });
      setDraft(automationRunPreferences(definitionRef.current.target.input_data));
    }
  };

  const onChange = (next: RunPreferences) => {
    setDraft(next);
    pending.current = next;
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void flush(), AUTOMATION_SAVE_DEBOUNCE_MS);
  };

  return (
    <div className="code-automation-settings" data-revision={revision}>
      <p className="code-settings-binding">
        <span>Automation</span> <strong title={p.summary.title}>{p.summary.title}</strong>
        <span className="code-settings-revision" data-testid="automation-revision">Revision {revision}</span>
      </p>
      <p className={`code-settings-save is-${save.status}`} role="status" aria-live="polite">
        {saveStateText(save) || "Changes are saved as a new revision and apply from the next run."}
      </p>
      <AfSettingsGroup id="code-settings-automation" title="Automation" help="Title, task, schedule and what happens with results.">
        <AutomationReviseForm
          key={`${p.summary.automation_id}:${revision}`}
          summary={p.summary}
          definition={p.definition}
          busy={p.busy || p.disabled}
          errors={formErrors}
          heading={null}
          submitLabel="Save"
          emailStatus={p.emailStatus}
          onOpenMyEmail={p.onOpenMyEmail}
          workflowPickerOptions={p.workflowPickerOptions}
          onSubmit={(form: ReviseForm) => {
            const changes = reviseChanges(p.summary, form, p.definition);
            if (changes === null) {
              setFormErrors(["Nothing changed."]);
              return;
            }
            if ("errors" in changes) {
              setFormErrors(changes.errors as string[]);
              return;
            }
            setFormErrors([]);
            setSave({ status: "saving" });
            const prepared = async (): Promise<AutomationChanges> => {
              const c = changes as AutomationChanges;
              return form.target && c.target && p.prepareTarget ? { ...c, target: await p.prepareTarget(c.target) } : c;
            };
            void prepared()
              .then((c) => p.onRevise(c, revision))
              .then(() => setSave({ status: "saved", revision: revision + 1 }))
              .catch((e) => {
                // A target the workflow's inputs refuse is a form problem (shown in the form);
                // a gateway refusal is a save outcome.
                if (isApiError(e)) setSave({ status: "error", message: saveErrorText(e) });
                else {
                  setSave({ status: "idle" });
                  setFormErrors([String((e as { message?: string } | null)?.message || e)]);
                }
              });
          }}
        />
      </AfSettingsGroup>
      {p.sections(draft, onChange)}
    </div>
  );
}
