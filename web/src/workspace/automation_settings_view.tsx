// Settings bound to an AUTOMATION (round 4; split into rail panels in round 5):
// the same sections as a conversation's settings, editing the automation's
// definition in place. ONE draft per selected automation (the hook below)
// feeds every rail panel, so Model, Workspace, Tools and Skills edits share
// one debounce and one revision line.
// Every change is saved as a new revision through the gateway
// (`PATCH /automations/{id}` with `expected_revision`); the revision shown
// is the definition's, re-read after each save. Pickers save at once; typed
// fields save when typing pauses (no Save button per section). The
// Workflow panel keeps the kit's Edit form for title / task / schedule /
// workflow / context / tool approval / email result (one Save for that form).
import React, { useEffect, useRef, useState } from "react";
import {
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

export type AutomationDetailLike = { summary: AutomationSummary; definition: AutomationDefinition };

export type AutomationSettings = {
  automationId: string;
  revision: number;
  draft: RunPreferences;
  /** A settings-panel change: kept in the draft, saved when typing pauses. */
  onChange: (next: RunPreferences) => void;
  save: SaveState;
  setSave: (s: SaveState) => void;
};

/**
 * The selected automation's settings draft, or null when no definition is
 * loaded. A new revision (ours or someone else's) replaces the draft unless an
 * edit is pending; switching automations saves a pending edit against the
 * automation it was made on (never against the newly selected one).
 */
export function useAutomationSettings(
  detail: AutomationDetailLike | null | undefined,
  onRevise: (automationId: string, changes: AutomationChanges, expectedRevision: number) => Promise<CommandReceipt>,
): AutomationSettings | null {
  const definition = detail?.definition ?? null;
  const automationId = detail?.summary.automation_id ?? "";
  const revision = definition?.revision ?? 0;
  const [draft, setDraft] = useState<RunPreferences>(() => automationRunPreferences(definition?.target.input_data));
  const [save, setSave] = useState<SaveState>({ status: "idle" });
  const pending = useRef<{ prefs: RunPreferences; automationId: string } | null>(null);
  const timer = useRef<number | null>(null);
  const definitionRef = useRef(definition);
  definitionRef.current = definition;
  const idRef = useRef(automationId);
  idRef.current = automationId;
  const onReviseRef = useRef(onRevise);
  onReviseRef.current = onRevise;
  const savedFor = useRef<{ id: string; def: AutomationDefinition | null }>({ id: automationId, def: definition });

  const flush = async () => {
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = null;
    const job = pending.current;
    pending.current = null;
    if (!job) return;
    // The definition the edit was made on: the current one only while the same automation is shown.
    const def = job.automationId === idRef.current ? definitionRef.current : savedFor.current.id === job.automationId ? savedFor.current.def : null;
    if (!def) return;
    const changes = automationSettingsChanges(def, job.prefs);
    if (!changes) return;
    const sameView = () => job.automationId === idRef.current;
    if (sameView()) setSave({ status: "saving" });
    try {
      await onReviseRef.current(job.automationId, changes, def.revision);
      if (sameView()) setSave({ status: "saved", revision: def.revision + 1 });
    } catch (e) {
      if (!sameView()) return;
      setSave({ status: "error", message: saveErrorText(e) });
      setDraft(automationRunPreferences(definitionRef.current?.target.input_data));
    }
  };

  // Another automation selected: save what was pending on the previous one, then show the new one.
  useEffect(() => {
    if (pending.current && pending.current.automationId !== automationId) void flush();
    savedFor.current = { id: automationId, def: definition };
    setSave({ status: "idle" });
    setDraft(automationRunPreferences(definition?.target.input_data));
  }, [automationId]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => {
    savedFor.current = { id: automationId, def: definition };
    if (!pending.current) setDraft(automationRunPreferences(definition?.target.input_data));
  }, [revision]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => {
    if (timer.current) window.clearTimeout(timer.current);
  }, []);

  if (!detail || !definition) return null;
  return {
    automationId,
    revision,
    draft,
    save,
    setSave,
    onChange: (next: RunPreferences) => {
      setDraft(next);
      pending.current = { prefs: next, automationId };
      if (timer.current) window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => void flush(), AUTOMATION_SAVE_DEBOUNCE_MS);
    },
  };
}

/** The first lines of every settings panel bound to an automation: what is edited, its revision, the save outcome. */
export function AutomationBinding(p: { summary: AutomationSummary; settings: AutomationSettings }): React.ReactElement {
  return (
    <>
      <p className="code-settings-binding">
        <span>Automation</span> <strong title={p.summary.title}>{p.summary.title}</strong>
        <span className="code-settings-revision" data-testid="automation-revision">Revision {p.settings.revision}</span>
      </p>
      <p className={`code-settings-save is-${p.settings.save.status}`} role="status" aria-live="polite">
        {saveStateText(p.settings.save) || "Changes are saved as a new revision and apply from the next run."}
      </p>
    </>
  );
}

export type AutomationDefinitionFormProps = {
  summary: AutomationSummary;
  definition: AutomationDefinition;
  settings: AutomationSettings;
  busy: boolean;
  disabled: boolean;
  emailStatus?: MyEmailStatus | null;
  onOpenMyEmail?: () => void;
  onRevise: (changes: AutomationChanges, expectedRevision: number) => Promise<CommandReceipt>;
  /** Checks a NEW workflow target against its input schema (the form's workflow picker). */
  prepareTarget?: (target: AutomationTarget) => Promise<AutomationTarget>;
  workflowPickerOptions?: AutomationWorkflowPickerOptions;
};

/** The automation's Workflow panel: the kit's Edit form (title, task, schedule, workflow, results), one Save. */
export function AutomationDefinitionForm(p: AutomationDefinitionFormProps): React.ReactElement {
  const revision = p.definition.revision;
  const [formErrors, setFormErrors] = useState<string[]>([]);
  const setSave = p.settings.setSave;
  return (
    <div className="code-automation-definition" data-revision={revision}>
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
    </div>
  );
}
