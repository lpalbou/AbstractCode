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
     * Optional rename: a Rename action beside archive and F2 on the row edit the name in place.
     * Called with the trimmed name, at most 120 characters ("" clears it), never for an unchanged
     * one. A returned promise keeps the input read-only until it settles; a rejection keeps the
     * typed text in the open input (the host shows the error). Omitted: the card is unchanged.
     */
    onRename?: (title: string) => void | Promise<void>;
    /** Told when the rename editor opens (true) and closes, after any pending save (false). */
    onRenameEditingChange?: (editing: boolean) => void;
    /** The id of the host's element saying why the last rename was refused (input described by it, marked invalid). */
    renameErrorId?: string;
    archiveDisabled?: boolean;
    disabled?: boolean;
    className?: string;
}
/** Compact, controlled conversation card. Reading, transport, archive confirmation and renaming belong to the host. */
export declare function SessionCard(p: SessionCardProps): React.ReactElement;
//# sourceMappingURL=session_card.d.ts.map