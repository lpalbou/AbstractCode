// Responsive layout state for the Code shell (responsive workstream, DESIGN.md §1, §5.2-5.3).
//
// Widths come from the kit's named breakpoints (AF_MEDIA): below `md` (1024 px) the
// conversation sidebar and the workspace inspector are overlay drawers; from `md`
// to `lg` (1024-1439 px) at most two panes are docked, so the inspector starts
// closed (a toggle); from `xl` (>= 1440 px) all three panes are docked.
import { useEffect, useRef, useState } from "react";

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

// ---- Pane modes (DESIGN §5.3) ------------------------------------------------------------
/** docked = xl (>= 1440, three panes); overlay = 1024-1439 (sidebar docked, inspector is an
 * overlay drawer: at most two docked panes); drawers = < 1024 (both side panes are drawers). */
export type PaneMode = "docked" | "overlay" | "drawers";

export function paneModeFrom(belowLg: boolean, belowMd: boolean): PaneMode {
  return belowMd ? "drawers" : belowLg ? "overlay" : "docked";
}

/** What the inspector does: `open` now, and `wanted` = the user's docked choice at xl. */
export type InspectorState = { open: boolean; wanted: boolean };

export function initialInspector(mode: PaneMode): InspectorState {
  return { open: mode === "docked", wanted: true };
}

/**
 * A resize that changes the pane mode: leaving xl folds the inspector away (remembering
 * whether it was docked open); coming back to xl restores the remembered choice. An overlay
 * opened below xl never becomes the docked preference.
 */
export function inspectorOnModeChange(state: InspectorState, from: PaneMode, to: PaneMode): InspectorState {
  if (from === to) return state;
  if (to === "docked") return { open: state.wanted, wanted: state.wanted };
  return { open: false, wanted: from === "docked" ? state.open : state.wanted };
}

/** The user toggles the inspector: at xl that is also the remembered docked choice. */
export function inspectorOnToggle(state: InspectorState, mode: PaneMode, open: boolean): InspectorState {
  return mode === "docked" ? { open, wanted: open } : { open, wanted: state.wanted };
}

/** The navigation drawer only exists below md: leaving that mode closes it. */
export function sidebarOnModeChange(open: boolean, to: PaneMode): boolean {
  return to === "drawers" ? open : false;
}

/**
 * Which layer one Escape closes. A kit dialog / drawer above the panes (Run settings, workflow
 * inputs, Schedule a task, appearance, about, sign-in) owns the key: nothing underneath closes
 * with it. Otherwise the navigation drawer, then an overlay inspector.
 */
export function escapeTarget(input: {
  key: string;
  defaultPrevented: boolean;
  overlayOpen: boolean;
  sidebarOpen: boolean;
  inspectorOpen: boolean;
  inspectorIsOverlay: boolean;
}): "sidebar" | "inspector" | null {
  if (input.key !== "Escape" || input.defaultPrevented || input.overlayOpen) return null;
  if (input.sidebarOpen) return "sidebar";
  if (input.inspectorOpen && input.inspectorIsOverlay) return "inspector";
  return null;
}

/** A dialog the kit renders itself (about, sign-in, critical action) is open in the page. */
export function kitDialogOpen(doc: Document | undefined = typeof document !== "undefined" ? document : undefined): boolean {
  return !!doc?.querySelector('[role="dialog"], [role="alertdialog"]');
}

// ---- On-screen keyboard -------------------------------------------------------------------
/** `--keyboard-inset` (written by the kit's installViewportVars) says a keyboard covers the page. */
export function keyboardOpen(inset: string | null | undefined): boolean {
  const px = Number.parseFloat(String(inset ?? "").trim());
  return Number.isFinite(px) && px > 0;
}

/**
 * Mirror `--keyboard-inset > 0` into `<html data-keyboard="open">` so CSS can compact the
 * chrome while typing (CSS cannot compare a custom property). Watches the inline style the
 * kit updates on every visual-viewport change. Idempotent; returns a cleanup.
 */
let keyboardCleanup: (() => void) | null = null;
export function installKeyboardFlag(): () => void {
  if (keyboardCleanup) return keyboardCleanup;
  if (typeof document === "undefined" || typeof MutationObserver === "undefined") return () => undefined;
  const html = document.documentElement;
  const sync = () => {
    const open = keyboardOpen(getComputedStyle(html).getPropertyValue("--keyboard-inset"));
    if (open && html.dataset.keyboard !== "open") html.dataset.keyboard = "open";
    else if (!open && html.dataset.keyboard) delete html.dataset.keyboard;
  };
  const observer = new MutationObserver(sync);
  observer.observe(html, { attributes: true, attributeFilter: ["style"] });
  window.visualViewport?.addEventListener("resize", sync);
  sync();
  keyboardCleanup = () => {
    observer.disconnect();
    window.visualViewport?.removeEventListener("resize", sync);
    keyboardCleanup = null;
  };
  return keyboardCleanup;
}

// ---- Drawer top -------------------------------------------------------------------------
/**
 * Where the app's kit drawers (Run settings, workflow inputs, voice) start: under the top bar,
 * or under the one-row chrome of phone landscape (the top bar is `display: contents` there).
 * Measured, so header density, the safe-area inset and the short layout are all honoured;
 * AfDrawer takes it as its `topOffset` prop.
 */
export function chromeBottom(doc: Document): number | null {
  const topbar = doc.querySelector<HTMLElement>(".code-topbar");
  if (!topbar) return null;
  const merged = getComputedStyle(topbar).display === "contents";
  const row = merged ? doc.querySelector<HTMLElement>(".code-toolbar") : topbar;
  const bottom = row?.getBoundingClientRect().bottom;
  return typeof bottom === "number" && bottom > 0 ? Math.round(bottom) : null;
}

export function useChromeBottom(fallback: number): number {
  const [top, setTop] = useState(fallback);
  useEffect(() => {
    if (typeof window === "undefined") return;
    const sync = () => setTop(chromeBottom(document) ?? fallback);
    sync();
    const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(sync) : null;
    const topbar = document.querySelector(".code-topbar");
    const toolbar = document.querySelector(".code-toolbar");
    if (ro && topbar) ro.observe(topbar);
    if (ro && toolbar) ro.observe(toolbar);
    window.addEventListener("resize", sync);
    return () => {
      ro?.disconnect();
      window.removeEventListener("resize", sync);
    };
  }, [fallback]);
  return top;
}
