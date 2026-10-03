// The right panel (round 4): the kit's vertical rail drawer with three
// panels — Activity, Files, Settings. Docked beside the conversation from
// 1024 px (resizable, width remembered), floating over it below; the rail
// itself always stays. Which panel is open is remembered for the docked
// layout only (a phone always starts with the content visible).
import React, { useCallback, useEffect, useState } from "react";
import { AF_MEDIA, AfRailDrawer, useAfMedia } from "@abstractframework/ui-kit";

export type RailPanel = "activity" | "files" | "settings";
/** The Settings panel's groups (anchors `#code-settings-<id>`). */
export type SettingsGroupId = "model" | "tools" | "workspace" | "voice";

export const RAIL_PANEL_KEY = "abstractcode.rail.panel.v1";
export const RAIL_WIDTH_KEY = "abstractcode.rail.width.v1";
const PANELS: readonly RailPanel[] = ["activity", "files", "settings"];

function readPanel(): RailPanel | null {
  try {
    const raw = window.localStorage.getItem(RAIL_PANEL_KEY);
    if (raw === "") return null;
    return PANELS.includes(raw as RailPanel) ? (raw as RailPanel) : null;
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
  const [panel, setPanelState] = useState<RailPanel | null>(() => (typeof window !== "undefined" && !window.matchMedia?.(AF_MEDIA.md).matches ? readPanel() : null));
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

/** Scroll the Settings panel to one group once it is visible. */
export function revealSettingsGroup(group: SettingsGroupId): void {
  window.setTimeout(() => {
    const el = document.getElementById(`code-settings-${group}`);
    el?.scrollIntoView({ block: "start", behavior: "smooth" });
    el?.querySelector<HTMLElement>("h3")?.focus?.({ preventScroll: true });
  }, 0);
}

export function CodeRightRail(props: {
  panel: RailPanel | null;
  onPanel: (next: RailPanel | null) => void;
  /** Rows waiting or running in Activity (the rail's badge). */
  activityBadge?: number;
  activityHint?: string;
  activity: React.ReactNode;
  files: React.ReactNode;
  settings: React.ReactNode;
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
      items={[
        { id: "activity", label: "Activity", icon: "activity", badge: props.activityBadge, hint: props.activityHint, content: props.activity },
        { id: "files", label: "Files", icon: "folder", content: props.files },
        { id: "settings", label: "Settings", icon: "cog", content: props.settings },
      ]}
    />
  );
}
