import type { KeyboardEvent } from "react";

/** Arrow navigation for small, automatically activated local tab strips. */
export function navigateTabs(event: KeyboardEvent<HTMLElement>): void {
  if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
  const tabs = [
    ...event.currentTarget.querySelectorAll<HTMLButtonElement>(
      'button[role="tab"]',
    ),
  ].filter((tab) => !tab.disabled);
  const index = tabs.indexOf(event.target as HTMLButtonElement);
  if (index < 0 || !tabs.length) return;
  event.preventDefault();
  const next =
    event.key === "Home"
      ? 0
      : event.key === "End"
        ? tabs.length - 1
        : (index + (event.key === "ArrowRight" ? 1 : -1) + tabs.length) %
          tabs.length;
  tabs[next].focus();
  tabs[next].click();
}
