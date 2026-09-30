// Responsive layout state for the Code shell (responsive workstream, DESIGN.md §1, §5.2-5.3).
//
// Widths come from the kit's named breakpoints (AF_MEDIA): below `md` (1024 px) the
// conversation sidebar and the workspace inspector are overlay drawers; from `md`
// to `lg` (1024-1439 px) at most two panes are docked, so the inspector starts
// closed (a toggle); from `xl` (>= 1440 px) all three panes are docked.
import { useEffect, useRef } from "react";
import { AF_MEDIA } from "@abstractframework/ui-kit";

type MatchMedia = (query: string) => { matches: boolean };

function safeMatch(query: string, matchMedia?: MatchMedia): boolean {
  try {
    const mm = matchMedia || (typeof window !== "undefined" ? window.matchMedia?.bind(window) : undefined);
    return !!mm && mm(query).matches;
  } catch {
    return false;
  }
}

/** The workspace inspector starts docked-open only where three panes fit (xl, >= 1440 px). */
export function inspectorOpenByDefault(matchMedia?: MatchMedia): boolean {
  return safeMatch(AF_MEDIA.xl, matchMedia);
}

/** Side panes are overlay drawers (not docked columns) below the md breakpoint. */
export function panesAreDrawers(matchMedia?: MatchMedia): boolean {
  return safeMatch(AF_MEDIA.md, matchMedia);
}

/**
 * Focus management for an overlay drawer: on open, focus `focusInside()`; on close,
 * give focus back to the element that had it before opening (the opener) when focus
 * was inside the drawer. Only active while `overlay` is true (docked panes keep focus).
 */
export function useDrawerFocus(
  open: boolean,
  overlay: boolean,
  container: () => HTMLElement | null,
  focusInside: () => HTMLElement | null,
): void {
  const opener = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (!overlay) return;
    if (open) {
      opener.current = (document.activeElement as HTMLElement | null) || null;
      const target = focusInside();
      // After the slide-in starts (the element is visible and focusable).
      const id = window.setTimeout(() => target?.focus(), 0);
      return () => window.clearTimeout(id);
    }
    const root = container();
    const active = document.activeElement;
    if (opener.current && (!active || active === document.body || (root && root.contains(active)))) {
      opener.current.focus?.();
    }
    opener.current = null;
    return undefined;
  }, [open, overlay]); // eslint-disable-line react-hooks/exhaustive-deps
}
