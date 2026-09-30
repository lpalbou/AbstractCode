import React from "react";

/**
 * The conversation sidebar: a docked column from 1024 px, an off-canvas drawer below. A closed
 * drawer is `inert` (out of the tab order and the accessibility tree); its CSS also sets
 * `visibility: hidden` (workspace.css, max-width 1023.98px).
 */
export function SidebarDrawer(props: { isDrawer: boolean; open: boolean; children: React.ReactNode }) {
  const closedDrawer = props.isDrawer && !props.open;
  return (
    <aside
      className="code-sidebar"
      aria-label="Conversations"
      {...(closedDrawer ? { inert: "" } : {})}
    >
      {props.children}
    </aside>
  );
}
