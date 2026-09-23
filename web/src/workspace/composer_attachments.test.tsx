import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ComposerAttachments } from "./composer_attachments";
import type { PendingUpload } from "./attachment_uploads";

const noop = () => {};
const upload = (
  name: string,
  status: PendingUpload["status"],
  message?: string,
): PendingUpload => ({
  id: `id-${name}`,
  name,
  size: 2048,
  status,
  file: new File(["x"], name),
  ...(message ? { message } : {}),
});

function render(uploads: PendingUpload[], sizes?: Map<string, number>) {
  return renderToStaticMarkup(
    <ComposerAttachments
      attachments={[{ $artifact: "art-1", filename: "notes.md" }]}
      uploads={uploads}
      sizes={sizes}
      onRemoveAttachment={noop}
      onRemoveUpload={noop}
      onRetryUpload={noop}
    />,
  );
}

describe("composer attachment chips", () => {
  it("labels every chip with its state and keeps chips focusable", () => {
    const html = render(
      [
        upload("a.txt", "queued"),
        upload("b.txt", "uploading"),
        upload("big.bin", "refused", "big.bin is 2 MB; the gateway accepts files up to 1 MB."),
        upload("c.txt", "failed", "Upload failed: store unavailable"),
      ],
      new Map([["art-1", 812]]),
    );
    expect(html).toContain('role="list" aria-label="Attached files"');
    expect(html).toContain('aria-label="notes.md, 812 B, attached"');
    expect(html).toContain('aria-label="a.txt, waiting to upload"');
    expect(html).toContain("Waiting · 2 KB");
    expect(html).toContain('aria-label="b.txt, uploading"');
    expect(html).toContain("Uploading · 2 KB");
    expect(html).toContain(
      'aria-label="big.bin, not attached: big.bin is 2 MB; the gateway accepts files up to 1 MB."',
    );
    expect(html).toContain('aria-label="c.txt, not attached: Upload failed: store unavailable"');
    expect(html.match(/tabindex="0"/g)?.length).toBe(5);
    for (const name of ["notes.md", "a.txt", "b.txt", "big.bin", "c.txt"])
      expect(html).toContain(`aria-label="Remove ${name}"`);
  });

  it("offers Retry only for an upload that failed, never for a refusal", () => {
    const html = render([
      upload("big.bin", "refused", "too big"),
      upload("c.txt", "failed", "Upload failed: x"),
    ]);
    expect(html).toContain('aria-label="Retry c.txt"');
    expect(html).not.toContain('aria-label="Retry big.bin"');
    expect(html.match(/code-attachment__spinner/g)).toBeNull();
  });

  it("shows no size it does not know", () => {
    expect(render([])).toContain('aria-label="notes.md, attached"');
  });
});
