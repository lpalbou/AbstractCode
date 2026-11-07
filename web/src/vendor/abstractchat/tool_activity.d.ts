import React from "react";
import type { WorkflowToolActivity } from "./workflow_evidence.js";
export declare function toolArguments(value: unknown): unknown;
/** A labeled preview only. Full arguments remain available on expansion. */
export declare function toolPreview(value: unknown): {
    label: string;
    value: string;
};
export declare function ToolActivity({ tool, showCopy, }: {
    tool: WorkflowToolActivity;
    showCopy?: boolean;
}): React.ReactElement;
/** Consecutive calls share one frame, not one full conversation card per call. */
export declare function ToolActivityGroup({ tools, showCopy, notes, }: {
    tools: WorkflowToolActivity[];
    showCopy?: boolean;
    /** Muted sentences shown directly under the group (the session-policy decision). */
    notes?: string[];
}): React.ReactElement;
//# sourceMappingURL=tool_activity.d.ts.map