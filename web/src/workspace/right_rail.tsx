// The right panels (round 5; kit AfDrawerSystem since the ui-primitives shell, backlog 0036):
// one rail icon per subject — Activity, Files, Model, Workflow, Workspace, Tools, Skills, Voice —
// plus the Docs assistant as a drawer-mode panel (opened from the top bar's book button, no rail
// icon). The six settings panels are bound to what is selected in the sidebar (a conversation's run
// settings, or an automation's definition). Docked beside the conversation from 1024 px (resizable,
// each panel's width remembered), floating over it below; the rail itself always stays. The kit
// remembers which panel is open for the docked layout only, under the same key as the width.
import React from "react";
import { AfDrawerSystem, type AfDrawerPanel, type IconName } from "@abstractframework/ui-kit";

export type RailPanel = "activity" | "files" | "model" | "workflow" | "workspace" | "tools" | "skills" | "voice";
/** The panels that edit the selection's settings (bound to a conversation or an automation). */
export type SettingsRailPanel = Exclude<RailPanel, "activity" | "files">;
/** Every panel of the right side: the rail's eight and the Docs assistant. */
export type CodePanel = RailPanel | "docs";

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

/** The kit's one storage key (widths per panel + the docked open panel); the 0.8.9 numeric width is kept. */
export const RAIL_WIDTH_KEY = "abstractcode.rail.width.v1";

export function CodeRightRail(props: {
  panel: CodePanel | null;
  onPanel: (next: CodePanel | null) => void;
  /** Rows waiting or running in Activity (the rail's badge). */
  activityBadge?: number;
  activityHint?: string;
  /** Every panel's content (all eight are required: a missing one is a type error, never a silent gap). */
  content: Record<RailPanel, React.ReactNode>;
  /** The Docs assistant panel (`codeDocsPanel`). */
  docs: AfDrawerPanel;
}): React.ReactElement {
  return (
    <AfDrawerSystem
      className="code-rail"
      ariaLabel="Workspace panels"
      idBase="code-rail"
      storageKey={RAIL_WIDTH_KEY}
      active={props.panel}
      onActiveChange={(id) => props.onPanel(id as CodePanel | null)}
      panels={[
        ...RAIL_ITEMS.map((item) => ({
          id: item.id,
          label: item.label,
          icon: item.icon,
          defaultWidth: 420,
          minWidth: 320,
          maxWidth: 900,
          content: props.content[item.id],
          ...(item.id === "activity" ? { badge: props.activityBadge, hint: props.activityHint } : {}),
        })),
        { ...props.docs, minWidth: 320, maxWidth: 900 },
      ]}
    />
  );
}
