import React from "react";
export interface SessionCardProps {
    sessionId: string;
    title: string;
    selected?: boolean;
    unread?: boolean;
    needsAttention?: boolean;
    working?: boolean;
    turnCount?: number | null;
    toolCount?: number | null;
    countsPartial?: boolean;
    updatedAt?: string | number | null;
    nowMs: number;
    onOpen: () => void;
    onArchive?: () => void;
    /**
     * Optional rename: a Rename action beside archive, F2 on the row and a double-click on the title
     * edit the name in place. Called with the trimmed name ("" clears it), never for an unchanged
     * one. A returned promise keeps the input disabled until it settles; a rejection keeps the
     * typed text in the open input (the host shows the error). Omitted: the card is unchanged.
     */
    onRename?: (title: string) => void | Promise<void>;
    archiveDisabled?: boolean;
    disabled?: boolean;
    className?: string;
}
/** Compact, controlled conversation card. Reading, transport, archive confirmation and renaming belong to the host. */
export declare function SessionCard(p: SessionCardProps): React.ReactElement;
//# sourceMappingURL=session_card.d.ts.map