/**
 * Composer upload queue: the pure rules behind the attachment chips.
 *
 * A file dropped, pasted or picked becomes a chip at once. It then waits
 * ("queued"), uploads ("uploading"), or is refused before any request
 * ("refused": over the gateway's `maxAttachmentBytes`, the ONLY size rule —
 * ADR-0026) or fails at the gateway ("failed", retryable). A chip that
 * uploads becomes an ordinary AttachmentRef and rides the next run exactly
 * as picked files always have.
 */

export type UploadStatus = "queued" | "uploading" | "failed" | "refused";

export type PendingUpload = {
  id: string;
  file: File;
  name: string;
  size: number;
  status: UploadStatus;
  message?: string;
};

/** Uploads in flight at once: parallel enough to be quick, few enough to read. */
export const UPLOAD_CONCURRENCY = 3;

const UNITS = ["KB", "MB", "GB", "TB"];

/** 1024-based, one decimal below 10 ("4.2 MB", "25 MB", "812 B"). */
export function formatBytes(bytes: number): string {
  const value = Math.max(0, Number(bytes) || 0);
  if (value < 1024) return `${value} B`;
  let scaled = value / 1024;
  let unit = 0;
  while (scaled >= 1024 && unit < UNITS.length - 1) {
    scaled /= 1024;
    unit += 1;
  }
  const rounded = scaled < 10 ? Math.round(scaled * 10) / 10 : Math.round(scaled);
  return `${rounded} ${UNITS[unit]}`;
}

function exactBytes(bytes: number): string {
  return `${Math.round(bytes).toLocaleString("en-US")} bytes`;
}

/**
 * The refusal for a file over the gateway's limit, or null when it may be
 * uploaded (no limit published = the gateway decides). The message always
 * carries both numbers; when rounding would print the same figure twice
 * ("25 MB is over 25 MB") it falls back to exact byte counts.
 */
export function uploadRefusal(
  file: { name: string; size: number },
  maxBytes: number | undefined,
): string | null {
  if (!maxBytes || !(maxBytes > 0) || file.size <= maxBytes) return null;
  let size = formatBytes(file.size);
  let limit = formatBytes(maxBytes);
  if (size === limit) {
    size = exactBytes(file.size);
    limit = exactBytes(maxBytes);
  }
  return `${file.name || "This file"} is ${size}; the gateway accepts files up to ${limit}.`;
}

/**
 * The gateway's reason for a failed upload, as the chip shows it. The client
 * prefixes its own call name ("attachments_upload failed: …"); the user needs
 * the gateway's words, not ours.
 */
export function uploadFailure(message: string): string {
  const reason = String(message || "")
    .replace(/^attachments_upload failed:?\s*/i, "")
    .trim();
  return `Upload failed: ${reason || "the gateway did not say why"}`;
}

/** Chip state for new files: refused ones never reach the network. */
export function queueUploads(
  files: File[],
  maxBytes: number | undefined,
  newId: () => string,
): PendingUpload[] {
  return files.map((file) => {
    const refusal = uploadRefusal(file, maxBytes);
    return {
      id: newId(),
      file,
      name: file.name || "Untitled file",
      size: file.size,
      status: refusal ? "refused" : "queued",
      ...(refusal ? { message: refusal } : {}),
    };
  });
}

/** Runs `worker` over `items` with at most `limit` in flight, in order. */
export async function runBounded<T>(
  items: readonly T[],
  limit: number,
  worker: (item: T) => Promise<void>,
): Promise<void> {
  let next = 0;
  const lanes = Array.from(
    { length: Math.max(1, Math.min(Math.floor(limit) || 1, items.length)) },
    async () => {
      while (next < items.length) {
        const item = items[next];
        next += 1;
        await worker(item);
      }
    },
  );
  await Promise.all(lanes);
}

function files(count: number): string {
  return `${count} ${count === 1 ? "file" : "files"}`;
}

/** The screen-reader summary after a batch settles ("2 files attached"). */
export function uploadAnnouncement(attached: number, notAttached: number): string {
  const parts: string[] = [];
  if (attached) parts.push(`${files(attached)} attached`);
  if (notAttached)
    parts.push(`${files(notAttached)} could not be attached`);
  return parts.join(", ");
}

/**
 * Why a new turn cannot start yet, or null. A chip the user can still see
 * must never be left behind silently by a send: uploads in flight are
 * awaited, and a refused/failed file must be retried or removed first.
 */
export function unattachedSendBlock(uploads: readonly PendingUpload[]): string | null {
  const waiting = uploads.filter(
    (item) => item.status === "queued" || item.status === "uploading",
  ).length;
  if (waiting)
    return `Wait for ${files(waiting)} to finish uploading.`;
  const broken = uploads.filter(
    (item) => item.status === "failed" || item.status === "refused",
  );
  if (!broken.length) return null;
  const retryable = broken.some((item) => item.status === "failed");
  if (broken.length === 1)
    return `"${broken[0].name}" was not attached. ${retryable ? "Retry or remove" : "Remove"} it before sending.`;
  return `${files(broken.length)} were not attached. ${retryable ? "Retry or remove" : "Remove"} them before sending.`;
}
