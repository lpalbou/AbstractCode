// The right panel (round 5): the kit's vertical rail drawer, one icon per
// subject — Activity, Files, Model, Workflow, Workspace, Tools, Skills, Voice.
// The six settings panels are bound to what is selected in the sidebar (a
// conversation's run settings, or an automation's definition). Docked beside
// the conversation from 1024 px (resizable, width remembered), floating over
// it below; the rail itself always stays. Which panel is open is remembered
// for the docked layout only (a phone always starts with the content visible).
import React, { useCallback, useEffect, useState } from "react";
import { AF_MEDIA, AfRailDrawer, useAfMedia, type IconName } from "@abstractframework/ui-kit";

export type RailPanel = "activity" | "files" | "model" | "workflow" | "workspace" | "tools" | "skills" | "voice";
/** The panels that edit the selection's settings (bound to a conversation or an automation). */
export type SettingsRailPanel = Exclude<RailPanel, "activity" | "files">;

/** Order, label (tooltip, accessible name, panel title) and icon of every rail item. */
export const RAIL_ITEMS: ReadonlyArray<{ id: RailPanel; label: string; icon: IconName }> = [
  { id: "activity", label: "Activity", icon: "activity" },
  { id: "files", label: "Files", icon: "file" },
  { id: "model", label: "Model", icon: "sparkle" },
  { id: "workflow", label: "Workflow", icon: "agent" },
  { id: "workspace", label: "Workspace", icon: "folder" },
  { id: "tools", label: "Tools", icon: "terminal" },
  { id: "skills", label: "Skills", icon: "list" },
  { id: "voice", label: "Voice", icon: "mic" },
];
const PANELS: readonly RailPanel[] = RAIL_ITEMS.map((item) => item.id);

export const RAIL_PANEL_KEY = "abstractcode.rail.panel.v2";
/** Round 4 stored "activity" | "files" | "settings"; "settings" became the Model panel. */
export const RAIL_PANEL_KEY_V1 = "abstractcode.rail.panel.v1";
export const RAIL_WIDTH_KEY = "abstractcode.rail.width.v1";

/** The remembered panel: v2 first, then a round-4 value, else none. */
export function readRailPanel(storage: Pick<Storage, "getItem"> | null | undefined): RailPanel | null {
  try {
    const raw = storage?.getItem(RAIL_PANEL_KEY);
    if (raw !== null && raw !== undefined) return PANELS.includes(raw as RailPanel) ? (raw as RailPanel) : null;
    const old = storage?.getItem(RAIL_PANEL_KEY_V1);
    if (old === "settings") return "model";
    return PANELS.includes(old as RailPanel) ? (old as RailPanel) : null;
  } catch {
    return null;
  }
}

function viewerStorage(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

/**
 * The open panel. Docked: restored from the last visit (default collapsed);
 * overlay: always starts collapsed and is never remembered.
 */
export function useRailPanel(): [RailPanel | null, (next: RailPanel | null) => void, boolean] {
  const overlay = useAfMedia(AF_MEDIA.md);
  const [panel, setPanelState] = useState<RailPanel | null>(() =>
    typeof window !== "undefined" && !window.matchMedia?.(AF_MEDIA.md).matches ? readRailPanel(viewerStorage()) : null,
  );
  useEffect(() => {
    if (overlay) setPanelState(null);
  }, [overlay]);
  const setPanel = useCallback(
    (next: RailPanel | null) => {
      setPanelState(next);
      if (overlay) return;
      try {
        window.localStorage.setItem(RAIL_PANEL_KEY, next ?? "");
      } catch {
        /* the in-memory choice still applies */
      }
    },
    [overlay],
  );
  return [panel, setPanel, overlay];
}

export function CodeRightRail(props: {
  panel: RailPanel | null;
  onPanel: (next: RailPanel | null) => void;
  /** Rows waiting or running in Activity (the rail's badge). */
  activityBadge?: number;
  activityHint?: string;
  /** Every panel's content (all eight are required: a missing one is a type error, never a silent gap). */
  content: Record<RailPanel, React.ReactNode>;
}): React.ReactElement {
  return (
    <AfRailDrawer
      className="code-rail"
      ariaLabel="Workspace panels"
      idBase="code-rail"
      storageKey={RAIL_WIDTH_KEY}
      defaultWidth={420}
      minWidth={320}
      maxWidth={900}
      active={props.panel}
      onActiveChange={(id) => props.onPanel(id as RailPanel | null)}
      items={RAIL_ITEMS.map((item) => ({
        id: item.id,
        label: item.label,
        icon: item.icon,
        content: props.content[item.id],
        ...(item.id === "activity" ? { badge: props.activityBadge, hint: props.activityHint } : {}),
      }))}
    />
  );
}
