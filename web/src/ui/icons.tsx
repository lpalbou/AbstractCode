import React from "react";

export type IconName =
  | "chat"
  | "plus"
  | "history"
  | "settings"
  | "user"
  | "bot"
  | "paperclip"
  | "mic"
  | "speaker"
  | "download"
  | "loader"
  | "info"
  | "warning"
  | "error"
  | "copy"
  | "check"
  | "x"
  | "chevronDown"
  | "chevronRight"
  | "trash"
  | "send";

function paths(name: IconName): React.ReactNode {
  switch (name) {
    case "chat":
      return <path d="M21 15a2 2 0 0 1-2 2H8l-5 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />;
    case "plus":
      return (
        <>
          <path d="M12 5v14" />
          <path d="M5 12h14" />
        </>
      );
    case "history":
      return (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M12 7v5l3 3" />
        </>
      );
    case "settings":
      return (
        <>
          <path d="M4 6h16" />
          <circle cx="8" cy="6" r="2" />
          <path d="M4 12h16" />
          <circle cx="16" cy="12" r="2" />
          <path d="M4 18h16" />
          <circle cx="10" cy="18" r="2" />
        </>
      );
    case "user":
      return (
        <>
          <circle cx="12" cy="8" r="4" />
          <path d="M4 21a8 8 0 0 1 16 0" />
        </>
      );
    case "bot":
      return (
        <>
          <path d="M12 2v3" />
          <rect x="5" y="6" width="14" height="14" rx="3" />
          <circle cx="9" cy="13" r="1" fill="currentColor" stroke="none" />
          <circle cx="15" cy="13" r="1" fill="currentColor" stroke="none" />
          <path d="M9 17h6" />
        </>
      );
    case "paperclip":
      return (
        <path d="M21.44 11.05l-9.19 9.19a5.5 5.5 0 0 1-7.78-7.78l9.19-9.19a3.5 3.5 0 0 1 4.95 4.95l-9.2 9.19a1.5 1.5 0 0 1-2.12-2.12l8.49-8.48" />
      );
    case "mic":
      return (
        <>
          <path d="M12 14a3 3 0 0 0 3-3V5a3 3 0 0 0-6 0v6a3 3 0 0 0 3 3z" />
          <path d="M19 11a7 7 0 0 1-14 0" />
          <path d="M12 18v4" />
          <path d="M8 22h8" />
        </>
      );
    case "speaker":
      return (
        <>
          <path d="M11 5L6 9H2v6h4l5 4V5z" />
          <path d="M15.5 8.5a5 5 0 0 1 0 7" />
          <path d="M18 6a9 9 0 0 1 0 12" />
        </>
      );
    case "download":
      return (
        <>
          <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
          <path d="M7 10l5 5 5-5" />
          <path d="M12 15V3" />
        </>
      );
    case "loader":
      return <path d="M21 12a9 9 0 1 1-3.3-6.9" />;
    case "info":
      return (
        <>
          <circle cx="12" cy="12" r="10" />
          <path d="M12 16v-4" />
          <circle cx="12" cy="8" r="0.75" fill="currentColor" stroke="none" />
        </>
      );
    case "warning":
      return (
        <>
          <path d="M12 3l10 18H2L12 3z" />
          <path d="M12 9v4" />
          <circle cx="12" cy="17" r="0.75" fill="currentColor" stroke="none" />
        </>
      );
    case "error":
      return (
        <>
          <circle cx="12" cy="12" r="10" />
          <path d="M15 9l-6 6" />
          <path d="M9 9l6 6" />
        </>
      );
    case "copy":
      return (
        <>
          <rect x="9" y="9" width="13" height="13" rx="2" />
          <path d="M5 15V5a2 2 0 0 1 2-2h10" />
        </>
      );
    case "check":
      return <path d="M20 6L9 17l-5-5" />;
    case "x":
      return (
        <>
          <path d="M18 6L6 18" />
          <path d="M6 6l12 12" />
        </>
      );
    case "chevronDown":
      return <path d="M6 9l6 6 6-6" />;
    case "chevronRight":
      return <path d="M9 18l6-6-6-6" />;
    case "trash":
      return (
        <>
          <path d="M3 6h18" />
          <path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
          <path d="M6 6l1 16h10l1-16" />
          <path d="M10 11v6" />
          <path d="M14 11v6" />
        </>
      );
    case "send":
      return (
        <>
          <path d="M22 2L11 13" />
          <path d="M22 2L15 22l-4-9-9-4 20-7z" />
        </>
      );
    default:
      return null;
  }
}

export function Icon({
  name,
  size = 16,
  className,
  title,
  ...props
}: {
  name: IconName;
  size?: number;
  title?: string;
} & Omit<React.SVGProps<SVGSVGElement>, "children">): React.ReactElement {
  const aria_hidden = title ? undefined : true;

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden={aria_hidden}
      role={title ? "img" : "presentation"}
      {...props}
    >
      {title ? <title>{title}</title> : null}
      {paths(name)}
    </svg>
  );
}
