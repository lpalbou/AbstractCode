import React from "react";
import { AfTooltip, Icon, type AfTooltipContent } from "@abstractframework/ui-kit";

/** Short labels stay visible; longer guidance uses the shared themed tooltip. */
export function SettingsHelp({ label, children }: { label: string; children: string | AfTooltipContent }): React.ReactElement {
  return <AfTooltip content={children}><button type="button" className="af-settings-help" aria-label={label} onClick={e => { e.preventDefault(); e.stopPropagation(); }}><Icon name="info" size={16} /></button></AfTooltip>;
}
