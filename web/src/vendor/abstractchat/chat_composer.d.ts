import React from "react";
/** Rounded multiline is the shared default; `legacy` restores the former flat layout. */
export type ChatComposerVariant = "compact" | "legacy";
export type ChatComposerProps = {
    value: string;
    onChange: (value: string) => void;
    onSubmit: () => void;
    /** Omit for the shared rounded multiline composer; `legacy` restores the former layout. */
    variant?: ChatComposerVariant;
    placeholder?: string;
    /** Accessible name for the message field. Falls back to its placeholder. */
    ariaLabel?: string;
    textareaId?: string;
    ariaDescribedBy?: string;
    disabled?: boolean;
    busy?: boolean;
    rows?: number;
    autoFocus?: boolean;
    className?: string;
    textareaClassName?: string;
    sendButtonClassName?: string;
    sendLabel?: string;
    busyLabel?: string;
    /** Host eligibility for attachment-only or other valid payloads. Busy/disabled still win. */
    canSubmit?: boolean;
    /** Preserve multiline-first chats that submit with Ctrl/Cmd+Enter. */
    submitKey?: "enter" | "mod-enter";
    actions?: React.ReactNode;
    /**
     * The primary round button turns into Stop (compact variant): an arrow-to-square
     * morph, same size and place. `active` = Stop right now; `stopping` = the stop
     * was asked and is settling (aria-busy). Absent = the button is only ever Send.
     */
    stop?: {
        active: boolean;
        stopping?: boolean;
        onStop: () => void;
        label?: string;
    };
    /** Controls next to Send, such as a microphone. */
    trailingActions?: React.ReactNode;
    /** Shared attachment control; the host owns its file chooser and upload lifecycle. */
    onAttach?: () => void;
    attachLabel?: string;
    attachDisabled?: boolean;
    attachPending?: boolean;
    /** Rendered inside the composer box above the textarea (e.g. attachment chips). */
    leading?: React.ReactNode;
    /** Absolutely positioned layer over the composer box (e.g. a drop zone). */
    overlay?: React.ReactNode;
    onPaste?: React.ClipboardEventHandler<HTMLTextAreaElement>;
};
export declare const ChatComposer: React.ForwardRefExoticComponent<ChatComposerProps & React.RefAttributes<HTMLTextAreaElement>>;
//# sourceMappingURL=chat_composer.d.ts.map