/** Preserve the gateway's VisualFlow descriptor as well as its JSON schema. */
export type InputSchema = Record<string, any>;
const object = (value: any): InputSchema =>
  value && typeof value === "object" && !Array.isArray(value) ? value : {};

/** Restore authored form values, never an old run's host permissions/routing.
 * Runtime namespaces are rebuilt from schema defaults and current Settings. */
export function restoreWorkflowFields(schema: InputSchema, input: unknown): Record<string, unknown> {
  const source = object(input);
  return Object.fromEntries(Object.keys(object(schema.properties)).flatMap(name => {
    if (["_runtime", "_limits"].includes(name) || !Object.prototype.hasOwnProperty.call(source, name)) return [];
    return [[name, structuredClone(source[name])]];
  }));
}

export function normalizeInputSchema(value: any): InputSchema | undefined {
  if (!value) return undefined;
  const raw = object(
    value.input_data_schema || value.input_schema || value.schema || value,
  );
  const properties = Object.fromEntries(
    Object.entries(object(raw.properties)).map(([key, field]) => [
      key,
      { ...object(field) },
    ]),
  );
  for (const pin of Array.isArray(value.inputs) ? value.inputs : []) {
    if (!pin?.id) continue;
    properties[pin.id] = {
      ...object(pin.schema),
      ...properties[pin.id],
      ...(pin.type ? { "x-abstract-type": pin.type } : {}),
      ...(pin.label ? { title: pin.label } : {}),
      ...(Object.prototype.hasOwnProperty.call(pin, "default")
        ? { default: pin.default }
        : {}),
    };
  }
  for (const [key, defaultValue] of Object.entries(object(value.defaults)))
    properties[key] = { ...properties[key], default: defaultValue };
  return { ...raw, properties };
}

/** Older Gateway v1 descriptors inferred required = !hasDefault. Their pin
 * flags are inferred too, so only the selected, version-pinned VisualFlow can
 * distinguish an author requirement from an optional runtime setting. */
export function reconcileVisualFlowSchema(
  schema: InputSchema,
  flow: unknown,
): InputSchema {
  const nodes = object(flow).nodes;
  if (!Array.isArray(nodes))
    throw new Error(
      "The workflow input declarations could not be verified. Please retry.",
    );
  const start = nodes.find(
    (node: any) => (node?.data?.nodeType || node?.type) === "on_flow_start",
  );
  if (!start || !Array.isArray(start.data?.outputs))
    throw new Error(
      "The workflow start inputs could not be verified. Please retry.",
    );
  const pins = start.data.outputs.filter(
    (pin: any) =>
      typeof pin?.id === "string" &&
      pin.id &&
      pin.type !== "execution" &&
      !["exec", "exec-out"].includes(pin.id),
  );
  const pinIds = new Set(pins.map((pin: any) => pin.id));
  const required = new Set<string>(
    (Array.isArray(schema.required) ? schema.required : []).filter(
      (id: string) => !pinIds.has(id),
    ),
  );
  const properties = { ...schema.properties };
  const defaults = object(start.data.pinDefaults);
  for (const pin of pins) {
    if (pin.required === true) required.add(pin.id);
    properties[pin.id] = {
      ...object(properties[pin.id]),
      ...object(pin.schema),
      ...(pin.type ? { "x-abstract-type": pin.type } : {}),
      ...(pin.label ? { title: pin.label } : {}),
      ...(Object.prototype.hasOwnProperty.call(defaults, pin.id)
        ? { default: structuredClone(defaults[pin.id]) }
        : {}),
    };
  }
  return { ...schema, properties, required: [...required] };
}
export function schemaDefaults(
  schema: InputSchema | undefined,
): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(schema?.properties || {}).flatMap(([key, field]) =>
      Object.prototype.hasOwnProperty.call(object(field), "default")
        ? [[key, structuredClone(object(field).default)]]
        : [],
    ),
  );
}
export function inputType(field: InputSchema): string {
  const type = Array.isArray(field.type)
    ? field.type.find((value: string) => value !== "null")
    : field.type;
  if (type) return type;
  const visual = field["x-abstract-type"];
  if (["tools", "assertions"].includes(visual)) return "array";
  if (["json_schema", "memory", "assertion"].includes(visual)) return "object";
  return "string";
}

/** Composer input is a declared text pin, never an invented agent contract.
 * Authors can disambiguate with x-abstract-role: prompt. Conventional names
 * cover existing published flows (including coding.v1's request pin). */
