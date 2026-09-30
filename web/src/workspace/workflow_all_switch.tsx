import { AfSwitch } from "@abstractframework/ui-kit";
import React from "react";

/** Why "Show all workflows" cannot change now, or null when it can. */
export function showAllWorkflowsReason(connected: boolean, locked: boolean): string | null {
  if (!connected) return "Connect to a gateway first.";
  if (locked) return "A run is in progress.";
  return null;
}

/**
 * The toolbar's "Show all workflows" switch (a saved preference; applies at once).
 * Phones show the short label "All"; the accessible name stays the full words.
 * Unavailable: the reason is visible text next to the switch (a tap must say why);
 * on a fine pointer with hover the toolbar keeps it for hover and assistive tech
 * only (workspace.css), since the tooltip is reachable there.
 */
export function ShowAllWorkflowsSwitch(props: {
  checked: boolean;
  connected: boolean;
  locked: boolean;
  onChange: (next: boolean) => void;
}): React.ReactElement {
  return (
    <AfSwitch
      id="code-workflow-all"
      className="code-workflow-all"
      variant="sm"
      action="show-all-workflows"
      ariaLabel="Show all workflows"
      label={
        <>
          <span className="code-workflow-all__long">Show all workflows</span>
          <span className="code-workflow-all__short" aria-hidden="true">All</span>
        </>
      }
      hint="List workflows that are not coding agents too"
      checked={props.checked}
      unavailableReason={showAllWorkflowsReason(props.connected, props.locked)}
      onChange={props.onChange}
    />
  );
}
