// Six shared rail panels: Activity, Files, Model, Agent, Workspace and Voice.
// Agent combines workflow, tools, skills and instructions. Legacy entry ids
// resolve to Agent. Documentation uses the shared standalone drawer.
import React from "react";
import { AfDrawerSystem, type AfDrawerPanel, type IconName } from "@abstractframework/ui-kit";

export type VisibleRailPanel = "activity" | "files" | "model" | "agent" | "workspace" | "voice";
export type RailPanel = VisibleRailPanel | "workflow" | "tools" | "skills";
/** The panels that edit the selection's settings (bound to a conversation or an automation). */
export type SettingsRailPanel = Exclude<RailPanel, "activity" | "files">;
/** Navigation ids, including the standalone Docs assistant. */
export type CodePanel = RailPanel | "docs";

/** Order, label (tooltip, accessible name, panel title) and icon of every rail item. */
export const RAIL_ITEMS: ReadonlyArray<{ id: VisibleRailPanel; label: string; icon: IconName }> = [
  { id: "activity", label: "Activity", icon: "activity" },
  { id: "files", label: "Files", icon: "file" },
  { id: "model", label: "Model", icon: "sparkle" },
  { id: "agent", label: "Agent", icon: "agent" },
  { id: "workspace", label: "Workspace", icon: "folder" },
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
  /** Every panel's content (all six are required: a missing one is a type error, never a silent gap). */
  content: Record<VisibleRailPanel, React.ReactNode>;
  /** The Docs assistant panel (`codeDocsPanel`). */
  docs?: AfDrawerPanel;
}): React.ReactElement {
  return (
    <AfDrawerSystem
      className="code-rail"
      ariaLabel="Workspace panels"
      idBase="code-rail"
      storageKey={RAIL_WIDTH_KEY}
      active={props.panel === "workflow" || props.panel === "tools" || props.panel === "skills" ? "agent" : props.panel}
      onActiveChange={(id) => props.onPanel(id as CodePanel | null)}
      panels={[
        ...RAIL_ITEMS.map((item) => ({
          id: item.id,
          label: item.label,
          icon: item.icon,
          defaultWidth: item.id === "model" ? 360 : 420,
          minWidth: 320,
          maxWidth: 900,
          content: props.content[item.id],
          ...(item.id === "activity" ? { badge: props.activityBadge, hint: props.activityHint } : {}),
        })),
        ...(props.docs ? [{ ...props.docs, minWidth: 320, maxWidth: 900 }] : []),
      ]}
    />
  );
}