export function workflowPromptProperty(schema?: InputSchema): string | undefined {
  const fields = schema?.properties || {};
  const isText = (name: string) => {
    const field = fields[name];
    return field && (field.type === "string" ||
      (Array.isArray(field.type) && field.type.includes("string")) ||
      (!field.type && field["x-abstract-type"] === "string"));
  };
  const explicit = Object.keys(fields).filter(name => fields[name]?.["x-abstract-role"] === "prompt" && isText(name));
  if (explicit.length) return explicit.length === 1 ? explicit[0] : undefined;
  return ["prompt", "request", "task", "message", "query", "input"].find(isText);
}
export function validateWorkflowInputs(
  schema: InputSchema | undefined,
  supplied: Record<string, unknown>,
): string[] {
  if (!schema) return [];
  const values = { ...schemaDefaults(schema), ...supplied };
  const errors: string[] = [];
  for (const name of Array.isArray(schema.required) ? schema.required : []) {
    if (values[name] === undefined)
      errors.push(`${schema.properties?.[name]?.title || name} is required.`);
  }
  for (const [name, value] of Object.entries(values)) {
    const field = schema.properties?.[name];
    if (!field || value === undefined) continue;
    const title = field.title || name;
    if (
      Array.isArray(field.enum) &&
      !field.enum.some(
        (candidate: unknown) =>
          JSON.stringify(candidate) === JSON.stringify(value),
      )
    )
      errors.push(`${title} must be one of the available choices.`);
    const types = Array.isArray(field.type)
      ? field.type
      : field.type
        ? [field.type]
        : [];
    if (value === null && (types.includes("null") || !types.length)) continue;
    const actual = Array.isArray(value)
      ? "array"
      : value === null
        ? "null"
        : typeof value;
    const type = types.includes(actual)
      ? actual
      : actual === "number" && types.includes("integer")
        ? "integer"
        : types[0];
    if (
      (type === "number" || type === "integer") &&
      (typeof value !== "number" ||
        !Number.isFinite(value) ||
        (type === "integer" && !Number.isInteger(value)))
    )
      errors.push(
        `${title} must be ${type === "integer" ? "a whole number" : "a number"}.`,
      );
    if (
      typeof value === "number" &&
      field.minimum !== undefined &&
      value < field.minimum
    )
      errors.push(`${title} must be at least ${field.minimum}.`);
    if (
      typeof value === "number" &&
      field.maximum !== undefined &&
      value > field.maximum
    )
      errors.push(`${title} must be at most ${field.maximum}.`);
    if (type === "string" && typeof value !== "string")
      errors.push(`${title} must be text.`);
    if (
      typeof value === "string" &&
      field.minLength !== undefined &&
      value.length < field.minLength
    )
      errors.push(
        `${title} must contain at least ${field.minLength} characters.`,
      );
    if (type === "boolean" && typeof value !== "boolean")
      errors.push(`${title} must be true or false.`);
    if (type === "array" && !Array.isArray(value))
      errors.push(`${title} must be a JSON array.`);
    if (
      type === "object" &&
      (value === null || typeof value !== "object" || Array.isArray(value))
    )
      errors.push(`${title} must be a JSON object.`);
  }
  return errors;
}

/** Presentation groups only; nothing here grants runtime settings to a flow. */
export function modelInputGroups(schema?: InputSchema): Array<{
  provider: string;
  model: string;
  reasoning?: string;
  title: string;
}> {
  const fields = schema?.properties || {};
  const groups: Array<{
    provider: string;
    model: string;
    reasoning?: string;
    title: string;
  }> = [];
  for (const [key, raw] of Object.entries(fields)) {
    const field = object(raw);
    if (
      !(
        key === "provider" ||
        key.endsWith("_provider") ||
        String(field["x-abstract-type"] || "").startsWith("provider")
      )
    )
      continue;
    const semantic = String(field["x-abstract-type"] || "");
    const namedModel = key.replace("provider", "model");
    const typedModels = Object.keys(fields).filter(
      (name) =>
        fields[name]?.["x-abstract-type"] ===
        semantic.replace(/^provider/, "model"),
    );
    const model =
      namedModel !== key && fields[namedModel]
        ? namedModel
        : typedModels.length === 1
          ? typedModels[0]
          : "";
    if (!model) continue;
    const family =
      key === "provider"
        ? ""
        : key.startsWith("provider_")
          ? key.slice(9)
          : key.endsWith("_provider")
            ? key.slice(0, -9)
            : semantic.replace(/^provider_?/, "");
    const prefix = family && family !== "text" ? `${family}_` : "";
    const reasoning = [
      `${prefix}reasoning`,
      `${prefix}thinking`,
      `${prefix}reasoning_effort`,
      `reasoning_${family}`,
      `thinking_${family}`,
    ].find((name) => fields[name]);
    groups.push({
      provider: key,
      model,
      reasoning,
      title: prefix ? prefix.replace(/_/g, " ").trim() : "Text model",
    });
  }
  return groups;
}
