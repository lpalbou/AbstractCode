// Collapsible lists inside the automation detail (DESIGN §12): the occurrences timeline and the
// folder browser. Each has a disclosure header (chevron, aria-expanded, 44 px on touch), open by
// default; the state persists per viewer (localStorage, best effort) like the sidebar panels.
//
// The occurrences timeline is rendered by the kit's AutomationPanel, which has no header slot of
// its own: `placeTimelineSlot` puts a host element right before the kit's timeline (or its "Load
// earlier occurrences" button) and the header is portalled into it. The kit's own nodes are never
// moved or removed; the host is re-placed whenever the panel re-renders its list.
import React, { useCallback, useLayoutEffect, useMemo, useState } from "react";
import { Icon } from "@abstractframework/ui-kit";

export type DetailPanel = "occurrences" | "folder";
export type DetailPanelsState = Record<DetailPanel, boolean>;

export const DETAIL_PANELS_KEY = "abstractcode.automation.panels";
export const DEFAULT_DETAIL_PANELS: DetailPanelsState = { occurrences: true, folder: true };

type ReadStore = Pick<Storage, "getItem"> | null | undefined;
type WriteStore = Pick<Storage, "setItem"> | null | undefined;

/** The stored state, or the defaults when storage is missing, blocked or holds anything else. */
export function readDetailPanels(storage: ReadStore): DetailPanelsState {
  try {
    const raw = storage?.getItem(DETAIL_PANELS_KEY);
    if (!raw) return { ...DEFAULT_DETAIL_PANELS };
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return { ...DEFAULT_DETAIL_PANELS };
    const value = parsed as Record<string, unknown>;
    return {
      occurrences: typeof value.occurrences === "boolean" ? value.occurrences : DEFAULT_DETAIL_PANELS.occurrences,
      folder: typeof value.folder === "boolean" ? value.folder : DEFAULT_DETAIL_PANELS.folder,
    };
  } catch {
    return { ...DEFAULT_DETAIL_PANELS };
  }
}

/** Best effort: a blocked or full storage never breaks the detail. */
export function writeDetailPanels(state: DetailPanelsState, storage: WriteStore): void {
  try {
    storage?.setItem(DETAIL_PANELS_KEY, JSON.stringify(state));
  } catch {
    // ignore: the state still applies for this page view
  }
}

export function toggleDetailPanel(state: DetailPanelsState, panel: DetailPanel): DetailPanelsState {
  return { ...state, [panel]: !state[panel] };
}

function viewerStorage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

export function useDetailPanels(): [DetailPanelsState, (panel: DetailPanel) => void] {
  const [panels, setPanels] = useState<DetailPanelsState>(() => readDetailPanels(viewerStorage()));
  const toggle = useCallback((panel: DetailPanel) => {
    setPanels((state) => {
      const next = toggleDetailPanel(state, panel);
      writeDetailPanels(next, viewerStorage());
      return next;
    });
  }, []);
  return [panels, toggle];
}

export const detailPanelIds = (panel: DetailPanel) => ({
  toggle: `code-detail-${panel}-toggle`,
  region: `code-detail-${panel}`,
});

/** The disclosure header of one detail list. */
export function DetailDisclosure(props: {
  panel: DetailPanel;
  label: React.ReactNode;
  open: boolean;
  onToggle(): void;
  count?: number;
}): React.ReactElement {
  const ids = detailPanelIds(props.panel);
  return (
    <button
      type="button"
      className="code-detail-toggle"
      id={ids.toggle}
      data-panel={props.panel}
      aria-expanded={props.open}
      aria-controls={ids.region}
      onClick={props.onToggle}
    >
      <Icon name="chevronDown" size={13} />
      <span className="code-detail-toggle__label">{props.label}</span>
      {typeof props.count === "number" ? <span className="code-detail-toggle__count">{props.count}</span> : null}
    </button>
  );
}

/** The kit's occurrence list inside an AutomationPanel root, and where its header goes. */
export const TIMELINE_SELECTOR = ".af-auto__timeline";
export const LOAD_MORE_SELECTOR = ".af-auto__more";

type SlotNode = {
  parentNode: SlotParent | null;
  nextSibling: unknown;
  id?: string;
  remove(): void;
};
type SlotParent = { insertBefore(node: unknown, before: unknown): unknown };
type SlotRoot = { querySelector(selector: string): (SlotNode & { previousElementSibling?: unknown }) | null };

/**
 * Put `host` right before the kit's "Load earlier occurrences" button, or before the timeline
 * when there is none; take it out when the panel shows no timeline. Returns whether the host
 * is placed. Idempotent (no DOM write when it already sits in the right place), so a
 * MutationObserver may call it on every change without looping.
 */
export function placeTimelineSlot(root: SlotRoot | null, host: SlotNode, regionId: string): boolean {
  const timeline = root?.querySelector(TIMELINE_SELECTOR) ?? null;
  if (!timeline || !timeline.parentNode) {
    if (host.parentNode) host.remove();
    return false;
  }
  if (timeline.id !== regionId) timeline.id = regionId;
  const more = root?.querySelector(LOAD_MORE_SELECTOR) ?? null;
  const anchor = more && more.parentNode === timeline.parentNode ? more : timeline;
  if (host.parentNode !== anchor.parentNode || host.nextSibling !== anchor) {
    anchor.parentNode!.insertBefore(host, anchor);
  }
  return true;
}

/** A host element kept right before the kit's timeline inside `root` (null while there is none). */
export function useTimelineSlot(root: React.RefObject<HTMLElement | null>, active: boolean): HTMLElement | null {
  const host = useMemo(() => {
    if (typeof document === "undefined") return null;
    const el = document.createElement("div");
    el.className = "code-detail-head code-detail-head--occurrences";
    return el;
  }, []);
  const [placed, setPlaced] = useState(false);
  useLayoutEffect(() => {
    const el = root.current;
    if (!host || !el || !active) {
      host?.remove();
      setPlaced(false);
      return;
    }
    const regionId = detailPanelIds("occurrences").region;
    const place = () => setPlaced(placeTimelineSlot(el, host, regionId));
    place();
    const observer = new MutationObserver(place);
    observer.observe(el, { childList: true, subtree: true });
    return () => {
      observer.disconnect();
      host.remove();
    };
  }, [root, host, active]);
  return placed ? host : null;
}
