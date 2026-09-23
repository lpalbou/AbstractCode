import React from "react";
import { navigateTabs } from "./tabs";
import { ProviderModelPicker } from "@abstractframework/ui-kit";
import { inputType, modelInputGroups } from "./input_schema";
import { modelDiscovery, discoveryForInput } from "./model_discovery";
export { schemaDefaults, validateWorkflowInputs } from "./input_schema";

type Schema = Record<string, any>;
type WorkflowInputsProps = {
  schema?: Schema;
  values: Record<string, unknown>;
  onChange: (values: Record<string, unknown>) => void;
  disabled?: boolean;
  omit?: string[];
  rawObject?: boolean;
  onEditorError?: (error: string) => void;
  defaultModel?: { provider: string; model: string };
  promptProperty?: string;
};

/** Chat owns prompt/context; Settings owns the common agent overrides. Other
 * optional pins are an expert surface, never onboarding for ordinary chat. */
export function AgentWorkflowInputs(
  props: WorkflowInputsProps,
): React.ReactElement {
  const names = Object.keys(props.schema?.properties || {});
  const required: string[] = props.schema?.required || [];
  const composerFields = [props.promptProperty || "prompt", "context", "messages", "attachments"];
  const settingsFields = [
    "provider",
    "model",
    "tools",
    "skills",
    "max_iterations",
    "use_context",
    "use_session_history",
  ];
  // Linked controls render from the provider row. Keep each group intact even
  // when only its model or reasoning pin is authored as required.
  const groups = modelInputGroups(props.schema).map((group) =>
    [group.provider, group.model, group.reasoning].filter(
      (name): name is string => Boolean(name),
    ),
  );
  const requiredFields = new Set(
    names.filter(
      (name) => required.includes(name) && !composerFields.includes(name),
    ),
  );
  for (const group of groups)
    if (group.some((name) => requiredFields.has(name)))
      group.forEach((name) => requiredFields.add(name));
  const advancedFields = new Set(
    names.filter(
      (name) =>
        !requiredFields.has(name) &&
        !composerFields.includes(name) &&
        !settingsFields.includes(name),
    ),
  );
  for (const group of groups)
    if (group.some((name) => advancedFields.has(name)))
      group.forEach((name) => advancedFields.add(name));
  return (
    <>
      <section
        className="code-settings-section"
        aria-label="Automatic agent configuration"
      >
        <h3>{requiredFields.size ? "Workflow inputs" : "Ready to chat"}</h3>
        <p className="code-field-help">
          {requiredFields.size
            ? "This workflow asks for the inputs below before it can run."
            : "No setup required. Write a message and press Send."}{" "}
          Optional values use workflow and Gateway defaults. Use Settings for
          model, workspace and tool permission overrides.
        </p>
      </section>
      {requiredFields.size ? (
        <WorkflowInputs
          {...props}
          omit={names.filter((name) => !requiredFields.has(name))}
          rawObject={false}
        />
      ) : null}
      {advancedFields.size ? (
        <details className="code-advanced-inputs">
          <summary>
            Advanced workflow inputs{" "}
            <span className="code-muted">Optional</span>
          </summary>
          <p className="code-field-help">
            Leave these unset to use the workflow and Gateway defaults. Only
            change them when you need a specific override.
          </p>
          <WorkflowInputs
            {...props}
            omit={names.filter((name) => !advancedFields.has(name))}
            rawObject={false}
          />
        </details>
      ) : null}
    </>
  );
}
export function withWorkflowAttachments(
  schema: Schema | undefined,
  values: Record<string, unknown>,
  attachments: unknown[],
): Record<string, unknown> {
  if (!attachments.length) return values;
  const field = schema?.properties?.attachments;
  if (!field || (field.type && field.type !== "array"))
    throw new Error(
      "This workflow does not declare an attachments input. Remove the attached files, or choose an agent or workflow that accepts them.",
    );
  return {
    ...values,
    attachments: [
      ...(Array.isArray(values.attachments) ? values.attachments : []),
      ...attachments,
    ],
  };
}

export type ParsedWorkflowInputObject =
  { ok: true; value: Record<string, unknown> } | { ok: false; error: string };

export function parseWorkflowInputObject(
  text: string,
): ParsedWorkflowInputObject {
  if (!text.trim()) return { ok: true, value: {} };
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return { ok: false, error: "Enter valid JSON." };
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    return {
      ok: false,
      error: "The workflow input payload must be a JSON object.",
    };
  }
  return { ok: true, value: parsed as Record<string, unknown> };
}

