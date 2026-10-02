import React from "react";

/** One loading treatment for the sidebar lists and selected automation. */
export function LoadingStatus({ children }: { children: React.ReactNode }): React.ReactElement {
  return <p className="code-history-empty code-loading-status" role="status">
    <span className="code-loading-spinner" aria-hidden="true" />{children}
  </p>;
}
