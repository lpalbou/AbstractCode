import { resolveSendableModelOptions, type ModelOptions, type SpeculationValue } from "@abstractframework/ui-kit";
import { modelDiscovery } from "./model_discovery";

type RunModel = { provider: string; model: string };
type RunPreferences = { provider?: string; model?: string; reasoning?: string; speculation?: SpeculationValue };
type FetchCapabilities = (model: string, provider: string) => Promise<unknown>;

/** How long a send waits for the model's capability card before the kit's fail-safe applies
 * (no MTP depth; an explicit Off and the reasoning level are kept). */
export const CAPABILITY_WAIT_MS = 10_000;

function bounded<T>(work: Promise<T>, ms: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const late = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error("The model's capabilities did not arrive in time.")), ms);
  });
  return Promise.race([work, late]).finally(() => clearTimeout(timer));
}

/**
 * The reasoning level and MTP value a run may carry (operator rule 2026-11-07): only what the
 * capability card of the model the run will use allows (the chosen provider/model, else the
 * default given). Nothing chosen = no request; an unreadable or late card = no MTP depth.
 */
export function sendableRunOptions(
  prefs: RunPreferences,
  defaultModel?: RunModel,
  fetchCapabilities: FetchCapabilities = modelDiscovery.fetchModelCapabilities,
  waitMs: number = CAPABILITY_WAIT_MS,
): Promise<ModelOptions> {
  const chosen = Boolean(String(prefs.provider || "").trim() && String(prefs.model || "").trim());
  const provider = chosen ? String(prefs.provider).trim() : defaultModel?.provider || "";
  const model = chosen ? String(prefs.model).trim() : defaultModel?.model || "";
  return resolveSendableModelOptions(
    { reasoning: prefs.reasoning, speculation: prefs.speculation },
    model ? () => bounded(Promise.resolve().then(() => fetchCapabilities(model, provider)), waitMs) : undefined,
  );
}

function pin(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

/** The provider/model a built run input names: its `_runtime` route, else its public pins. */
export function inputRunModel(input: unknown): RunModel | undefined {
  const root = input && typeof input === "object" ? (input as Record<string, unknown>) : {};
  const runtime = root._runtime && typeof root._runtime === "object" ? (root._runtime as Record<string, unknown>) : {};
  for (const source of [runtime, root]) {
    const provider = pin(source.provider);
    const model = pin(source.model);
    if (provider && model) return { provider, model };
  }
  return undefined;
}

/**
 * Build a run input whose reasoning and MTP are checked against the model THAT input runs on:
 * the input is drafted without model options, its provider/model (Settings choice, else the
 * target workflow's own pins) names the card to read, else the gateway default's model.
 */
export async function guardedRunInput<T>(
  build: (options: ModelOptions) => T,
  prefs: RunPreferences,
  gatewayDefault: RunModel | undefined,
  fetchCapabilities?: FetchCapabilities,
  waitMs?: number,
): Promise<T> {
  const target = inputRunModel(build({})) || gatewayDefault;
  const route = { ...prefs, provider: target?.provider || "", model: target?.model || "" };
  return build(await sendableRunOptions(route, undefined, fetchCapabilities, waitMs));
}