export function WorkflowInputs({
  schema,
  values,
  onChange,
  disabled = false,
  omit = [],
  rawObject = false,
  onEditorError,
  defaultModel,
}: WorkflowInputsProps): React.ReactElement {
  const [mode, setMode] = React.useState<"fields" | "json">("fields");
  const properties = Object.entries(schema?.properties || {}).filter(
    ([key]) => !omit.includes(key),
  );
  const modelGroups = modelInputGroups(schema);
  const grouped = new Set(
    modelGroups.flatMap((group) =>
      [group.provider, group.model, group.reasoning].filter(Boolean),
    ),
  );
  const selectMode = (next: "fields" | "json") => {
    setMode(next);
    onEditorError?.("");
  };
  const fields = properties.length ? (
    <div className="code-input-fields">
      {properties.map(([name, raw]) => {
        const field: Schema = {
          ...(raw as Schema),
          type: inputType(raw as Schema),
        };
        const group = modelGroups.find((item) => item.provider === name);
        if (group)
          return (
            <section className="code-settings-section" key={name}>
              <h3>{group.title}</h3>
              <ProviderModelPicker
                value={{
                  provider: String(values[group.provider] || ""),
                  model: String(values[group.model] || ""),
                  ...(group.reasoning
                    ? { reasoning: String(values[group.reasoning] || "") }
                    : {}),
                }}
                onChange={(next) =>
                  onChange({
                    ...values,
                    [group.provider]: next.provider,
                    [group.model]: next.model,
                    ...(group.reasoning
                      ? { [group.reasoning]: next.reasoning || "" }
                      : {}),
                  })
                }
                {...discoveryForInput(group.provider, field["x-abstract-type"])}
                fetchModelCapabilities={
                  group.reasoning
                    ? modelDiscovery.fetchModelCapabilities
                    : undefined
                }
                effectiveDefault={
                  group.provider === "provider" ? defaultModel : undefined
                }
                defaultHint={
                  group.provider === "provider" && defaultModel?.model
                    ? `Gateway default: ${defaultModel.provider} · ${defaultModel.model}. Workflow defaults take precedence.`
                    : "Empty routing pins inherit the Gateway defaults. Registered workflow defaults are loaded above any Gateway default."
                }
                disabled={disabled}
              />
            </section>
          );
        if (grouped.has(name)) return null;
        const id = `workflow-input-${name}`;
        const label = String(field.title || name).replace(/_/g, " ");
        const required =
          Array.isArray(schema?.required) && schema.required.includes(name);
        const update = (value: unknown) => {
          const next = { ...values };
          if (value === undefined) delete next[name];
          else next[name] = value;
          onChange(next);
        };
        return (
          <div className="code-field" key={name}>
            <label htmlFor={id}>
              {label}
              {required ? <span aria-label="required"> *</span> : null}
            </label>
            {field.enum ? (
              <select
                id={id}
                value={
                  values[name] === undefined ? "" : JSON.stringify(values[name])
                }
                disabled={disabled}
                onChange={(e) =>
                  update(
                    e.target.value ? JSON.parse(e.target.value) : undefined,
                  )
                }
              >
                <option value="">Choose {label}</option>
                {field.enum.map((value: unknown) => (
                  <option
                    key={JSON.stringify(value)}
                    value={JSON.stringify(value)}
                  >
                    {String(value)}
                  </option>
                ))}
              </select>
            ) : field.type === "boolean" ? (
              <select
                id={id}
                value={values[name] === undefined ? "" : String(values[name])}
                disabled={disabled}
                onChange={(e) =>
                  update(e.target.value ? e.target.value === "true" : undefined)
                }
              >
                <option value="">Workflow default</option>
                <option value="true">Yes</option>
                <option value="false">No</option>
              </select>
            ) : field.type === "number" || field.type === "integer" ? (
              <input
                id={id}
                type="number"
                value={
                  typeof values[name] === "number" ? String(values[name]) : ""
                }
                min={field.minimum}
                max={field.maximum}
                step={field.type === "integer" ? 1 : "any"}
                disabled={disabled}
                onChange={(e) =>
                  update(
                    e.target.value === "" ? undefined : Number(e.target.value),
                  )
                }
              />
            ) : field.type === "object" || field.type === "array" ? (
              <JsonInput
                key={name}
                id={id}
                value={values[name]}
                disabled={disabled}
                onChange={update}
              />
            ) : (
              <textarea
                id={id}
                rows={2}
                value={String(values[name] ?? "")}
                disabled={disabled}
                onChange={(e) => update(e.target.value)}
              />
            )}
            {field.description ? (
              <p className="code-field-help" id={`${id}-help`}>
                {field.description}
              </p>
            ) : null}
          </div>
        );
      })}
    </div>
  ) : (
    <p className="code-muted">
      This workflow declares no named top-level fields. Use JSON payload mode to
      provide registered or additional inputs.
    </p>
  );

  if (!rawObject) return fields;
  return (
    <div>
      <div
        className="code-settings-tabs"
        role="tablist"
        aria-label="Workflow input editor"
        onKeyDown={navigateTabs}
      >
        <button
          type="button"
          role="tab"
          tabIndex={mode === "fields" ? 0 : -1}
          aria-selected={mode === "fields"}
          onClick={() => selectMode("fields")}
        >
          Fields
        </button>
        <button
          type="button"
          role="tab"
          tabIndex={mode === "json" ? 0 : -1}
          aria-selected={mode === "json"}
          onClick={() => selectMode("json")}
        >
          JSON payload
        </button>
      </div>
      {mode === "json" ? (
        <>
          <JsonObjectInput
            value={values}
            disabled={disabled}
            onChange={onChange}
            onError={(message) => onEditorError?.(message)}
          />
          <p className="code-field-help">
            The complete object is sent unchanged. Nested and additional
            properties are allowed here; the gateway validates it against the
            registered workflow schema.
          </p>
        </>
      ) : (
        fields
      )}
    </div>
  );
}

