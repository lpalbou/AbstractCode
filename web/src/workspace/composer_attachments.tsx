import React from "react";
import { Icon } from "@abstractframework/ui-kit";
import type { AttachmentRef } from "../lib/types";
import { formatBytes, type PendingUpload } from "./attachment_uploads";

/**
 * The chips inside the composer: attached files first, then files still in
 * flight (Waiting / Uploading) or not attached (refused over the gateway
 * limit, or failed with the gateway's reason and a Retry). Every chip is
 * focusable and names its state for screen readers.
 */
export function ComposerAttachments({
  attachments,
  uploads,
  sizes,
  onRemoveAttachment,
  onRemoveUpload,
  onRetryUpload,
}: {
  attachments: AttachmentRef[];
  uploads: PendingUpload[];
  /** Sizes known locally for uploaded refs, keyed by `$artifact`. */
  sizes?: ReadonlyMap<string, number>;
  onRemoveAttachment: (index: number) => void;
  onRemoveUpload: (id: string) => void;
  onRetryUpload: (id: string) => void;
}) {
  return (
    <div className="code-attachments" role="list" aria-label="Attached files">
      {attachments.map((item, index) => {
        const name = item.filename || item.path || "Attachment";
        const bytes =
          typeof item.size_bytes === "number"
            ? item.size_bytes
            : sizes?.get(item.$artifact);
        const size = typeof bytes === "number" ? formatBytes(bytes) : "";
        return (
          <div
            key={`${item.$artifact}:${index}`}
            role="listitem"
            tabIndex={0}
            className="code-attachment is-attached"
            aria-label={`${name}${size ? `, ${size}` : ""}, attached`}
          >
            <span className="code-attachment__icon" aria-hidden="true">
              <Icon name="paperclip" size={13} />
            </span>
            <span className="code-attachment__body">
              <span className="code-attachment__name" title={name}>
                {name}
              </span>
              {size ? <span className="code-attachment__meta">{size}</span> : null}
            </span>
            <button
              type="button"
              className="code-attachment__action"
              aria-label={`Remove ${name}`}
              title={`Remove ${name}`}
              onClick={() => onRemoveAttachment(index)}
            >
              <Icon name="x" size={12} />
            </button>
          </div>
        );
      })}
      {uploads.map((item) => {
        const pending = item.status === "queued" || item.status === "uploading";
        const meta =
          item.status === "queued"
            ? `Waiting · ${formatBytes(item.size)}`
            : item.status === "uploading"
              ? `Uploading · ${formatBytes(item.size)}`
              : item.message || "Not attached";
        const state =
          item.status === "queued"
            ? "waiting to upload"
            : item.status === "uploading"
              ? "uploading"
              : `not attached: ${meta}`;
        return (
          <div
            key={item.id}
            role="listitem"
            tabIndex={0}
            className={`code-attachment is-${item.status}`}
            aria-label={`${item.name}, ${state}`}
            aria-busy={pending || undefined}
          >
            <span className="code-attachment__icon" aria-hidden="true">
              {pending ? (
                <span className="code-attachment__spinner" />
              ) : (
                <Icon name="warning" size={13} />
              )}
            </span>
            <span className="code-attachment__body">
              <span className="code-attachment__name" title={item.name}>
                {item.name}
              </span>
              <span className="code-attachment__meta">{meta}</span>
            </span>
            {item.status === "failed" ? (
              <button
                type="button"
                className="code-attachment__action code-attachment__retry"
                aria-label={`Retry ${item.name}`}
                title="Retry upload"
                onClick={() => onRetryUpload(item.id)}
              >
                <Icon name="refresh" size={12} />
                <span>Retry</span>
              </button>
            ) : null}
            <button
              type="button"
              className="code-attachment__action"
              aria-label={`Remove ${item.name}`}
              title={`Remove ${item.name}`}
              onClick={() => onRemoveUpload(item.id)}
            >
              <Icon name="x" size={12} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
