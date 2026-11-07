import { afterEach, describe, expect, it, vi } from "vitest";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { useConversationView } from "./conversation_view";

// Operator rule 2026-11-07: Code opens on its Conversation view every time; Avatar is a switch
// for the current visit only (it used to be remembered in localStorage and reopen on Avatar).
function Probe({ scope }: { scope: string }) {
  const view = useConversationView(scope);
  return <span data-mode={view.mode}>{view.preferences.completion.audio ? "cues" : "quiet"}</span>;
}

describe("Code conversation view", () => {
  afterEach(() => vi.unstubAllGlobals());
  it("opens on Conversation even when this browser last used Avatar", () => {
    const store = new Map<string, string>([
      ["abstract.conversation:abstractcode:me", JSON.stringify({ mode: "avatar", preferences: { completion: { audio: false } } })],
    ]);
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    });
    const html = renderToStaticMarkup(<Probe scope="abstractcode:me" />);
    expect(html).toContain('data-mode="chat"');
    // The saved cue preferences still apply: only the view is not remembered.
    expect(html).toContain("quiet");
  });
});
