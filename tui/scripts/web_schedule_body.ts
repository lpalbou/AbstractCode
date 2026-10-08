// The Code web's `POST /api/gateway/automations` bodies for /schedule parity (R17.2).
//
// Runs the web's OWN functions — `buildWorkflowInput` (web/src/workspace/catalog.ts),
// `schemaDefaults` / `validateWorkflowInputs` / `normalizeInputSchema` (input_schema.ts),
// `withAutomationWorkspace` (automation_workspaces.tsx) and the kit's `buildCreateRequest`,
// `withAutomationTools`, `automationToolSelection` (ui-kit dist, the version the web resolves) —
// for a few conversations, and writes the bodies to tests/fixtures/schedule/web_schedule_bodies.json.
// The few lines that are not exported are mirrored here with their source line:
//   - app.tsx `currentWorkflowInput(text, values, [], true, …)` (the options it passes, then the
//     forAutomation deletes);
//   - app.tsx `buildInput` (conversation workflow: {...schemaDefaults(schema), ...inputs}; a picked
//     workflow: schemaDefaults(nextSchema) + validateWorkflowInputs);
//   - app.tsx `initialTools` + automations_view.tsx NewAutomationDialog `onSubmit` + the kit
//     dialog's `submit` (the tool selection, then the workspace).
// `cargo test schedule_body_equals_the_web` builds the same cases in Rust and compares.
//
// Run (from anywhere; web/node_modules installed):
//   web/node_modules/.bin/vite-node --root . tui/scripts/web_schedule_body.ts        (repo root)
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { buildWorkflowInput } from "../../web/src/workspace/catalog";
import { normalizeInputSchema, schemaDefaults, validateWorkflowInputs } from "../../web/src/workspace/input_schema";
import { withAutomationWorkspace } from "../../web/src/workspace/automation_workspaces";
import { automationToolSelection, buildCreateRequest, withAutomationTools } from "../../web/node_modules/@abstractframework/ui-kit/dist/index.js";

const here = dirname(fileURLToPath(import.meta.url));
const fixtures = join(here, "..", "tests", "fixtures", "schedule");
const read = (name: string) => JSON.parse(readFileSync(join(fixtures, name), "utf8"));
const AGENT = "abstractcode.agent.v1";

/** A neutral case: what the conversation holds and what the dialog chose. */
type Case = {
  name: string;
  /** The schema fixture of the workflow whose inputs run. */
  schema: string;
  /** The conversation's own workflow target (the web's `automationTarget`). */
  conversationTarget: Record<string, unknown>;
  /** A workflow picked in the dialog (the web's `chosenTarget`), if any. */
  picked?: Record<string, unknown>;
  conversation: {
    provider: string; model: string; reasoning: string; stream: "on" | "off" | "gateway_default";
    maxIterations: number; maxTokens: number; skills: string[];
    /** The conversation's enabled tools when it customised them (`/tools`), else null. */
    customizedTools: string[] | null;
  };
  dialog: {
    prompt: string; title: string;
    when: { kind: "every"; amount: number; unit: "m" | "h" | "d" } | { kind: "once"; at: string } | { kind: "email" };
    email?: { every: { amount: number; unit: "m" | "h" | "d" } | null; maxBatch: number | null; fromIn: string; fromDomainIn: string; toIn: string; subjectContains: string; hasAttachment: "any" | "yes" | "no" };
    emailUsable: boolean; notifyEmail: boolean; recipients: { mode: "self" | "list"; addresses: string };
    context: "independent" | "growing"; growingMaxTokens: number; toolApproval: "auto" | "ask";
    /** The Tools section's final selection: "initial" keeps the dialog's first value. */
    tools: "initial" | string[] | null;
    startAt: string; count: number | null; until: string;
    workspace: { posture: string; default_mode: string; folders: { path: string; mode: string }[] } | null;
  };
};

