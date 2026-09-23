import { gateway, gatewayRequest } from "./transport";
export const modelDiscovery = {
  fetchProviders: async () => {
    const data = await gateway.discovery_providers();
    const items = data.providers || data.items;
    return Array.isArray(items)
      ? items.map((item: any) =>
          typeof item === "string" ? { name: item } : item,
        )
      : [];
  },
  fetchModels: async (provider: string) => {
    const data = await gateway.discovery_provider_models(provider);
    const items = data.models || data.items;
    return Array.isArray(items)
      ? items
          .map((item: any) =>
            typeof item === "string" ? item : item.id || item.name,
          )
          .filter(Boolean)
      : [];
  },
  fetchModelCapabilities: (model: string, provider = "") =>
    gateway.discovery_model_capabilities(model, provider),
};
export function defaultTextRoute(
  data: any,
): { provider: string; model: string } | undefined {
  for (const key of ["output.text", "input.text"]) {
    const route = (Array.isArray(data?.routes) ? data.routes : []).find(
      (item: any) =>
        item.key === key &&
        item.source !== "not_configured" &&
        item.provider &&
        item.model,
    );
    if (route) return { provider: route.provider, model: route.model };
  }
  return undefined;
}
export const fetchDefaultModel = () =>
  gatewayRequest("/api/gateway/config/capability-defaults").then(
    defaultTextRoute,
  );

/** Match media pins to their own task catalogs, never to the text-model list. */
export function discoveryForInput(providerPin: string, semanticType?: string) {
  if (providerPin.startsWith("provider_"))
    providerPin = `${providerPin.slice(9)}_provider`;
  else if (
    semanticType?.startsWith("provider_") &&
    !providerPin.endsWith("_provider")
  )
    providerPin = `${semanticType.slice(9)}_provider`;
  const task = (
    {
      image_provider: "text_to_image",
      image_edit_provider: "image_to_image",
      image_upscale_provider: "image_upscale",
      video_provider: "text_to_video",
      image_to_video_provider: "image_to_video",
    } as Record<string, string>
  )[providerPin];
  if (!task && !["music_provider", "voice_provider"].includes(providerPin))
    return modelDiscovery;
  const voice = providerPin === "voice_provider";
  const path = task
    ? `/api/gateway/vision/provider_models?task=${task}`
    : voice
      ? "/api/gateway/audio/speech/models?compact=true"
      : "/api/gateway/audio/music/models?task=text_to_music";
  const providersPath = task
    ? `${path}&providers_only=true`
    : voice
      ? "/api/gateway/voice/voices?providers_only=true&compact=true"
      : "/api/gateway/audio/music/providers?task=text_to_music";
  const names = (items: any) =>
    Array.isArray(items)
      ? items
          .map((item: any) =>
            typeof item === "string" ? item : item.id || item.name,
          )
          .filter(Boolean)
      : [];
  return {
    fetchProviders: async () => {
      const data = await gatewayRequest(providersPath);
      if (data.error) throw new Error(data.error);
      return names(
        data.providers || data.available_providers || data.items,
      ).map((name: string) => ({ name }));
    },
    fetchModels: async (provider: string) => {
      const data = await gatewayRequest(
        `${path}&provider=${encodeURIComponent(provider)}`,
      );
      if (data.error) throw new Error(data.error);
      return names(
        data.models_by_provider?.[provider] ||
          data.tts_models_by_provider?.[provider] ||
          data.music_models_by_provider?.[provider] ||
          data.models ||
          data.items,
      );
    },
  };
}
