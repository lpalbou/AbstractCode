import { resolveSendableModelOptions, type ModelOptions, type SpeculationValue } from "@abstractframework/ui-kit";
import { modelDiscovery } from "./model_discovery";

/**
 * The reasoning level and MTP value a run may carry (operator rule 2026-11-07): only what the
 * capability card of the model the run will use allows (the chosen provider/model, else the
 * default the Model settings show). Nothing chosen = no request; an unreadable card = no MTP.
 */
export function sendableRunOptions(
  prefs: { provider?: string; model?: string; reasoning?: string; speculation?: SpeculationValue },
  defaultModel?: { provider: string; model: string },
  fetchCapabilities: (model: string, provider: string) => Promise<unknown> = modelDiscovery.fetchModelCapabilities,
): Promise<ModelOptions> {
  const chosen = Boolean(String(prefs.provider || "").trim() && String(prefs.model || "").trim());
  const provider = chosen ? String(prefs.provider).trim() : defaultModel?.provider || "";
  const model = chosen ? String(prefs.model).trim() : defaultModel?.model || "";
  return resolveSendableModelOptions(
    { reasoning: prefs.reasoning, speculation: prefs.speculation },
    model ? () => fetchCapabilities(model, provider) : undefined,
  );
}