function JsonObjectInput({
  value,
  onChange,
  onError,
  disabled,
}: {
  value: Record<string, unknown>;
  onChange: (value: Record<string, unknown>) => void;
  onError: (error: string) => void;
  disabled: boolean;
}) {
  const [text, setText] = React.useState(JSON.stringify(value, null, 2));
  const [error, setError] = React.useState("");
  const emitted = React.useRef<Record<string, unknown>>(value);
  React.useEffect(() => {
    if (value === emitted.current) return;
    emitted.current = value;
    setText(JSON.stringify(value, null, 2));
    setError("");
    onError("");
  }, [value, onError]);
  return (
    <div className="code-field">
      <label htmlFor="workflow-input-payload">Complete input object</label>
      <textarea
        id="workflow-input-payload"
        className="code-mono"
        rows={12}
        value={text}
        disabled={disabled}
        aria-invalid={Boolean(error)}
        onChange={(event) => {
          setText(event.target.value);
          const result = parseWorkflowInputObject(event.target.value);
          if (!result.ok) {
            setError(result.error);
            onError(result.error);
            return;
          }
          emitted.current = result.value;
          setError("");
          onError("");
          onChange(result.value);
        }}
      />
      {error ? (
        <span role="alert" className="code-error-text">
          {error}
        </span>
      ) : null}
    </div>
  );
}

function JsonInput({
  id,
  value,
  onChange,
  disabled,
}: {
  id: string;
  value: unknown;
  onChange: (value: unknown) => void;
  disabled: boolean;
}) {
  const [text, setText] = React.useState(
    value === undefined ? "" : JSON.stringify(value, null, 2),
  );
  const [error, setError] = React.useState("");
  const emitted = React.useRef(value);
  React.useEffect(() => {
    if (value === emitted.current) return;
    emitted.current = value;
    setText(value === undefined ? "" : JSON.stringify(value, null, 2));
    setError("");
  }, [value]);
  return (
    <>
      <textarea
        id={id}
        className="code-mono"
        rows={4}
        value={text}
        disabled={disabled}
        aria-invalid={Boolean(error)}
        onChange={(e) => {
          setText(e.target.value);
          try {
            const parsed = e.target.value.trim()
              ? JSON.parse(e.target.value)
              : undefined;
            emitted.current = parsed;
            onChange(parsed);
            setError("");
          } catch {
            emitted.current = e.target.value;
            onChange(e.target.value);
            setError("Enter valid JSON.");
          }
        }}
      />
      {error ? (
        <span role="alert" className="code-error-text">
          {error}
        </span>
      ) : null}
    </>
  );
}
