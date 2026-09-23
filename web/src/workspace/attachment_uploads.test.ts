import { describe, expect, it } from "vitest";
import {
  UPLOAD_CONCURRENCY,
  formatBytes,
  queueUploads,
  runBounded,
  unattachedSendBlock,
  uploadAnnouncement,
  uploadFailure,
  uploadRefusal,
  type PendingUpload,
} from "./attachment_uploads";

const MB = 1024 * 1024;
const file = (name: string, size: number) =>
  new File([new Uint8Array(size)], name, { type: "application/octet-stream" });

describe("composer upload rules", () => {
  it("formats sizes readably", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(812)).toBe("812 B");
    expect(formatBytes(4.24 * 1024)).toBe("4.2 KB");
    expect(formatBytes(25 * MB)).toBe("25 MB");
    expect(formatBytes(12.4 * MB)).toBe("12 MB");
    expect(formatBytes(3 * 1024 * MB)).toBe("3 GB");
  });

  it("refuses over the gateway limit with both numbers, and only then", () => {
    expect(uploadRefusal({ name: "big.bin", size: 30 * MB }, 25 * MB)).toBe(
      "big.bin is 30 MB; the gateway accepts files up to 25 MB.",
    );
    expect(uploadRefusal({ name: "ok.bin", size: 25 * MB }, 25 * MB)).toBeNull();
    expect(uploadRefusal({ name: "any.bin", size: 900 * MB }, undefined)).toBeNull();
    expect(uploadRefusal({ name: "any.bin", size: 10 }, 0)).toBeNull();
  });

  it("never prints the same figure twice: rounding ties fall back to bytes", () => {
    expect(uploadRefusal({ name: "edge.bin", size: 25 * MB + 1 }, 25 * MB)).toBe(
      "edge.bin is 26,214,401 bytes; the gateway accepts files up to 26,214,400 bytes.",
    );
  });

  it("queues new files, refusing oversize ones before any request", () => {
    let n = 0;
    const items = queueUploads(
      [file("a.txt", 10), file("huge.iso", 2048)],
      1024,
      () => `id-${++n}`,
    );
    expect(items.map((i) => [i.id, i.name, i.status])).toEqual([
      ["id-1", "a.txt", "queued"],
      ["id-2", "huge.iso", "refused"],
    ]);
    expect(items[1].message).toBe(
      "huge.iso is 2 KB; the gateway accepts files up to 1 KB.",
    );
    expect(items[0].message).toBeUndefined();
  });

  it("uploads with bounded concurrency, every item exactly once", async () => {
    let inFlight = 0;
    let peak = 0;
    const seen: number[] = [];
    await runBounded([1, 2, 3, 4, 5, 6, 7], UPLOAD_CONCURRENCY, async (item) => {
      inFlight += 1;
      peak = Math.max(peak, inFlight);
      await new Promise((resolve) => setTimeout(resolve, item % 3));
      seen.push(item);
      inFlight -= 1;
    });
    expect(peak).toBe(UPLOAD_CONCURRENCY);
    expect([...seen].sort()).toEqual([1, 2, 3, 4, 5, 6, 7]);
    await runBounded([], 3, async () => {
      throw new Error("never called");
    });
  });

  it("shows the gateway's reason for a failed upload, not the client's call name", () => {
    expect(
      uploadFailure("attachments_upload failed: Attachment store is temporarily unavailable"),
    ).toBe("Upload failed: Attachment store is temporarily unavailable");
    expect(uploadFailure("Failed to fetch")).toBe("Upload failed: Failed to fetch");
    expect(uploadFailure("")).toBe("Upload failed: the gateway did not say why");
  });

  it("announces a settled batch", () => {
    expect(uploadAnnouncement(2, 0)).toBe("2 files attached");
    expect(uploadAnnouncement(1, 0)).toBe("1 file attached");
    expect(uploadAnnouncement(1, 1)).toBe(
      "1 file attached, 1 file could not be attached",
    );
    expect(uploadAnnouncement(0, 0)).toBe("");
  });

  it("blocks a new turn while a visible chip would be left behind", () => {
    const chip = (name: string, status: PendingUpload["status"]): PendingUpload => ({
      id: name,
      name,
      size: 1,
      status,
      file: file(name, 1),
    });
    expect(unattachedSendBlock([])).toBeNull();
    expect(
      unattachedSendBlock([chip("a", "uploading"), chip("b", "queued")]),
    ).toBe("Wait for 2 files to finish uploading.");
    expect(unattachedSendBlock([chip("a", "failed")])).toBe(
      '"a" was not attached. Retry or remove it before sending.',
    );
    expect(unattachedSendBlock([chip("a", "refused")])).toBe(
      '"a" was not attached. Remove it before sending.',
    );
    expect(
      unattachedSendBlock([chip("a", "refused"), chip("b", "failed")]),
    ).toBe("2 files were not attached. Retry or remove them before sending.");
  });
});
