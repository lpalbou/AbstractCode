import { createGatewayModelTransport, gatewayApiPath, gatewayTextDefault } from "@abstractframework/ui-kit";
import { gatewayRequest } from "./transport";

/**
 * The text model picker's transport is the kit's (backlog 0034): discovery/providers,
 * discovery/providers/{p}/models?capability_route=output.text, discovery/models/capabilities and
 * config/capability-defaults ("Gateway default · provider / model · reasoning · MTP").
 */
export const modelDiscovery = createGatewayModelTransport((path) => gatewayRequest(path));

/** The text route the gateway defaults to (`GET config/capability-defaults`, read by the kit), or undefined. */
export function defaultTextRoute(data: unknown): { provider: string; model: string } | undefined {
  try {
    const value = gatewayTextDefault(data);
    return value ? { provider: value.provider, model: value.model } : undefined;
  } catch {
    return undefined;
  }
}

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
    ? gatewayApiPath(`vision/provider_models?task=${task}`)
    : voice
      ? gatewayApiPath("audio/speech/models?compact=true")
      : gatewayApiPath("audio/music/models?task=text_to_music");
  const providersPath = task
    ? `${path}&providers_only=true`
    : voice
      ? gatewayApiPath("voice/voices?providers_only=true&compact=true")
      : gatewayApiPath("audio/music/providers?task=text_to_music");
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
