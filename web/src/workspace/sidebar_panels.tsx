// The sidebar's two stacking drawers (round 4, DESIGN §3), Automations above Conversations, both
// full width, docked sidebar and phone/tablet drawer alike. Each header is a full-width 44 px row
// on the raised surface (ui-surface-2): the disclosure toggle (chevron + label) on the left, the
// section's "+" / refresh actions on the right as sibling buttons that never toggle. Both headers
// are always visible; the layout follows the two open states (CSS on `.code-sidebar-lists`):
//   - both closed: the two header rows at the top;
//   - Automations open: it takes the space above the Conversations header, which sits mid-height;
//   - Conversations open (Automations closed): it takes the rest below its header;
//   - both open: an even split, the Conversations header pinned mid-height.
// Each list scrolls inside its own drawer (never clipped under the other header); the bottom
// block stays pinned. The two open states persist per viewer (localStorage).
import React, { useCallback, useState } from "react";
import { Icon } from "@abstractframework/ui-kit";

export type SidebarPanel = "automations" | "conversations";
export type PanelsState = Record<SidebarPanel, boolean>;

export const SIDEBAR_PANELS_KEY = "abstractcode.sidebar.panels";
export const DEFAULT_PANELS: PanelsState = { automations: true, conversations: true };

type ReadStore = Pick<Storage, "getItem"> | null | undefined;
type WriteStore = Pick<Storage, "setItem"> | null | undefined;

/** The stored state, or the defaults when storage is missing, blocked or holds anything else. */
export function readPanels(storage: ReadStore): PanelsState {
  try {
    const raw = storage?.getItem(SIDEBAR_PANELS_KEY);
    if (!raw) return { ...DEFAULT_PANELS };
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return { ...DEFAULT_PANELS };
    const value = parsed as Record<string, unknown>;
    return {
      automations: typeof value.automations === "boolean" ? value.automations : DEFAULT_PANELS.automations,
      conversations: typeof value.conversations === "boolean" ? value.conversations : DEFAULT_PANELS.conversations,
    };
  } catch {
    return { ...DEFAULT_PANELS };
  }
}

/** Best effort: a blocked or full storage never breaks the sidebar. */
export function writePanels(state: PanelsState, storage: WriteStore): void {
  try {
    storage?.setItem(SIDEBAR_PANELS_KEY, JSON.stringify(state));
  } catch {
    // ignore: the state still applies for this page view
  }
}

export function togglePanel(state: PanelsState, panel: SidebarPanel): PanelsState {
  return { ...state, [panel]: !state[panel] };
}

function viewerStorage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

export function useSidebarPanels(): [PanelsState, (panel: SidebarPanel) => void] {
  const [panels, setPanels] = useState<PanelsState>(() => readPanels(viewerStorage()));
  const toggle = useCallback((panel: SidebarPanel) => {
    setPanels((state) => {
      const next = togglePanel(state, panel);
      writePanels(next, viewerStorage());
      return next;
    });
  }, []);
  return [panels, toggle];
}

export const panelIds = (panel: SidebarPanel) => ({ toggle: `code-panel-${panel}-toggle`, region: `code-panel-${panel}` });

/** The panel header: the disclosure toggle, then the section's own actions as siblings. */
export function PanelHeader(props: {
  panel: SidebarPanel;
  label: React.ReactNode;
  open: boolean;
  onToggle(): void;
  actions?: React.ReactNode;
}): React.ReactElement {
  const ids = panelIds(props.panel);
  return (
    <div className="code-panel-header">
      <button
        type="button"
        className="code-panel-toggle"
        id={ids.toggle}
        aria-expanded={props.open}
        aria-controls={ids.region}
        onClick={props.onToggle}
      >
        <Icon name="chevronDown" size={14} />
        <span>{props.label}</span>
      </button>
      {props.actions ? <span className="code-panel-actions">{props.actions}</span> : null}
    </div>
  );
}

/** The drawers' frame (between the search field and the bottom block); its two data attributes
 * drive the fold/split layout. */
export function SidebarLists(props: { panels: PanelsState; children: React.ReactNode }): React.ReactElement {
  return (
    <div
      className="code-sidebar-lists"
      data-automations={props.panels.automations ? "open" : "closed"}
      data-conversations={props.panels.conversations ? "open" : "closed"}
    >
      {props.children}
    </div>
  );
}

/** The Conversations panel of the sidebar (the Automations one lives in automations_view.tsx). */
export function ConversationsPanel(props: {
  open: boolean;
  onToggle(): void;
  actions?: React.ReactNode;
  children: React.ReactNode;
}): React.ReactElement {
  const ids = panelIds("conversations");
  return (
    <section className="code-panel code-conversations" data-open={props.open ? "true" : "false"}>
      <PanelHeader panel="conversations" label="Conversations" open={props.open} onToggle={props.onToggle} actions={props.actions} />
      <nav className="code-sessions" id={ids.region} aria-labelledby={ids.toggle} hidden={!props.open}>
        {props.children}
      </nav>
    </section>
  );
}

/** The workspace folder's last segment, or "Gateway managed" when the run names none. */
export function workspaceLabel(path: string): string {
  return path.split("/").filter(Boolean).pop() || "Gateway managed";
}

/**
 * The sidebar's bottom Workspace row: icon | label + value | chevron on one grid row. The value
 * (often a per-conversation folder named by a session id) stays on one line with an ellipsis and
 * never runs under the chevron; the full path is the row's tooltip.
 */
export function WorkspaceRow(props: { path: string; onOpen(): void }): React.ReactElement {
  const value = workspaceLabel(props.path);
  return (
    <button
      type="button"
      className="code-workspace-row"
      title={props.path ? `Workspace: ${props.path}` : "Workspace: gateway managed"}
      onClick={props.onOpen}
    >
      <Icon name="terminal" size={17} />
      <span>
        <strong>Workspace</strong>
        <small>{value}</small>
      </span>
      <Icon name="chevronRight" size={14} />
    </button>
  );
}