const cases: Case[] = [
  {
    name: "inherit",
    schema: "input_schema_basic_agent.json",
    conversationTarget: { flow_id: "@default", interface: AGENT },
    conversation: { provider: "lmstudio", model: "qwen3-4b", reasoning: "high", stream: "on", maxIterations: 0, maxTokens: 0, skills: ["coredoc"], customizedTools: null },
    dialog: {
      prompt: "Report the free memory of this computer\nand warn above 90%", title: "", when: { kind: "every", amount: 24, unit: "h" },
      emailUsable: false, notifyEmail: false, recipients: { mode: "self", addresses: "" },
      context: "independent", growingMaxTokens: 50000, toolApproval: "auto", tools: "initial",
      startAt: "", count: null, until: "",
      workspace: { posture: "allowed_only", default_mode: "rw", folders: [{ path: "/Users/ada/home/work", mode: "ro" }] },
    },
  },
  {
    name: "custom",
    schema: "input_schema_basic_agent.json",
    conversationTarget: { bundle_ref: "basic-agent@0.0.5", flow_id: "81795ea9" },
    conversation: {
      provider: "openai", model: "gpt-5-mini", reasoning: "", stream: "off", maxIterations: 30, maxTokens: 65536, skills: [],
      customizedTools: ["edit_file", "analyze_code", "execute_command", "list_files", "read_file", "search_files", "write_file"],
    },
    dialog: {
      prompt: "Summarise the invoices that arrived", title: "Invoice digest", when: { kind: "email" },
      email: { every: { amount: 2, unit: "h" }, maxBatch: 20, fromIn: "Billing@Example.test, ap@vendor.test", fromDomainIn: "", toIn: "", subjectContains: "invoice", hasAttachment: "yes" },
      emailUsable: true, notifyEmail: true, recipients: { mode: "list", addresses: "boss@example.test" },
      context: "growing", growingMaxTokens: 20000, toolApproval: "ask",
      tools: ["analyze_code", "list_files", "read_file", "search_files"],
      startAt: "", count: null, until: "", workspace: null,
    },
  },
  {
    name: "picked",
    schema: "input_schema_react_agent.json",
    conversationTarget: { bundle_ref: "basic-agent@0.0.5", flow_id: "81795ea9" },
    picked: { bundle_ref: "react-agent@0.1.0", flow_id: "react" },
    conversation: { provider: "", model: "", reasoning: "medium", stream: "gateway_default", maxIterations: 0, maxTokens: 0, skills: ["coredoc", "release"], customizedTools: null },
    dialog: {
      prompt: "Check the build", title: "", when: { kind: "every", amount: 8, unit: "h" },
      emailUsable: true, notifyEmail: true, recipients: { mode: "self", addresses: "" },
      context: "independent", growingMaxTokens: 50000, toolApproval: "auto", tools: "initial",
      startAt: "2026-11-01 08:00", count: 5, until: "2026-12-01 18:30", workspace: null,
    },
  },
];

const local = (typed: string) => typed.replace(" ", "T");

/** app.tsx currentWorkflowInput(text, values, [], true, workflow, schema) — the automation case. */
function currentWorkflowInput(c: Case, text: string, values: Record<string, unknown>, schema: Record<string, any>) {
  const workflow = { id: "w", workflowId: "w", flowId: "f", name: "w", description: "", interfaces: [AGENT] } as any;
  const conv = c.conversation;
  const input = buildWorkflowInput({
    workflow,
    inputSchema: schema,
    schemaInputs: values as any,
    prompt: text,
    promptProperty: "prompt",
    model: { provider: conv.provider, model: conv.model },
    reasoning: conv.reasoning || undefined,
    speculation: undefined,
    streamReplies: conv.stream,
    systemPromptExtra: undefined,
    attachments: [],
    limits: { maxIterations: conv.maxIterations || undefined, maxTokens: conv.maxTokens || undefined },
    workspace: { root: "/Users/ada/conversation-folder" },
    tools: conv.customizedTools ?? undefined,
    toolPolicy: conv.customizedTools ? { autoApproveTools: [], requireApprovalTools: conv.customizedTools } : undefined,
    skills: conv.skills.length ? conv.skills : undefined,
  });
  delete input.workspace_root;
  delete input.workspace_access_mode;
  delete input.workspace_allowed_paths;
  delete input.workspace;
  if (input._runtime && typeof input._runtime === "object") delete (input._runtime as Record<string, unknown>).tool_policy;
  return input;
}

