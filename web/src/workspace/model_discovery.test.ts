import { beforeEach, describe, expect, it, vi } from "vitest";
const request = vi.hoisted(() => vi.fn());
vi.mock("./transport", () => ({
  gatewayRequest: request,
  gateway: {
    discovery_providers: vi.fn(),
    discovery_provider_models: vi.fn(),
    discovery_model_capabilities: vi.fn(),
  },
}));
import { defaultTextRoute, discoveryForInput } from "./model_discovery";
describe("model discovery routing", () => {
  beforeEach(() => request.mockReset());
  it("uses only configured text defaults", () => {
    expect(
      defaultTextRoute({
        routes: [
          {
            key: "output.text",
            provider: "bad",
            model: "bad",
            source: "not_configured",
          },
          {
            key: "input.text",
            provider: "endpoint:local",
            model: "local",
            source: "config",
          },
        ],
      }),
    ).toEqual({ provider: "endpoint:local", model: "local" });
  });
  it("never offers music model ids as providers", async () => {
    request.mockResolvedValue({ items: [{ id: "music-engine" }] });
    expect(await discoveryForInput("music_provider").fetchProviders()).toEqual([
      { name: "music-engine" },
    ]);
    expect(request).toHaveBeenCalledWith(
      "/api/gateway/audio/music/providers?task=text_to_music",
    );
  });
  it("discovers image editing by task and chosen provider", async () => {
    request.mockResolvedValue({ models: ["edit-model"] });
    expect(
      await discoveryForInput("image_edit_provider").fetchModels(
        "image engine",
      ),
    ).toEqual(["edit-model"]);
    expect(request).toHaveBeenCalledWith(
      "/api/gateway/vision/provider_models?task=image_to_image&provider=image%20engine",
    );
  });
  it("does not swallow catalog errors", async () => {
    request.mockResolvedValue({ error: "Discovery unavailable" });
    await expect(
      discoveryForInput("video_provider").fetchProviders(),
    ).rejects.toThrow("Discovery unavailable");
  });
});
