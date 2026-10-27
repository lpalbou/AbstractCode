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
import { readFileSync } from "node:fs";
import { defaultTextRoute, discoveryForInput, modelDiscovery } from "./model_discovery";
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
  it("the text picker uses the kit transport and names the gateway default", async () => {
    request.mockResolvedValue({ routes: [{ key: "output.text", provider: "ollama", model: "qwen3", reasoning: "high", source: "config" }] });
    expect(await modelDiscovery.fetchGatewayDefault()).toMatchObject({ provider: "ollama", model: "qwen3", reasoning: "high" });
    expect(request).toHaveBeenCalledWith("api/gateway/config/capability-defaults");
    request.mockResolvedValue({ items: [{ name: "ollama" }] });
    await modelDiscovery.fetchProviders();
    expect(request).toHaveBeenLastCalledWith("api/gateway/discovery/providers?include_models=false");
    const settings = readFileSync(new URL("./settings_panel.tsx", import.meta.url), "utf8");
    expect(settings).toContain("fetchGatewayDefault: modelDiscovery.fetchGatewayDefault");
    expect(settings).not.toMatch(/discovery_providers|discovery_provider_models/);
  });
  it("never offers music model ids as providers", async () => {
    request.mockResolvedValue({ items: [{ id: "music-engine" }] });
    expect(await discoveryForInput("music_provider").fetchProviders()).toEqual([
      { name: "music-engine" },
    ]);
    expect(request).toHaveBeenCalledWith(
      "api/gateway/audio/music/providers?task=text_to_music",
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
      "api/gateway/vision/provider_models?task=image_to_image&provider=image%20engine",
    );
  });
  it("does not swallow catalog errors", async () => {
    request.mockResolvedValue({ error: "Discovery unavailable" });
    await expect(
      discoveryForInput("video_provider").fetchProviders(),
    ).rejects.toThrow("Discovery unavailable");
  });
});