const out: Record<string, unknown> = {};
for (const c of cases) {
  const schema = normalizeInputSchema(read(c.schema))!;
  const d = c.dialog;
  // app.tsx: the conversation's inputs = schemaDefaults(schema) (app.tsx:634).
  const inputs = schemaDefaults(schema);
  const initialTools = c.picked ? null : automationToolSelection(c.conversation.customizedTools ? { ...inputs, tools: c.conversation.customizedTools } : inputs);
  const selectedTools = d.tools === "initial" ? initialTools : d.tools;
  const emailKind = d.when.kind === "email" && d.emailUsable;
  const form: any = {
    prompt: d.prompt,
    when: d.when.kind === "once" ? { kind: "once", at: local(d.when.at) } : d.when.kind === "every" ? { kind: "every", amount: d.when.amount, unit: d.when.unit } : { kind: "every", amount: 24, unit: "h" },
    ...(emailKind ? { trigger: "email", email: { ...d.email!, usesModel: true } } : {}),
    ...(d.emailUsable && d.notifyEmail ? { notifyEmail: true } : {}),
    ...(d.emailUsable && d.recipients.mode === "list" ? { emailRecipients: d.recipients } : {}),
    context: d.context,
    growingMaxTokens: d.growingMaxTokens,
    toolApproval: d.toolApproval,
    title: d.title,
    ...(d.when.kind === "every" && d.startAt ? { startAt: local(d.startAt) } : {}),
    ...(d.when.kind === "every" && d.count !== null ? { count: d.count } : {}),
    ...(d.when.kind === "every" && d.until ? { until: local(d.until) } : {}),
  };
  const target = (c.picked ?? c.conversationTarget) as any;
  // The kit dialog's submit (availableTools defined in the Code web).
  const probe = buildCreateRequest(form, { target, requestId: "" });
  if (!probe.ok) throw new Error(`${c.name}: ${probe.errors.join(" ")}`);
  const prepared = { ...probe.body, target: { ...probe.body.target, input_data: withAutomationTools(probe.body.target.input_data || {}, selectedTools) } };
  const body = { ...prepared, request_id: "rid-parity" };
  // app.tsx buildInput.
  let built: Record<string, unknown>;
  if (!c.picked) built = currentWorkflowInput(c, String(body.target.input_data?.prompt || ""), { ...schemaDefaults(schema), ...inputs }, schema);
  else {
    built = currentWorkflowInput(c, String(body.target.input_data?.prompt || ""), schemaDefaults(schema), schema);
    const problems = validateWorkflowInputs(schema, built);
    if (problems.length) throw new Error(`${c.name}: ${problems.join(" ")}`);
  }
  // NewAutomationDialog onSubmit.
  const tooled = withAutomationTools(built as any, automationToolSelection(body.target.input_data as any));
  const input = withAutomationWorkspace(tooled, d.workspace as any);
  out[c.name] = { case: c, body: { ...body, target: { ...body.target, input_data: input } } };
}
const sorted = (v: unknown): unknown =>
  Array.isArray(v) ? v.map(sorted) : v && typeof v === "object" ? Object.fromEntries(Object.keys(v as object).sort().map((k) => [k, sorted((v as any)[k])])) : v;
writeFileSync(join(fixtures, "web_schedule_bodies.json"), JSON.stringify(sorted(out), null, 2) + "\n");
console.log(`wrote ${join(fixtures, "web_schedule_bodies.json")} (${cases.length} cases)`);
