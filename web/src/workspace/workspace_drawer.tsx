import React, { useEffect, useState } from "react";
import { AfDrawer, AfTabs } from "@abstractframework/ui-kit";
import { useDrawerFocus } from "./layout";

export const WORKSPACE_SECTIONS = [
  ["activity", "Activity"],
  ["files", "Files"],
  ["model", "Model & behavior"],
  ["tools", "Tools & skills"],
  ["workspace", "Workspace"],
  ["voice", "Voice"],
] as const;
export type WorkspaceSection = typeof WORKSPACE_SECTIONS[number][0];
const tabs = WORKSPACE_SECTIONS.map(([id, label]) => ({ id, label }));

/** Shared tabs, with visited content kept mounted to preserve unfinished edits. */
export function WorkspaceDrawer(props: {
  open: boolean;
  onClose(): void;
  section: WorkspaceSection;
  onSection(section: WorkspaceSection): void;
  topOffset: number;
  pages: Record<WorkspaceSection, React.ReactNode>;
}) {
  const [visited, setVisited] = useState<Set<WorkspaceSection>>(() => new Set());
  useEffect(() => {
    if (props.open) setVisited(previous => previous.has(props.section)
      ? previous : new Set([...previous, props.section]));
  }, [props.open, props.section]);
  useDrawerFocus(props.open, true,
    () => document.querySelector(".code-workspace-drawer"),
    () => document.querySelector('.code-workspace-drawer [role="tab"][aria-selected="true"]'));
  return (
    <AfDrawer open={props.open} onClose={props.onClose} label="Workspace & settings"
      title="Workspace & settings" width={760} topOffset={props.topOffset} className="code-workspace-drawer">
      <AfTabs tabs={tabs} value={props.section} onChange={id => props.onSection(id as WorkspaceSection)}
        ariaLabel="Workspace sections" idBase="code-workspace" className="code-workspace-tabs">
        {WORKSPACE_SECTIONS.map(([id]) => (
          <div key={id} hidden={props.section !== id}>
            {visited.has(id) || (props.open && props.section === id) ? props.pages[id] : null}
          </div>
        ))}
      </AfTabs>
    </AfDrawer>
  );
}
