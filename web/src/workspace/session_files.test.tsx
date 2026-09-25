import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import {
  FilePreview,
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
    expect(html).toContain("2.0 KB");
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
      { status: "ready", kind: "text", text: "x", partial: true },
      { ...entry, size_bytes: 3 * 1024 * 1024 },
    );
    expect(html).toContain("Attach to conversation");
    expect(html).toContain(`Showing the first ${formatBytes(512 * 1024)} of 3.0 MB`);
  });

  it("surfaces a gateway error instead of an empty preview", () => {
    expect(render({ status: "error", message: "Preview failed (404): no route" })).toContain(
      "Preview failed (404): no route",
    );
  });
});
